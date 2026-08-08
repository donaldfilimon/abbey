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
pub use similarity::{similar_to_id_filtered, similar_to_text, similar_to_text_filtered};
pub use sqlite::SqliteMemory;
#[cfg(feature = "wdbx")]
pub use wdbx::{WdbxMemory, lock_store_dir};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub source_type: String,
    pub source_ref: String,
    /// Canonical project root associated with this memory, when known.
    #[serde(default)]
    pub project: String,
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
            project: current_project(),
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

/// Backend-neutral memory query constraints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryFilter {
    pub retention: Option<String>,
    pub tag: Option<String>,
    pub source_type: Option<String>,
    pub source_ref: Option<String>,
    pub project: Option<String>,
    /// Inclusive lower RFC 3339 boundary, normalized to UTC.
    pub since: Option<String>,
    /// Inclusive upper RFC 3339 boundary, normalized to UTC.
    pub until: Option<String>,
}

impl MemoryFilter {
    /// Validate and normalize one RFC 3339 timestamp to UTC.
    pub fn normalize_timestamp(raw: &str) -> anyhow::Result<String> {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|time| {
                time.with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
            })
            .map_err(|error| anyhow::anyhow!("invalid RFC 3339 timestamp {raw:?}: {error}"))
    }

    /// Parse and validate timestamp boundaries before a backend is queried.
    pub fn new(
        retention: Option<String>,
        tag: Option<String>,
        source_type: Option<String>,
        source_ref: Option<String>,
        project: Option<String>,
        since: Option<String>,
        until: Option<String>,
    ) -> anyhow::Result<Self> {
        let since = normalize_bound(since, "since")?;
        let until = normalize_bound(until, "until")?;
        if since
            .as_ref()
            .zip(until.as_ref())
            .is_some_and(|(a, b)| a > b)
        {
            anyhow::bail!("memory filter --since must not be later than --until");
        }
        Ok(Self {
            retention,
            tag,
            source_type,
            source_ref,
            project,
            since,
            until,
        })
    }

    #[must_use]
    pub fn matches(&self, record: &MemoryRecord) -> bool {
        self.retention
            .as_deref()
            .is_none_or(|value| record.retention == value)
            && self
                .tag
                .as_deref()
                .is_none_or(|value| record.tags.iter().any(|tag| tag == value))
            && self
                .source_type
                .as_deref()
                .is_none_or(|value| record.source_type == value)
            && self
                .source_ref
                .as_deref()
                .is_none_or(|value| record.source_ref == value)
            && self
                .project
                .as_deref()
                .is_none_or(|value| record.project == value)
            && self.timestamp_matches(record)
    }

    fn timestamp_matches(&self, record: &MemoryRecord) -> bool {
        if self.since.is_none() && self.until.is_none() {
            return true;
        }
        let Ok(recorded) = chrono::DateTime::parse_from_rfc3339(&record.timestamp) else {
            return false;
        };
        let since_ok = self.since.as_deref().is_none_or(|raw| {
            chrono::DateTime::parse_from_rfc3339(raw).is_ok_and(|since| recorded >= since)
        });
        let until_ok = self.until.as_deref().is_none_or(|raw| {
            chrono::DateTime::parse_from_rfc3339(raw).is_ok_and(|until| recorded <= until)
        });
        since_ok && until_ok
    }
}

fn normalize_bound(value: Option<String>, label: &str) -> anyhow::Result<Option<String>> {
    value
        .map(|raw| {
            MemoryFilter::normalize_timestamp(&raw).map_err(|error| {
                anyhow::anyhow!("invalid --{label} RFC 3339 timestamp {raw:?}: {error}")
            })
        })
        .transpose()
}

fn current_project() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .output();
    output
        .ok()
        .filter(|result| result.status.success())
        .and_then(|result| String::from_utf8(result.stdout).ok())
        .map(|root| root.trim().to_string())
        .filter(|root| !root.is_empty())
        .unwrap_or_else(|| cwd.canonicalize().unwrap_or(cwd).display().to_string())
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
    fn search_keyword_with(
        &self,
        query: &str,
        filter: &MemoryFilter,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        let needle = query.to_ascii_lowercase();
        Ok(self
            .filter_with(filter, 1000)?
            .into_iter()
            .filter(|record| {
                record.summary.to_ascii_lowercase().contains(&needle)
                    || record.payload.to_ascii_lowercase().contains(&needle)
                    || record.provenance.to_ascii_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }
    fn filter(
        &self,
        retention: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        self.filter_with(
            &MemoryFilter {
                retention: retention.map(str::to_string),
                tag: tag.map(str::to_string),
                ..MemoryFilter::default()
            },
            limit,
        )
    }
    fn filter_with(&self, filter: &MemoryFilter, limit: usize)
    -> anyhow::Result<Vec<MemoryRecord>>;
    fn promote(&self, id: &str, new_retention: &str) -> anyhow::Result<()>;
    /// Store `new_rec` and mark `old_id` obsolete, preserving both.
    fn supersede(&self, old_id: &str, new_rec: MemoryRecord) -> anyhow::Result<()>;
    fn reflect(&self) -> anyhow::Result<ReflectReport>;
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    #[test]
    fn bounds_are_inclusive_and_all_metadata_dimensions_are_exact() {
        let mut record = MemoryRecord::new_stm("summary", "payload");
        record.timestamp = "2026-08-08T12:00:00Z".into();
        record.retention = "ltm".into();
        record.tags.push("preference".into());
        record.source_type = "route".into();
        record.source_ref = "route-7".into();
        record.project = "/project/abbey".into();
        let filter = MemoryFilter::new(
            Some("ltm".into()),
            Some("preference".into()),
            Some("route".into()),
            Some("route-7".into()),
            Some("/project/abbey".into()),
            Some("2026-08-08T08:00:00-04:00".into()),
            Some("2026-08-08T12:00:00Z".into()),
        )
        .unwrap();
        assert!(filter.matches(&record));
        record.timestamp = "not-a-timestamp".into();
        assert!(!filter.matches(&record));
    }

    #[test]
    fn malformed_or_reversed_bounds_fail_instead_of_broadening() {
        assert!(
            MemoryFilter::new(None, None, None, None, None, Some("yesterday".into()), None)
                .is_err()
        );
        assert!(
            MemoryFilter::new(
                None,
                None,
                None,
                None,
                None,
                Some("2026-08-09T00:00:00Z".into()),
                Some("2026-08-08T00:00:00Z".into()),
            )
            .is_err()
        );
    }
}
