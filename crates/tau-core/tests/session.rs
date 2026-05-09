use chrono::Utc;
use tau_core::session::{events_to_messages, read_events, SessionEvent, SessionStore};
use tau_llm::{ContentBlock, Role};
use tempfile::tempdir;

#[tokio::test]
async fn jsonl_session_round_trip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut store = SessionStore::create_at(dir.path(), "test-model", path.clone())
        .await
        .unwrap();
    store
        .append(&SessionEvent::UserMessage {
            timestamp: Utc::now(),
            content: "hello".to_string(),
        })
        .await
        .unwrap();
    store
        .append(&SessionEvent::AssistantMessage {
            timestamp: Utc::now(),
            content: vec![ContentBlock::Text {
                text: "hi".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
        })
        .await
        .unwrap();

    let events = read_events(&path).await.unwrap();
    assert_eq!(events.len(), 3);
    let messages = events_to_messages(&events);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, Role::User);
    assert_eq!(messages[1].role, Role::Assistant);
}
