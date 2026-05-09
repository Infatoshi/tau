# tau

tau is a minimal Rust coding-agent harness. It streams provider output, executes tools without permission prompts, and stores every session event as append-only JSONL.

## Build

```sh
cargo build --release
```

## Run

```sh
ANTHROPIC_API_KEY=sk-... ./target/release/tau
ANTHROPIC_API_KEY=sk-... ./target/release/tau -p "read Cargo.toml"
ANTHROPIC_API_KEY=sk-... ./target/release/tau --tui
OPENAI_API_KEY=sk-... ./target/release/tau --provider openai-responses -p "list files"
OPENAI_API_KEY=sk-or-... ./target/release/tau --provider openai-chat --base-url https://openrouter.ai/api/v1 --model anthropic/claude-sonnet-4 -p "hello"
ZAI_API_KEY=sk-... ./target/release/tau --provider zai -p "read Cargo.toml"
OPENROUTER_API_KEY=sk-or-... ./target/release/tau --provider openrouter --model anthropic/claude-sonnet-4 -p "hello"
GEMINI_API_KEY=... ./target/release/tau --provider gemini -p "hello"
./target/release/tau --list-models
./target/release/tau list
./target/release/tau resume <short_hash>
```

Use `--model` to override the provider default. Supported providers are `anthropic`, `openai-responses`, `openai-chat`, `zai`, `kimi`, `minimax`, `deepseek`, `openrouter`, `groq`, `cerebras`, `xai`, and `gemini`. The generic `openai-chat` provider accepts `--base-url` for any other OpenAI-compatible API.

## Provider Environment Variables

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

## Interactive Commands

```text
/compact
/help
```

Set `RUST_LOG` for debug output, for example `RUST_LOG=tau_core=info ./target/release/tau`.

## Config

tau reads `~/.tau/config.toml` when present:

```toml
provider = "zai"
default_model = "glm-5.1"
sandbox_mode = "yolo"
```

CLI flags override config values. `provider` uses the same names as `--provider`. `sandbox_mode = "yolo"` trusts tool execution and allows `bash`, `edit`, and `write`; any other value is treated as read-only for those tools. `read` remains available.

## Current Limitations

There is no token counting, hooks, LSP integration, or live provider model discovery.
