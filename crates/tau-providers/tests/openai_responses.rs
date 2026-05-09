use tau_llm::{ContentBlock, Message, Role, StreamEvent};
use tau_providers::{messages_to_responses_input, parse_openai_responses_sse};

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

#[test]
fn pipes_responses_call_id_back_as_function_call_output() {
    let messages = vec![
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"path": "Cargo.toml"}),
            }],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "[workspace]".to_string(),
                is_error: false,
            }],
        },
    ];

    let input = messages_to_responses_input(&messages);
    assert_eq!(input[0]["type"], "function_call");
    assert_eq!(input[0]["id"], "call_1");
    assert_eq!(input[0]["call_id"], "call_1");
    assert_eq!(input[1]["type"], "function_call_output");
    assert_eq!(input[1]["call_id"], "call_1");
    assert_eq!(input[1]["output"], "[workspace]");
}
