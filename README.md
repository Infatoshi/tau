# τ

![tau demo](assets/tau-demo.gif)

minimal rust coding-agent harness

computer use is intentionally high-trust: tau acts on grounded accessibility targets, verifies outcomes, and fails closed instead of guessing screen coordinates.

## install

```sh
cargo run --bin tau-install
```

1. the first `tau` run creates `~/.tau/config.yaml`

2. api keys go in a project `.env`, or `~/.tau/.env`.

3. harness reads `AGENTS.md`

4. dev/testing workflow is `cargo fmt --check && cargo test && cargo clippy --all-targets --all-features -- -D warnings`
