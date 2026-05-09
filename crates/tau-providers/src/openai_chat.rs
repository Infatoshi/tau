use async_trait::async_trait;
use futures::TryStreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tau_llm::{
    ContentBlock, Provider, ProviderRequest, ProviderStream, Role, StreamEvent, ToolCall,
};
use tokio_util::sync::CancellationToken;

pub struct OpenAiChatProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiChatProvider {
    pub fn from_env(model: Option<String>, base_url: Option<String>) -> anyhow::Result<Self> {
        let api_key = if base_url.as_deref().is_some_and(|url| url.contains("z.ai")) {
            std::env::var("ZAI_API_KEY").or_else(|_| std::env::var("OPENAI_API_KEY"))
        } else {
            std::env::var("OPENAI_API_KEY").or_else(|_| std::env::var("ZAI_API_KEY"))
        }
        .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY or ZAI_API_KEY is required"))?;
        Ok(Self::new(api_key, model, base_url))
    }

    pub fn new(api_key: String, model: Option<String>, base_url: Option<String>) -> Self {
        let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let client = if base_url.contains("z.ai") {
            reqwest::Client::builder().http1_only().build()
        } else {
            reqwest::Client::builder().build()
        }
        .expect("reqwest client builder succeeds");
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gpt-4o".to_string()),
            base_url,
            client,
        }
    }
}

#[async_trait]
impl Provider for OpenAiChatProvider {
    async fn stream(
        &self,
        request: ProviderRequest,
        cancellation: CancellationToken,
    ) -> anyhow::Result<ProviderStream> {
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };
        let mut messages = vec![json!({
            "role": "system",
            "content": request.system
        })];
        messages.extend(messages_to_chat(&request.messages));
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "max_tokens": request.max_tokens
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema
                            }
                        })
                    })
                    .collect(),
            );
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OpenAI Chat Completions request failed: {status} {text}"
            ));
        }
        let byte_stream = response.bytes_stream();
        let stream = async_stream::try_stream! {
            let mut buffer = String::new();
            let mut calls = ChatState::default();
            let mut started = false;
            futures::pin_mut!(byte_stream);
            while let Some(chunk) = byte_stream.try_next().await? {
                if cancellation.is_cancelled() {
                    break;
                }
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(idx) = buffer.find("\n\n") {
                    let raw = buffer[..idx].to_string();
                    buffer = buffer[idx + 2..].to_string();
                    for event in parse_one_event(&raw, &mut calls, &mut started)? {
                        yield event;
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

pub fn parse_openai_chat_sse(input: &str) -> anyhow::Result<Vec<StreamEvent>> {
    let mut calls = ChatState::default();
    let mut started = false;
    let mut events = Vec::new();
    for raw in input.split("\n\n") {
        if raw.trim().is_empty() {
            continue;
        }
        events.extend(parse_one_event(raw, &mut calls, &mut started)?);
    }
    Ok(events)
}

#[derive(Default)]
struct ChatState {
    calls: HashMap<u64, PendingChatCall>,
}

struct PendingChatCall {
    id: String,
    name: String,
    json: String,
}

fn parse_one_event(
    raw: &str,
    calls: &mut ChatState,
    started: &mut bool,
) -> anyhow::Result<Vec<StreamEvent>> {
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
    let chunk: ChatChunk = serde_json::from_str(&data)?;
    let mut out = Vec::new();
    if !*started {
        *started = true;
        out.push(StreamEvent::MessageStart);
    }
    let Some(choice) = chunk.choices.into_iter().next() else {
        return Ok(out);
    };
    if let Some(content) = choice.delta.content {
        out.push(StreamEvent::TextDelta { text: content });
    }
    for tool_call in choice.delta.tool_calls {
        let entry = calls.calls.entry(tool_call.index).or_insert_with(|| {
            let id = tool_call.id.clone().unwrap_or_default();
            let name = tool_call
                .function
                .as_ref()
                .and_then(|function| function.name.clone())
                .unwrap_or_default();
            out.push(StreamEvent::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
            });
            PendingChatCall {
                id,
                name,
                json: String::new(),
            }
        });
        if let Some(id) = tool_call.id {
            entry.id = id;
        }
        if let Some(function) = tool_call.function {
            if let Some(name) = function.name {
                entry.name = name;
            }
            if let Some(arguments) = function.arguments {
                entry.json.push_str(&arguments);
                out.push(StreamEvent::ToolCallDelta {
                    id: entry.id.clone(),
                    json_delta: arguments,
                });
            }
        }
    }
    if let Some(reason) = choice.finish_reason {
        let mut indexes = calls.calls.keys().copied().collect::<Vec<_>>();
        indexes.sort_unstable();
        for index in indexes {
            if let Some(call) = calls.calls.remove(&index) {
                out.push(done_event(call)?);
            }
        }
        out.push(StreamEvent::MessageStop {
            stop_reason: Some(reason),
        });
    }
    Ok(out)
}

fn done_event(call: PendingChatCall) -> anyhow::Result<StreamEvent> {
    let input = if call.json.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(&call.json)?
    };
    Ok(StreamEvent::ToolCallDone {
        call: ToolCall {
            id: call.id,
            name: call.name,
            input,
        },
    })
}

fn messages_to_chat(messages: &[tau_llm::Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for message in messages {
        match message.role {
            Role::User => {
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            out.push(json!({ "role": "user", "content": text }));
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => {
                            out.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content
                            }));
                        }
                        ContentBlock::ToolUse { .. } => {}
                    }
                }
            }
            Role::Assistant => {
                let text = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                let tool_calls = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolUse { id, name, input } => Some(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": input.to_string()
                            }
                        })),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if tool_calls.is_empty() {
                    out.push(json!({ "role": "assistant", "content": text }));
                } else {
                    out.push(json!({
                        "role": "assistant",
                        "content": if text.is_empty() { Value::Null } else { Value::String(text) },
                        "tool_calls": tool_calls
                    }));
                }
                for block in &message.content {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } = block
                    {
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content
                        }));
                    }
                }
            }
        }
    }
    out
}

#[derive(Deserialize)]
struct ChatChunk {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolCallDelta>,
}

#[derive(Deserialize)]
struct ChatToolCallDelta {
    index: u64,
    id: Option<String>,
    function: Option<ChatFunctionDelta>,
}

#[derive(Deserialize)]
struct ChatFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}
