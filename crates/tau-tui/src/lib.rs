use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAX_INPUT_LINES: u16 = 5;
const EXIT_WINDOW: Duration = Duration::from_millis(1500);
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

pub struct TuiApp {
    model: String,
    cwd: PathBuf,
    session_hash: String,
    messages: Vec<MessageEntry>,
    hidden_before: usize,
    input: InputEditor,
    history: Vec<String>,
    history_index: Option<usize>,
    scroll: u16,
    stick_to_bottom: bool,
    running: bool,
    status_hint: Option<String>,
    last_ctrl_c: Option<Instant>,
    tool_names: HashMap<String, String>,
}

pub struct TuiConfig {
    pub model: String,
    pub cwd: PathBuf,
    pub session_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    AssistantTextDelta(String),
    AssistantTextEnd,
    ToolCallStart {
        name: String,
        input: serde_json::Value,
        id: String,
    },
    ToolCallEnd {
        id: String,
        output: String,
        is_error: bool,
    },
    Error(String),
    TurnComplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserInput {
    Message(String),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    ExitRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageKind {
    User,
    Assistant,
    ToolCall,
    ToolResult { is_error: bool },
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageEntry {
    kind: MessageKind,
    label: String,
    content: String,
}

#[derive(Default)]
struct InputEditor {
    text: String,
    cursor: usize,
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TuiApp {
    pub fn new(config: TuiConfig) -> Self {
        Self {
            model: config.model,
            cwd: config.cwd,
            session_hash: config.session_hash,
            messages: Vec::new(),
            hidden_before: 0,
            input: InputEditor::default(),
            history: Vec::new(),
            history_index: None,
            scroll: 0,
            stick_to_bottom: true,
            running: false,
            status_hint: None,
            last_ctrl_c: None,
            tool_names: HashMap::new(),
        }
    }

    pub async fn run(
        &mut self,
        mut agent_events: mpsc::Receiver<AgentEvent>,
        user_input_tx: mpsc::Sender<UserInput>,
        cancellation: CancellationToken,
    ) -> anyhow::Result<RunOutcome> {
        let mut guard = TerminalGuard::enter()?;
        let mut ticker = interval(FRAME_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        self.draw(guard.terminal_mut())?;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.clear_expired_status();
                    while event::poll(Duration::ZERO).context("poll terminal event")? {
                        let event = event::read().context("read terminal event")?;
                        if self.handle_terminal_event(event, &user_input_tx, &cancellation).await? {
                            return Ok(RunOutcome::ExitRequested);
                        }
                    }
                    self.draw(guard.terminal_mut())?;
                }
                event = agent_events.recv() => {
                    if let Some(event) = event {
                        self.apply_agent_event(event);
                        self.draw(guard.terminal_mut())?;
                    }
                }
            }
        }
    }

    pub fn apply_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::AssistantTextDelta(delta) => {
                self.running = true;
                match self.messages.last_mut() {
                    Some(last) if last.kind == MessageKind::Assistant => {
                        last.content.push_str(&delta)
                    }
                    _ => self.messages.push(MessageEntry {
                        kind: MessageKind::Assistant,
                        label: "assistant".to_string(),
                        content: delta,
                    }),
                }
                self.auto_scroll();
            }
            AgentEvent::AssistantTextEnd => {
                self.running = false;
                self.status_hint = Some("Assistant response complete".to_string());
            }
            AgentEvent::ToolCallStart { name, input, id } => {
                self.running = true;
                self.tool_names.insert(id.clone(), name.clone());
                self.messages.push(MessageEntry {
                    kind: MessageKind::ToolCall,
                    label: format!("tool: {name}"),
                    content: pretty_json(&input),
                });
                self.auto_scroll();
            }
            AgentEvent::ToolCallEnd {
                id,
                output,
                is_error,
            } => {
                self.running = false;
                let name = self
                    .tool_names
                    .remove(&id)
                    .unwrap_or_else(|| id.chars().take(8).collect());
                let status = if is_error { "error" } else { "ok" };
                self.messages.push(MessageEntry {
                    kind: MessageKind::ToolResult { is_error },
                    label: format!("tool result: {name} {status}"),
                    content: output,
                });
                self.auto_scroll();
            }
            AgentEvent::Error(message) => {
                self.running = false;
                self.messages.push(MessageEntry {
                    kind: MessageKind::Error,
                    label: "error".to_string(),
                    content: message,
                });
                self.auto_scroll();
            }
            AgentEvent::TurnComplete => {
                self.running = false;
                self.status_hint = Some("Turn complete".to_string());
            }
        }
    }

    fn visible_messages(&self) -> &[MessageEntry] {
        &self.messages[self.hidden_before.min(self.messages.len())..]
    }

    async fn handle_terminal_event(
        &mut self,
        event: Event,
        user_input_tx: &mpsc::Sender<UserInput>,
        cancellation: &CancellationToken,
    ) -> anyhow::Result<bool> {
        let Event::Key(key) = event else {
            return Ok(false);
        };
        if key.kind == KeyEventKind::Release {
            return Ok(false);
        }

        match key {
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => {
                return self.handle_ctrl_c(user_input_tx, cancellation).await;
            }
            KeyEvent {
                code: KeyCode::Char('d'),
                modifiers,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && self.input.is_empty() => {
                return Ok(true);
            }
            KeyEvent {
                code: KeyCode::Char('l'),
                modifiers,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => {
                self.hidden_before = self.messages.len();
                self.scroll = 0;
                self.stick_to_bottom = true;
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers,
                ..
            } if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                self.input.insert('\n');
            }
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => self.submit_input(user_input_tx).await?,
            KeyEvent {
                code: KeyCode::Char('\\'),
                ..
            } if self.input.cursor == self.input.text.len() => {
                self.input.insert('\n');
            }
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => self.input.backspace(),
            KeyEvent {
                code: KeyCode::Delete,
                ..
            } => self.input.delete(),
            KeyEvent {
                code: KeyCode::Left,
                ..
            } => self.input.move_left(),
            KeyEvent {
                code: KeyCode::Right,
                ..
            } => self.input.move_right(),
            KeyEvent {
                code: KeyCode::Home,
                ..
            } => self.input.cursor = 0,
            KeyEvent {
                code: KeyCode::End,
                modifiers,
                ..
            } if modifiers.is_empty() => {
                self.scroll = 0;
                self.stick_to_bottom = true;
                self.input.cursor = self.input.text.len();
            }
            KeyEvent {
                code: KeyCode::PageUp,
                ..
            } => {
                self.scroll = self.scroll.saturating_add(8);
                self.stick_to_bottom = false;
            }
            KeyEvent {
                code: KeyCode::PageDown,
                ..
            } => {
                self.scroll = self.scroll.saturating_sub(8);
                self.stick_to_bottom = self.scroll == 0;
            }
            KeyEvent {
                code: KeyCode::Up, ..
            } => self.history_prev(),
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => self.history_next(),
            KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            } if !modifiers.contains(KeyModifiers::CONTROL) => self.input.insert(ch),
            _ => {}
        }
        Ok(false)
    }

    async fn handle_ctrl_c(
        &mut self,
        user_input_tx: &mpsc::Sender<UserInput>,
        cancellation: &CancellationToken,
    ) -> anyhow::Result<bool> {
        if self.running {
            cancellation.cancel();
            let _ = user_input_tx.send(UserInput::Cancel).await;
            self.running = false;
            self.status_hint = Some("Cancelled".to_string());
            return Ok(false);
        }
        if !self.input.is_empty() {
            self.input.clear();
            self.history_index = None;
            return Ok(false);
        }

        let now = Instant::now();
        let exit = self
            .last_ctrl_c
            .is_some_and(|last| now.duration_since(last) <= EXIT_WINDOW);
        self.last_ctrl_c = Some(now);
        if exit {
            Ok(true)
        } else {
            self.status_hint = Some("Press Ctrl-C again to exit".to_string());
            Ok(false)
        }
    }

    async fn submit_input(
        &mut self,
        user_input_tx: &mpsc::Sender<UserInput>,
    ) -> anyhow::Result<()> {
        let text = self.input.text.trim_end().to_string();
        if text.trim().is_empty() {
            return Ok(());
        }
        self.messages.push(MessageEntry {
            kind: MessageKind::User,
            label: "you".to_string(),
            content: text.clone(),
        });
        self.history.push(text.clone());
        self.history_index = None;
        self.input.clear();
        self.running = true;
        self.auto_scroll();
        user_input_tx
            .send(UserInput::Message(text))
            .await
            .context("send user input")?;
        Ok(())
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = self
            .history_index
            .map_or(self.history.len().saturating_sub(1), |index| {
                index.saturating_sub(1)
            });
        self.history_index = Some(next);
        self.input.set(self.history[next].clone());
    }

    fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 >= self.history.len() {
            self.history_index = None;
            self.input.clear();
        } else {
            let next = index + 1;
            self.history_index = Some(next);
            self.input.set(self.history[next].clone());
        }
    }

    fn clear_expired_status(&mut self) {
        if self
            .last_ctrl_c
            .is_some_and(|last| last.elapsed() > EXIT_WINDOW)
        {
            self.last_ctrl_c = None;
            if self.status_hint.as_deref() == Some("Press Ctrl-C again to exit") {
                self.status_hint = None;
            }
        }
    }

    fn auto_scroll(&mut self) {
        if self.stick_to_bottom {
            self.scroll = 0;
        }
    }

    fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> anyhow::Result<()> {
        terminal.draw(|frame| self.render(frame))?;
        Ok(())
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.width < 20 || area.height < 5 {
            let paragraph =
                Paragraph::new("Terminal too small").style(Style::default().fg(Color::Yellow));
            frame.render_widget(paragraph, area);
            return;
        }

        let input_height = self.input_height(area.width);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(input_height),
            ])
            .split(area);
        self.render_scrollback(frame, chunks[0]);
        self.render_status(frame, chunks[1]);
        self.render_input(frame, chunks[2]);
    }

    fn render_scrollback(&self, frame: &mut Frame<'_>, area: Rect) {
        let width = area.width.saturating_sub(2).max(1);
        let lines = self.scrollback_lines(width);
        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::BOTTOM))
            .scroll((self.scroll, 0));
        frame.render_widget(paragraph, area);
    }

    fn render_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let cwd = truncate_middle(&cwd_label(&self.cwd), area.width.saturating_div(3).max(8));
        let hash = self.session_hash.chars().take(8).collect::<String>();
        let base = format!("model: {}  cwd: {}  session: {}", self.model, cwd, hash);
        let text = match &self.status_hint {
            Some(hint) => format!("{base}  {hint}"),
            None => base,
        };
        let line = Line::from(Span::styled(
            truncate_to_width(&text, area.width),
            Style::default().fg(Color::Black).bg(Color::Gray),
        ));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_input(&self, frame: &mut Frame<'_>, area: Rect) {
        let text = if self.input.text.is_empty() {
            Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Green)),
                Span::styled("", Style::default()),
            ])
        } else {
            Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Green)),
                Span::raw(self.input.text.clone()),
            ])
        };
        let paragraph = Paragraph::new(text)
            .block(Block::default().borders(Borders::TOP))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
        let (x, y) = self.cursor_position(area);
        frame.set_cursor_position((x, y));
    }

    fn scrollback_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for message in self.visible_messages() {
            let style = match message.kind {
                MessageKind::User => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                MessageKind::Assistant => Style::default().fg(Color::White),
                MessageKind::ToolCall => Style::default().fg(Color::Yellow),
                MessageKind::ToolResult { is_error: true } | MessageKind::Error => {
                    Style::default().fg(Color::Red)
                }
                MessageKind::ToolResult { is_error: false } => Style::default().fg(Color::Green),
            };
            lines.push(Line::from(Span::styled(
                format!("[{}]", message.label),
                style,
            )));
            let prefix = match message.kind {
                MessageKind::ToolCall | MessageKind::ToolResult { .. } => "│ ",
                _ => "  ",
            };
            for wrapped in wrap_text(&message.content, width.saturating_sub(2).max(1)) {
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::raw(wrapped),
                ]));
            }
            lines.push(Line::raw(""));
        }
        if lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
    }

    fn input_height(&self, width: u16) -> u16 {
        let content_width = width.saturating_sub(4).max(1);
        let wrapped_lines = wrap_text(&self.input.text, content_width).len().max(1) as u16;
        wrapped_lines.min(MAX_INPUT_LINES).saturating_add(1)
    }

    fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let content_width = area.width.saturating_sub(4).max(1);
        let before_cursor = &self.input.text[..self.input.cursor];
        let mut row = 0u16;
        let mut col = 0u16;
        for ch in before_cursor.chars() {
            if ch == '\n' {
                row = row.saturating_add(1);
                col = 0;
                continue;
            }
            let width = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
            if col.saturating_add(width) > content_width {
                row = row.saturating_add(1);
                col = 0;
            }
            col = col.saturating_add(width);
        }
        let max_row = area.height.saturating_sub(2);
        (
            area.x
                .saturating_add(2)
                .saturating_add(col.min(content_width)),
            area.y.saturating_add(1).saturating_add(row.min(max_row)),
        )
    }
}

impl InputEditor {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    fn set(&mut self, text: String) {
        self.cursor = text.len();
        self.text = text;
    }

    fn insert(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((idx, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.text.drain(idx..self.cursor);
            self.cursor = idx;
        }
    }

    fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map_or(self.text.len(), |(offset, _)| self.cursor + offset);
        self.text.drain(self.cursor..end);
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((idx, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = idx;
        }
    }

    fn move_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        self.cursor = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map_or(self.text.len(), |(offset, _)| self.cursor + offset);
    }
}

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        enable_raw_mode().context("enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).context("create terminal")?;
        Ok(Self { terminal })
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn cwd_label(cwd: &std::path::Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| cwd.display().to_string(), ToString::to_string)
}

fn truncate_middle(text: &str, width: u16) -> String {
    if UnicodeWidthStr::width(text) <= usize::from(width) {
        return text.to_string();
    }
    if width <= 3 {
        return truncate_to_width(text, width);
    }
    let keep = usize::from(width.saturating_sub(3));
    let front = keep / 2;
    let back = keep.saturating_sub(front);
    let prefix: String = text.chars().take(front).collect();
    let suffix: String = text
        .chars()
        .rev()
        .take(back)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

fn truncate_to_width(text: &str, width: u16) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    let max = usize::from(width);
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > max {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out
}

fn wrap_text(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        let mut current = String::new();
        let mut current_width = 0usize;
        for word in split_words(raw_line) {
            let word_width = UnicodeWidthStr::width(word.as_str());
            if current_width > 0 && current_width + word_width > width {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }
            if word_width > width {
                for ch in word.chars() {
                    let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
                    if current_width > 0 && current_width + ch_width > width {
                        lines.push(current);
                        current = String::new();
                        current_width = 0;
                    }
                    current.push(ch);
                    current_width += ch_width;
                }
            } else {
                current.push_str(&word);
                current_width += word_width;
            }
        }
        lines.push(current);
    }
    lines
}

fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in line.chars() {
        current.push(ch);
        if ch.is_whitespace() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn app() -> TuiApp {
        TuiApp::new(TuiConfig {
            model: "test-model".to_string(),
            cwd: PathBuf::from("/tmp/tau"),
            session_hash: "abcdef123456".to_string(),
        })
    }

    #[test]
    fn agent_events_update_message_buffer() {
        let mut app = app();
        app.apply_agent_event(AgentEvent::AssistantTextDelta("hello".to_string()));
        app.apply_agent_event(AgentEvent::AssistantTextDelta(" world".to_string()));
        app.apply_agent_event(AgentEvent::ToolCallStart {
            name: "read".to_string(),
            input: serde_json::json!({"path":"Cargo.toml"}),
            id: "tool-1".to_string(),
        });
        app.apply_agent_event(AgentEvent::ToolCallEnd {
            id: "tool-1".to_string(),
            output: "contents".to_string(),
            is_error: false,
        });
        app.apply_agent_event(AgentEvent::TurnComplete);

        let messages = app.visible_messages();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].kind, MessageKind::Assistant);
        assert_eq!(messages[0].content, "hello world");
        assert_eq!(messages[1].label, "tool: read");
        assert!(messages[1].content.contains("Cargo.toml"));
        assert_eq!(messages[2].label, "tool result: read ok");
        assert_eq!(messages[2].content, "contents");
        assert!(!app.running);
    }

    #[test]
    fn renders_on_small_test_backend_without_panicking() {
        let mut app = app();
        let backend = TestBackend::new(19, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        app.draw(&mut terminal).unwrap();
    }
}
