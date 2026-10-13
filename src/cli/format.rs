//! Terminal formatting shared by the command handlers.

use std::io::IsTerminal;
use std::sync::OnceLock;

pub fn bold(s: &str) -> String {
    paint("1", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn accent(s: &str) -> String {
    paint("38;5;214", s)
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}

/// `s` in the colour or weight `code` when stdout shows colours, else as it
/// is: `doctor > out.txt` wrote escape codes into the file.
fn paint(code: &str, s: &str) -> String {
    static ON: OnceLock<bool> = OnceLock::new();
    painted(*ON.get_or_init(|| colours(std::io::stdout().is_terminal())), code, s)
}

/// Whether to colour output that goes to a terminal (`terminal`) or not:
/// only a terminal, and not when `NO_COLOR` is set to anything (no-color.org).
pub(crate) fn colours(terminal: bool) -> bool {
    terminal && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

fn painted(on: bool, code: &str, s: &str) -> String {
    if on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
}

pub fn hours(mins: i64) -> String {
    if mins == 0 {
        "-".into()
    } else if mins < 60 {
        format!("{mins}m")
    } else {
        format!("{:.1}h", mins as f64 / 60.0)
    }
}

pub fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

pub fn textwrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut cur = String::new();
    for w in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_only_when_asked_for() {
        assert_eq!(painted(true, "31", "no"), "\x1b[31mno\x1b[0m");
        assert_eq!(painted(false, "31", "no"), "no");
        assert!(!colours(false), "never into a file or a pipe");
    }
}
