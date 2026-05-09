use crate::errors;
use async_trait::async_trait;
use futures::TryStreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tau_llm::{
    ContentBlock, Provider, ProviderRequest, ProviderStream, Role, StreamEvent, ToolCall,
};
use tokio_util::sync::CancellationToken;

pub struct OpenAiResponsesProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiResponsesProvider {
    pub fn from_env(model: Option<String>) -> anyhow::Result<Self> {
        let api_key =
            std::env::var("OPENAI_API_KEY").map_err(|_| errors::missing_env("OPENAI_API_KEY"))?;
        Ok(Self::new(api_key, model))
    }

    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gpt-5".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Provider for OpenAiResponsesProvider {
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
        let mut body = json!({
            "model": model,
            "instructions": request.system,
            "input": messages_to_responses_input(&request.messages),
            "stream": true,
            "max_output_tokens": request.max_tokens
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema
                        })
                    })
                    .collect(),
            );
        }
        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(errors::request_failed("OpenAI Responses", status, text));
        }
        let byte_stream = response.bytes_stream();
        let stream = async_stream::try_stream! {
            let mut buffer = String::new();
            let mut calls = ResponsesState::default();
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

pub fn parse_openai_responses_sse(input: &str) -> anyhow::Result<Vec<StreamEvent>> {
    let mut calls = ResponsesState::default();
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
struct ResponsesState {
    calls: HashMap<String, PendingCall>,
    item_to_call: HashMap<String, String>,
}

struct PendingCall {
    id: String,
    name: String,
    json: String,
    done: bool,
}

fn parse_one_event(
    raw: &str,
    calls: &mut ResponsesState,
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
    let event: ResponsesEvent = serde_json::from_str(&data)?;
    let mut out = Vec::new();
    match event {
        ResponsesEvent::ResponseCreated | ResponsesEvent::ResponseInProgress => {
            if !*started {
                *started = true;
                out.push(StreamEvent::MessageStart);
            }
        }
        ResponsesEvent::ResponseOutputItemAdded { item } => {
            if item.kind == "function_call" {
                let call_id = item.call_id.unwrap_or_else(|| item.id.clone());
                let name = item.name.unwrap_or_default();
                calls.item_to_call.insert(item.id, call_id.clone());
                calls.calls.insert(
                    call_id.clone(),
                    PendingCall {
                        id: call_id.clone(),
                        name: name.clone(),
                        json: String::new(),
                        done: false,
                    },
                );
                out.push(StreamEvent::ToolCallStart { id: call_id, name });
            }
        }
        ResponsesEvent::ResponseOutputTextDelta { delta } => {
            out.push(StreamEvent::TextDelta { text: delta });
        }
        ResponsesEvent::ResponseFunctionCallArgumentsDelta {
            item_id,
            call_id,
            delta,
        } => {
            let key =
                call_id.or_else(|| item_id.and_then(|id| calls.item_to_call.get(&id).cloned()));
            if let Some(key) = key {
                if let Some(call) = calls.calls.get_mut(&key) {
                    call.json.push_str(&delta);
                    out.push(StreamEvent::ToolCallDelta {
                        id: call.id.clone(),
                        json_delta: delta,
                    });
                }
            }
        }
        ResponsesEvent::ResponseFunctionCallArgumentsDone { item_id, call_id } => {
            let key =
                call_id.or_else(|| item_id.and_then(|id| calls.item_to_call.get(&id).cloned()));
            if let Some(key) = key {
                if let Some(call) = calls.calls.get_mut(&key) {
                    call.done = true;
                }
            }
        }
        ResponsesEvent::ResponseOutputItemDone { item } => {
            let call_id = item
                .call_id
                .or_else(|| calls.item_to_call.get(&item.id).cloned());
            if item.kind == "function_call" {
                if let Some(call_id) = call_id {
                    if let Some(call) = calls.calls.remove(&call_id) {
                        out.push(done_event(call)?);
                    }
                }
            }
        }
        ResponsesEvent::ResponseCompleted { response } => {
            for call in calls.calls.drain().map(|(_, call)| call) {
                out.push(done_event(call)?);
            }
            out.push(StreamEvent::MessageStop {
                stop_reason: response.status,
            });
        }
        ResponsesEvent::ResponseFailed { response } => {
            out.push(StreamEvent::Error {
                message: response
                    .error
                    .and_then(|error| error.message)
                    .unwrap_or_else(|| "OpenAI Responses request failed".to_string()),
            });
        }
        ResponsesEvent::ResponseError { message, error } => {
            out.push(StreamEvent::Error {
                message: message
                    .or(error)
                    .unwrap_or_else(|| "OpenAI Responses error".to_string()),
            });
        }
        ResponsesEvent::ResponseOutputTextDone | ResponsesEvent::Unknown => {}
    }
    Ok(out)
}

fn done_event(call: PendingCall) -> anyhow::Result<StreamEvent> {
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

pub fn messages_to_responses_input(messages: &[tau_llm::Message]) -> Vec<Value> {
    messages
        .iter()
        .flat_map(|message| match message.role {
            Role::User => message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => json!({
                        "type": "function_call_output",
                        "call_id": tool_use_id,
                        "output": content
                    }),
                    ContentBlock::Text { text } => json!({
                        "role": "user",
                        "content": text
                    }),
                    ContentBlock::ToolUse { id, name, input } => json!({
                        "type": "function_call",
                        "id": id,
                        "call_id": id,
                        "name": name,
                        "arguments": input.to_string()
                    }),
                })
                .collect::<Vec<_>>(),
            Role::Assistant => message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => json!({
                        "role": "assistant",
                        "content": text
                    }),
                    ContentBlock::ToolUse { id, name, input } => json!({
                        "type": "function_call",
                        "id": id,
                        "call_id": id,
                        "name": name,
                        "arguments": input.to_string()
                    }),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => json!({
                        "type": "function_call_output",
                        "call_id": tool_use_id,
                        "output": content
                    }),
                })
                .collect::<Vec<_>>(),
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ResponsesEvent {
    #[serde(rename = "response.created")]
    ResponseCreated,
    #[serde(rename = "response.in_progress")]
    ResponseInProgress,
    #[serde(rename = "response.output_item.added")]
    ResponseOutputItemAdded { item: ResponsesItem },
    #[serde(rename = "response.output_text.delta")]
    ResponseOutputTextDelta { delta: String },
    #[serde(rename = "response.output_text.done")]
    ResponseOutputTextDone,
    #[serde(rename = "response.function_call_arguments.delta")]
    ResponseFunctionCallArgumentsDelta {
        item_id: Option<String>,
        call_id: Option<String>,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    ResponseFunctionCallArgumentsDone {
        item_id: Option<String>,
        call_id: Option<String>,
    },
    #[serde(rename = "response.output_item.done")]
    ResponseOutputItemDone { item: ResponsesItem },
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: ResponsesResponse },
    #[serde(rename = "response.failed")]
    ResponseFailed { response: ResponsesResponse },
    #[serde(rename = "response.error")]
    ResponseError {
        message: Option<String>,
        error: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct ResponsesItem {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    call_id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesResponse {
    status: Option<String>,
    error: Option<ResponsesError>,
}

#[derive(Deserialize)]
struct ResponsesError {
    message: Option<String>,
}
