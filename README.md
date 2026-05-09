# tau

Minimal Rust coding-agent harness with JSONL sessions, local tools, multiple LLM providers, and an optional ratatui frontend.

## Install

```sh
cargo build --release
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/tau" ~/.local/bin/tau
```

Make sure `~/.local/bin` is on your `PATH`, then run `tau` from any directory.

## Config

tau reads `~/.tau/config.toml`:

```toml
provider = "zai"
default_model = "glm-5.1"
sandbox_mode = "yolo"
```

CLI flags override config:

```sh
tau --provider openrouter --model anthropic/claude-sonnet-4
tau --provider gemini -p "hello"
tau --tui
tau list
tau resume <hash>
```

## API Keys

API keys can come from:

- exported shell env vars, including values loaded by `.zshrc`
- a project `.env`
- `~/.tau/.env`

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

Supported providers: `anthropic`, `openai-responses`, `openai-chat`, `zai`, `kimi`, `minimax`, `deepseek`, `openrouter`, `groq`, `cerebras`, `xai`, `gemini`.

## Commands

```text
tau                       interactive REPL
tau -p "prompt"           one-shot prompt
tau --tui                 terminal UI
tau list                  recent sessions
tau resume <hash>         resume a session
tau --list-models         provider defaults
```

Interactive commands:

```text
/compact
/help
```

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Set `RUST_LOG` for debug output, for example:

```sh
RUST_LOG=tau_core=info tau
```
