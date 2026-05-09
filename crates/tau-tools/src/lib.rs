use anyhow::{anyhow, Context};
use async_trait::async_trait;
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tau_core::{Tool, ToolResult};
use tau_llm::ToolSchema;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const READ_LIMIT: u64 = 10 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 100 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    ReadOnly,
    Yolo,
}

impl SandboxMode {
    pub fn from_config(value: &str) -> Self {
        if value.eq_ignore_ascii_case("yolo") {
            Self::Yolo
        } else {
            Self::ReadOnly
        }
    }
}

pub struct PermissionedTool<T> {
    inner: T,
    mode: SandboxMode,
}

impl<T> PermissionedTool<T> {
    pub fn new(inner: T, mode: SandboxMode) -> Self {
        Self { inner, mode }
    }
}

#[async_trait]
impl<T> Tool for PermissionedTool<T>
where
    T: Tool + Send + Sync,
{
    fn schema(&self) -> ToolSchema {
        self.inner.schema()
    }

    async fn execute(
        &self,
        input: Value,
        cancellation: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        if self.mode != SandboxMode::Yolo {
            return Ok(error_result(
                "tool blocked by sandbox_mode; set sandbox_mode = \"yolo\" in ~/.tau/config.toml to allow write/edit/bash",
            ));
        }
        self.inner.execute(input, cancellation).await
    }
}

pub struct ReadTool {
    cwd: PathBuf,
}

impl ReadTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read".to_string(),
            description: "Read a file, optionally with a 1-indexed inclusive line range."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer", "minimum": 1 },
                    "end_line": { "type": "integer", "minimum": 1 }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, input: Value, _: CancellationToken) -> anyhow::Result<ToolResult> {
        let args: ReadArgs = serde_json::from_value(input)?;
        let path = resolve_path(&self.cwd, &args.path);
        let meta = fs::metadata(&path).await?;
        if !meta.is_file() {
            return Ok(error_result("not a regular file"));
        }
        if meta.len() > READ_LIMIT {
            return Ok(error_result("file too large"));
        }
        let content = fs::read_to_string(&path).await?;
        let content = match (args.start_line, args.end_line) {
            (None, None) => content,
            (start, end) => slice_lines(&content, start.unwrap_or(1), end),
        };
        Ok(ToolResult {
            content,
            is_error: false,
        })
    }
}

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

pub struct WriteTool {
    cwd: PathBuf,
}

impl WriteTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write".to_string(),
            description:
                "Write content to a file atomically, creating parent directories as needed."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn execute(&self, input: Value, _: CancellationToken) -> anyhow::Result<ToolResult> {
        let args: WriteArgs = serde_json::from_value(input)?;
        let path = resolve_path(&self.cwd, &args.path);
        atomic_write(&path, args.content.as_bytes()).await?;
        Ok(ToolResult {
            content: format!("wrote {} bytes to {}", args.content.len(), path.display()),
            is_error: false,
        })
    }
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

pub struct EditTool {
    cwd: PathBuf,
}

impl EditTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for EditTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "edit".to_string(),
            description: "Edit a UTF-8 file using exact text replacement.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    async fn execute(&self, input: Value, _: CancellationToken) -> anyhow::Result<ToolResult> {
        let args: EditArgs = serde_json::from_value(input)?;
        let path = resolve_path(&self.cwd, &args.path);

        if args.old_string.is_empty() {
            return Ok(error_result("old_string must not be empty"));
        }
        if symlink_points_outside_cwd(&self.cwd, &path).await? {
            return Ok(error_result(&format!(
                "refusing to edit symlink outside cwd: {}",
                path.display()
            )));
        }

        let bytes = fs::read(&path).await?;
        let (has_bom, content_bytes) = strip_bom(&bytes);
        let content = String::from_utf8(content_bytes.to_vec())
            .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
        let line_ending = detect_line_ending(&content);
        let normalized_content = normalize_line_endings(&content, line_ending);
        let old_string = normalize_line_endings(&args.old_string, line_ending);
        let new_string = normalize_line_endings(&args.new_string, line_ending);
        let occurrences = normalized_content.matches(&old_string).count();

        if occurrences == 0 {
            return Ok(error_result(&format!(
                "old_string not found in {}",
                path.display()
            )));
        }
        if !args.replace_all.unwrap_or(false) && occurrences > 1 {
            return Ok(error_result(&format!(
                "old_string is not unique in {} ({} occurrences). Provide more surrounding context or set replace_all=true.",
                path.display(),
                occurrences
            )));
        }

        let replaced = if args.replace_all.unwrap_or(false) {
            normalized_content.replace(&old_string, &new_string)
        } else {
            normalized_content.replacen(&old_string, &new_string, 1)
        };
        let output = restore_line_endings(&replaced, line_ending);
        let mut output_bytes = Vec::new();
        if has_bom {
            output_bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        output_bytes.extend_from_slice(output.as_bytes());
        atomic_write(&path, &output_bytes).await?;

        Ok(ToolResult {
            content: format!(
                "replaced {} occurrence(s) in {}",
                occurrences,
                path.display()
            ),
            is_error: false,
        })
    }
}

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".to_string(),
            description: "Execute a shell command with /bin/bash -lc.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 600000 }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(
        &self,
        input: Value,
        cancellation: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let args: BashArgs = serde_json::from_value(input)?;
        run_bash(args.command, args.timeout_ms, cancellation).await
    }
}

#[derive(Deserialize)]
struct BashArgs {
    command: String,
    timeout_ms: Option<u64>,
}

pub async fn run_bash(
    command: String,
    timeout_ms: Option<u64>,
    cancellation: CancellationToken,
) -> anyhow::Result<ToolResult> {
    let timeout_ms = timeout_ms.unwrap_or(120_000).min(600_000);
    let mut child = Command::new("/bin/bash")
        .arg("-lc")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let pid = child.id().map(|id| Pid::from_raw(id as i32));
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let out_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let wait = child.wait();
    tokio::pin!(wait);
    let status = tokio::select! {
        result = timeout(Duration::from_millis(timeout_ms), &mut wait) => {
            match result {
                Ok(status) => Some(status?),
                Err(_) => {
                    terminate_group(pid).await;
                    None
                }
            }
        }
        () = cancellation.cancelled() => {
            terminate_group(pid).await;
            None
        }
    };

    let mut output = Vec::new();
    if let Ok(Ok(mut stdout)) = out_task.await {
        output.append(&mut stdout);
    }
    if let Ok(Ok(mut stderr)) = err_task.await {
        output.append(&mut stderr);
    }
    let total = output.len();
    let mut content =
        String::from_utf8_lossy(&output[..output.len().min(OUTPUT_LIMIT)]).to_string();
    if total > OUTPUT_LIMIT {
        content.push_str(&format!("\n[output truncated, {total} bytes total]"));
    }
    let is_error = status.map(|s| !s.success()).unwrap_or(true);
    if status.is_none() && content.is_empty() {
        content = "cancelled or timed out".to_string();
    }
    Ok(ToolResult { content, is_error })
}

async fn terminate_group(pid: Option<Pid>) {
    if let Some(pid) = pid {
        let _ = killpg(pid, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = killpg(pid, Signal::SIGKILL);
    }
}

fn resolve_path(cwd: &Path, path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
        let meta = fs::metadata(parent).await?;
        if !meta.is_dir() {
            return Err(anyhow!(
                "parent path is not a directory: {}",
                parent.display()
            ));
        }
    }
    let tmp_path = temp_path(path);
    fs::write(&tmp_path, bytes).await?;
    fs::rename(&tmp_path, path).await?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from(".tau-tmp"));
    name.push(".tau-tmp");
    path.with_file_name(name)
}

async fn symlink_points_outside_cwd(cwd: &Path, path: &Path) -> anyhow::Result<bool> {
    let meta = fs::symlink_metadata(path).await?;
    if !meta.file_type().is_symlink() {
        return Ok(false);
    }
    let link = fs::read_link(path).await?;
    let target = if link.is_absolute() {
        link
    } else {
        path.parent().unwrap_or_else(|| Path::new("")).join(link)
    };
    let cwd = fs::canonicalize(cwd).await?;
    let target = fs::canonicalize(target).await?;
    Ok(!target.starts_with(cwd))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LineEnding {
    Lf,
    Crlf,
}

fn detect_line_ending(content: &str) -> LineEnding {
    match content.find('\n') {
        Some(0) => LineEnding::Lf,
        Some(idx) if content.as_bytes().get(idx - 1) == Some(&b'\r') => LineEnding::Crlf,
        Some(_) => LineEnding::Lf,
        None => LineEnding::Lf,
    }
}

fn normalize_line_endings(content: &str, line_ending: LineEnding) -> String {
    match line_ending {
        LineEnding::Lf => content.to_string(),
        LineEnding::Crlf => content.replace("\r\n", "\n"),
    }
}

fn restore_line_endings(content: &str, line_ending: LineEnding) -> String {
    match line_ending {
        LineEnding::Lf => content.to_string(),
        LineEnding::Crlf => content.replace('\n', "\r\n"),
    }
}

fn strip_bom(bytes: &[u8]) -> (bool, &[u8]) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (true, &bytes[3..])
    } else {
        (false, bytes)
    }
}

fn slice_lines(content: &str, start: usize, end: Option<usize>) -> String {
    content
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let line_no = idx + 1;
            if line_no >= start && end.is_none_or(|end| line_no <= end) {
                Some(line)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn error_result(message: &str) -> ToolResult {
    ToolResult {
        content: message.to_string(),
        is_error: true,
    }
}
