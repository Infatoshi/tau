use clap::{Parser, Subcommand};
use tau_core::agent::StdoutDisplay;
use tau_core::session::{list_recent, SessionEvent};
use tau_core::{Agent, SessionStore, Tool};
use tau_providers::AnthropicProvider;
use tau_tools::{BashTool, ReadTool};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

const DEFAULT_MODEL: &str = "claude-sonnet-4-5";

#[derive(Parser)]
#[command(name = "tau")]
struct Cli {
    #[arg(short = 'p', long = "prompt")]
    prompt: Option<String>,
    #[arg(long, default_value = DEFAULT_MODEL)]
    model: String,
    #[command(subcommand)]
    command: Option<Command>,
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
            let model = session_model(&events).unwrap_or(cli.model);
            let mut agent = agent(&model, &events).await?;
            repl(&mut agent, &mut session).await?;
        }
        None => {
            let cwd = std::env::current_dir()?;
            let mut session = SessionStore::create(&cwd, &cli.model).await?;
            let mut agent = agent(&cli.model, &[]).await?;
            if let Some(prompt) = cli.prompt {
                let cancellation = CancellationToken::new();
                install_single_cancel(cancellation.clone());
                let mut display = StdoutDisplay;
                agent
                    .run_user_turn(prompt, &mut session, &mut display, cancellation)
                    .await?;
            } else {
                repl(&mut agent, &mut session).await?;
            }
        }
    }
    Ok(())
}

async fn agent(model: &str, events: &[SessionEvent]) -> anyhow::Result<Agent> {
    let cwd = std::env::current_dir()?;
    let provider = Arc::new(AnthropicProvider::from_env()?);
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(ReadTool::new(cwd.clone())), Arc::new(BashTool)];
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

async fn repl(agent: &mut Agent, session: &mut SessionStore) -> anyhow::Result<()> {
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
