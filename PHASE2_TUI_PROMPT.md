# tau Phase 2 — ratatui TUI

You are extending an existing Rust coding-agent harness called `tau` at `~/tau`. Phase 1 shipped a plain stdin/stdout REPL. Now build a `ratatui` + `crossterm` TUI as the `tau-tui` crate (stub already exists at `crates/tau-tui/` with a `Cargo.toml` listing `tau-core`, `tau-llm`, `ratatui`, `crossterm`, `tokio`, `tokio-util`, `unicode-width`, `futures`, `anyhow`).

## Strict scope

You may only modify files inside `crates/tau-tui/`. Do not touch any other crate. Do not modify the workspace `Cargo.toml`. Do not touch `tau-cli`.

## Reference

- Read `crates/tau-cli/src/main.rs` to understand the current REPL flow, signal handling, and how the agent loop is invoked.
- Read `crates/tau-core/src/agent.rs` to understand the agent loop's event surface — what gets streamed, what callbacks or channels are exposed.
- Skim `~/pi/packages/tui/src/` for layout ideas only (don't port code; that framework is bespoke and we explicitly want ratatui idioms instead).

## Goal

A full-screen TUI that the CLI can use as an alternative to the plain REPL. The CLI will have a flag (we'll wire it later in `tau-cli`) to choose between `--tui` (this) and the existing plain REPL (default).

## Layout

Three regions:

```
+---------------------------------------------+
| message scrollback                          |
| (assistant text, tool calls, tool results)  |
|                                             |
|                                             |
+---------------------------------------------+
| status line: model, cwd, session-hash       |
+---------------------------------------------+
| > input editor (multi-line, grows up to ~5) |
+---------------------------------------------+
```

- Scrollback grows from top, messages appended at bottom, auto-scroll to bottom on new content. PageUp/PageDown to scroll, End to jump to bottom.
- Tool calls and results render as visually distinct blocks (e.g., a left bar `│` and a label like `[tool: read]`). Use ratatui blocks/borders.
- Streaming assistant text updates in place as bytes arrive.
- Input editor: single-line by default, expands up to 5 lines as user types. Enter submits; Shift+Enter (or `\` line continuation if Shift+Enter isn't reliably detectable) inserts a newline.
- Status line shows: current model, cwd (truncated to last component if too long), session short-hash.

## Key bindings

- `Enter`: submit input (if not empty).
- `Shift+Enter` or `Alt+Enter`: insert literal newline in input.
- `Ctrl-C`:
    - If a model stream or tool is running: cancel it (signal cancellation via `tokio_util::sync::CancellationToken` provided by the caller).
    - If idle and input is empty: this is "first Ctrl-C" — show a status hint "Press Ctrl-C again to exit" and start a 1.5s timer.
    - If pressed again within 1.5s while idle: signal exit to the caller (don't kill the process directly; return from the run loop with an `ExitRequested` outcome so the CLI can save the session).
    - If pressed while input is non-empty: clear the input.
- `Ctrl-D` on empty input: same as double Ctrl-C exit signal.
- `Ctrl-L`: redraw / clear scrollback view (don't lose conversation, just clear the visual buffer).
- `Up`/`Down` arrows in input: navigate input history (sessions of past user messages from this run).
- `PageUp`/`PageDown`: scroll scrollback.

## Public API

The crate should expose roughly:

```rust
pub struct TuiApp { /* ... */ }

impl TuiApp {
    pub fn new(/* config: model name, cwd, session id, etc. */) -> Self;

    /// Run the TUI loop. Receives an mpsc::Receiver of events from the agent
    /// (assistant text deltas, tool call started, tool call completed, errors,
    /// turn complete, etc.) and returns user inputs via an mpsc::Sender.
    /// Returns when the user requests exit.
    pub async fn run(
        &mut self,
        agent_events: tokio::sync::mpsc::Receiver<AgentEvent>,
        user_input_tx: tokio::sync::mpsc::Sender<UserInput>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<RunOutcome>;
}

pub enum AgentEvent {
    AssistantTextDelta(String),
    AssistantTextEnd,
    ToolCallStart { name: String, input: serde_json::Value, id: String },
    ToolCallEnd { id: String, output: String, is_error: bool },
    Error(String),
    TurnComplete,
}

pub enum UserInput {
    Message(String),
    Cancel,
}

pub enum RunOutcome {
    ExitRequested,
}
```

You can adjust the exact types if needed, but keep the principle: the TUI is decoupled from the agent loop via channels. The CLI will glue them together later.

## Rendering details

- Use `ratatui` with the `crossterm` backend.
- Enable raw mode and alternate screen on enter; restore on exit (use a guard / drop impl so panics don't leave the terminal broken).
- Handle `Resize` events by redrawing.
- Use `unicode-width` to measure displayed widths for wrapping. Don't assume 1 char = 1 column.
- Word-wrap message content to terminal width.
- Streaming text should not flicker: render at most ~30 fps (use a 33ms tick or batch coalesce events between renders).

## Tests

Add at minimum:
- A test that constructs the app state, feeds it a series of `AgentEvent`s programmatically (without actually rendering to a real terminal), and asserts the in-memory message buffer contains the expected entries. Use `ratatui::backend::TestBackend` if you want to assert exact rendered cells.

## Integration note

Append to `crates/tau-tui/INTEGRATION.md` (create it) describing exactly how `tau-cli/src/main.rs` should:
1. Add a `--tui` flag.
2. Construct the `TuiApp`.
3. Set up the channels.
4. Spawn the agent loop on a tokio task that produces `AgentEvent`s into the channel and consumes `UserInput`s.
5. Handle the `RunOutcome::ExitRequested` by saving the session and printing the resume hint.

Provide ready-to-paste code blocks.

## Done criteria

- `cargo build --release` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test -p tau-tui` passes.
- `crates/tau-tui/INTEGRATION.md` exists with wiring guidance.
- No files outside `crates/tau-tui/` modified.

## Constraints

- No emojis. No em dashes in user-facing strings. Minimal comments.
- No `unwrap()` in production paths (tests are OK).
- The TUI must not panic on terminals smaller than 20x5; degrade gracefully.

Begin.
