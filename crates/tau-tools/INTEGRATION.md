# Edit and Write Tool Registration

Add the new tools wherever `tau-cli` registers the existing `ReadTool` and `BashTool`.

```rust
use tau_tools::{EditTool, WriteTool};
```

In the agent setup, after the cwd is available:

```rust
agent.register_tool(Arc::new(EditTool::new(cwd.clone())));
agent.register_tool(Arc::new(WriteTool::new(cwd.clone())));
```
