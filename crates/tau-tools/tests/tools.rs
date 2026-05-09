use tau_core::Tool;
use tau_tools::{run_bash, BashTool, ReadTool};
use serde_json::json;
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
