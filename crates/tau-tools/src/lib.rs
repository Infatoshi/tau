use async_trait::async_trait;
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
use tau_core::{Tool, ToolResult};
use tau_llm::ToolSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const READ_LIMIT: u64 = 10 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 100 * 1024;

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
