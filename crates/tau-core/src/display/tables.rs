use unicode_width::UnicodeWidthStr;

use super::ansi::{BOLD, CYAN, RESET};
use super::markdown::strip_inline_markers;

pub fn is_table_separator(line: &str) -> bool {
    line.starts_with('|')
        && line.ends_with('|')
        && line
            .trim_matches('|')
            .chars()
            .all(|ch| ch == '-' || ch == ':' || ch == '|' || ch.is_whitespace())
}

pub fn is_table_row(line: &str) -> bool {
    line.starts_with('|') && line.ends_with('|') && line.matches('|').count() >= 2
}

pub fn format_table(lines: &[&str]) -> String {
    let rows = lines
        .iter()
        .filter(|line| !is_table_separator(line.trim()))
        .map(|line| parse_table_row(line))
        .collect::<Vec<_>>();
    let Some(header) = rows.first() else {
        return String::new();
    };
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths = (0..column_count)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| UnicodeWidthStr::width(cell.as_str()))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let border = table_border("├", "┼", "┤", &widths);
    let mut out = String::new();
    out.push_str(&table_border("┌", "┬", "┐", &widths));
    out.push('\n');
    out.push_str(&table_row(header, &widths, true));
    out.push('\n');
    out.push_str(&border);
    out.push('\n');
    for row in rows.iter().skip(1) {
        out.push_str(&table_row(row, &widths, false));
        out.push('\n');
        out.push_str(&border);
        out.push('\n');
    }
    if rows.len() > 1 {
        let remove_len = border.len() + 1;
        out.truncate(out.len().saturating_sub(remove_len));
    }
    out.push_str(&table_border("└", "┴", "┘", &widths));
    out.push('\n');
    out
}

fn parse_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| strip_inline_markers(cell.trim()))
        .collect()
}

fn table_border(left: &str, middle: &str, right: &str, widths: &[usize]) -> String {
    let spans = widths
        .iter()
        .map(|width| "─".repeat(width + 2))
        .collect::<Vec<_>>();
    format!("{CYAN}{left}{}{right}{RESET}", spans.join(middle))
}

fn table_row(row: &[String], widths: &[usize], header: bool) -> String {
    let mut out = format!("{CYAN}│{RESET}");
    for (index, width) in widths.iter().enumerate() {
        let cell = row.get(index).map_or("", String::as_str);
        let padding = width.saturating_sub(UnicodeWidthStr::width(cell));
        let padded = format!(" {cell}{} ", " ".repeat(padding));
        if header {
            out.push_str(&format!("{BOLD}{padded}{RESET}{CYAN}│{RESET}"));
        } else {
            out.push_str(&format!("{padded}{CYAN}│{RESET}"));
        }
    }
    out
}
