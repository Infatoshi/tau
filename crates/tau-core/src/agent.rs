use crate::session::{events_to_messages, SessionEvent, SessionStore};
use crate::tool::Tool;
use chrono::Utc;
use futures::StreamExt;
use tau_llm::{ContentBlock, Message, Provider, ProviderRequest, Role, StreamEvent, ToolCall};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub trait AgentDisplay {
    fn assistant_delta(&mut self, text: &str) -> anyhow::Result<()>;
    fn tool_call(&mut self, call: &ToolCall) -> anyhow::Result<()>;
    fn tool_result(&mut self, call: &ToolCall, content: &str, is_error: bool)
        -> anyhow::Result<()>;
}

pub struct StdoutDisplay;

impl AgentDisplay for StdoutDisplay {
    fn assistant_delta(&mut self, text: &str) -> anyhow::Result<()> {
        print!("{text}");
        std::io::stdout().flush()?;
        Ok(())
    }

    fn tool_call(&mut self, call: &ToolCall) -> anyhow::Result<()> {
        println!("\n[tool call: {} {}]", call.name, call.input);
        Ok(())
    }

    fn tool_result(
        &mut self,
        call: &ToolCall,
        content: &str,
        is_error: bool,
    ) -> anyhow::Result<()> {
        let status = if is_error { "error" } else { "ok" };
        println!("[tool result: {} {status}]\n{content}", call.name);
        Ok(())
    }
}

pub struct Agent {
    provider: Arc<dyn Provider>,
    tools: HashMap<String, Arc<dyn Tool>>,
    pub messages: Vec<Message>,
    model: String,
    system: String,
}

impl Agent {
    pub fn new(
        provider: Arc<dyn Provider>,
        tools: Vec<Arc<dyn Tool>>,
        model: String,
        system: String,
    ) -> Self {
        Self {
            provider,
            tools: tools
                .into_iter()
                .map(|tool| (tool.schema().name.clone(), tool))
                .collect(),
            messages: Vec::new(),
            model,
            system,
        }
    }

    pub fn from_events(
        provider: Arc<dyn Provider>,
        tools: Vec<Arc<dyn Tool>>,
        model: String,
        system: String,
        events: &[SessionEvent],
    ) -> Self {
        let mut agent = Self::new(provider, tools, model, system);
        agent.messages = events_to_messages(events);
        agent
    }

    pub async fn run_user_turn(
        &mut self,
        user_text: String,
        session: &mut SessionStore,
        display: &mut dyn AgentDisplay,
        cancellation: CancellationToken,
    ) -> anyhow::Result<()> {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: user_text.clone(),
            }],
        });
        session
            .append(&SessionEvent::UserMessage {
                timestamp: Utc::now(),
                content: user_text,
            })
            .await?;
        self.continue_until_done(session, display, cancellation)
            .await
    }

    pub async fn continue_until_done(
        &mut self,
        session: &mut SessionStore,
        display: &mut dyn AgentDisplay,
        cancellation: CancellationToken,
    ) -> anyhow::Result<()> {
        loop {
            let tool_schemas = self.tools.values().map(|tool| tool.schema()).collect();
            let request = ProviderRequest {
                model: self.model.clone(),
                system: self.system.clone(),
                messages: self.messages.clone(),
                tools: tool_schemas,
                max_tokens: 8192,
            };
            let mut stream = self.provider.stream(request, cancellation.clone()).await?;
            let mut assistant_content = Vec::new();
            let mut text = String::new();
            let mut tool_calls = Vec::new();
            let mut stop_reason = None;

            while let Some(event) = stream.next().await {
                if cancellation.is_cancelled() {
                    return Err(anyhow::anyhow!("cancelled"));
                }
                match event? {
                    StreamEvent::MessageStart => {}
                    StreamEvent::TextDelta { text: delta } => {
                        display.assistant_delta(&delta)?;
                        session
                            .append(&SessionEvent::AssistantTextDelta {
                                timestamp: Utc::now(),
                                text: delta.clone(),
                            })
                            .await?;
                        text.push_str(&delta);
                    }
                    StreamEvent::ToolCallStart { id, name } => {
                        let call = ToolCall {
                            id,
                            name,
                            input: serde_json::Value::Null,
                        };
                        display.tool_call(&call)?;
                    }
                    StreamEvent::ToolCallDelta { .. } => {}
                    StreamEvent::ToolCallDone { call } => {
                        session
                            .append(&SessionEvent::ToolCall {
                                timestamp: Utc::now(),
                                call: call.clone(),
                            })
                            .await?;
                        tool_calls.push(call);
                    }
                    StreamEvent::MessageStop {
                        stop_reason: reason,
                    } => stop_reason = reason,
                    StreamEvent::Error { message } => return Err(anyhow::anyhow!(message)),
                }
            }

            if !text.is_empty() {
                assistant_content.push(ContentBlock::Text { text });
            }
            for call in &tool_calls {
                assistant_content.push(ContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                });
            }
            self.messages.push(Message {
                role: Role::Assistant,
                content: assistant_content.clone(),
            });
            session
                .append(&SessionEvent::AssistantMessage {
                    timestamp: Utc::now(),
                    content: assistant_content,
                    stop_reason,
                })
                .await?;

            if tool_calls.is_empty() {
                println!();
                return Ok(());
            }

            let mut result_blocks = Vec::new();
            for call in tool_calls {
                let Some(tool) = self.tools.get(&call.name) else {
                    let content = format!("unknown tool: {}", call.name);
                    display.tool_result(&call, &content, true)?;
                    result_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: call.id,
                        content,
                        is_error: true,
                    });
                    continue;
                };
                let result = tool
                    .execute(call.input.clone(), cancellation.clone())
                    .await?;
                display.tool_result(&call, &result.content, result.is_error)?;
                session
                    .append(&SessionEvent::ToolResult {
                        timestamp: Utc::now(),
                        tool_use_id: call.id.clone(),
                        content: result.content.clone(),
                        is_error: result.is_error,
                    })
                    .await?;
                result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: call.id,
                    content: result.content,
                    is_error: result.is_error,
                });
            }
            self.messages.push(Message {
                role: Role::User,
                content: result_blocks,
            });
        }
    }
}
