//! Replaceable memory backends (SQLite interim; WDBX later).

pub mod map;
pub mod similarity;
mod sqlite;
#[cfg(feature = "wdbx")]
mod wdbx;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use map::{coordinates, nearest_to, primary_topic};
pub use similarity::{similar_to_id, similar_to_text};
pub use sqlite::SqliteMemory;
#[cfg(feature = "wdbx")]
pub use wdbx::{WdbxMemory, lock_store_dir};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub source_type: String,
    pub source_ref: String,
    pub timestamp: String,
    pub origin: String,
    pub payload: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub embedding_ref: Option<String>,
    pub confidence: f32,
    pub provenance: String,
    /// stm | ltm | activity | train_candidate
    pub retention: String,
    pub supersedes: Option<String>,
    #[serde(default = "default_classification")]
    pub classification: String,
    #[serde(default)]
    pub obsolete: bool,
}

fn default_classification() -> String {
    "internal".into()
}

impl MemoryRecord {
    pub fn new_stm(summary: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source_type: "session".into(),
            source_ref: String::new(),
            timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            origin: "system".into(),
            payload: payload.into(),
            summary: summary.into(),
            tags: vec!["stm".into()],
            embedding_ref: None,
            confidence: 0.7,
            provenance: "abbey session".into(),
            retention: "stm".into(),
            supersedes: None,
            classification: "internal".into(),
            obsolete: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReflectReport {
    pub duplicate_summaries: Vec<(String, String)>,
    pub low_confidence: Vec<String>,
    pub superseded: Vec<String>,
}

/// Open the backend named by config/`ABBEY_MEMORY_BACKEND`.
///
/// `wdbx` is only selectable when the crate was built with `--features wdbx`;
/// otherwise it falls back to SQLite rather than failing a session, and
/// [`backend_status`] reports why.
pub fn open_backend(state_dir: &Path, backend: &str) -> anyhow::Result<Box<dyn MemoryStore>> {
    open_backend_with_timeout(state_dir, backend, DEFAULT_LOCK_TIMEOUT)
}

/// How long a WDBX open waits for another process's lock before giving up.
pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// Interactive read-only callers (the TUI redraw) must not stall for ten seconds
/// on a lock, so they pass something short and report the failure instead.
pub fn open_backend_with_timeout(
    state_dir: &Path,
    backend: &str,
    timeout: Duration,
) -> anyhow::Result<Box<dyn MemoryStore>> {
    let _ = timeout; // only the wdbx backend can block
    match resolved_backend(backend) {
        Backend::Sqlite => Ok(Box::new(SqliteMemory::open(
            &SqliteMemory::path_for_state_dir(state_dir),
        )?)),
        #[cfg(feature = "wdbx")]
        Backend::Wdbx => Ok(Box::new(WdbxMemory::open_with_timeout(
            &WdbxMemory::path_for_state_dir(state_dir),
            timeout,
        )?)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Sqlite,
    #[cfg(feature = "wdbx")]
    Wdbx,
}

fn resolved_backend(requested: &str) -> Backend {
    match requested.trim().to_ascii_lowercase().as_str() {
        #[cfg(feature = "wdbx")]
        "wdbx" => Backend::Wdbx,
        _ => Backend::Sqlite,
    }
}

/// Whether this binary was compiled with the in-process WDBX backend linked in.
pub fn feature_status() -> String {
    if cfg!(feature = "wdbx") {
        "wdbx:       in-process abi-wdbx linked (feature `wdbx` on)".into()
    } else {
        "wdbx:       in-process backend NOT linked (rebuild with `--features wdbx`)".into()
    }
}

/// Where the configured backend keeps its store — a file for SQLite, a
/// directory for WDBX. Pure: it never creates anything, so read-only callers
/// can test `.exists()` before opening.
pub fn backend_path(state_dir: &Path, backend: &str) -> PathBuf {
    match resolved_backend(backend) {
        #[cfg(feature = "wdbx")]
        Backend::Wdbx => WdbxMemory::path_for_state_dir(state_dir),
        Backend::Sqlite => SqliteMemory::path_for_state_dir(state_dir),
    }
}

/// One honest line describing which backend a run will actually use.
pub fn backend_status(state_dir: &Path, backend: &str) -> String {
    let requested = backend.trim().to_ascii_lowercase();
    let path = backend_path(state_dir, &requested);
    let present = if path.exists() {
        "present"
    } else {
        "will create on first write"
    };
    match resolved_backend(&requested) {
        #[cfg(feature = "wdbx")]
        Backend::Wdbx => format!(
            "memory:     wdbx {} ({present}, in-process abi-wdbx)",
            path.display()
        ),
        Backend::Sqlite => {
            let note = if requested == "wdbx" {
                " [requested wdbx — binary built without `--features wdbx`]"
            } else {
                ""
            };
            format!("memory:     sqlite {} ({present}){note}", path.display())
        }
    }
}

/// First `n` **chars** of `s` (never slices mid-codepoint).
fn char_prefix(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Shared reflection pass so every backend reports duplicates / low confidence /
/// superseded identically. Never deletes — it only reports.
pub fn reflect_over(all: &[MemoryRecord]) -> ReflectReport {
    let mut report = ReflectReport::default();
    for r in all {
        if r.confidence < 0.4 {
            report.low_confidence.push(r.id.clone());
        }
        if r.supersedes.is_some() || r.obsolete {
            report.superseded.push(r.id.clone());
        }
    }
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            let a = char_prefix(&all[i].summary, 24);
            let b = char_prefix(&all[j].summary, 24);
            if a.chars().count() >= 12 && a == b {
                report
                    .duplicate_summaries
                    .push((all[i].id.clone(), all[j].id.clone()));
            }
        }
    }
    report
}

/// Reject `train_candidate` records without provenance, on every backend.
pub fn validate_train(rec: &MemoryRecord) -> anyhow::Result<()> {
    if rec.retention == "train_candidate" && rec.provenance.trim().is_empty() {
        anyhow::bail!("train_candidate requires non-empty provenance");
    }
    Ok(())
}

pub trait MemoryStore {
    fn store(&self, rec: MemoryRecord) -> anyhow::Result<()>;
    fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>>;
    fn update(&self, rec: MemoryRecord) -> anyhow::Result<()>;
    /// Mark obsolete — never deletes (see the no-silent-deletes rule).
    fn invalidate(&self, id: &str) -> anyhow::Result<()>;
    fn search_keyword(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>>;
    fn filter(
        &self,
        retention: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    fn promote(&self, id: &str, new_retention: &str) -> anyhow::Result<()>;
    /// Store `new_rec` and mark `old_id` obsolete, preserving both.
    fn supersede(&self, old_id: &str, new_rec: MemoryRecord) -> anyhow::Result<()>;
    fn reflect(&self) -> anyhow::Result<ReflectReport>;
}
