use serde_json::Value;
use tau_llm::ToolCall;

use super::ansi::{dim, BLUE, CYAN, DIM, GREEN, MAGENTA, RED, RESET, YELLOW};

struct ToolStyle {
    icon: &'static str,
    color: &'static str,
}

pub fn print_tool_call(call: &ToolCall) {
    let style = tool_style(&call.name);
    println!(
        "\n\n{}{} tool {}{}",
        style.color, style.icon, call.name, RESET
    );
    for line in format_tool_input(&call.input).lines() {
        println!("    {line}");
    }
}

pub fn print_tool_result(call: &ToolCall, content: &str, is_error: bool) {
    let style = tool_style(&call.name);
    let (status, status_color) = if is_error {
        ("failed", RED)
    } else if content.trim().is_empty() {
        ("empty", DIM)
    } else {
        ("ok", GREEN)
    };
    println!(
        "{}{} result {}{}  {}{}{}",
        style.color, style.icon, call.name, RESET, status_color, status, RESET
    );
    let output = compact_tool_output(content, 3, 12);
    for line in output.lines() {
        println!("    {line}");
    }
    if content.is_empty() {
        println!("    {}", dim("no output"));
    }
    println!();
}

fn tool_style(name: &str) -> ToolStyle {
    match name {
        "read" => ToolStyle {
            icon: "◇",
            color: CYAN,
        },
        "bash" => ToolStyle {
            icon: "▸",
            color: BLUE,
        },
        "edit" => ToolStyle {
            icon: "✎",
            color: YELLOW,
        },
        "write" => ToolStyle {
            icon: "◆",
            color: MAGENTA,
        },
        _ => ToolStyle {
            icon: "●",
            color: DIM,
        },
    }
}

fn format_tool_input(input: &Value) -> String {
    if input.is_null() {
        return dim("no input");
    }
    let Some(object) = input.as_object() else {
        return truncate_display_line(&input.to_string(), 160);
    };
    let mut out = String::new();
    for (key, value) in object {
        if let Some(text) = value.as_str() {
            if text.contains('\n') {
                out.push_str(&format!("{key}:\n"));
                for line in compact_tool_output(text, 3, 12).lines() {
                    out.push_str("  ");
                    out.push_str(&truncate_display_line(line, 160));
                    out.push('\n');
                }
            } else {
                out.push_str(&format!("{key}: {}\n", truncate_display_line(text, 160)));
            }
        } else {
            let value = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
            out.push_str(&format!("{key}: {}\n", truncate_display_line(&value, 160)));
        }
    }
    out.trim_end_matches('\n').to_string()
}

fn compact_tool_output(content: &str, head: usize, max_lines: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() <= max_lines {
        return content.to_string();
    }
    let omitted = lines.len().saturating_sub(head);
    let mut out = String::new();
    for line in lines.iter().take(head) {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!("{}... omitted {omitted} lines ...{}", DIM, RESET));
    out
}

fn truncate_display_line(line: &str, max_chars: usize) -> String {
    let count = line.chars().count();
    if count <= max_chars {
        return line.to_string();
    }
    let keep = max_chars.saturating_sub(24);
    let prefix = line.chars().take(keep).collect::<String>();
    format!("{prefix}{DIM} ... truncated {count} chars ...{RESET}")
}
