//! Shared "please fix last failure" prompt construction.

use crate::agent::{MAX_PROMPT_ARGV_BYTES, truncate_utf8_bytes};
use anyhow::{Result, bail};
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

/// Captured command output ceiling (fits under argv clamp with room for the wrapper).
const MAX_CAPTURE_BYTES: usize = MAX_PROMPT_ARGV_BYTES.saturating_sub(512);

pub fn build_prompt(explicit: &[String]) -> Result<String> {
    if !explicit.is_empty() {
        return Ok(truncate_utf8_bytes(
            &explicit.join(" "),
            MAX_PROMPT_ARGV_BYTES,
        ));
    }
    if let Ok(path) = std::env::var("CURSOR_AGENT_COMPLETED_PATH") {
        if let Ok(text) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() >= 2 {
                let cmd = lines[0];
                let code = lines[lines.len() - 1];
                let out =
                    truncate_utf8_bytes(&lines[1..lines.len() - 1].join("\n"), MAX_CAPTURE_BYTES);
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
            let body = truncate_utf8_bytes(buf.trim(), MAX_CAPTURE_BYTES);
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
