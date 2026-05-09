# tau-cli TUI Integration

This crate is intentionally decoupled from the agent loop. `tau-cli` owns sessions, tools, providers, and saving. The TUI owns terminal input and rendering, then exchanges typed channel events with the CLI glue.

## 1. Add a `--tui` flag

```rust
#[derive(Parser)]
#[command(name = "tau")]
struct Cli {
    #[arg(short = 'p', long = "prompt")]
    prompt: Option<String>,
    #[arg(long, default_value = DEFAULT_MODEL)]
    model: String,
    #[arg(long)]
    tui: bool,
    #[command(subcommand)]
    command: Option<Command>,
}
```

Use it only for interactive runs. Keep `--prompt` on the existing plain path.

## 2. Construct `TuiApp`

After creating or resuming the session:

```rust
use tau_tui::{RunOutcome, TuiApp, TuiConfig};

let cwd = std::env::current_dir()?;
let mut app = TuiApp::new(TuiConfig {
    model: model.clone(),
    cwd,
    session_hash: session.short_hash().to_string(),
});
```

## 3. Set up channels

```rust
use tokio::sync::mpsc;
use tau_tui::{AgentEvent, UserInput};
use tokio_util::sync::CancellationToken;

let (agent_event_tx, agent_event_rx) = mpsc::channel::<AgentEvent>(128);
let (user_input_tx, mut user_input_rx) = mpsc::channel::<UserInput>(32);
let cancellation = CancellationToken::new();
```

## 4. Spawn the agent loop

Add a small display adapter in `tau-cli/src/main.rs`:

```rust
use tau_core::agent::AgentDisplay;
use tau_llm::ToolCall;

struct TuiDisplay {
    tx: mpsc::Sender<AgentEvent>,
}

impl AgentDisplay for TuiDisplay {
    fn assistant_delta(&mut self, text: &str) -> anyhow::Result<()> {
        let _ = self.tx.try_send(AgentEvent::AssistantTextDelta(text.to_string()));
        Ok(())
    }

    fn tool_call(&mut self, call: &ToolCall) -> anyhow::Result<()> {
        let _ = self.tx.try_send(AgentEvent::ToolCallStart {
            name: call.name.clone(),
            input: call.input.clone(),
            id: call.id.clone(),
        });
        Ok(())
    }

    fn tool_result(&mut self, call: &ToolCall, content: &str, is_error: bool) -> anyhow::Result<()> {
        let _ = self.tx.try_send(AgentEvent::ToolCallEnd {
            id: call.id.clone(),
            output: content.to_string(),
            is_error,
        });
        Ok(())
    }
}
```

Then wire the loop. This shape keeps receiving `UserInput::Cancel` while a turn is running:

```rust
let agent_task = tokio::spawn(async move {
    let mut display = TuiDisplay {
        tx: agent_event_tx.clone(),
    };

    while let Some(input) = user_input_rx.recv().await {
        let UserInput::Message(text) = input else {
            continue;
        };

        let turn_cancel = CancellationToken::new();
        let turn = agent.run_user_turn(text, &mut session, &mut display, turn_cancel.clone());
        tokio::pin!(turn);

        loop {
            tokio::select! {
                result = &mut turn => {
                    match result {
                        Ok(()) => {
                            let _ = agent_event_tx.send(AgentEvent::TurnComplete).await;
                        }
                        Err(err) => {
                            let _ = agent_event_tx.send(AgentEvent::Error(format!("{err:#}"))).await;
                        }
                    }
                    break;
                }
                input = user_input_rx.recv() => {
                    match input {
                        Some(UserInput::Cancel) => turn_cancel.cancel(),
                        Some(UserInput::Message(_)) => {
                            let _ = agent_event_tx
                                .send(AgentEvent::Error("busy running the current turn".to_string()))
                                .await;
                        }
                        None => {
                            turn_cancel.cancel();
                            break;
                        }
                    }
                }
            }
        }
    }

    anyhow::Ok(session)
});
```

The top-level token passed into `TuiApp::run` is the UI signal source for Ctrl-C. The per-turn token is what interrupts the model stream or tool call.

## 5. Handle exit and save

```rust
let outcome = app
    .run(agent_event_rx, user_input_tx, cancellation.clone())
    .await?;

match outcome {
    RunOutcome::ExitRequested => {
        drop(cancellation);
        let session = agent_task.await??;
        println!(
            "Session saved. Resume with: tau resume {}",
            session.short_hash()
        );
    }
}
```

The exact ownership shape may change when this is pasted into `tau-cli`, but the boundary should remain the same: CLI converts `AgentDisplay` callbacks into `AgentEvent`s, and TUI converts keyboard actions into `UserInput`s.
