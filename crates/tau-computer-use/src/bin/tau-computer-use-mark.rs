use serde_json::json;
use tau_computer_use::ComputerUseTool;
use tau_core::Tool;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    let (input, duration_ms) = match args[0].as_str() {
        "app" => app_input(&args)?,
        "point" => point_input(&args)?,
        _ => {
            print_usage();
            std::process::exit(2);
        }
    };

    let tool = ComputerUseTool::default();
    let result = tool.execute(input, CancellationToken::new()).await?;
    println!("{}", result.content);
    if result.is_error {
        std::process::exit(1);
    }
    tokio::time::sleep(std::time::Duration::from_millis(duration_ms)).await;
    tool.cleanup().await?;
    Ok(())
}

fn app_input(args: &[String]) -> anyhow::Result<(serde_json::Value, u64)> {
    let Some(app) = args.get(1) else {
        anyhow::bail!("missing app name");
    };
    let duration_ms = parse_duration(args.get(2))?;
    Ok((
        json!({
            "action": "mark_app",
            "app": app,
            "duration_ms": duration_ms
        }),
        duration_ms,
    ))
}

fn point_input(args: &[String]) -> anyhow::Result<(serde_json::Value, u64)> {
    if args.len() < 3 {
        anyhow::bail!("point requires x y");
    }
    let x = args[1].parse::<i64>()?;
    let y = args[2].parse::<i64>()?;
    let duration_ms = parse_duration(args.get(3))?;
    Ok((
        json!({
            "action": "show_tau",
            "x": x,
            "y": y,
            "duration_ms": duration_ms
        }),
        duration_ms,
    ))
}

fn parse_duration(value: Option<&String>) -> anyhow::Result<u64> {
    value
        .map(|value| value.parse::<u64>())
        .transpose()
        .map(|duration| duration.unwrap_or(3_000))
        .map_err(Into::into)
}

fn print_usage() {
    eprintln!(
        "usage:\n  tau-computer-use-mark app APP_NAME [duration_ms]\n  tau-computer-use-mark point X Y [duration_ms]"
    );
}
