# tau v0.1 — Phase 1 Vertical Slice

You are building a new Rust coding-agent harness called `tau`. It is a fresh project, not a port. Build it at the current working directory (`~/tau`).

## Reference (read-only, do not copy)

A TypeScript reference implementation called Pi exists at `~/pi`. You may **read** it for understanding provider quirks, JSONL session shape, tool semantics, and Anthropic streaming details. Specifically these are useful:

- `~/pi/packages/agent/src/agent-loop.ts` — the loop shape
- `~/pi/packages/agent/src/harness/agent-harness.ts` — harness structure
- `~/pi/packages/ai/src/providers/anthropic.ts` — Anthropic streaming, partial JSON tool-call deltas, tool_use handling
- `~/pi/packages/ai/src/stream.ts` — canonical event types
- `~/pi/packages/agent/src/harness/session/storage/jsonl.ts` — JSONL session storage shape

Do **not** port the code. Read it, understand the shape, then write idiomatic Rust.

## Design thesis (this is load-bearing — do not violate)

- Minimal harness. The model is RL-trained as a coding agent. Don't pad the system prompt.
- User owns the context window. No hidden system reminders. No auto-pruning of tool output. No LSP injection. No "may or may not be relevant" reminders.
- YOLO by default. No permission dialogs. Tools execute.
- v0.1 ships **only** what's listed below. Resist scope creep. No hooks, no token counting, no compaction, no TUI yet, no config file.

## Workspace layout (Cargo workspace)

```
~/tau/
  Cargo.toml          # workspace root, [workspace] with members
  crates/
    tau-core/         # message types, agent loop, session JSONL
    tau-llm/          # provider trait, canonical events, message/content types
    tau-providers/    # anthropic only for v0.1
    tau-tools/        # read, bash, tool registry, Tool trait re-exported from tau-llm or defined here
    tau-cli/          # binary crate `tau`, args parsing, REPL, signal handling
  README.md           # one-page: what tau is, build/run instructions
  .gitignore
```

You decide the exact crate-internal module structure. Keep it boring.

## Dependency direction

```
tau-cli -> tau-core, tau-providers, tau-tools, tau-llm
tau-core -> tau-llm
tau-providers -> tau-llm
tau-tools -> tau-llm (for the Tool trait if it lives there) or define Tool in tau-tools and have tau-core depend on tau-tools
```

Pick whichever puts the `Tool` trait in the cleanest place. My preference: `Tool` trait in `tau-core` (since the agent loop calls it), tool *implementations* in `tau-tools`.

## Functional scope for v0.1

### CLI surface

```
tau                          # interactive REPL on stdin/stdout (no ratatui yet)
tau -p "prompt here"         # print mode: one-shot, prints assistant text + tool calls/results, exits
tau resume <hash>            # resume a session by short hash
tau list                     # list recent sessions (hash, timestamp, first user message preview)
```

Use `clap` derive for arg parsing.

### Interactive REPL behavior

- Print a prompt `> ` on its own line.
- User types a message, hits Enter, sees streaming assistant text and tool execution inline.
- After the assistant turn ends, print `> ` again.
- **Single Ctrl-C**: cancel the current in-flight model stream or running tool. Return to the `> ` prompt.
- **Double Ctrl-C within 1.5 seconds**: save session, print `Session saved. Resume with: tau resume <short_hash>`, exit cleanly.
- Ctrl-D on empty input: same as double Ctrl-C (save and exit).

### Session model

- Sessions stored at `~/.tau/sessions/<uuid>.jsonl`, one JSON object per line.
- Each line is one event: user message, assistant text chunk (or finalized message), tool call, tool result, model change, session metadata header.
- Session ID is a UUID v4. Short hash is the first 8 chars.
- On every event, append immediately and flush. Crash-safe.
- `tau resume <hash>` finds the session by short-hash prefix match (error if ambiguous), replays all messages into the in-memory conversation, then enters interactive mode.
- `tau list` reads `~/.tau/sessions/`, prints `<short_hash>  <iso_timestamp>  <first_user_message_preview_truncated_to_60_chars>`, sorted by mtime descending, top 20.

### Provider: Anthropic Messages API

- Streaming endpoint: `POST https://api.anthropic.com/v1/messages` with `stream: true`.
- API key from `ANTHROPIC_API_KEY` env var. Error clearly if missing.
- Default model: `claude-opus-4-5` (or `claude-sonnet-4-5` if you want lower-cost default — pick one and document it). Allow override via `--model` CLI flag.
- Implement SSE parsing yourself with `reqwest` + `eventsource-stream` (or hand-roll). Do **not** use the `anthropic-rs` crate — its tool-use support is incomplete.
- Handle these event types: `message_start`, `content_block_start`, `content_block_delta` (with `text_delta` and `input_json_delta` variants), `content_block_stop`, `message_delta`, `message_stop`, `error`.
- Tool calls arrive as `content_block_start` with `type: tool_use`, then `input_json_delta` chunks accumulating the JSON arguments string, then `content_block_stop`. Parse the accumulated JSON at stop time.
- Pass tools to the API in the Anthropic schema format. The agent loop sends `tool_use` blocks back as `tool_result` blocks in the next user-role message (Anthropic's convention).
- Set `max_tokens` to something sane (e.g., 8192). Set anthropic-version header.

### Tools (just two for phase 1)

**`read`**
- Args: `{ "path": string, "start_line"?: number, "end_line"?: number }`
- Reads the file at path. Optional line range (1-indexed, inclusive). Returns content as a string.
- Errors: file not found, permission denied, not a regular file, file too large (cap at 10MB by default).
- Path resolution: relative to current working directory at agent start.

**`bash`**
- Args: `{ "command": string, "timeout_ms"?: number }`
- Executes via `/bin/bash -lc <command>`.
- Default timeout 120 seconds. User-overridable up to 600 seconds.
- Captures stdout+stderr combined.
- **Process group**: spawn with `setsid()` (or `process_group(0)` on Unix) so Ctrl-C in the harness kills the whole process tree, not just bash.
- On Ctrl-C while a tool is running: send SIGTERM to the process group, wait 200ms, send SIGKILL. Return a tool result indicating cancellation.
- Cap output at 100KB. If exceeded, truncate and append `[output truncated, X bytes total]`.

### Agent loop

The loop, in `tau-core`:

1. User sends a message. Append to conversation.
2. Call provider's streaming method with the full conversation + tool schemas.
3. As events stream in, accumulate assistant text and tool calls. Display assistant text as it streams.
4. When the assistant turn ends with tool calls, execute each tool call (sequentially for v0.1), display results.
5. Append tool results to conversation as a user-role message with `tool_result` blocks.
6. Loop back to step 2 (call provider again with updated conversation) until the assistant turn ends with no tool calls (a "done" stop reason).
7. Return control to the user prompt.

Cancellation (Ctrl-C) must be cooperative: use `tokio::sync::CancellationToken` (from `tokio-util`) threaded through the provider stream and tool execution. On cancellation, drop the in-flight stream and abort tool execution.

### System prompt

Keep it minimal. Something like:

```
You are tau, a coding agent. You have access to tools. Use them to help the user with software engineering tasks. The current working directory is {cwd}. The current date is {date}.
```

That's it. No "be helpful, harmless, honest." No tool-use examples. No safety boilerplate.

### Async runtime

`tokio` with the `rt-multi-thread` feature. Use `tokio::main` in `tau-cli`.

### Suggested dependencies

```
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
reqwest = { version = "0.12", features = ["stream", "json"] }
eventsource-stream = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
thiserror = "2"
async-trait = "0.1"
futures = "0.3"
nix = { version = "0.29", features = ["signal", "process"] }  # for Unix process groups
ctrlc = "3"  # or use tokio::signal::ctrl_c()
dirs = "5"   # for ~/.tau path
```

You may add others as needed. Keep dependencies minimal.

## Success criteria

You are done when all of the following work end-to-end:

1. `cargo build --release` succeeds with no warnings (use `#![deny(warnings)]` is not required, but no warnings in your own code).
2. `cargo clippy --all-targets -- -D warnings` passes.
3. `cargo test` passes. Write tests for: bash output capture, bash timeout, bash cancellation kills process group, read with line range, JSONL session round-trip (write events, read them back), Anthropic SSE parser on a recorded fixture.
4. `ANTHROPIC_API_KEY=sk-... ./target/release/tau -p "Read the file ~/tau/Cargo.toml and tell me what crates are in the workspace"` prints the assistant's response with a tool call to `read` and the result.
5. `ANTHROPIC_API_KEY=sk-... ./target/release/tau` enters interactive mode. Single Ctrl-C cancels in-flight ops. Double Ctrl-C exits with a resume hash printed.
6. `./target/release/tau resume <hash>` loads a prior session and continues it.
7. `./target/release/tau list` prints recent sessions.

## Constraints

- No emojis in code, comments, or output.
- No em dashes in user-facing strings.
- No comments explaining what code does. Only comments where the *why* is non-obvious (a workaround, an invariant, a subtle protocol requirement).
- No `// TODO` markers in committed code unless genuinely necessary; if you skip something, document it in `README.md` under "Known limitations" instead.
- One short README.md at workspace root: what tau is, how to build, how to run, current limitations.

## Workflow

1. Read the reference TS files listed above.
2. Stub out the workspace and crates.
3. Build the Anthropic provider end-to-end first (with a hardcoded test prompt and no tools), get streaming text working, *then* add tool support.
4. Add the `read` tool, then `bash`.
5. Wire up the REPL and signal handling.
6. Add session JSONL.
7. Add `resume` and `list` subcommands.
8. Write tests as you go, not at the end.
9. Run `cargo build`, `cargo clippy`, `cargo test` repeatedly. Fix everything before declaring done.

When you finish, write a SUMMARY.md at `~/tau/SUMMARY.md` listing: what's implemented, what was harder than expected, anything you punted on, and the next-phase recommendations for adding more providers and a ratatui TUI.

Begin.
