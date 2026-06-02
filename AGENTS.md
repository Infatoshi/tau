# tau

## Project Shape

`tau` is a small Rust coding-agent harness. Keep the repo boring and easy to inspect.

- `crates/tau-llm`: provider trait, canonical messages, content blocks, stream events.
- `crates/tau-providers`: provider adapters and provider-specific protocol conversion.
- `crates/tau-core`: agent loop, display formatting, session JSONL storage, tool trait.
- `crates/tau-tools`: concrete local tools: `read`, `bash`, `edit`, `write`.
- `crates/tau-computer-use-layout`: shared screen coordinates, frames, and layout parsing.
- `crates/tau-desktop-capture`: desktop screenshot capture boundary.
- `crates/tau-computer-use`: macOS accessibility-backed `computer_use` tool, plus its tau marker visual helper.
- `crates/tau-tui`: ratatui frontend.
- `crates/tau-cli`: CLI entrypoint, config, provider/tool wiring, REPL/TUI glue.

Keep `crates/`. This is the conventional Cargo workspace layout for a multi-crate Rust project and avoids cluttering the repo root with six packages.

## Project Journey

This repo started as a minimal harness and was hardened through live terminal demos. The painful parts are now part of the design:

- Provider APIs drift, so provider quirks belong at provider boundaries, not in the agent loop.
- Terminal output gets messy quickly, so display code lives under `tau-core/src/display/` and is split by concern.
- Session resume must be exact. A resumed hash should hydrate one session only, not nearby or historical sessions.
- The model should know its runtime context: provider, model, harness, sandbox mode, cwd, date, tools, and injected project instructions.
- First-run setup should be boring: create `~/.tau/config.yaml`, read env keys from predictable places, and let `tau` run from anywhere after install.
- The repo should feel like Rust all the way down. Prefer Rust binaries over shell scripts when the behavior belongs to tau itself.
- Computer use is high-trust by design: observe a target, ground the action in a real element or locked app state, act, verify, and fail closed when the target is not exposed.

Do not undo these decisions casually. If a change cuts across them, leave a short note in this file explaining the new rule.

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

Project-specific harness instructions are read from the current working directory:

- `AGENTS.md` is preferred.
- `CLAUDE.md` is used only when `AGENTS.md` is absent.
- If both exist, only `AGENTS.md` is injected into the tau system prompt.

On first run tau creates `~/.tau/config.yaml`. YAML is the only supported config format:

```yaml
provider: zai
default_model: glm-4.6v
sandbox_mode: yolo
```

CLI flags override config values. `sandbox_mode = "yolo"` allows `bash`, `edit`, `write`, and `computer_use`; other values keep those risky tools blocked. `read` remains available.

API keys are read from exported shell environment variables, a project `.env`, or `~/.tau/.env`.

## Contribution Principles

Make tau easy to extend without making it clever.

- Keep each new capability close to its owner crate. Provider behavior goes in `tau-providers`; simple file/shell tool behavior goes in `tau-tools`; specialized OS/plugin-style tools get their own crate; display behavior goes in `tau-core/src/display`; CLI/config glue goes in `tau-cli`.
- Prefer boring structs, enums, and helper functions over framework-like abstractions.
- Add a file only when it creates a clear boundary. Good examples: `display/tables.rs`, provider adapters, crate-local `errors.rs`.
- Keep error construction out of hot-path business logic when it starts to sprawl. Use the crate-local `errors.rs` pattern.
- Preserve the canonical tau message/tool model in `tau-llm`. Adapt provider oddities into that model instead of leaking provider shapes through the rest of the codebase.
- Every new provider needs a default model, base URL, env var rule, and at least parser/formatting coverage if it changes protocol shape.
- Every new terminal format needs a small focused formatter and a regression test when practical.
- Avoid hidden magic. If tau injects context, reads a file, loads a config, or enables a tool, the behavior should be discoverable in code and explainable to the model.
- `computer_use` is macOS accessibility via `osascript`: UI trees plus focus-app/element-index click/type/paste/press-key/set-value, linear visual-only tau movement, and a persistent tau marker. `focus_app` shows the tau marker, locks computer_use to that exact frontmost window, and keeps it visible until the computer-use turn ends. Input actions require the locked window to remain frontmost; if the user changes focus, input is blocked and the focus lock is revoked for the turn instead of re-focusing. It is not screenshot understanding, OCR, raw pointer automation, or a private app API.
- Raw coordinate mouse clicks and scrolls are disabled. If `get_app_state` does not expose the target element, stop and report the limitation instead of guessing coordinates, scrolling around, or clicking browser chrome.
- If `focus_app` or an input action is blocked because the frontmost window changed, stop and report the blocker instead of retrying or re-focusing.
- For browser navigation, use `command+l`, `paste_text` the full URL, press Return, then verify the address bar URL from `get_app_state`; title text alone is not enough. If the requested URL redirects, report the final URL exactly.
- For Slack/Discord channel or DM navigation, prefer `focus_app` plus the app quick switcher (`command+k`) and verify the resulting window title/state instead of guessing raw coordinates from a sparse tree.
- Computer-use cursor visuals are tau marker helpers in `tau-computer-use`, not generated runtime assets.
- Keep README tiny. Durable contributor detail belongs here; larger specs belong in `SPEC.md`.

## Provider Notes

Named provider modes currently include `anthropic`, `openai-responses`, `openai-chat`, `zai`, `kimi`, `minimax`, `deepseek`, `openrouter`, `groq`, `cerebras`, `xai`, and `gemini`.

Most named non-Anthropic providers are OpenAI Chat Completions-compatible wrappers with provider-specific base URLs and API-key environment variables. Keep provider-specific quirks isolated in `tau-providers` or in the provider selection table in `tau-cli`.

Provider env vars:

```text
anthropic         ANTHROPIC_API_KEY
openai-responses  OPENAI_API_KEY
openai-chat       OPENAI_API_KEY or ZAI_API_KEY for z.ai URLs
zai               ZAI_API_KEY
kimi              MOONSHOT_API_KEY or KIMI_API_KEY
minimax           MINIMAX_API_KEY
deepseek          DEEPSEEK_API_KEY
openrouter        OPENROUTER_API_KEY
groq              GROQ_API_KEY
cerebras          CEREBRAS_API_KEY
xai               XAI_API_KEY
gemini            GEMINI_API_KEY or GOOGLE_API_KEY
```

OpenAI Responses has both an item `id` and a `call_id`; `tau` maps the canonical tool id to the Responses `call_id` so tool results can be piped back as `function_call_output.call_id`.

## Style

- Prefer small, local changes over broad refactors.
- Keep provider formatting/parsing isolated from the core agent loop.
- Keep terminal formatting in `tau-core/src/display/`, split by concern when useful.
- Do not commit generated logs, build artifacts, API keys, or local smoke-test files.
- Do not add retry loops, fallbacks, or broad compatibility shims until the exact failure mode is understood.
- Do not paste giant prompt text into core logic. Put stable harness policy in one place and keep dynamic runtime facts structured.
- Do not let a demo fix become architecture. Patch the root cause, then remove the temporary path.
- Do not make install/setup depend on untracked local shell state.
