use async_trait::async_trait;
use futures::TryStreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tau_llm::{
    ContentBlock, Provider, ProviderRequest, ProviderStream, Role, StreamEvent, ToolCall,
};
use tokio_util::sync::CancellationToken;

pub struct AnthropicProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY is required"))?;
        Ok(Self {
            api_key,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn stream(
        &self,
        request: ProviderRequest,
        cancellation: CancellationToken,
    ) -> anyhow::Result<ProviderStream> {
        let body = json!({
            "model": request.model,
            "system": request.system,
            "messages": request.messages.iter().map(message_to_anthropic).collect::<Vec<_>>(),
            "tools": request.tools,
            "max_tokens": request.max_tokens,
            "stream": true
        });
        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Anthropic request failed: {status} {text}"));
        }
        let byte_stream = response.bytes_stream();
        let stream = async_stream::try_stream! {
            let mut buffer = String::new();
            let mut blocks = BlockState::default();
            futures::pin_mut!(byte_stream);
            while let Some(chunk) = byte_stream.try_next().await? {
                if cancellation.is_cancelled() {
                    break;
                }
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(idx) = buffer.find("\n\n") {
                    let raw = buffer[..idx].to_string();
                    buffer = buffer[idx + 2..].to_string();
                    for event in parse_one_event(&raw, &mut blocks)? {
                        yield event;
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

pub fn parse_anthropic_sse(input: &str) -> anyhow::Result<Vec<StreamEvent>> {
    let mut blocks = BlockState::default();
    let mut events = Vec::new();
    for raw in input.split("\n\n") {
        if raw.trim().is_empty() {
            continue;
        }
        events.extend(parse_one_event(raw, &mut blocks)?);
    }
    Ok(events)
}

#[derive(Default)]
struct BlockState {
    tool_by_index: HashMap<u64, PendingTool>,
}

struct PendingTool {
    id: String,
    name: String,
    json: String,
}

fn parse_one_event(raw: &str, blocks: &mut BlockState) -> anyhow::Result<Vec<StreamEvent>> {
    let mut data_lines = Vec::new();
    for line in raw.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start());
        }
    }
    if data_lines.is_empty() {
        return Ok(Vec::new());
    }
    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return Ok(Vec::new());
    }
    let event: AnthropicEvent = serde_json::from_str(&data)?;
    let mut out = Vec::new();
    match event {
        AnthropicEvent::MessageStart { .. } => out.push(StreamEvent::MessageStart),
        AnthropicEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            if content_block.kind == "tool_use" {
                let id = content_block.id.unwrap_or_default();
                let name = content_block.name.unwrap_or_default();
                blocks.tool_by_index.insert(
                    index,
                    PendingTool {
                        id: id.clone(),
                        name: name.clone(),
                        json: String::new(),
                    },
                );
                out.push(StreamEvent::ToolCallStart { id, name });
            }
        }
        AnthropicEvent::ContentBlockDelta { index, delta } => match delta.kind.as_str() {
            "text_delta" => out.push(StreamEvent::TextDelta {
                text: delta.text.unwrap_or_default(),
            }),
            "input_json_delta" => {
                if let Some(tool) = blocks.tool_by_index.get_mut(&index) {
                    let partial = delta.partial_json.unwrap_or_default();
                    tool.json.push_str(&partial);
                    out.push(StreamEvent::ToolCallDelta {
                        id: tool.id.clone(),
                        json_delta: partial,
                    });
                }
            }
            _ => {}
        },
        AnthropicEvent::ContentBlockStop { index } => {
            if let Some(tool) = blocks.tool_by_index.remove(&index) {
                let input = if tool.json.trim().is_empty() {
                    Value::Object(Default::default())
                } else {
                    serde_json::from_str(&tool.json)?
                };
                out.push(StreamEvent::ToolCallDone {
                    call: ToolCall {
                        id: tool.id,
                        name: tool.name,
                        input,
                    },
                });
            }
        }
        AnthropicEvent::MessageDelta { delta } => out.push(StreamEvent::MessageStop {
            stop_reason: delta.stop_reason,
        }),
        AnthropicEvent::MessageStop => {}
        AnthropicEvent::Error { error } => out.push(StreamEvent::Error {
            message: error.message,
        }),
    }
    Ok(out)
}

fn message_to_anthropic(message: &tau_llm::Message) -> Value {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    json!({
        "role": role,
        "content": message.content.iter().map(block_to_anthropic).collect::<Vec<_>>()
    })
}

fn block_to_anthropic(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content,
            "is_error": is_error
        }),
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicEvent {
    MessageStart {
        #[serde(rename = "message")]
        _message: Value,
    },
    ContentBlockStart {
        index: u64,
        content_block: ContentBlockStart,
    },
    ContentBlockDelta {
        index: u64,
        delta: Delta,
    },
    ContentBlockStop {
        index: u64,
    },
    MessageDelta {
        delta: MessageDelta,
    },
    MessageStop,
    Error {
        error: AnthropicError,
    },
}

#[derive(Deserialize)]
struct ContentBlockStart {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    partial_json: Option<String>,
}

#[derive(Deserialize)]
struct MessageDelta {
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicError {
    message: String,
}
