# tau

tau is a minimal Rust coding-agent harness. It streams Anthropic Messages API output, executes tools without permission prompts, and stores every session event as append-only JSONL.

## Build

```sh
cargo build --release
```

## Run

```sh
ANTHROPIC_API_KEY=sk-... ./target/release/tau
ANTHROPIC_API_KEY=sk-... ./target/release/tau -p "read Cargo.toml"
./target/release/tau list
./target/release/tau resume <short_hash>
```

Use `--model` to override the default `claude-sonnet-4-5`.

## Current Limitations

v0.1 only supports Anthropic, stdin/stdout interaction, append-only JSONL sessions, and two tools: `read` and `bash`. There is no TUI, config file, token counting, context pruning, hooks, LSP integration, or compaction.
