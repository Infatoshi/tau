use chrono::Utc;
use tau_core::session::{events_to_messages, read_events, SessionEvent, SessionStore};
use tau_llm::{ContentBlock, Role};
use tempfile::tempdir;

#[tokio::test]
async fn created_session_short_hash_matches_filename() {
    let dir = tempdir().unwrap();
    let store = SessionStore::create(dir.path(), "test-model")
        .await
        .unwrap();
    let stem = store
        .path()
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap();
    assert!(stem.starts_with(&store.short_hash()));
}

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

#[tokio::test]
async fn compact_event_replaces_prior_messages_on_resume() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut store = SessionStore::create_at(dir.path(), "test-model", path.clone())
        .await
        .unwrap();
    store
        .append(&SessionEvent::UserMessage {
            timestamp: Utc::now(),
            content: "old request".to_string(),
        })
        .await
        .unwrap();
    store
        .append(&SessionEvent::Compact {
            timestamp: Utc::now(),
            summary: "new compact summary".to_string(),
        })
        .await
        .unwrap();

    let events = read_events(&path).await.unwrap();
    let messages = events_to_messages(&events);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, Role::User);
    assert!(matches!(
        &messages[0].content[0],
        ContentBlock::Text { text } if text.contains("new compact summary")
    ));
}
