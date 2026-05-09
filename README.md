# tau

Minimal Rust coding-agent harness with JSONL sessions, local tools, multiple providers, and an optional TUI.

## Install

```sh
cargo build --release
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/tau" ~/.local/bin/tau
```

Make sure `~/.local/bin` is on `PATH`.

## Configure

Create `~/.tau/config.toml`:

```toml
provider = "zai"
default_model = "glm-5.1"
sandbox_mode = "yolo"
```

Put API keys in your shell environment, a project `.env`, or `~/.tau/.env`.

## Use

```sh
tau
tau -p "read Cargo.toml"
tau --tui
tau list
tau resume <hash>
tau --list-models
```

Interactive commands:

```text
/compact
/help
```

Project-specific agent instructions go in `AGENTS.md` or `CLAUDE.md`. If both exist, tau reads only `AGENTS.md`.

Developer notes live in `AGENTS.md`.
