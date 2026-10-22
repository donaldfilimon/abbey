//! The append-only training ledger and the current-artifact pointer.
//!
//! The ledger is evidence, in the same shape as the route log: one JSON row
//! per operation, appended, never rewritten, never deleted. Every row carries
//! a domain-separated digest of its own content, so a row that was altered
//! after the fact is detectable.
//!
//! Every artifact file is retained. A rollback therefore restores a previous
//! artifact rather than destroying the current one, and the rollback itself is
//! a ledger row — which is what makes a rollback reversible.

use crate::train::digest::{LEDGER_DOMAIN, digest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// The ledger file inside the training state directory.
pub fn ledger_path(state_dir: &Path) -> PathBuf {
    state_dir.join("train").join("ledger.jsonl")
}

/// The current-artifact pointer inside the training state directory.
pub fn current_path(state_dir: &Path) -> PathBuf {
    state_dir.join("train").join("current.json")
}

/// The materialized copy of the current artifact.
pub fn current_artifact_path(state_dir: &Path) -> PathBuf {
    state_dir.join("train").join("current.artifact.json")
}

/// One ledger row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRow {
    /// When the operation happened (UTC, second resolution).
    pub ts: String,
    /// The operation: `prepare`, `run`, or `rollback`.
    pub op: String,
    /// The run this row belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The corpus source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The dataset digest the run was built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_digest: Option<String>,
    /// The path of the prepared trainer dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_path: Option<String>,
    /// The digest of the run manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_digest: Option<String>,
    /// The consent digest that authorised the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent_digest: Option<String>,
    /// The artifact the run produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    /// The artifact's digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    /// The corpus-reproduction score the artifact earned.
    ///
    /// The field is named for what it measures: how faithfully the artifact
    /// reproduces the corpus it was trained on. It is not a held-out quality
    /// score, and rows written before the metric changed carry no value here
    /// rather than a value measured by a different rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reproduction_score: Option<f64>,
    /// The corpus-reproduction score of the artifact that was current before
    /// this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_reproduction_score: Option<f64>,
    /// The metric the scores were measured with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    /// The result of the validation-leak check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_leak: Option<String>,
    /// The decision the run reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// Free-form detail.
    #[serde(default)]
    pub detail: String,
    /// The digest of this row's own content.
    pub row_digest: String,
}

impl LedgerRow {
    /// A row with only the operation and its timestamp filled in.
    #[must_use]
    pub fn new(op: &str) -> Self {
        Self {
            ts: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            op: op.to_string(),
            run_id: None,
            source: None,
            dataset_digest: None,
            dataset_path: None,
            manifest_digest: None,
            consent_digest: None,
            artifact_path: None,
            artifact_digest: None,
            reproduction_score: None,
            baseline_reproduction_score: None,
            metric: None,
            validation_leak: None,
            decision: None,
            detail: String::new(),
            row_digest: String::new(),
        }
    }

    /// The digest of this row's content, with the digest field itself empty.
    #[must_use]
    pub fn content_digest(&self) -> String {
        let mut copy = self.clone();
        copy.row_digest = String::new();
        let bytes = serde_json::to_vec(&copy).unwrap_or_default();
        digest(LEDGER_DOMAIN, &bytes)
    }

    /// Whether the stored digest still matches the row's content.
    #[must_use]
    pub fn digest_matches(&self) -> bool {
        !self.row_digest.is_empty() && self.row_digest == self.content_digest()
    }
}

/// Append a row to the ledger and stamp it with its own digest.
pub fn append(state_dir: &Path, row: &mut LedgerRow) -> Result<()> {
    row.row_digest = row.content_digest();
    let path = ledger_path(state_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    serde_json::to_writer(&mut file, row)?;
    file.write_all(b"\n")?;
    file.flush()?;
    // The row is evidence, so its durability is part of the write.
    file.sync_all()
        .with_context(|| format!("sync {}", path.display()))?;
    Ok(())
}

/// Read every row, oldest first. Malformed lines are skipped, never removed.
pub fn read(state_dir: &Path) -> Result<Vec<LedgerRow>> {
    let path = ledger_path(state_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for line in BufReader::new(File::open(&path)?).lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(row) = serde_json::from_str::<LedgerRow>(&line) {
            rows.push(row);
        }
    }
    Ok(rows)
}

/// The pointer at the artifact that is currently installed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurrentArtifact {
    /// The run that produced the artifact.
    pub run_id: String,
    /// The retained artifact file of that run.
    pub artifact_path: String,
    /// The artifact's digest.
    pub artifact_digest: String,
    /// The corpus-reproduction score the artifact earned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reproduction_score: Option<f64>,
    /// When the pointer was written.
    pub updated_at: String,
}

/// Read the current-artifact pointer.
///
/// A pointer that exists but does not parse is an error, not an absent
/// pointer. Treating a corrupted pointer as "no current artifact" would let a
/// later run promote over the artifact it names, and would silently drop the
/// fact that something is wrong.
pub fn read_current(state_dir: &Path) -> Result<Option<CurrentArtifact>> {
    let path = current_path(state_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let current = serde_json::from_str::<CurrentArtifact>(&text).with_context(|| {
        format!(
            "the current-artifact pointer at {} is unreadable; refusing to treat it as absent",
            path.display()
        )
    })?;
    Ok(Some(current))
}

/// Write the current-artifact pointer.
///
/// The write is atomic: the pointer is written to a temporary file beside it
/// and renamed over the old one, so a crash leaves either the old pointer or
/// the new one — never a half-written file that would read as corrupt.
pub fn write_current(state_dir: &Path, current: &CurrentArtifact) -> Result<()> {
    let path = current_path(state_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(current)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, &bytes).with_context(|| format!("write {}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .with_context(|| format!("publish the current-artifact pointer at {}", path.display()))?;
    Ok(())
}

/// The runs whose artifact was promoted, oldest first.
#[must_use]
pub fn promoted_runs(rows: &[LedgerRow]) -> Vec<&LedgerRow> {
    rows.iter()
        .filter(|row| row.op == "run" && row.decision.as_deref() == Some("promoted"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_carries_its_own_digest_and_it_detects_tampering() {
        let mut row = LedgerRow::new("run");
        row.run_id = Some("run-1".into());
        append_row(&mut row);
        assert!(row.digest_matches());
        let mut tampered = row.clone();
        tampered.detail = "edited".into();
        assert!(!tampered.digest_matches());
    }

    fn append_row(row: &mut LedgerRow) {
        row.row_digest = row.content_digest();
    }

    #[test]
    fn the_ledger_keeps_every_row() {
        let dir = std::env::temp_dir().join(format!("abbey-train-ledger-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for index in 0..3 {
            let mut row = LedgerRow::new("run");
            row.run_id = Some(format!("run-{index}"));
            append(&dir, &mut row).unwrap();
        }
        let rows = read(&dir).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].run_id.as_deref(), Some("run-0"));
        assert!(rows.iter().all(|row| row.digest_matches()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupted_pointer_is_an_error_and_not_an_absent_artifact() {
        let dir = std::env::temp_dir().join(format!("abbey-train-pointer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Nothing written yet: there is no current artifact.
        assert!(read_current(&dir).unwrap().is_none());

        fs::create_dir_all(current_path(&dir).parent().unwrap()).unwrap();
        fs::write(current_path(&dir), "{not json").unwrap();
        // A pointer that exists but does not parse must fail closed: reading
        // it as "absent" would let a later run promote over the artifact it
        // names.
        let error = read_current(&dir).unwrap_err().to_string();
        assert!(error.contains("unreadable"), "{error}");

        // A well-formed pointer round-trips.
        let current = CurrentArtifact {
            run_id: "run-1".into(),
            artifact_path: "/state/train/run-1/artifact.json".into(),
            artifact_digest: "sha256:abc".into(),
            reproduction_score: None,
            updated_at: "2026-10-22T00:00:00Z".into(),
        };
        write_current(&dir, &current).unwrap();
        assert_eq!(read_current(&dir).unwrap(), Some(current));
        let _ = fs::remove_dir_all(&dir);
    }
}
