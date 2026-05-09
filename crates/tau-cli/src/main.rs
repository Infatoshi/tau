use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tau_core::session::{list_recent, SessionEvent};
use tau_core::{Agent, SessionStore, Tool};
use tau_core::{AgentDisplay, StdoutDisplay};
use tau_llm::{ContentBlock, Provider, ToolCall};
use tau_providers::{AnthropicProvider, OpenAiChatProvider, OpenAiResponsesProvider};
use tau_tools::{BashTool, EditTool, PermissionedTool, ReadTool, SandboxMode, WriteTool};
use tau_tui::{AgentEvent, RunOutcome, TuiApp, TuiConfig, UserInput};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod errors;

const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5";
const DEFAULT_OPENAI_RESPONSES_MODEL: &str = "gpt-5";
const DEFAULT_OPENAI_CHAT_MODEL: &str = "gpt-4o";
const DEFAULT_ZAI_MODEL: &str = "glm-5.1";
const DEFAULT_KIMI_MODEL: &str = "kimi-k2.5";
const DEFAULT_MINIMAX_MODEL: &str = "MiniMax-M2.7";
const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_OPENROUTER_MODEL: &str = "openrouter/auto";
const DEFAULT_GROQ_MODEL: &str = "openai/gpt-oss-120b";
const DEFAULT_CEREBRAS_MODEL: &str = "gpt-oss-120b";
const DEFAULT_XAI_MODEL: &str = "grok-4.20-reasoning";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";

#[derive(Parser)]
#[command(name = "tau")]
struct Cli {
    #[arg(short = 'p', long = "prompt")]
    prompt: Option<String>,
    #[arg(long, value_enum)]
    provider: Option<ProviderKind>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    sandbox_mode: Option<String>,
    #[arg(long)]
    tui: bool,
    #[arg(long)]
    list_models: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProviderKind {
    Anthropic,
    OpenaiResponses,
    OpenaiChat,
    Zai,
    Kimi,
    Minimax,
    Deepseek,
    Openrouter,
    Groq,
    Cerebras,
    Xai,
    Gemini,
}

impl ProviderKind {
    fn name(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenaiResponses => "openai-responses",
            ProviderKind::OpenaiChat => "openai-chat",
            ProviderKind::Zai => "zai",
            ProviderKind::Kimi => "kimi",
            ProviderKind::Minimax => "minimax",
            ProviderKind::Deepseek => "deepseek",
            ProviderKind::Openrouter => "openrouter",
            ProviderKind::Groq => "groq",
            ProviderKind::Cerebras => "cerebras",
            ProviderKind::Xai => "xai",
            ProviderKind::Gemini => "gemini",
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Resume { hash: String },
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging()?;
    load_env_files();
    let cli = Cli::parse();
    let config = TauConfig::load().await?;
    if cli.list_models {
        print_models(provider_kind(&cli, &config)?);
        return Ok(());
    }
    match cli.command {
        Some(Command::List) => {
            for (hash, timestamp, preview) in list_recent(20).await? {
                println!("{}  {}  {}", hash, timestamp.to_rfc3339(), preview);
            }
        }
        Some(Command::Resume { ref hash }) => {
            let (mut session, events) = SessionStore::open_by_hash(hash).await?;
            let provider = provider_kind(&cli, &config)?;
            let model =
                session_model(&events).unwrap_or_else(|| selected_model(&cli, &config, provider));
            let mut agent = build_agent(
                provider,
                &model,
                cli.base_url.clone(),
                sandbox_mode(&cli, &config),
                &events,
            )?;
            if !cli.tui {
                replay_repl_transcript(&events)?;
            }
            run_interactive(&mut agent, &mut session, &model, cli.tui).await?;
        }
        None => {
            let provider = provider_kind(&cli, &config)?;
            let model = selected_model(&cli, &config, provider);
            let cwd = std::env::current_dir()?;
            let mut session = SessionStore::create(&cwd, &model).await?;
            let mut agent = build_agent(
                provider,
                &model,
                cli.base_url.clone(),
                sandbox_mode(&cli, &config),
                &[],
            )?;
            if let Some(prompt) = cli.prompt {
                let cancellation = CancellationToken::new();
                install_single_cancel(cancellation.clone());
                let mut display = StdoutDisplay::default();
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

fn load_env_files() {
    let _ = dotenvy::dotenv();
    if let Some(home) = dirs::home_dir() {
        let _ = dotenvy::from_path(home.join(".tau").join(".env"));
    }
}

#[derive(Default, Deserialize)]
struct TauConfig {
    provider: Option<ProviderKind>,
    default_model: Option<String>,
    sandbox_mode: Option<String>,
}

impl TauConfig {
    async fn load() -> anyhow::Result<Self> {
        let Some(home) = dirs::home_dir() else {
            return Ok(Self::default());
        };
        let path = home.join(".tau").join("config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = tokio::fs::read_to_string(&path).await?;
        toml::from_str(&content).map_err(|err| errors::parse_config(&path, err))
    }
}

fn selected_model(cli: &Cli, config: &TauConfig, provider: ProviderKind) -> String {
    cli.model
        .clone()
        .or_else(|| config.default_model.clone())
        .unwrap_or_else(|| default_model(provider))
}

fn provider_kind(cli: &Cli, config: &TauConfig) -> anyhow::Result<ProviderKind> {
    Ok(cli
        .provider
        .or(config.provider)
        .unwrap_or(ProviderKind::Anthropic))
}

fn sandbox_mode(cli: &Cli, config: &TauConfig) -> SandboxMode {
    cli.sandbox_mode
        .as_deref()
        .or(config.sandbox_mode.as_deref())
        .map(SandboxMode::from_config)
        .unwrap_or(SandboxMode::ReadOnly)
}

fn default_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Anthropic => DEFAULT_ANTHROPIC_MODEL.to_string(),
        ProviderKind::OpenaiResponses => DEFAULT_OPENAI_RESPONSES_MODEL.to_string(),
        ProviderKind::OpenaiChat => DEFAULT_OPENAI_CHAT_MODEL.to_string(),
        ProviderKind::Zai => DEFAULT_ZAI_MODEL.to_string(),
        ProviderKind::Kimi => DEFAULT_KIMI_MODEL.to_string(),
        ProviderKind::Minimax => DEFAULT_MINIMAX_MODEL.to_string(),
        ProviderKind::Deepseek => DEFAULT_DEEPSEEK_MODEL.to_string(),
        ProviderKind::Openrouter => DEFAULT_OPENROUTER_MODEL.to_string(),
        ProviderKind::Groq => DEFAULT_GROQ_MODEL.to_string(),
        ProviderKind::Cerebras => DEFAULT_CEREBRAS_MODEL.to_string(),
        ProviderKind::Xai => DEFAULT_XAI_MODEL.to_string(),
        ProviderKind::Gemini => DEFAULT_GEMINI_MODEL.to_string(),
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
        ProviderKind::Zai
        | ProviderKind::Kimi
        | ProviderKind::Minimax
        | ProviderKind::Deepseek
        | ProviderKind::Openrouter
        | ProviderKind::Groq
        | ProviderKind::Cerebras
        | ProviderKind::Xai
        | ProviderKind::Gemini => {
            let spec = chat_provider_spec(kind);
            let api_key = read_api_key(spec.env_vars)?;
            Arc::new(OpenAiChatProvider::new(
                api_key,
                Some(model.to_string()),
                Some(base_url.unwrap_or_else(|| spec.base_url.to_string())),
            ))
        }
    })
}

struct ChatProviderSpec {
    base_url: &'static str,
    env_vars: &'static [&'static str],
}

fn chat_provider_spec(kind: ProviderKind) -> ChatProviderSpec {
    match kind {
        ProviderKind::Zai => ChatProviderSpec {
            base_url: "https://api.z.ai/api/coding/paas/v4",
            env_vars: &["ZAI_API_KEY"],
        },
        ProviderKind::Kimi => ChatProviderSpec {
            base_url: "https://api.moonshot.ai/v1",
            env_vars: &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        },
        ProviderKind::Minimax => ChatProviderSpec {
            base_url: "https://api.minimax.io/v1",
            env_vars: &["MINIMAX_API_KEY"],
        },
        ProviderKind::Deepseek => ChatProviderSpec {
            base_url: "https://api.deepseek.com",
            env_vars: &["DEEPSEEK_API_KEY"],
        },
        ProviderKind::Openrouter => ChatProviderSpec {
            base_url: "https://openrouter.ai/api/v1",
            env_vars: &["OPENROUTER_API_KEY"],
        },
        ProviderKind::Groq => ChatProviderSpec {
            base_url: "https://api.groq.com/openai/v1",
            env_vars: &["GROQ_API_KEY"],
        },
        ProviderKind::Cerebras => ChatProviderSpec {
            base_url: "https://api.cerebras.ai/v1",
            env_vars: &["CEREBRAS_API_KEY"],
        },
        ProviderKind::Xai => ChatProviderSpec {
            base_url: "https://api.x.ai/v1",
            env_vars: &["XAI_API_KEY"],
        },
        ProviderKind::Gemini => ChatProviderSpec {
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            env_vars: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        },
        ProviderKind::Anthropic | ProviderKind::OpenaiResponses | ProviderKind::OpenaiChat => {
            unreachable!("not a named chat-compatible provider")
        }
    }
}

fn read_api_key(env_vars: &[&str]) -> anyhow::Result<String> {
    for env_var in env_vars {
        if let Ok(value) = std::env::var(env_var) {
            return Ok(value);
        }
    }
    Err(errors::missing_any_env(env_vars))
}

fn build_agent(
    kind: ProviderKind,
    model: &str,
    base_url: Option<String>,
    sandbox_mode: SandboxMode,
    events: &[SessionEvent],
) -> anyhow::Result<Agent> {
    let cwd = std::env::current_dir()?;
    let provider = build_provider(kind, model, base_url)?;
    let tools = default_tools(&cwd, sandbox_mode);
    let date = chrono::Local::now().date_naive();
    let mut system = format!(
        "You are tau, a coding agent. You have access to tools. Use them to help the user with software engineering tasks. The active provider is {} and the active model is {}. The current working directory is {}. The current date is {}.",
        kind.name(),
        model,
        cwd.display(),
        date
    );
    if let Some((source, instructions)) = project_instructions(&cwd)? {
        system.push_str(&format!(
            "\n\nProject instructions from {}:\n\n{}",
            source, instructions
        ));
    }
    Ok(Agent::from_events(
        provider,
        tools,
        model.to_string(),
        system,
        events,
    ))
}

fn project_instructions(cwd: &Path) -> anyhow::Result<Option<(String, String)>> {
    for file_name in ["AGENTS.md", "CLAUDE.md"] {
        let path = cwd.join(file_name);
        if path.is_file() {
            let content = std::fs::read_to_string(&path)?;
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            return Ok(Some((file_name.to_string(), trimmed.to_string())));
        }
    }
    Ok(None)
}

fn default_tools(cwd: &Path, sandbox_mode: SandboxMode) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadTool::new(cwd.to_path_buf())),
        Arc::new(PermissionedTool::new(BashTool, sandbox_mode)),
        Arc::new(PermissionedTool::new(
            EditTool::new(cwd.to_path_buf()),
            sandbox_mode,
        )),
        Arc::new(PermissionedTool::new(
            WriteTool::new(cwd.to_path_buf()),
            sandbox_mode,
        )),
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
    let mut ctrl_c = CtrlCChannel::new();
    print_prompt()?;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    print_exit(session);
                    return Ok(());
                };
                if line.trim().is_empty() {
                    print_prompt()?;
                    continue;
                }
                if handle_slash_command(agent, session, line.trim()).await? {
                    print_prompt()?;
                    continue;
                }
                let exit = {
                    let cancellation = CancellationToken::new();
                    let mut display = StdoutDisplay::default();
                    let turn = agent.run_user_turn(line, session, &mut display, cancellation.clone());
                    tokio::pin!(turn);
                    tokio::select! {
                        result = &mut turn => {
                            if let Err(err) = result {
                                eprintln!("error: {err:#}");
                            }
                            false
                        }
                        _ = ctrl_c.recv() => {
                            cancellation.cancel();
                            let _ = (&mut turn).await;
                            println!("cancelled");
                            false
                        }
                    }
                };
                if exit {
                    print_exit(session);
                    return Ok(());
                }
                print_prompt()?;
            }
            _ = ctrl_c.recv() => {
                print_exit(session);
                std::process::exit(0);
            }
        }
    }
}

fn replay_repl_transcript(events: &[SessionEvent]) -> anyhow::Result<()> {
    let has_messages = events.iter().any(|event| {
        matches!(
            event,
            SessionEvent::UserMessage { .. } | SessionEvent::AssistantMessage { .. }
        )
    });
    if !has_messages {
        return Ok(());
    }

    let mut last_user = None;
    let mut last_assistant = None;
    for event in events.iter().rev() {
        if last_assistant.is_none() {
            if let SessionEvent::AssistantMessage { content, .. } = event {
                last_assistant = assistant_text_preview(content);
                continue;
            }
        }
        if last_user.is_none() {
            if let SessionEvent::UserMessage { content, .. } = event {
                last_user = Some(content.clone());
            }
        }
        if last_user.is_some() && last_assistant.is_some() {
            break;
        }
    }

    println!("\n\x1b[2mresumed session, showing last exchange\x1b[0m");
    if let Some(user) = last_user {
        println!("  \x1b[1;36m❯\x1b[0m {}", compact_preview(&user));
    }
    if let Some(assistant) = last_assistant {
        println!("{}", compact_preview(&assistant));
    }
    println!();
    Ok(())
}

fn assistant_text_preview(content: &[ContentBlock]) -> Option<String> {
    let text = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn compact_preview(text: &str) -> String {
    const MAX_LINES: usize = 3;
    const MAX_CHARS: usize = 240;

    let normalized = text.replace('\r', "");
    let mut lines = normalized.lines();
    let mut preview = lines
        .by_ref()
        .take(MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let omitted_lines = lines.count();
    if omitted_lines > 0 {
        preview.push_str(&format!(
            "\n\x1b[2m... omitted {omitted_lines} lines ...\x1b[0m"
        ));
    }

    if preview.chars().count() > MAX_CHARS {
        let prefix = preview
            .chars()
            .take(MAX_CHARS.saturating_sub(20))
            .collect::<String>();
        format!("{prefix}\x1b[2m... truncated ...\x1b[0m")
    } else {
        preview
    }
}

struct CtrlCChannel {
    rx: mpsc::Receiver<()>,
    handle: JoinHandle<()>,
}

impl CtrlCChannel {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel(8);
        let handle = tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                if tx.send(()).await.is_err() {
                    break;
                }
            }
        });
        Self { rx, handle }
    }

    async fn recv(&mut self) -> Option<()> {
        self.rx.recv().await
    }
}

impl Drop for CtrlCChannel {
    fn drop(&mut self) {
        self.handle.abort();
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

async fn run_tui(agent: &mut Agent, session: &mut SessionStore, model: &str) -> anyhow::Result<()> {
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
            if handle_slash_command_tui(agent, session, text.trim(), &agent_event_tx).await? {
                continue;
            }

            let turn_cancel = CancellationToken::new();
            let mut display = TuiDisplay {
                tx: agent_event_tx.clone(),
            };
            let turn = agent.run_user_turn(text, session, &mut display, turn_cancel.clone());
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

fn print_exit(session: &SessionStore) {
    println!(
        "\nSession saved. Resume with: tau resume {}",
        session.short_hash()
    );
}

fn print_prompt() -> anyhow::Result<()> {
    print!("  \x1b[1;36m❯\x1b[0m ");
    std::io::stdout().flush()?;
    Ok(())
}

async fn handle_slash_command(
    agent: &mut Agent,
    session: &mut SessionStore,
    line: &str,
) -> anyhow::Result<bool> {
    match line {
        "/compact" => {
            let cancellation = CancellationToken::new();
            install_single_cancel(cancellation.clone());
            let summary = agent.compact_context(session, cancellation).await?;
            println!("Compacted context:\n{summary}");
            Ok(true)
        }
        "/help" => {
            println!("Commands: /compact, /help");
            Ok(true)
        }
        _ if line.starts_with('/') => {
            println!("unknown command: {line}");
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn handle_slash_command_tui(
    agent: &mut Agent,
    session: &mut SessionStore,
    line: &str,
    tx: &mpsc::Sender<AgentEvent>,
) -> anyhow::Result<bool> {
    match line {
        "/compact" => {
            let summary = agent
                .compact_context(session, CancellationToken::new())
                .await?;
            let _ = tx
                .send(AgentEvent::AssistantTextDelta(format!(
                    "Compacted context:\n{summary}"
                )))
                .await;
            let _ = tx.send(AgentEvent::TurnComplete).await;
            Ok(true)
        }
        "/help" => {
            let _ = tx
                .send(AgentEvent::AssistantTextDelta(
                    "Commands: /compact, /help".to_string(),
                ))
                .await;
            let _ = tx.send(AgentEvent::TurnComplete).await;
            Ok(true)
        }
        _ if line.starts_with('/') => {
            let _ = tx
                .send(AgentEvent::Error(format!("unknown command: {line}")))
                .await;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn print_models(kind: ProviderKind) {
    match kind {
        ProviderKind::Anthropic => {
            println!("anthropic");
            println!("  default: {DEFAULT_ANTHROPIC_MODEL}");
            println!("  known:   claude-sonnet-4-5");
        }
        ProviderKind::OpenaiResponses => {
            println!("openai-responses");
            println!("  default: {DEFAULT_OPENAI_RESPONSES_MODEL}");
            println!("  known:   gpt-5");
        }
        ProviderKind::OpenaiChat => {
            println!("openai-chat");
            println!("  default: {DEFAULT_OPENAI_CHAT_MODEL}");
            println!("  known:   gpt-4o");
            println!("  note:    use --base-url for any OpenAI-compatible endpoint.");
        }
        ProviderKind::Zai => print_named_chat_provider(
            "zai",
            DEFAULT_ZAI_MODEL,
            "ZAI_API_KEY",
            "https://api.z.ai/api/coding/paas/v4",
        ),
        ProviderKind::Kimi => print_named_chat_provider(
            "kimi",
            DEFAULT_KIMI_MODEL,
            "MOONSHOT_API_KEY or KIMI_API_KEY",
            "https://api.moonshot.ai/v1",
        ),
        ProviderKind::Minimax => print_named_chat_provider(
            "minimax",
            DEFAULT_MINIMAX_MODEL,
            "MINIMAX_API_KEY",
            "https://api.minimax.io/v1",
        ),
        ProviderKind::Deepseek => print_named_chat_provider(
            "deepseek",
            DEFAULT_DEEPSEEK_MODEL,
            "DEEPSEEK_API_KEY",
            "https://api.deepseek.com",
        ),
        ProviderKind::Openrouter => print_named_chat_provider(
            "openrouter",
            DEFAULT_OPENROUTER_MODEL,
            "OPENROUTER_API_KEY",
            "https://openrouter.ai/api/v1",
        ),
        ProviderKind::Groq => print_named_chat_provider(
            "groq",
            DEFAULT_GROQ_MODEL,
            "GROQ_API_KEY",
            "https://api.groq.com/openai/v1",
        ),
        ProviderKind::Cerebras => print_named_chat_provider(
            "cerebras",
            DEFAULT_CEREBRAS_MODEL,
            "CEREBRAS_API_KEY",
            "https://api.cerebras.ai/v1",
        ),
        ProviderKind::Xai => print_named_chat_provider(
            "xai",
            DEFAULT_XAI_MODEL,
            "XAI_API_KEY",
            "https://api.x.ai/v1",
        ),
        ProviderKind::Gemini => print_named_chat_provider(
            "gemini",
            DEFAULT_GEMINI_MODEL,
            "GEMINI_API_KEY or GOOGLE_API_KEY",
            "https://generativelanguage.googleapis.com/v1beta/openai",
        ),
    }
}

fn print_named_chat_provider(name: &str, default_model: &str, env: &str, base_url: &str) {
    println!("{name}");
    println!("  default: {default_model}");
    println!("  env:     {env}");
    println!("  base:    {base_url}");
    println!("  note:    OpenAI Chat Completions-compatible.");
}

fn init_logging() -> anyhow::Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(errors::init_logging)
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
