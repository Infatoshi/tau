use tau_llm::StreamEvent;
use tau_providers::parse_openai_responses_sse;

#[test]
fn parses_text_and_function_call_arguments() {
    let fixture = include_str!("fixtures/openai_responses_tool.sse");
    let events = parse_openai_responses_sse(fixture).unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, StreamEvent::TextDelta { text } if text == "I'll read it.")));
    let call = events
        .iter()
        .find_map(|event| match event {
            StreamEvent::ToolCallDone { call } => Some(call),
            _ => None,
        })
        .unwrap();
    assert_eq!(call.id, "call_1");
    assert_eq!(call.name, "read");
    assert_eq!(call.input["path"], "Cargo.toml");
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::MessageStop { stop_reason } if stop_reason.as_deref() == Some("completed")
    )));
}
