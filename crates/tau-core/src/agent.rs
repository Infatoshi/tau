use crate::display::AgentDisplay;
use crate::errors;
use crate::session::{events_to_messages, SessionEvent, SessionStore};
use crate::tool::Tool;
use chrono::Utc;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tau_llm::{ContentBlock, Message, Provider, ProviderRequest, Role, StreamEvent};
use tokio_util::sync::CancellationToken;

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
        display: &mut (dyn AgentDisplay + Send),
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
        display: &mut (dyn AgentDisplay + Send),
        cancellation: CancellationToken,
    ) -> anyhow::Result<()> {
        let result = self
            .continue_until_done_inner(session, display, cancellation)
            .await;
        let cleanup = self.cleanup_tools().await;
        match (result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
        }
    }

    async fn continue_until_done_inner(
        &mut self,
        session: &mut SessionStore,
        display: &mut (dyn AgentDisplay + Send),
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
                    return Err(errors::cancelled());
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
                    StreamEvent::ToolCallStart { .. } => {}
                    StreamEvent::ToolCallDelta { .. } => {}
                    StreamEvent::ToolCallDone { call } => {
                        session
                            .append(&SessionEvent::ToolCall {
                                timestamp: Utc::now(),
                                call: call.clone(),
                            })
                            .await?;
                        display.tool_call(&call)?;
                        tool_calls.push(call);
                    }
                    StreamEvent::MessageStop {
                        stop_reason: reason,
                    } => stop_reason = reason,
                    StreamEvent::Error { message } => return Err(errors::provider_error(message)),
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
                display.assistant_done()?;
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

    async fn cleanup_tools(&self) -> anyhow::Result<()> {
        for tool in self.tools.values() {
            tool.cleanup().await?;
        }
        Ok(())
    }

    pub async fn compact_context(
        &mut self,
        session: &mut SessionStore,
        cancellation: CancellationToken,
    ) -> anyhow::Result<String> {
        if self.messages.is_empty() {
            return Ok("No prior conversation to compact.".to_string());
        }
        let request = ProviderRequest {
            model: self.model.clone(),
            system: "Summarize the conversation for a coding agent that will continue from the compacted context. Preserve user intent, current repo state, decisions made, files changed, commands run, unresolved blockers, and exact next steps. Be concise but complete.".to_string(),
            messages: self.messages.clone(),
            tools: Vec::new(),
            max_tokens: 2048,
        };
        let mut stream = self.provider.stream(request, cancellation.clone()).await?;
        let mut summary = String::new();
        while let Some(event) = stream.next().await {
            if cancellation.is_cancelled() {
                return Err(errors::cancelled());
            }
            match event? {
                StreamEvent::TextDelta { text } => summary.push_str(&text),
                StreamEvent::Error { message } => return Err(errors::provider_error(message)),
                _ => {}
            }
        }
        let summary = summary.trim().to_string();
        if summary.is_empty() {
            return Err(errors::empty_compaction_summary());
        }
        session
            .append(&SessionEvent::Compact {
                timestamp: Utc::now(),
                summary: summary.clone(),
            })
            .await?;
        self.messages = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: format!("Conversation summary so far:\n\n{summary}"),
            }],
        }];
        tracing::info!(session = %session.short_hash(), "compacted session context");
        Ok(summary)
    }
}
