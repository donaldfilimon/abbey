//! Corpus construction: curated memory pairs and Abbey's own specs.
//!
//! Two sources, one shape. Every pair carries a stable id, a prompt, and a
//! response; the id is what the deterministic split keys on, so the same
//! record lands in the same split in every run.
//!
//! ## The memory source
//!
//! The curated `train_candidate` records — the corpus `abbey learn` already
//! curates. A record qualifies when it is a live `train_candidate`, carries
//! non-empty provenance, carries confidence of at least
//! [`READY_CONFIDENCE`] (the bar `abbey learn review` calls "ready"), and its
//! payload deserializes into the prompt/response pair shape that
//! `abbey learn distill` writes. A distilled pair sits at confidence 0.6, so it
//! is *not* in the corpus until a human promotes it.
//!
//! The pair shape is reused, not re-derived: [`crate::distill::DistillPair`]
//! is the exact type `abbey learn distill` stores.

use crate::memory::{MemoryRecord, MemoryStore};
use crate::train::digest::{DATASET_DOMAIN, digest};
use crate::train::redaction::redact;
use crate::train::specs;
use crate::train::split::{self, Split, assign};
use anyhow::Result;
use std::path::Path;

/// The confidence a `train_candidate` must reach to count as curated.
///
/// This is the *same* bar `abbey learn review` calls "ready", re-exported
/// from the learn module that owns `train_candidate` curation: a review and
/// a training run must not disagree about which record is curated.
pub use crate::learn::READY_CONFIDENCE;

/// Where a corpus came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The curated `train_candidate` memory records.
    Memory,
    /// Abbey's own claim registry and goal ledger.
    Specs,
}

impl Source {
    /// Parse a `--source` value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "memory" => Some(Self::Memory),
            "specs" | "spec" => Some(Self::Specs),
            _ => None,
        }
    }

    /// Stable machine label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Specs => "specs",
        }
    }
}

/// One prompt/response pair, already redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// Stable record id. The split keys on this.
    pub id: String,
    /// The prompt.
    pub prompt: String,
    /// The response.
    pub response: String,
}

/// The exact training text of a pair.
///
/// This is the text one pair contributes to the trainer's corpus: the prompt,
/// a newline, and the response. The dataset rows carry the same text, so
/// this is also the text the leak check looks for.
#[must_use]
pub fn pair_text(pair: &Pair) -> String {
    format!("{}\n{}", pair.prompt, pair.response)
}

/// Why a candidate record did not make it into the corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The record was invalidated.
    Obsolete,
    /// The record carries no provenance.
    MissingProvenance,
    /// The record is below the curated confidence bar.
    BelowConfidence,
    /// The payload is not a prompt/response pair.
    NotAPair,
    /// The prompt or the response is blank.
    Blank,
}

impl SkipReason {
    /// Stable machine label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Obsolete => "obsolete",
            Self::MissingProvenance => "missing_provenance",
            Self::BelowConfidence => "below_confidence",
            Self::NotAPair => "not_a_prompt_response_pair",
            Self::Blank => "blank_prompt_or_response",
        }
    }
}

/// A built corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Corpus {
    /// Where the corpus came from.
    pub source: Option<Source>,
    /// The redacted pairs, in stable id order.
    pub pairs: Vec<Pair>,
    /// How many pairs the redaction actually changed.
    pub redacted_pairs: usize,
    /// How many candidates were skipped, per reason.
    pub skipped: Vec<(SkipReason, usize)>,
    /// The specs report, when the specs source was used.
    pub specs: Option<specs::SpecsReport>,
}

impl Corpus {
    /// The pairs that go into the trainer's dataset.
    #[must_use]
    pub fn train(&self, train_percent: u64) -> Vec<&Pair> {
        self.pairs
            .iter()
            .filter(|pair| assign(&pair.id, train_percent) == Split::Train)
            .collect()
    }

    /// The pairs held out of the trainer's dataset.
    #[must_use]
    pub fn validation(&self, train_percent: u64) -> Vec<&Pair> {
        self.pairs
            .iter()
            .filter(|pair| assign(&pair.id, train_percent) == Split::Validation)
            .collect()
    }

    /// How many candidates were skipped for `reason`.
    #[must_use]
    pub fn skipped_count(&self, reason: SkipReason) -> usize {
        self.skipped
            .iter()
            .find(|(r, _)| *r == reason)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }
}

/// The digest of a corpus.
///
/// The digest covers the redacted pairs **in the order they are given** — the
/// corpus builders keep that order canonical, split-grouped and then by id —
/// plus the split rule and the ratio that produced the split labels.
/// Identical corpora therefore produce identical digests, and any difference
/// in content, in order, or in the split rule produces a different one.
#[must_use]
pub fn dataset_digest(pairs: &[Pair], train_percent: u64) -> String {
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(split::SPLIT_RULE.as_bytes());
    payload.push(0u8);
    payload.extend_from_slice(train_percent.to_string().as_bytes());
    payload.push(0u8);
    for pair in pairs {
        let split = assign(&pair.id, train_percent);
        payload.extend_from_slice(split.as_str().as_bytes());
        payload.push(0u8);
        payload.extend_from_slice(pair.id.as_bytes());
        payload.push(0u8);
        payload.extend_from_slice(pair.prompt.as_bytes());
        payload.push(0u8);
        payload.extend_from_slice(pair.response.as_bytes());
        payload.push(0u8);
    }
    digest(DATASET_DOMAIN, &payload)
}

/// Sort pairs into split order: the training split first, then the validation
/// split, each group ordered by id.
pub fn sort_into_split_order(pairs: &mut [Pair], train_percent: u64) {
    pairs.sort_by(|left, right| {
        let left_rank = assign(&left.id, train_percent).rank();
        let right_rank = assign(&right.id, train_percent).rank();
        (left_rank, &left.id).cmp(&(right_rank, &right.id))
    });
}

/// Build the memory corpus from an open store.
pub fn memory_corpus(mem: &dyn MemoryStore) -> Result<Corpus> {
    let mut corpus = Corpus {
        source: Some(Source::Memory),
        ..Corpus::default()
    };
    let mut counts: Vec<(SkipReason, usize)> = Vec::new();
    let bump = |reason: SkipReason, counts: &mut Vec<(SkipReason, usize)>| match counts
        .iter_mut()
        .find(|(r, _)| *r == reason)
    {
        Some((_, n)) => *n += 1,
        None => counts.push((reason, 1)),
    };
    for record in mem.filter(Some("train_candidate"), None, 10_000)? {
        if record.obsolete {
            bump(SkipReason::Obsolete, &mut counts);
            continue;
        }
        if record.provenance.trim().is_empty() {
            bump(SkipReason::MissingProvenance, &mut counts);
            continue;
        }
        if record.confidence < READY_CONFIDENCE {
            bump(SkipReason::BelowConfidence, &mut counts);
            continue;
        }
        let raw = match pair_from_record(&record) {
            Some(pair) => pair,
            None => {
                bump(SkipReason::NotAPair, &mut counts);
                continue;
            }
        };
        if raw.prompt.trim().is_empty() || raw.response.trim().is_empty() {
            bump(SkipReason::Blank, &mut counts);
            continue;
        }
        let prompt = redact(&raw.prompt);
        let response = redact(&raw.response);
        if prompt.changed || response.changed {
            corpus.redacted_pairs += 1;
        }
        corpus.pairs.push(Pair {
            id: raw.id,
            prompt: prompt.text,
            response: response.text,
        });
    }
    corpus.pairs.sort_by(|a, b| a.id.cmp(&b.id));
    sort_into_split_order(&mut corpus.pairs, split::TRAIN_PERCENT);
    corpus.skipped = counts;
    Ok(corpus)
}

/// The prompt/response pair a memory record carries, if it carries one.
///
/// The pair is returned unredacted: the caller redacts it and counts the
/// change, so the "how many pairs were redacted" report is exact.
fn pair_from_record(record: &MemoryRecord) -> Option<Pair> {
    // Reused, not re-derived: this is the exact type `abbey learn distill`
    // writes into the record payload.
    let pair: crate::distill::DistillPair = serde_json::from_str(&record.payload).ok()?;
    Some(Pair {
        id: record.id.clone(),
        prompt: pair.prompt,
        response: pair.response,
    })
}

/// Build the specs corpus from the compiled registry and a goal ledger.
#[must_use]
pub fn specs_corpus(goals_path: Option<&Path>) -> Corpus {
    let (mut pairs, report) = specs::corpus(goals_path);
    sort_into_split_order(&mut pairs, split::TRAIN_PERCENT);
    Corpus {
        source: Some(Source::Specs),
        pairs,
        redacted_pairs: report.redacted_pairs,
        skipped: Vec::new(),
        specs: Some(report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryStore, SqliteMemory};

    #[test]
    fn source_parsing_is_exact() {
        assert_eq!(Source::parse("memory"), Some(Source::Memory));
        assert_eq!(Source::parse("SPECS"), Some(Source::Specs));
        assert_eq!(Source::parse("quesar"), None);
    }

    #[test]
    fn only_ready_pairs_enter_the_memory_corpus() {
        let dir = std::env::temp_dir().join(format!("abbey-train-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory.sqlite");
        let mem: Box<dyn MemoryStore> = Box::new(SqliteMemory::open(&path).unwrap());

        let mut ready = MemoryRecord::new_stm(
            "ready",
            serde_json::to_string(&crate::distill::DistillPair {
                prompt: "q".into(),
                response: "a".into(),
                teacher: "fm:system".into(),
                on_device: true,
            })
            .unwrap(),
        );
        ready.retention = "train_candidate".into();
        ready.provenance = "teacher fm:system @ now".into();
        ready.confidence = 0.95;
        mem.store(ready).unwrap();

        let mut uncurated = MemoryRecord::new_stm(
            "uncurated",
            serde_json::to_string(&crate::distill::DistillPair {
                prompt: "q".into(),
                response: "a".into(),
                teacher: "fm:system".into(),
                on_device: true,
            })
            .unwrap(),
        );
        uncurated.retention = "train_candidate".into();
        uncurated.provenance = "teacher fm:system @ now".into();
        uncurated.confidence = 0.6;
        mem.store(uncurated).unwrap();

        let mut hand = MemoryRecord::new_stm("hand written", "free text");
        hand.retention = "train_candidate".into();
        hand.provenance = "user correction @ now".into();
        hand.confidence = 0.95;
        mem.store(hand).unwrap();

        let corpus = memory_corpus(mem.as_ref()).unwrap();
        assert_eq!(corpus.pairs.len(), 1);
        assert_eq!(corpus.skipped_count(SkipReason::BelowConfidence), 1);
        assert_eq!(corpus.skipped_count(SkipReason::NotAPair), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
