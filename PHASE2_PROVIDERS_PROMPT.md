# tau Phase 2 — OpenAI Responses + Chat Completions providers

You are extending an existing Rust coding-agent harness called `tau` at `~/tau`. Phase 1 shipped Anthropic streaming. Now add two OpenAI-family providers.

## Strict scope

You may only modify files inside `crates/tau-providers/`. Do not touch any other crate. Do not modify the workspace `Cargo.toml`. Do not touch `tau-cli`.

## Reference

- Read `crates/tau-providers/src/anthropic.rs` carefully — match its style, error handling, and SSE parsing pattern.
- Read `crates/tau-llm/src/lib.rs` to understand the canonical `Provider` trait, message types, and event types you must produce.
- TS reference (read for protocol understanding only):
    - `~/pi/packages/ai/src/providers/openai-responses.ts`
    - `~/pi/packages/ai/src/providers/openai-completions.ts`
    - `~/pi/packages/ai/src/providers/transform-messages.ts`

## Provider 1: OpenAI Responses API

- Endpoint: `POST https://api.openai.com/v1/responses` with `stream: true`.
- API key from `OPENAI_API_KEY` env var.
- Default model: `gpt-5` (override via constructor argument).
- SSE event types to handle (typed semantic events):
    - `response.created`, `response.in_progress`
    - `response.output_item.added` (track function-call items here)
    - `response.output_text.delta` → emit text delta
    - `response.output_text.done`
    - `response.function_call_arguments.delta` → accumulate JSON args for the matching tool call
    - `response.function_call_arguments.done` → finalize tool call
    - `response.output_item.done`
    - `response.completed` → emit message-end with stop reason
    - `response.failed`, `response.error` → emit error
- Tool calls: OpenAI Responses uses `function_call` items. Each item has an `id`, `call_id`, and `name`. The `call_id` is what you echo back in the next `function_call_output` input item.
- Sending tool results back: in the next request, add an input item of `{ "type": "function_call_output", "call_id": <call_id>, "output": <string> }`.
- Convert canonical messages to OpenAI Responses input format. Assistant messages with tool_use blocks become `function_call` items; tool results become `function_call_output` items.
- Tools schema: pass as top-level `tools: [{ "type": "function", "name": ..., "description": ..., "parameters": ... }]`.

## Provider 2: OpenAI Chat Completions API

- Endpoint: `POST https://api.openai.com/v1/chat/completions` with `stream: true`.
- Same `OPENAI_API_KEY` env var, but allow a `base_url` constructor argument so this can target OpenRouter, Groq, xAI, or any OpenAI-compatible endpoint. Default `https://api.openai.com/v1`.
- Default model: `gpt-4o` (override via constructor).
- SSE format: `data: {json}\n\n` lines, terminated by `data: [DONE]`.
- Streaming chunks have `choices[0].delta` with optional `content` and optional `tool_calls` (an array indexed by `index`).
- Tool calls are streamed as deltas: `tool_calls[i]` may carry partial `function.arguments` strings. Accumulate by index.
- When `choices[0].finish_reason` is set (`stop`, `tool_calls`, `length`, etc.), the turn is done.
- Tool result format going back: assistant message with `tool_calls`, then a `role: tool` message per result with `tool_call_id` matching.
- Tools schema: `tools: [{ "type": "function", "function": { "name": ..., "description": ..., "parameters": ... } }]`.

## Trait conformance

Both providers must implement the same `Provider` trait the Anthropic one does. Their `stream` method returns the same canonical event stream type. The point of the abstraction is that `tau-core::agent` doesn't know which provider it's calling.

If you discover the existing trait surface is missing something needed for OpenAI semantics (for example, reasoning items, `previous_response_id`, or function-call grouping), document it in `crates/tau-providers/INTEGRATION.md` rather than changing `tau-llm`. We'll review and harmonize in a later phase.

## Module structure

- `crates/tau-providers/src/openai_responses.rs` — Responses API
- `crates/tau-providers/src/openai_chat.rs` — Chat Completions API
- Update `crates/tau-providers/src/lib.rs` to add `pub mod openai_responses; pub mod openai_chat;` and re-export the public types: `OpenAiResponsesProvider`, `OpenAiChatProvider`, plus their SSE parsers if helpful.

## Tests

Add tests under `crates/tau-providers/tests/`:
- `openai_responses.rs`: an SSE fixture string covering text deltas + a function-call argument delta sequence + completion. Parse it and assert the resulting canonical events.
- `openai_chat.rs`: an SSE fixture covering text content, indexed tool_call argument deltas, finish_reason=`tool_calls`. Parse and assert.

Match the testing pattern in `crates/tau-providers/tests/anthropic.rs`.

## Integration note

Append to `crates/tau-providers/INTEGRATION.md` (create it) describing exactly what `tau-cli` needs to do to support a `--provider` flag selecting between Anthropic / OpenAI Responses / OpenAI Chat. Include sample code snippets.

## Done criteria

- `cargo build --release` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test -p tau-providers` passes with all your new tests plus the existing Anthropic test.
- `crates/tau-providers/INTEGRATION.md` exists with wiring guidance.
- No files outside `crates/tau-providers/` modified.

## Constraints

- No emojis. No em dashes in user-facing strings. Minimal comments. No TODO markers.
- If a request fails (4xx/5xx), surface the error body in the returned error so the user can debug.
- Use `reqwest` with the existing workspace dependency. Do not add new HTTP crates.

Begin.
