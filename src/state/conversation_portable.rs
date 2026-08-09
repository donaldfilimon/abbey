//! Pre-cutover conversation mirrors for non-Unix hosts.
//!
//! Phase 4B.5's canonical journal requires Unix no-follow, ownership, mode,
//! directory-fsync, and atomic-replace runtime proof. Until Windows receives
//! equivalent DACL/reparse-point/replace evidence, it keeps the exact legacy
//! file behavior and makes no canonicalization claim.

use super::{AbbeyState, HistoryEntry, read_first_line};
use anyhow::{Result, bail};
use chrono::Utc;
use std::fs;
use std::io::Write as _;

pub(super) fn ensure_ready(_state: &AbbeyState) -> Result<()> {
    Ok(())
}

pub(super) fn read_chat(state: &AbbeyState) -> Result<Option<String>> {
    if let Some(id) = read_first_line(&state.active_chat_file()) {
        return Ok(Some(id));
    }
    if state.per_cwd {
        return Ok(read_first_line(&state.chat_file));
    }
    Ok(None)
}

pub(super) fn save_chat(state: &AbbeyState, id: &str) -> Result<()> {
    let id = id.trim();
    if id.is_empty() {
        bail!("empty chat id");
    }
    let active = state.active_chat_file();
    if let Some(parent) = active.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(active, format!("{id}\n"))?;
    fs::write(&state.chat_file, format!("{id}\n"))?;
    fs::write(
        state.chat_file.with_extension("export"),
        format!("ABBEY_CHAT_ID={id}\n"),
    )?;
    let mut history = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.history_file)?;
    writeln!(
        history,
        "{}\t{}\t{}",
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        id,
        state.cwd.display()
    )?;
    Ok(())
}

pub(super) fn clear_legacy_chat(state: &AbbeyState, all: bool) -> Result<()> {
    let _ = fs::remove_file(state.active_chat_file());
    if all || !state.per_cwd {
        let _ = fs::remove_file(&state.chat_file);
        let _ = fs::remove_file(state.chat_file.with_extension("export"));
    }
    if all && let Ok(entries) = fs::read_dir(&state.cwd_dir) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

pub(super) fn history(state: &AbbeyState, count: usize) -> Result<Vec<HistoryEntry>> {
    let text = match fs::read_to_string(&state.history_file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(text
        .lines()
        .rev()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some(HistoryEntry {
                timestamp: parts.next()?.to_owned(),
                chat_id: parts.next()?.to_owned(),
                cwd: parts.next().unwrap_or("").to_owned(),
            })
        })
        .take(count)
        .collect())
}

pub(super) fn compact_history(state: &AbbeyState, keep: usize) -> Result<usize> {
    if !state.history_file.exists() {
        return Ok(0);
    }
    let text = fs::read_to_string(&state.history_file)?;
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let start = lines.len().saturating_sub(keep.max(1));
    let kept = &lines[start..];
    let mut output = kept.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    fs::write(&state.history_file, output)?;
    Ok(kept.len())
}

pub(super) fn write_model(state: &AbbeyState, model: &str) -> Result<()> {
    fs::write(&state.model_file, format!("{model}\n"))?;
    Ok(())
}

pub(super) fn clear_model(state: &AbbeyState) -> Result<()> {
    let _ = fs::remove_file(&state.model_file);
    Ok(())
}
