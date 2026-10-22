//! Run directories, the trainer's dataset file, and the run manifest.
//!
//! A run is a directory under `<state>/train/`. It holds the dataset the
//! trainer reads, the manifest that describes it, and — after a successful
//! training run — the artifact the trainer wrote. Every file in it is
//! retained, which is what makes a rollback reversible.

use crate::train::dataset::{self, Corpus, Pair, dataset_digest};
use crate::train::digest::{MANIFEST_DOMAIN, digest};
use crate::train::split;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The trainer's dataset file inside a run directory.
pub const DATASET_FILE_NAME: &str = "dataset.jsonl";
/// The manifest file inside a run directory.
pub const MANIFEST_FILE_NAME: &str = "manifest.json";

/// The root of the training state.
#[must_use]
pub fn train_root(state_dir: &Path) -> PathBuf {
    state_dir.join("train")
}

/// A fresh run id: sortable by time and unique per run.
#[must_use]
pub fn new_run_id() -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let unique = uuid::Uuid::new_v4().simple().to_string();
    format!("{stamp}-{}", &unique[..8])
}

/// One pair as the manifest records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPair {
    /// The stable record id.
    pub id: String,
    /// The split the pair belongs to.
    pub split: String,
    /// The redacted prompt.
    pub prompt: String,
    /// The redacted response.
    pub response: String,
}

/// What a prepared run recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The run id.
    pub run_id: String,
    /// The corpus source.
    pub source: String,
    /// When the run was prepared.
    pub created_at: String,
    /// The digest of the corpus the dataset was built from.
    pub dataset_digest: String,
    /// The digest of the consent file that authorised the run.
    pub consent_digest: String,
    /// The split rule that was applied.
    pub split_rule: String,
    /// The training percentage the split rule was applied with.
    pub train_percent: u64,
    /// How many pairs the corpus holds.
    pub total_pairs: usize,
    /// How many pairs went into the trainer's dataset.
    pub train_pairs: usize,
    /// How many pairs were held out of the trainer's dataset.
    pub validation_pairs: usize,
    /// How many pairs the redaction changed.
    pub redacted_pairs: usize,
    /// The pairs themselves, redacted.
    pub pairs: Vec<ManifestPair>,
}

impl Manifest {
    /// The pairs that were held out of the trainer's dataset.
    ///
    /// These are the pairs that supply the probe seeds. The manifest is the
    /// run's own record of the split, so a later evaluation of this run uses
    /// the same held-out pairs the run itself held out.
    #[must_use]
    pub fn validation_pairs(&self) -> Vec<Pair> {
        self.pairs
            .iter()
            .filter(|pair| pair.split == split::Split::Validation.as_str())
            .map(|pair| Pair {
                id: pair.id.clone(),
                prompt: pair.prompt.clone(),
                response: pair.response.clone(),
            })
            .collect()
    }
}

/// Read a prepared run's manifest.
pub fn read_manifest(path: &Path) -> Result<Manifest> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// The files a prepared run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The run directory.
    pub run_dir: PathBuf,
    /// The trainer's dataset.
    pub dataset: PathBuf,
    /// The manifest.
    pub manifest_path: PathBuf,
    /// The manifest itself.
    pub manifest: Manifest,
    /// The digest of the manifest file.
    pub manifest_digest: String,
}

/// Prepare a run: write the dataset and the manifest.
pub fn write_run(
    state_dir: &Path,
    run_id: &str,
    corpus: &Corpus,
    source: &str,
    consent_digest: &str,
) -> Result<Prepared> {
    let run_dir = train_root(state_dir).join(run_id);
    std::fs::create_dir_all(&run_dir).with_context(|| format!("create {}", run_dir.display()))?;
    let dataset = run_dir.join(DATASET_FILE_NAME);
    let manifest_path = run_dir.join(MANIFEST_FILE_NAME);

    let train_pairs = corpus.train(split::TRAIN_PERCENT);
    let validation_pairs = corpus.validation(split::TRAIN_PERCENT);
    let mut rows = String::new();
    for pair in &train_pairs {
        rows.push_str(&dataset_line(pair));
        rows.push('\n');
    }
    std::fs::write(&dataset, rows.as_bytes())
        .with_context(|| format!("write {}", dataset.display()))?;

    let manifest = Manifest {
        run_id: run_id.to_string(),
        source: source.to_string(),
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        dataset_digest: dataset_digest(&corpus.pairs, split::TRAIN_PERCENT),
        consent_digest: consent_digest.to_string(),
        split_rule: split::SPLIT_RULE.to_string(),
        train_percent: split::TRAIN_PERCENT,
        total_pairs: corpus.pairs.len(),
        train_pairs: train_pairs.len(),
        validation_pairs: validation_pairs.len(),
        redacted_pairs: corpus.redacted_pairs,
        pairs: corpus
            .pairs
            .iter()
            .map(|pair| ManifestPair {
                id: pair.id.clone(),
                split: split::assign(&pair.id, split::TRAIN_PERCENT)
                    .as_str()
                    .to_string(),
                prompt: pair.prompt.clone(),
                response: pair.response.clone(),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    std::fs::write(&manifest_path, &bytes)
        .with_context(|| format!("write {}", manifest_path.display()))?;
    let manifest_digest = digest(MANIFEST_DOMAIN, &bytes);

    Ok(Prepared {
        run_dir,
        dataset,
        manifest_path,
        manifest,
        manifest_digest,
    })
}

/// One trainer dataset row.
///
/// The shape is fixed by the trainer's own contract: `abi nn train --field
/// text` reads the `text` field of each JSON object, and the training text of
/// a pair is its prompt followed by its response.
#[must_use]
pub fn dataset_line(pair: &Pair) -> String {
    let text = dataset::pair_text(pair);
    serde_json::json!({ "text": text }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(id: &str) -> Pair {
        Pair {
            id: id.to_string(),
            prompt: "q".into(),
            response: "a".into(),
        }
    }

    #[test]
    fn a_dataset_row_is_the_text_field_the_trainer_reads() {
        let line = dataset_line(&pair("x"));
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["text"], "q\na");
    }

    #[test]
    fn a_prepared_run_writes_the_dataset_and_the_manifest() {
        let dir = std::env::temp_dir().join(format!("abbey-train-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut corpus = Corpus {
            source: Some(crate::train::dataset::Source::Memory),
            ..Corpus::default()
        };
        corpus.pairs.push(pair("memory:1"));
        corpus.pairs.push(pair("memory:2"));
        let prepared = write_run(&dir, "run-1", &corpus, "memory", "sha256:consent").unwrap();
        assert!(prepared.dataset.is_file());
        assert!(prepared.manifest_path.is_file());
        assert_eq!(prepared.manifest.total_pairs, 2);
        assert!(prepared.manifest_digest.starts_with("sha256:"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
