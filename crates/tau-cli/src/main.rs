use clap::{Parser, Subcommand, ValueEnum};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tau_core::agent::{AgentDisplay, StdoutDisplay};
use tau_core::session::{list_recent, SessionEvent};
use tau_core::{Agent, SessionStore, Tool};
use tau_llm::{Provider, ToolCall};
use tau_providers::{AnthropicProvider, OpenAiChatProvider, OpenAiResponsesProvider};
use tau_tools::{BashTool, EditTool, ReadTool, WriteTool};
use tau_tui::{AgentEvent, RunOutcome, TuiApp, TuiConfig, UserInput};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5";
const DEFAULT_OPENAI_RESPONSES_MODEL: &str = "gpt-5";
const DEFAULT_OPENAI_CHAT_MODEL: &str = "gpt-4o";

#[derive(Parser)]
#[command(name = "tau")]
struct Cli {
    #[arg(short = 'p', long = "prompt")]
    prompt: Option<String>,
    #[arg(long, value_enum, default_value_t = ProviderKind::Anthropic)]
    provider: ProviderKind,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    tui: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum ProviderKind {
    Anthropic,
    OpenaiResponses,
    OpenaiChat,
}

#[derive(Subcommand)]
enum Command {
    Resume { hash: String },
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::List) => {
            for (hash, timestamp, preview) in list_recent(20).await? {
                println!("{}  {}  {}", hash, timestamp.to_rfc3339(), preview);
            }
        }
        Some(Command::Resume { hash }) => {
            let (mut session, events) = SessionStore::open_by_hash(&hash).await?;
            let model = session_model(&events).unwrap_or_else(|| default_model(cli.provider));
            let mut agent = build_agent(cli.provider, &model, cli.base_url.clone(), &events)?;
            run_interactive(&mut agent, &mut session, &model, cli.tui).await?;
        }
        None => {
            let model = cli
                .model
                .clone()
                .unwrap_or_else(|| default_model(cli.provider));
            let cwd = std::env::current_dir()?;
            let mut session = SessionStore::create(&cwd, &model).await?;
            let mut agent = build_agent(cli.provider, &model, cli.base_url.clone(), &[])?;
            if let Some(prompt) = cli.prompt {
                let cancellation = CancellationToken::new();
                install_single_cancel(cancellation.clone());
                let mut display = StdoutDisplay;
                agent
                    .run_user_turn(prompt, &mut session, &mut display, cancellation)
                    .await?;
            } else {
                run_interactive(&mut agent, &mut session, &model, cli.tui).await?;
            }
        }
    }
    Ok(())
}

fn default_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Anthropic => DEFAULT_ANTHROPIC_MODEL.to_string(),
        ProviderKind::OpenaiResponses => DEFAULT_OPENAI_RESPONSES_MODEL.to_string(),
        ProviderKind::OpenaiChat => DEFAULT_OPENAI_CHAT_MODEL.to_string(),
    }
}

fn build_provider(
    kind: ProviderKind,
    model: &str,
    base_url: Option<String>,
) -> anyhow::Result<Arc<dyn Provider>> {
    Ok(match kind {
        ProviderKind::Anthropic => Arc::new(AnthropicProvider::from_env()?),
        ProviderKind::OpenaiResponses => {
            Arc::new(OpenAiResponsesProvider::from_env(Some(model.to_string()))?)
        }
        ProviderKind::OpenaiChat => Arc::new(OpenAiChatProvider::from_env(
            Some(model.to_string()),
            base_url,
        )?),
    })
}

fn build_agent(
    kind: ProviderKind,
    model: &str,
    base_url: Option<String>,
    events: &[SessionEvent],
) -> anyhow::Result<Agent> {
    let cwd = std::env::current_dir()?;
    let provider = build_provider(kind, model, base_url)?;
    let tools = default_tools(&cwd);
    let date = chrono::Local::now().date_naive();
    let system = format!(
        "You are tau, a coding agent. You have access to tools. Use them to help the user with software engineering tasks. The current working directory is {}. The current date is {}.",
        cwd.display(),
        date
    );
    Ok(Agent::from_events(
        provider,
        tools,
        model.to_string(),
        system,
        events,
    ))
}

fn default_tools(cwd: &Path) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadTool::new(cwd.to_path_buf())),
        Arc::new(BashTool),
        Arc::new(EditTool::new(cwd.to_path_buf())),
        Arc::new(WriteTool::new(cwd.to_path_buf())),
    ]
}

async fn run_interactive(
    agent: &mut Agent,
    session: &mut SessionStore,
    model: &str,
    tui: bool,
) -> anyhow::Result<()> {
    if tui {
        run_tui(agent, session, model).await
    } else {
        run_repl(agent, session).await
    }
}

async fn run_repl(agent: &mut Agent, session: &mut SessionStore) -> anyhow::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut last_ctrl_c: Option<Instant> = None;
    loop {
        println!("> ");
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    print_exit(session);
                    return Ok(());
                };
                if line.trim().is_empty() {
                    continue;
                }
                let exit = {
                    let cancellation = CancellationToken::new();
                    let mut display = StdoutDisplay;
                    let turn = agent.run_user_turn(line, session, &mut display, cancellation.clone());
                    tokio::pin!(turn);
                    tokio::select! {
                        result = &mut turn => {
                            if let Err(err) = result {
                                eprintln!("error: {err:#}");
                            }
                            false
                        }
                        _ = tokio::signal::ctrl_c() => {
                            let exit = double_ctrl_c(&mut last_ctrl_c);
                            cancellation.cancel();
                            let _ = (&mut turn).await;
                            if !exit {
                                println!("cancelled");
                            }
                            exit
                        }
                    }
                };
                if exit {
                    print_exit(session);
                    return Ok(());
                }
            }
            _ = tokio::signal::ctrl_c() => {
                if double_ctrl_c(&mut last_ctrl_c) {
                    print_exit(session);
                    return Ok(());
                }
                println!("press Ctrl-C again within 1.5 seconds to exit");
            }
        }
    }
}

struct TuiDisplay {
    tx: mpsc::Sender<AgentEvent>,
}

impl AgentDisplay for TuiDisplay {
    fn assistant_delta(&mut self, text: &str) -> anyhow::Result<()> {
        let _ = self
            .tx
            .try_send(AgentEvent::AssistantTextDelta(text.to_string()));
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

    fn tool_result(
        &mut self,
        call: &ToolCall,
        content: &str,
        is_error: bool,
    ) -> anyhow::Result<()> {
        let _ = self.tx.try_send(AgentEvent::ToolCallEnd {
            id: call.id.clone(),
            output: content.to_string(),
            is_error,
        });
        Ok(())
    }
}

async fn run_tui(
    agent: &mut Agent,
    session: &mut SessionStore,
    model: &str,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let mut app = TuiApp::new(TuiConfig {
        model: model.to_string(),
        cwd,
        session_hash: session.short_hash().to_string(),
    });

    let (agent_event_tx, agent_event_rx) = mpsc::channel::<AgentEvent>(128);
    let (user_input_tx, mut user_input_rx) = mpsc::channel::<UserInput>(32);
    let cancellation = CancellationToken::new();

    let agent_loop = async {
        while let Some(input) = user_input_rx.recv().await {
            let UserInput::Message(text) = input else {
                continue;
            };

            let turn_cancel = CancellationToken::new();
            let mut display = TuiDisplay {
                tx: agent_event_tx.clone(),
            };
            let turn =
                agent.run_user_turn(text, session, &mut display, turn_cancel.clone());
            tokio::pin!(turn);

            loop {
                tokio::select! {
                    result = &mut turn => {
                        match result {
                            Ok(()) => {
                                let _ = agent_event_tx.send(AgentEvent::TurnComplete).await;
                            }
                            Err(err) => {
                                let _ = agent_event_tx
                                    .send(AgentEvent::Error(format!("{err:#}")))
                                    .await;
                            }
                        }
                        break;
                    }
                    next = user_input_rx.recv() => {
                        match next {
                            Some(UserInput::Cancel) => turn_cancel.cancel(),
                            Some(UserInput::Message(_)) => {
                                let _ = agent_event_tx
                                    .send(AgentEvent::Error(
                                        "busy running the current turn".to_string(),
                                    ))
                                    .await;
                            }
                            None => {
                                turn_cancel.cancel();
                                let _ = (&mut turn).await;
                                return anyhow::Ok(());
                            }
                        }
                    }
                }
            }
        }
        anyhow::Ok(())
    };

    let outcome = tokio::select! {
        ui = app.run(agent_event_rx, user_input_tx, cancellation.clone()) => ui?,
        agent_result = agent_loop => {
            agent_result?;
            RunOutcome::ExitRequested
        }
    };

    match outcome {
        RunOutcome::ExitRequested => {
            print_exit(session);
            Ok(())
        }
    }
}

fn double_ctrl_c(last: &mut Option<Instant>) -> bool {
    let now = Instant::now();
    let double = last.is_some_and(|prev| now.duration_since(prev) <= Duration::from_millis(1500));
    *last = Some(now);
    double
}

fn print_exit(session: &SessionStore) {
    println!(
        "Session saved. Resume with: tau resume {}",
        session.short_hash()
    );
}

fn install_single_cancel(cancellation: CancellationToken) {
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancellation.cancel();
    });
}

fn session_model(events: &[SessionEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        SessionEvent::Session { model, .. } | SessionEvent::ModelChange { model, .. } => {
            Some(model.clone())
        }
        _ => None,
    })
}
