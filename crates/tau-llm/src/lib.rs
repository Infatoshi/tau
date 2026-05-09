use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart,
    TextDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, json_delta: String },
    ToolCallDone { call: ToolCall },
    MessageStop { stop_reason: Option<String> },
    Error { message: String },
}

pub type ProviderStream = BoxStream<'static, anyhow::Result<StreamEvent>>;

#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream(
        &self,
        request: ProviderRequest,
        cancellation: CancellationToken,
    ) -> anyhow::Result<ProviderStream>;
}
