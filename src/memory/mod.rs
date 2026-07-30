//! Replaceable memory backends (SQLite interim; WDBX later).

mod sqlite;
#[cfg(feature = "wdbx")]
mod wdbx;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// A memory's position in Abbey's 3-D map.
///
/// The axes are **deterministic and interpretable**, not a learned embedding
/// space — Abbey has no embedder, and pretending otherwise would make the
/// distances mean something they don't:
///
/// * `x` — topic bucket: a stable hash of the record's primary topic tag, so
///   everything you have taught her about one subject lands in one column.
/// * `y` — recency: log-compressed hours since the record was written, so
///   today spreads out and years compress.
/// * `z` — consolidation: retention depth (activity → stm → ltm →
///   train_candidate) lifted by confidence.
///
/// Nearest-neighbour in this space answers "what else do I know about this
/// subject, from around this time, at this level of consolidation".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl MemoryPoint {
    pub fn distance_to(self, other: Self) -> f32 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
    }
}

/// Retention layers, innermost last — the `z` axis's integer part.
const CONSOLIDATION: [&str; 4] = ["activity", "stm", "ltm", "train_candidate"];

/// Tags that describe storage, not subject — skipped when picking a topic.
const NON_TOPIC_TAGS: [&str; 5] = ["activity", "stm", "ltm", "train_candidate", "self-learn"];

/// Shown for records carrying no subject tag.
pub const UNTAGGED_TOPIC: &str = "(untagged)";

/// The tag Abbey treats as a record's subject.
///
/// Records with no subject tag all share one column rather than being spread
/// by a hash of their wording: scattering them would *look* like topic
/// clustering while actually encoding "which record", which is worse than
/// visibly having no topic. Give a memory a subject with
/// `abbey memory put --tag <subject>`.
pub fn primary_topic(rec: &MemoryRecord) -> &str {
    rec.tags
        .iter()
        .map(String::as_str)
        .find(|t| !NON_TOPIC_TAGS.contains(t))
        .unwrap_or(UNTAGGED_TOPIC)
}

/// FNV-1a — small, stable across runs and platforms (unlike `DefaultHasher`,
/// whose output is explicitly not guaranteed stable).
pub(super) fn stable_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Hours between `timestamp` (`%Y-%m-%dT%H:%M:%SZ`) and now; 0 if unparseable.
fn hours_since(timestamp: &str) -> f64 {
    let Ok(then) = chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%SZ") else {
        return 0.0;
    };
    let now = chrono::Utc::now().naive_utc();
    (now - then).num_seconds().max(0) as f64 / 3600.0
}

/// Place a record in the 3-D map.
pub fn coordinates(rec: &MemoryRecord) -> MemoryPoint {
    let topic = primary_topic(rec);
    // 64 columns: wide enough that unrelated subjects rarely collide, small
    // enough that the map stays legible.
    let x = (stable_hash(topic) % 64) as f32;
    let y = (hours_since(&rec.timestamp) + 1.0).log2() as f32;
    let depth = CONSOLIDATION
        .iter()
        .position(|l| *l == rec.retention)
        .unwrap_or(0) as f32;
    let z = depth + rec.confidence.clamp(0.0, 1.0);
    MemoryPoint { x, y, z }
}

/// Records nearest `target`, closest first, as `(distance, record)`.
pub fn nearest(
    records: &[MemoryRecord],
    target: MemoryPoint,
    limit: usize,
) -> Vec<(f32, &MemoryRecord)> {
    let mut scored: Vec<(f32, &MemoryRecord)> = records
        .iter()
        .map(|r| (coordinates(r).distance_to(target), r))
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    scored.truncate(limit);
    scored
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
    #[allow(dead_code)]
    fn invalidate(&self, id: &str) -> anyhow::Result<()>;
    fn search_keyword(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>>;
    fn filter(
        &self,
        retention: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    fn promote(&self, id: &str, new_retention: &str) -> anyhow::Result<()>;
    #[allow(dead_code)]
    fn supersede(&self, old_id: &str, new_rec: MemoryRecord) -> anyhow::Result<()>;
    fn reflect(&self) -> anyhow::Result<ReflectReport>;
}

#[cfg(test)]
mod map_tests {
    use super::*;

    fn rec(summary: &str, topic: &str, retention: &str, confidence: f32) -> MemoryRecord {
        let mut r = MemoryRecord::new_stm(summary, "body");
        r.tags = vec![retention.into(), topic.into()];
        r.retention = retention.into();
        r.confidence = confidence;
        r
    }

    #[test]
    fn same_topic_shares_a_column_different_topics_usually_do_not() {
        let a = coordinates(&rec("a", "guitar", "ltm", 0.8));
        let b = coordinates(&rec("b", "guitar", "stm", 0.5));
        let c = coordinates(&rec("c", "woodworking", "ltm", 0.8));
        assert_eq!(a.x, b.x, "one subject is one column");
        assert_ne!(a.x, c.x, "different subjects separate");
    }

    #[test]
    fn consolidation_lifts_z_and_confidence_fine_tunes_it() {
        let activity = coordinates(&rec("a", "t", "activity", 0.0));
        let stm = coordinates(&rec("a", "t", "stm", 0.0));
        let ltm = coordinates(&rec("a", "t", "ltm", 0.0));
        let train = coordinates(&rec("a", "t", "train_candidate", 0.0));
        assert!(activity.z < stm.z && stm.z < ltm.z && ltm.z < train.z);

        let unsure = coordinates(&rec("a", "t", "ltm", 0.1));
        let certain = coordinates(&rec("a", "t", "ltm", 0.9));
        assert!(certain.z > unsure.z, "confidence lifts within a layer");
    }

    /// `hours_since` parses a fixed format and silently yields 0.0 on drift, so
    /// the recency axis needs an explicit test with controlled timestamps.
    #[test]
    fn recency_axis_increases_with_age() {
        let stamp = |ago_hours: i64| {
            (chrono::Utc::now() - chrono::Duration::hours(ago_hours))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        };
        let mut now = rec("now", "t", "ltm", 0.5);
        now.timestamp = stamp(0);
        let mut yesterday = rec("yesterday", "t", "ltm", 0.5);
        yesterday.timestamp = stamp(24);
        let mut last_year = rec("last year", "t", "ltm", 0.5);
        last_year.timestamp = stamp(24 * 365);

        let (y0, y1, y2) = (
            coordinates(&now).y,
            coordinates(&yesterday).y,
            coordinates(&last_year).y,
        );
        assert!(
            y0 < y1 && y1 < y2,
            "older memories sit further out: {y0} {y1} {y2}"
        );
        assert!(
            y2 - y1 < y1 - y0 + 10.0,
            "log compression keeps old memories from running away"
        );
    }

    #[test]
    fn unparseable_timestamp_does_not_panic() {
        let mut r = rec("broken", "t", "ltm", 0.5);
        r.timestamp = "not-a-timestamp".into();
        assert_eq!(coordinates(&r).y, 0.0, "drift degrades to 0, not a crash");
    }

    #[test]
    fn placement_is_deterministic_across_calls() {
        let r = rec("stable", "guitar", "ltm", 0.8);
        assert_eq!(coordinates(&r), coordinates(&r));
    }

    #[test]
    fn retention_and_storage_tags_are_not_mistaken_for_a_subject() {
        let mut r = MemoryRecord::new_stm("s", "b");
        r.tags = vec!["stm".into(), "self-learn".into(), "guitar".into()];
        assert_eq!(primary_topic(&r), "guitar");
    }

    #[test]
    fn nearest_returns_closest_first_and_respects_the_limit() {
        let anchor = rec("anchor", "guitar", "ltm", 0.8);
        let same_topic = rec("same", "guitar", "ltm", 0.8);
        let other = rec("other", "woodworking", "activity", 0.1);
        let all = vec![anchor.clone(), same_topic, other];

        let got = nearest(&all, coordinates(&anchor), 2);
        assert_eq!(got.len(), 2, "limit honoured");
        assert!(got[0].0 <= got[1].0, "closest first");
        assert_ne!(
            got[1].1.summary, "other",
            "a same-topic memory outranks an unrelated one"
        );
    }
}

#[cfg(test)]
mod topic_tests {
    use super::*;

    #[test]
    fn untagged_records_are_labelled_not_scattered() {
        let mut a = MemoryRecord::new_stm("plays guitar", "b");
        a.tags = vec!["stm".into()];
        a.retention = "ltm".into();
        let mut b = MemoryRecord::new_stm("builds furniture", "b");
        b.tags = vec!["stm".into()];
        b.retention = "ltm".into();

        assert_eq!(primary_topic(&a), UNTAGGED_TOPIC);
        assert_eq!(
            coordinates(&a).x,
            coordinates(&b).x,
            "untagged memories share one visible column rather than a fake topic spread"
        );

        a.tags.push("guitar".into());
        assert_eq!(primary_topic(&a), "guitar");
        assert_ne!(
            coordinates(&a).x,
            coordinates(&b).x,
            "tagging moves it into its own subject column"
        );
    }
}
