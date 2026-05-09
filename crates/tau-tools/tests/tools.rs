use serde_json::json;
use std::fs as std_fs;
use tau_core::Tool;
use tau_tools::{run_bash, BashTool, EditTool, PermissionedTool, ReadTool, SandboxMode, WriteTool};
use tempfile::tempdir;
use tokio::fs;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn read_with_line_range() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sample.txt");
    fs::write(&path, "one\ntwo\nthree\nfour\n").await.unwrap();
    let tool = ReadTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "start_line": 2, "end_line": 3}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "two\nthree");
}

#[tokio::test]
async fn bash_output_capture() {
    let result = BashTool
        .execute(json!({"command": "printf hello"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "hello");
}

#[tokio::test]
async fn permissioned_tool_blocks_risky_tools_without_yolo() {
    let result = PermissionedTool::new(BashTool, SandboxMode::ReadOnly)
        .execute(json!({"command": "printf hello"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("sandbox_mode"));
}

#[tokio::test]
async fn permissioned_tool_allows_risky_tools_with_yolo() {
    let result = PermissionedTool::new(BashTool, SandboxMode::Yolo)
        .execute(json!({"command": "printf hello"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "hello");
}

#[tokio::test]
async fn bash_timeout() {
    let result = run_bash("sleep 5".to_string(), Some(50), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
}

#[tokio::test]
async fn bash_cancellation_kills_process_group() {
    let dir = tempdir().unwrap();
    let marker = dir.path().join("marker");
    let command = format!(
        "trap 'echo term > {}' TERM; sleep 30 & wait",
        marker.display()
    );
    let token = CancellationToken::new();
    let cloned = token.clone();
    let handle = tokio::spawn(async move { run_bash(command, Some(10_000), cloned).await });
    sleep(Duration::from_millis(100)).await;
    token.cancel();
    let result = handle.await.unwrap().unwrap();
    assert!(result.is_error);
}

#[tokio::test]
async fn write_new_file_with_content() {
    let dir = tempdir().unwrap();
    let tool = WriteTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "new.txt", "content": "hello"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt"))
            .await
            .unwrap(),
        "hello"
    );
}

#[tokio::test]
async fn write_overwrites_existing_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("existing.txt");
    fs::write(&path, "old").await.unwrap();
    let tool = WriteTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "existing.txt", "content": "new"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(fs::read_to_string(path).await.unwrap(), "new");
}

#[tokio::test]
async fn write_creates_parent_directories() {
    let dir = tempdir().unwrap();
    let tool = WriteTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "a/b/c.txt", "content": "nested"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("a/b/c.txt"))
            .await
            .unwrap(),
        "nested"
    );
}

#[tokio::test]
async fn write_removes_tmp_file_after_success() {
    let dir = tempdir().unwrap();
    let tool = WriteTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "atomic.txt", "content": "complete"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(!dir.path().join("atomic.txt.tau-tmp").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("atomic.txt"))
            .await
            .unwrap(),
        "complete"
    );
}

#[tokio::test]
async fn edit_replaces_unique_match() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), "hello world\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "world", "new_string": "tau"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("sample.txt"))
            .await
            .unwrap(),
        "hello tau\n"
    );
}

#[tokio::test]
async fn edit_errors_on_zero_matches() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), "hello\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "missing", "new_string": "tau"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("old_string not found"));
}

#[tokio::test]
async fn edit_errors_on_multiple_matches_without_replace_all() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), "same\nsame\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "same", "new_string": "done"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("old_string is not unique"));
}

#[tokio::test]
async fn edit_replaces_all_when_requested() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), "same\nsame\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "same", "new_string": "done", "replace_all": true}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("sample.txt"))
            .await
            .unwrap(),
        "done\ndone\n"
    );
}

#[tokio::test]
async fn edit_preserves_crlf_line_endings() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), b"one\r\ntwo\r\nthree\r\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "two\n", "new_string": "TWO\n"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read(dir.path().join("sample.txt")).await.unwrap(),
        b"one\r\nTWO\r\nthree\r\n"
    );
}

#[tokio::test]
async fn edit_preserves_lf_line_endings() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), b"one\ntwo\nthree\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "two\n", "new_string": "TWO\n"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read(dir.path().join("sample.txt")).await.unwrap(),
        b"one\nTWO\nthree\n"
    );
}

#[tokio::test]
async fn edit_preserves_bom() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.txt"), b"\xEF\xBB\xBFhello\n")
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.txt", "old_string": "hello", "new_string": "hi"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read(dir.path().join("sample.txt")).await.unwrap(),
        b"\xEF\xBB\xBFhi\n"
    );
}

#[tokio::test]
async fn edit_errors_on_non_utf8_input() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("sample.bin"), [0xFF, 0xFE, 0xFD])
        .await
        .unwrap();
    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "sample.bin", "old_string": "x", "new_string": "y"}),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn edit_errors_on_symlink_outside_cwd() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let outside_file = outside.path().join("outside.txt");
    fs::write(&outside_file, "secret").await.unwrap();
    std_fs::create_dir(dir.path().join("links")).unwrap();
    std::os::unix::fs::symlink(&outside_file, dir.path().join("links/outside.txt")).unwrap();

    let tool = EditTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(
            json!({"path": "links/outside.txt", "old_string": "secret", "new_string": "changed"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert_eq!(fs::read_to_string(outside_file).await.unwrap(), "secret");
}

#[tokio::test]
async fn write_edit_read_round_trip() {
    let dir = tempdir().unwrap();
    let write = WriteTool::new(dir.path().to_path_buf());
    let edit = EditTool::new(dir.path().to_path_buf());
    let read = ReadTool::new(dir.path().to_path_buf());

    let result = write
        .execute(
            json!({"path": "round/trip.txt", "content": "before\n"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);

    let result = edit
        .execute(
            json!({"path": "round/trip.txt", "old_string": "before", "new_string": "after"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);

    let result = read
        .execute(json!({"path": "round/trip.txt"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "after\n");
}
