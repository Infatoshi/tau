# tau v0.1 Summary

## Implemented

- Cargo workspace with `tau-core`, `tau-llm`, `tau-providers`, `tau-tools`, and `tau-cli`.
- Anthropic Messages API streaming provider with hand-rolled SSE parsing for text deltas, tool-use starts, partial JSON argument deltas, tool-use completion, stop reasons, and API errors.
- Minimal agent loop that streams assistant text, accumulates tool calls, executes tools sequentially, sends Anthropic `tool_result` blocks back as a user message, and repeats until no tool calls remain.
- `read` tool with current-working-directory-relative path resolution, `~/` expansion, regular-file checks, 10 MB cap, and 1-indexed inclusive line ranges.
- `bash` tool using `/bin/bash -lc`, process-group spawning, timeout handling, cooperative cancellation, SIGTERM then SIGKILL cleanup, combined stdout/stderr capture, and 100 KB output truncation.
- Append-flushed JSONL sessions under `~/.tau/sessions/<uuid>.jsonl`, short-hash resume, in-memory replay, and recent-session listing.
- CLI modes: interactive REPL, `-p` one-shot prompt, `resume <hash>`, and `list`.
- Tests for read line ranges, bash output, bash timeout, bash cancellation path, JSONL round-trip, and Anthropic SSE tool-call parsing.

## Harder Than Expected

- The bash cancellation path is the subtle part: Ctrl-C must cancel the token and still let the in-flight future observe cancellation so the process group gets terminated.
- Session replay has to preserve Anthropic's assistant/tool-result alternation. Tool results are reconstructed as user-role `tool_result` blocks before the next assistant or user message.

## Punted

- No live Anthropic smoke test was run in this sandbox because there is no API key and network access is restricted.

## Next Phase Recommendations

- Add a provider registry and keep provider-specific message conversion inside provider crates.
- Add a ratatui TUI as a separate `tau-tui` crate or a feature-gated module in `tau-cli`, without changing the core loop.
- Add richer session event views before adding compaction or token accounting.
- Add integration tests with a local mock Anthropic SSE server so provider behavior stays testable without live credentials.
