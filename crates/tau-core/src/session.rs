use chrono::{DateTime, Utc};
use tau_llm::{ContentBlock, Message, Role, ToolCall};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    Session {
        version: u32,
        id: Uuid,
        timestamp: DateTime<Utc>,
        cwd: String,
        model: String,
    },
    UserMessage {
        timestamp: DateTime<Utc>,
        content: String,
    },
    AssistantTextDelta {
        timestamp: DateTime<Utc>,
        text: String,
    },
    AssistantMessage {
        timestamp: DateTime<Utc>,
        content: Vec<ContentBlock>,
        stop_reason: Option<String>,
    },
    ToolCall {
        timestamp: DateTime<Utc>,
        call: ToolCall,
    },
    ToolResult {
        timestamp: DateTime<Utc>,
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    ModelChange {
        timestamp: DateTime<Utc>,
        model: String,
    },
}

#[derive(Debug)]
pub struct SessionStore {
    id: Uuid,
    path: PathBuf,
    file: File,
}

impl SessionStore {
    pub async fn create(cwd: &Path, model: &str) -> anyhow::Result<Self> {
        let id = Uuid::new_v4();
        let path = sessions_dir()?.join(format!("{id}.jsonl"));
        Self::create_at(cwd, model, path).await
    }

    pub async fn create_at(cwd: &Path, model: &str, path: PathBuf) -> anyhow::Result<Self> {
        let id = Uuid::new_v4();
        fs::create_dir_all(path.parent().expect("session path has parent")).await?;
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .await?;
        let mut store = Self { id, path, file };
        store
            .append(&SessionEvent::Session {
                version: 1,
                id,
                timestamp: Utc::now(),
                cwd: cwd.display().to_string(),
                model: model.to_string(),
            })
            .await?;
        Ok(store)
    }

    pub async fn open_by_hash(hash: &str) -> anyhow::Result<(Self, Vec<SessionEvent>)> {
        let path = resolve_hash(hash).await?;
        let events = read_events(&path).await?;
        let id = events
            .iter()
            .find_map(|event| match event {
                SessionEvent::Session { id, .. } => Some(*id),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("session missing header"))?;
        let file = OpenOptions::new().append(true).open(&path).await?;
        Ok((Self { id, path, file }, events))
    }

    pub async fn append(&mut self, event: &SessionEvent) -> anyhow::Result<()> {
        let line = serde_json::to_string(event)?;
        self.file.write_all(line.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.file.flush().await?;
        Ok(())
    }

    pub fn short_hash(&self) -> String {
        self.id.to_string()[..8].to_string()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn events_to_messages(events: &[SessionEvent]) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut pending_results = Vec::new();
    for event in events {
        match event {
            SessionEvent::UserMessage { content, .. } => {
                if !pending_results.is_empty() {
                    messages.push(Message {
                        role: Role::User,
                        content: std::mem::take(&mut pending_results),
                    });
                }
                messages.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: content.clone(),
                    }],
                });
            }
            SessionEvent::AssistantMessage { content, .. } => {
                if !pending_results.is_empty() {
                    messages.push(Message {
                        role: Role::User,
                        content: std::mem::take(&mut pending_results),
                    });
                }
                messages.push(Message {
                    role: Role::Assistant,
                    content: content.clone(),
                });
            }
            SessionEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => {
                pending_results.push(ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                });
            }
            _ => {}
        }
    }
    if !pending_results.is_empty() {
        messages.push(Message {
            role: Role::User,
            content: pending_results,
        });
    }
    messages
}

pub async fn read_events(path: &Path) -> anyhow::Result<Vec<SessionEvent>> {
    let file = File::open(path).await?;
    let mut lines = BufReader::new(file).lines();
    let mut events = Vec::new();
    while let Some(line) = lines.next_line().await? {
        if !line.trim().is_empty() {
            events.push(serde_json::from_str(&line)?);
        }
    }
    Ok(events)
}

pub async fn list_recent(limit: usize) -> anyhow::Result<Vec<(String, DateTime<Utc>, String)>> {
    let dir = sessions_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = fs::read_dir(dir).await?;
    let mut rows = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let modified = entry.metadata().await?.modified()?;
        let events = read_events(&path).await.unwrap_or_default();
        let first_user = events
            .iter()
            .find_map(|event| match event {
                SessionEvent::UserMessage { content, .. } => Some(preview(content, 60)),
                _ => None,
            })
            .unwrap_or_default();
        let hash = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .chars()
            .take(8)
            .collect();
        rows.push((hash, DateTime::<Utc>::from(modified), first_user));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows.truncate(limit);
    Ok(rows)
}

fn preview(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push_str("...");
    }
    out.replace('\n', " ")
}

fn sessions_dir() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not find home directory"))?;
    Ok(home.join(".tau").join("sessions"))
}

async fn resolve_hash(hash: &str) -> anyhow::Result<PathBuf> {
    let dir = sessions_dir()?;
    let mut matches = Vec::new();
    let mut entries = fs::read_dir(&dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.starts_with(hash) {
            matches.push(path);
        }
    }
    match matches.len() {
        0 => Err(anyhow::anyhow!("no session matches {hash}")),
        1 => Ok(matches.remove(0)),
        _ => Err(anyhow::anyhow!("ambiguous session hash {hash}")),
    }
}
