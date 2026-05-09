use tau_llm::StreamEvent;
use tau_providers::parse_anthropic_sse;

#[test]
fn parses_tool_json_deltas() {
    let fixture = include_str!("fixtures/anthropic_tool.sse");
    let events = parse_anthropic_sse(fixture).unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::TextDelta { text } if text == "I'll read it."
    )));
    let call = events
        .iter()
        .find_map(|event| match event {
            StreamEvent::ToolCallDone { call } => Some(call),
            _ => None,
        })
        .unwrap();
    assert_eq!(call.id, "toolu_1");
    assert_eq!(call.name, "read");
    assert_eq!(call.input["path"], "Cargo.toml");
}
