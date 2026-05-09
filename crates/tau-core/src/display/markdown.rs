use super::ansi::{BOLD, CYAN, GRAY, MAGENTA, RESET, YELLOW};
use super::tables;

pub fn looks_like_markdown(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with('#')
            || trimmed.starts_with('|')
            || trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("> ")
            || trimmed.starts_with("```")
            || trimmed.starts_with("---")
            || (trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit())
                && trimmed.contains(". "))
            || trimmed.contains("**")
            || trimmed.contains('`')
    })
}

pub fn render_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    let lines = text.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            out.push_str(&format!(
                "{GRAY}{}{RESET}\n",
                if in_code { "┌ code" } else { "└" }
            ));
            index += 1;
            continue;
        }
        if in_code {
            out.push_str(&format!("{GRAY}│{RESET} {line}\n"));
            index += 1;
            continue;
        }
        if trimmed.is_empty() {
            out.push('\n');
            index += 1;
            continue;
        }
        if tables::is_table_row(trimmed)
            && index + 1 < lines.len()
            && tables::is_table_separator(lines[index + 1].trim())
        {
            let start = index;
            index += 2;
            while index < lines.len() && tables::is_table_row(lines[index].trim()) {
                index += 1;
            }
            out.push_str(&tables::format_table(&lines[start..index]));
            continue;
        }
        if let Some(level) = heading_level(trimmed) {
            let title = trimmed[level..].trim();
            out.push_str(&format!(
                "\n{CYAN}{BOLD}{} {}{RESET}\n",
                "▰".repeat(level.min(3)),
                render_inline(title)
            ));
            index += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push_str(&format!("{GRAY}│ {RESET}{}\n", render_inline(rest)));
            index += 1;
            continue;
        }
        if trimmed == "---" || trimmed == "***" {
            out.push_str(&format!("{GRAY}{}\n{RESET}", "─".repeat(72)));
            index += 1;
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            out.push_str(&format!("{YELLOW} • {RESET}{}\n", render_inline(rest)));
            index += 1;
            continue;
        }
        if let Some((marker, rest)) = numbered_item(trimmed) {
            out.push_str(&format!(
                "{YELLOW}{marker:>3}{RESET} {}\n",
                render_inline(rest)
            ));
            index += 1;
            continue;
        }
        out.push_str(&render_inline(line));
        out.push('\n');
        index += 1;
    }
    out
}

pub fn strip_inline_markers(text: &str) -> String {
    text.replace("**", "").replace('`', "")
}

fn heading_level(line: &str) -> Option<usize> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&level) && line.chars().nth(level) == Some(' ') {
        Some(level)
    } else {
        None
    }
}

fn numbered_item(line: &str) -> Option<(&str, &str)> {
    let dot = line.find(". ")?;
    if dot > 0 && line[..dot].chars().all(|ch| ch.is_ascii_digit()) {
        Some((&line[..=dot], &line[dot + 2..]))
    } else {
        None
    }
}

fn render_inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut bold = false;
    let mut code = false;
    while !rest.is_empty() {
        if let Some(next) = rest.strip_prefix("**") {
            out.push_str(if bold { RESET } else { BOLD });
            bold = !bold;
            rest = next;
        } else if let Some(next) = rest.strip_prefix('`') {
            out.push_str(if code { RESET } else { MAGENTA });
            code = !code;
            rest = next;
        } else {
            let ch = rest.chars().next().expect("rest is not empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    if bold || code {
        out.push_str(RESET);
    }
    out
}
