# tau Phase 2 — edit and write tools

You are extending an existing Rust coding-agent harness called `tau` at `~/tau`. Phase 1 already shipped: workspace, `tau-core` (agent loop, Tool trait, sessions), `tau-llm`, `tau-providers` (Anthropic), `tau-tools` (read, bash), `tau-cli`. All tests pass, build is clean.

## Your job

Add two new tools — `edit` and `write` — to the `tau-tools` crate. Match the existing code style (read `crates/tau-tools/src/lib.rs` for the `ReadTool` and `BashTool` implementations).

## Strict scope

You may only modify files in `crates/tau-tools/`. Do not touch any other crate. Do not modify the workspace `Cargo.toml`. Do not touch `tau-cli`.

## Reference

The TypeScript reference at `~/pi` has battle-tested edit semantics. Read these for the *behavior* (not the code):
- `~/pi/packages/agent/src/harness/agent-harness.ts` — search for the edit tool registration
- Any file in `~/pi/packages/coding-agent` matching `edit`

The Rust `Tool` trait you must implement:

```rust
// in tau-core
#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, input: Value, cancellation: CancellationToken) -> anyhow::Result<ToolResult>;
}
```

## Tool: `write`

- Args: `{ "path": string, "content": string }`
- Resolves `path` relative to the agent cwd (mirror `ReadTool`'s cwd handling, including `~/` expansion).
- Creates parent directories if needed.
- Writes content atomically: write to `{path}.tau-tmp`, then rename. This avoids partial-file corruption on crash.
- Returns success message: `wrote {bytes} bytes to {resolved_path}`.
- Errors: permission denied, parent path is not a directory, etc.

## Tool: `edit`

This is the high-risk tool. Get the semantics exactly right.

- Args: `{ "path": string, "old_string": string, "new_string": string, "replace_all"?: boolean }`
- Resolves path same as `read`/`write`.
- Reads the file as UTF-8.
- If `replace_all` is false (default):
    - Counts occurrences of `old_string`.
    - 0 occurrences → error: `old_string not found in {path}`.
    - 2+ occurrences → error: `old_string is not unique in {path} ({n} occurrences). Provide more surrounding context or set replace_all=true.`
    - Exactly 1 occurrence → replace it.
- If `replace_all` is true:
    - 0 occurrences → error.
    - N occurrences → replace all.
- Preserves the file's original line ending convention. If the file has CRLF line endings, output stays CRLF. If LF, stays LF. Detect by looking at the first line ending in the file.
- Preserves a leading BOM if present.
- Writes back atomically (same `.tau-tmp` + rename pattern).
- Refuses to operate on non-UTF-8 files (return error).
- Refuses if the file is a symlink to outside the cwd (basic safety).
- Returns: `replaced {n} occurrence(s) in {resolved_path}`.

## Tests (write these in `crates/tau-tools/tests/`)

Add to the existing `tests/tools.rs` or create new test files. Cover at minimum:

**For `write`**:
- Writes a new file with content.
- Overwrites an existing file.
- Creates parent directories.
- Atomic: if you simulate a write failure mid-rename, the original file is intact (you can test by checking the `.tau-tmp` cleanup, or skip if mocking is hard).

**For `edit`**:
- Single replacement when unique.
- Errors on zero matches.
- Errors on multiple matches without `replace_all`.
- Replaces all when `replace_all: true`.
- Preserves CRLF line endings on a CRLF input.
- Preserves LF line endings on a LF input.
- Preserves BOM if present in source.
- Errors on non-UTF-8 input.
- Errors on symlink-out-of-cwd (use a `tempfile` setup).
- Round-trip: write a file, edit it, read it back, verify content.

## Re-exports

Update `crates/tau-tools/src/lib.rs` to publicly export `EditTool` and `WriteTool` alongside `ReadTool` and `BashTool`.

## Integration note

You must NOT touch `tau-cli`. Instead, append a section to `crates/tau-tools/INTEGRATION.md` (create it) describing the exact lines `tau-cli/src/main.rs` (or wherever tools are registered) needs to add, like:

```rust
use tau_tools::{EditTool, WriteTool};
// ... in the agent setup:
agent.register_tool(Arc::new(EditTool::new(cwd.clone())));
agent.register_tool(Arc::new(WriteTool::new(cwd.clone())));
```

## Done criteria

- `cargo build --release` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test -p tau-tools` passes with all your new tests plus the existing 4.
- `crates/tau-tools/INTEGRATION.md` exists.
- No files outside `crates/tau-tools/` modified.

## Constraints

- No emojis. No em dashes. No comments explaining what code does (only why, when non-obvious). No TODO markers in committed code.

Begin.
