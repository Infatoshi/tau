# tau Agent Notes

## Project Shape

`tau` is a small Rust coding-agent harness. Keep the repo boring and easy to inspect.

- `crates/tau-llm`: provider trait, canonical messages, content blocks, stream events.
- `crates/tau-providers`: provider adapters and provider-specific protocol conversion.
- `crates/tau-core`: agent loop, display formatting, session JSONL storage, tool trait.
- `crates/tau-tools`: concrete local tools: `read`, `bash`, `edit`, `write`.
- `crates/tau-tui`: ratatui frontend.
- `crates/tau-cli`: CLI entrypoint, config, provider/tool wiring, REPL/TUI glue.

Keep `crates/`. This is the conventional Cargo workspace layout for a multi-crate Rust project and avoids cluttering the repo root with six packages.

## Commands

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Run those before declaring non-trivial changes complete.

## Runtime

Sessions are append-only JSONL files under `~/.tau/sessions/`. `tau resume <hash>` should resolve a single session and hydrate only that session's messages. The terminal resume preview should stay short; the provider still receives the full hydrated context.

`~/.tau/config.toml` may set:

```toml
provider = "zai"
default_model = "glm-5.1"
sandbox_mode = "yolo"
```

CLI flags override config values. `sandbox_mode = "yolo"` allows `bash`, `edit`, and `write`; other values keep those risky tools blocked. `read` remains available.

## Provider Notes

Named provider modes currently include `anthropic`, `openai-responses`, `openai-chat`, `zai`, `kimi`, `minimax`, `deepseek`, `openrouter`, `groq`, `cerebras`, `xai`, and `gemini`.

Most named non-Anthropic providers are OpenAI Chat Completions-compatible wrappers with provider-specific base URLs and API-key environment variables. Keep provider-specific quirks isolated in `tau-providers` or in the provider selection table in `tau-cli`.

OpenAI Responses has both an item `id` and a `call_id`; `tau` maps the canonical tool id to the Responses `call_id` so tool results can be piped back as `function_call_output.call_id`.

## Style

- Prefer small, local changes over broad refactors.
- Keep provider formatting/parsing isolated from the core agent loop.
- Keep terminal formatting in `tau-core/src/display/`, split by concern when useful.
- Do not commit generated logs, build artifacts, API keys, or local smoke-test files.
