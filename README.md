# tau

![tau thumbnail](assets/thumbnail.png)

Minimal Rust coding-agent harness with JSONL sessions, local tools, multiple providers, and an optional TUI.

## Install

```sh
cargo build --release
mkdir -p ~/.local/bin
ln -sf "$PWD/target/release/tau" ~/.local/bin/tau
grep -q 'HOME/.local/bin' ~/.zshrc || echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
```

Open a new shell, then run `tau` from anywhere.

## Configure

Create/edit `~/.tau/config.toml`:

```toml
provider = "zai"
default_model = "glm-5.1"
sandbox_mode = "yolo"
```

Put API keys in your shell environment, a project `.env`, or `~/.tau/.env`.

Project-specific agent instructions go in `AGENTS.md` or `CLAUDE.md`. If both exist, tau reads only `AGENTS.md`.

Developer notes live in `AGENTS.md`.

## demo

```sh
tau -p "Which provider and model are active? Read README.md and summarize this repo in one sentence."
tau
```

## dev

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
