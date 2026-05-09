use std::path::Path;
use tau_core::ToolResult;

pub fn blocked_by_sandbox() -> ToolResult {
    tool_error(
        "tool blocked by sandbox_mode; set sandbox_mode: yolo in ~/.tau/config.yaml to allow write/edit/bash",
    )
}

pub fn not_regular_file() -> ToolResult {
    tool_error("not a regular file")
}

pub fn file_too_large() -> ToolResult {
    tool_error("file too large")
}

pub fn empty_old_string() -> ToolResult {
    tool_error("old_string must not be empty")
}

pub fn symlink_outside_cwd(path: &Path) -> ToolResult {
    tool_error(&format!(
        "refusing to edit symlink outside cwd: {}",
        path.display()
    ))
}

pub fn old_string_not_found(path: &Path) -> ToolResult {
    tool_error(&format!("old_string not found in {}", path.display()))
}

pub fn old_string_not_unique(path: &Path, occurrences: usize) -> ToolResult {
    tool_error(&format!(
        "old_string is not unique in {} ({} occurrences). Provide more surrounding context or set replace_all=true.",
        path.display(),
        occurrences
    ))
}

pub fn parent_not_directory(parent: &Path) -> anyhow::Error {
    anyhow::anyhow!("parent path is not a directory: {}", parent.display())
}

pub fn tool_error(message: &str) -> ToolResult {
    ToolResult {
        content: message.to_string(),
        is_error: true,
    }
}
