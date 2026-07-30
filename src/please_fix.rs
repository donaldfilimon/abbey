//! Shared "please fix last failure" prompt construction.

use crate::agent::{MAX_PROMPT_ARGV_BYTES, truncate_utf8_bytes};
use anyhow::{Result, bail};
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

/// Soft ceiling for capture body before wrapping into the please-fix prompt.
const MAX_CAPTURE_BYTES: usize = 24 * 1024;

/// Lines that are agent/TUI chrome, not failure signal.
fn is_noise_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    lower.starts_with("tip: hit shift+tab")
        || lower.contains(" tokens") && (lower.contains("working") || lower.contains("thinking"))
        || matches!(
            t.chars().next(),
            Some('⠀' | '⠠' | '⠰' | '⠘' | '⠸' | '⠌' | '⠐' | '⠈')
        )
        || lower == "working"
        || lower == "thinking"
        || lower.starts_with("loading conversation")
        || lower.starts_with("used ")
        || lower.starts_with("using ")
        || lower.starts_with("reading ")
        || lower.starts_with("running ")
        || lower.starts_with("→ /")
        || lower.starts_with("/model ")
        || lower.starts_with("available models")
        || lower.starts_with("filter:")
        || lower.starts_with("type to filter")
        || lower.starts_with("max mode:")
        || lower.contains("more below")
}

/// Keep error-shaped lines and a short head/tail of the rest.
fn summarize_capture(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut signal: Vec<&str> = Vec::new();
    let mut rest: Vec<&str> = Vec::new();
    for line in &lines {
        if is_noise_line(line) {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let looks_like_signal = lower.contains("error")
            || lower.contains("failed")
            || lower.contains("panic")
            || lower.contains("os error")
            || lower.contains("argument list too long")
            || lower.contains("e2big")
            || lower.contains("exit code")
            || line.trim_start().starts_with("abbey:");
        if looks_like_signal {
            signal.push(line);
        } else {
            rest.push(line);
        }
    }

    let mut out = String::new();
    if !signal.is_empty() {
        out.push_str("## Failure signals\n");
        // Dedup while preserving order; cap count.
        let mut seen = std::collections::HashSet::new();
        let mut n = 0;
        for line in signal {
            let key = line.trim();
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            out.push_str(key);
            out.push('\n');
            n += 1;
            if n >= 40 {
                out.push_str("… [more signal lines omitted]\n");
                break;
            }
        }
        out.push('\n');
    }

    // Short context: first 20 + last 40 non-noise lines.
    if !rest.is_empty() {
        out.push_str("## Context (trimmed)\n");
        let head_n = 20.min(rest.len());
        let tail_n = 40.min(rest.len().saturating_sub(head_n));
        for line in &rest[..head_n] {
            out.push_str(line);
            out.push('\n');
        }
        if head_n + tail_n < rest.len() {
            out.push_str(&format!(
                "… [{} lines omitted] …\n",
                rest.len() - head_n - tail_n
            ));
        }
        if tail_n > 0 {
            for line in &rest[rest.len() - tail_n..] {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    if out.trim().is_empty() {
        // Fall back to raw truncate if everything looked like noise.
        return truncate_utf8_bytes(body, MAX_CAPTURE_BYTES);
    }
    truncate_utf8_bytes(out.trim_end(), MAX_CAPTURE_BYTES)
}

pub fn build_prompt(explicit: &[String]) -> Result<String> {
    if !explicit.is_empty() {
        return Ok(truncate_utf8_bytes(
            &explicit.join(" "),
            MAX_PROMPT_ARGV_BYTES,
        ));
    }
    if let Ok(path) = std::env::var("CURSOR_AGENT_COMPLETED_PATH") {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() >= 2 {
                let cmd = lines[0];
                let code = lines[lines.len() - 1];
                let raw = lines[1..lines.len() - 1].join("\n");
                let out = summarize_capture(&raw);
                eprintln!(
                    "abbey: please-fix capture {} bytes → {} bytes summarized",
                    raw.len(),
                    out.len()
                );
                return Ok(truncate_utf8_bytes(
                    &format!(
                        "I just ran the command: \"{cmd}\", which exited with code {code}. The output was:\n\n{out}\n\nPlease help me fix it."
                    ),
                    MAX_PROMPT_ARGV_BYTES,
                ));
            }
        }
    }
    if !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        if !buf.trim().is_empty() {
            let body = summarize_capture(buf.trim());
            return Ok(truncate_utf8_bytes(
                &format!("Please fix this failure:\n\n{body}"),
                MAX_PROMPT_ARGV_BYTES,
            ));
        }
    }
    if let Some(cmd) = last_shell_command() {
        eprintln!("abbey: using last history command (no Cursor capture found)");
        return Ok(format!(
            "The last shell command was: \"{cmd}\"\n\nIt likely failed. Please diagnose and fix it in this workspace."
        ));
    }
    bail!(
        "please-fix: no failed command captured.\n\
         Pipe an error, or: abbey please-fix <description>"
    )
}

/// Soft variant for TUI (never bails — always returns something runnable).
pub fn build_prompt_soft(explicit: &str) -> String {
    let t = explicit.trim();
    if !t.is_empty() {
        return truncate_utf8_bytes(t, MAX_PROMPT_ARGV_BYTES);
    }
    match build_prompt(&[]) {
        Ok(p) => p,
        Err(_) => "Please help me fix the last failure in this workspace.".into(),
    }
}

fn last_shell_command() -> Option<String> {
    let hist = std::env::var("HISTFILE")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".zsh_history")))?;
    let text = std::fs::read_to_string(hist).ok()?;
    text.lines()
        .rev()
        .map(strip_zsh_hist)
        .find(|l| {
            let t = l.trim();
            !t.is_empty()
                && !t.starts_with("abbey")
                && !t.starts_with("please-fix")
                && !t.starts_with("agent")
        })
        .map(|s| s.to_string())
}

fn strip_zsh_hist(line: &str) -> &str {
    line.strip_prefix(": ")
        .and_then(|s| s.split_once(';').map(|(_, c)| c))
        .unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_keeps_e2big_and_drops_agent_chrome() {
        let body = "\
Working\n\
Tip: Hit shift+tab to enable Plan Mode for large or complex changes.\n\
Thinking  27 tokens\n\
abbey: exec /Users/donaldfilimon/.local/bin/cursor-agent: Argument list too long (os error 7)\n\
% \n\
Available models\n\
Filter: \n\
Auto\n";
        let out = summarize_capture(body);
        assert!(out.contains("Argument list too long"));
        assert!(out.contains("## Failure signals"));
        assert!(!out.contains("Tip: Hit shift+tab"));
        assert!(!out.contains("Thinking  27 tokens"));
        assert!(out.len() < body.len());
    }

    #[test]
    fn summarize_empty_noise_falls_back() {
        let body = "Working\nThinking\nTip: Hit shift+tab\n";
        let out = summarize_capture(body);
        // Fallback may still contain noise, but must not panic and must be capped.
        assert!(out.len() <= MAX_CAPTURE_BYTES);
    }
}
