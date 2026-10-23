//! Offline, deterministic evaluation of a trained artifact.
//!
//! ## What the score is
//!
//! The score is a **training-fidelity** measure: how faithfully the artifact
//! reproduces **the corpus it was trained on**, when it is asked to continue
//! that corpus from a seed character.
//!
//! It is **not** a held-out quality measure, **not** a generalisation
//! measure, and **not** a comparison against a reference answer. The
//! delegated trainer's public surface (`abi nn train`, `abi nn sample`)
//! exposes no loss on unseen text, so a held-out quality metric is not
//! available at all. The corpus-reproduction score is the honest substitute,
//! and it is named for what it is everywhere it is reported.
//!
//! ## How one probe is scored
//!
//! The delegated sampler is a character-level *continuation* model with no
//! prompt conditioning. `abi nn sample --checkpoint … --seed c --n k`:
//!
//! * resolves the seed to the **first occurrence of `c` in the checkpoint's
//!   own corpus**, falling back to corpus index 0 when `c` does not occur;
//! * fills a context window of `seq_len` characters with the corpus text
//!   *starting at* that occurrence; and
//! * greedily emits `k` characters, each one the model's best guess for the
//!   character that follows the window.
//!
//! A faithful artifact therefore continues the corpus at `start + seq_len`,
//! where `start` is the seed's first occurrence. The expectation for a probe
//! is exactly that text — the corpus from `start + seq_len`, wrapped at the
//! end of the corpus the way the trainer's own windows wrap — truncated to
//! the number of characters sampled. The score of one probe is
//! `lcp(sample, expected) / expected.len()`, and the artifact's score is the
//! mean over the probes.
//!
//! ## Where the probes come from
//!
//! The probes are the first character of each **held-out** pair's prompt,
//! deduplicated and capped at [`MAX_EVALUATIONS`]. The held-out split
//! therefore chooses *where* the artifact is probed; the corpus it is
//! compared against is the corpus the artifact was trained on. The holdout is
//! what keeps the probe positions independent of the training text, and
//! [`leak_check`] is what keeps that true: a validation pair whose text
//! appears in the training corpus is a leak, and a leak is never promoted.
//!
//! ## Failing safe
//!
//! The expectation assumes the sampler resolves the seed and the context
//! window exactly as described above. If the sibling changes either, the
//! sample stops matching the expectation and the score falls to zero. There
//! is no tolerance and no substitute metric: a score of zero is reported as
//! zero.
//!
//! Nothing here reaches the network, and the sampler is greedy with a fixed
//! seed, so the same artifact and the same corpus always produce the same
//! score.

use crate::train::dataset::Pair;
use crate::train::trainer::run_sample;
use anyhow::Result;
use std::path::Path;
use std::time::Duration;

/// The identifier of the metric, recorded in every ledger row that carries a
/// score.
pub const METRIC: &str = "abbey.train.metric.corpus-reproduction.v1";

/// How many probes one evaluation scores.
pub const MAX_EVALUATIONS: usize = 32;

/// How many characters one probe samples.
pub const PROBE_LENGTH: usize = 64;

/// The context window assumed when the artifact does not declare one.
///
/// The delegated trainer's default configuration uses a two-character context
/// window, and the artifact it writes records its own `seq_len`. A wrong
/// assumption cannot inflate a score: the sample would stop matching the
/// expectation and the score would fall to zero.
pub const DEFAULT_CONTEXT_LENGTH: usize = 2;

/// The promotion floor.
///
/// A score of exactly `0.0` means the artifact reproduced no prefix of the
/// corpus at all — no evidence that it learned anything — so promotion
/// requires a score **above** this floor. The baseline comparison
/// (`score >= baseline`) is the gate that actually protects the current
/// artifact; this floor only rejects a total absence of signal.
pub const MINIMUM_PROMOTION_SCORE: f64 = 0.0;

/// One evaluation result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// The mean corpus-reproduction score in `[0, 1]`.
    pub score: f64,
    /// How many probes were scored.
    pub probes: usize,
    /// The context window the expectation was built with.
    pub context_length: usize,
}

/// What the pipeline decided about a trained artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The artifact is better than (or equal to) the baseline and clears the
    /// floor. It becomes the current artifact.
    Promoted,
    /// The artifact scores below the previous current artifact. The previous
    /// artifact is kept.
    Regression,
    /// The artifact matches the baseline but does not clear the floor. The
    /// previous artifact is kept.
    BelowMinimum,
    /// There is no previous artifact and the artifact does not clear the
    /// floor. Nothing is promoted.
    TrainedNotPromoted,
    /// There was nothing to score (no probe). Nothing is promoted.
    NotEvaluated,
    /// A held-out pair was found inside the training corpus. The holdout is
    /// not a holdout, so nothing is promoted.
    ValidationLeak,
}

impl Decision {
    /// Stable machine label used in the ledger.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Promoted => "promoted",
            Self::Regression => "regression",
            Self::BelowMinimum => "below_minimum",
            Self::TrainedNotPromoted => "trained_not_promoted",
            Self::NotEvaluated => "not_evaluated",
            Self::ValidationLeak => "validation_leak",
        }
    }

    /// Human label used in printed output.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Promoted => "promoted",
            Self::Regression => "regression",
            Self::BelowMinimum => "below minimum",
            Self::TrainedNotPromoted => "trained, not promoted",
            Self::NotEvaluated => "not evaluated",
            Self::ValidationLeak => "validation leak",
        }
    }

    /// Whether the decision installs the new artifact as the current one.
    #[must_use]
    pub fn promotes(self) -> bool {
        matches!(self, Self::Promoted)
    }
}

/// Decide what happens to a trained artifact.
///
/// The baseline comparison comes first: an artifact that scores below the one
/// that is already installed is a regression, whatever the floor says. The
/// floor then rejects an artifact that shows no evidence at all.
///
/// A leaked holdout is decided by the caller, before this function is
/// consulted: a validation pair inside the training corpus is never promoted,
/// whatever it scores.
#[must_use]
pub fn decide(score: Option<f64>, baseline: Option<f64>) -> Decision {
    let Some(score) = score else {
        return Decision::NotEvaluated;
    };
    match baseline {
        Some(baseline) => {
            if score < baseline {
                return Decision::Regression;
            }
            if score <= MINIMUM_PROMOTION_SCORE {
                return Decision::BelowMinimum;
            }
            Decision::Promoted
        }
        None => {
            if score <= MINIMUM_PROMOTION_SCORE {
                return Decision::TrainedNotPromoted;
            }
            Decision::Promoted
        }
    }
}

/// The probe seeds: the first character of each held-out pair's prompt.
///
/// The seeds are deduplicated and capped at [`MAX_EVALUATIONS`]. A pair with
/// a blank prompt contributes no seed.
#[must_use]
pub fn probe_seeds(pairs: &[&Pair]) -> Vec<char> {
    let mut seeds: Vec<char> = Vec::new();
    for pair in pairs {
        let Some(first) = pair.prompt.chars().next() else {
            continue;
        };
        if seeds.contains(&first) {
            continue;
        }
        seeds.push(first);
        if seeds.len() >= MAX_EVALUATIONS {
            break;
        }
    }
    seeds
}

/// Score an artifact for how faithfully it reproduces its own training
/// corpus.
///
/// `corpus` is the text the artifact was trained on — see
/// [`crate::train::trainer::read_corpus`] — and `seeds` are the probe seed
/// characters (see [`probe_seeds`]). The score is the mean over the probes
/// of the normalised longest common prefix between the sample and the corpus
/// continuation the artifact was asked to produce.
pub fn score_artifact(
    binary: &Path,
    artifact: &Path,
    corpus: &str,
    seeds: &[char],
    timeout: Duration,
) -> Result<Score> {
    anyhow::ensure!(
        artifact.is_file(),
        "artifact is missing: {}",
        artifact.display()
    );
    let corpus_bytes = corpus.as_bytes();
    let context = context_length(artifact);
    if corpus_bytes.is_empty() {
        return Ok(Score {
            score: 0.0,
            probes: 0,
            context_length: context,
        });
    }
    let mut total = 0.0f64;
    let mut probes = 0usize;
    let mut seen: Vec<char> = Vec::new();
    for seed in seeds.iter().take(MAX_EVALUATIONS) {
        if seen.contains(seed) {
            continue;
        }
        seen.push(*seed);
        // The seed resolves to the first occurrence of its first byte in the
        // corpus, exactly as the sibling's sampler resolves it in the
        // checkpoint's own corpus.
        let start = corpus_bytes
            .iter()
            .position(|byte| *byte == seed_byte(*seed))
            .unwrap_or(0);
        let expected_bytes =
            wrapped_slice(corpus_bytes, start.saturating_add(context), PROBE_LENGTH);
        let expected = String::from_utf8_lossy(&expected_bytes);
        let expected_chars = expected.chars().count();
        if expected_chars == 0 {
            continue;
        }
        let sample = run_sample(binary, artifact, *seed, PROBE_LENGTH, timeout)?;
        let prefix = common_prefix_len(&sample, &expected);
        total += prefix as f64 / expected_chars as f64;
        probes += 1;
    }
    if probes == 0 {
        return Ok(Score {
            score: 0.0,
            probes: 0,
            context_length: context,
        });
    }
    Ok(Score {
        score: total / probes as f64,
        probes,
        context_length: context,
    })
}

/// The model's context window, as the artifact declares it.
///
/// The delegated trainer writes a JSON checkpoint whose `seq_len` is the
/// model's context length, and that checkpoint is what the sampler loads. An
/// artifact that is not such a checkpoint — a fixture, or a trainer with
/// another format — falls back to [`DEFAULT_CONTEXT_LENGTH`]. A wrong guess
/// cannot inflate a score: the sample would stop matching the expectation,
/// and the score would fall to zero.
#[must_use]
pub fn context_length(artifact: &Path) -> usize {
    let Ok(bytes) = std::fs::read(artifact) else {
        return DEFAULT_CONTEXT_LENGTH;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return DEFAULT_CONTEXT_LENGTH;
    };
    value
        .get("seq_len")
        .and_then(serde_json::Value::as_u64)
        .and_then(|length| usize::try_from(length).ok())
        .filter(|length| *length > 0)
        .unwrap_or(DEFAULT_CONTEXT_LENGTH)
}

/// The result of the validation-leak check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeakReport {
    /// How many validation pairs were checked.
    pub checked: usize,
    /// The ids of the validation pairs whose exact text is in the corpus.
    pub leaked: Vec<String>,
}

impl LeakReport {
    /// Whether the holdout is clean.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.leaked.is_empty()
    }

    /// A bounded summary, for the ledger and the printed output.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.leaked.is_empty() {
            return "none".to_string();
        }
        format!(
            "{} validation pair(s) present in the training corpus",
            self.leaked.len()
        )
    }
}

/// Check that no held-out pair's text is inside the training corpus.
///
/// The delegated trainer cannot score unseen text, so the holdout is not a
/// scoring holdout: it is the *probe* holdout, and it earns that keep only
/// while the validation text is absent from the corpus the artifact was
/// trained on. A pair's exact text is its prompt, a newline, and its
/// response — the same text the trainer's dataset carries.
#[must_use]
pub fn leak_check(corpus: &str, validation: &[&Pair]) -> LeakReport {
    let mut report = LeakReport {
        checked: validation.len(),
        leaked: Vec::new(),
    };
    for pair in validation {
        let text = crate::train::dataset::pair_text(pair);
        if text.is_empty() {
            continue;
        }
        if corpus.contains(&text) {
            report.leaked.push(pair.id.clone());
        }
    }
    report
}

/// The longest common prefix length of two strings, in characters.
#[must_use]
pub fn common_prefix_len(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .count()
}

/// The first byte of a seed character, the way the sibling's CLI reads it.
fn seed_byte(seed: char) -> u8 {
    let mut bytes = [0u8; 4];
    seed.encode_utf8(&mut bytes).as_bytes()[0]
}

/// `len` bytes of `bytes`, starting at `start` and wrapping at the end.
///
/// The trainer's own windows wrap at the end of the corpus, so a faithful
/// artifact continues the corpus past its end; the expectation wraps the same
/// way.
fn wrapped_slice(bytes: &[u8], start: usize, len: usize) -> Vec<u8> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let base = start % bytes.len();
    (0..len)
        .map(|offset| bytes[(base + offset) % bytes.len()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_prefix_is_counted_in_characters() {
        assert_eq!(common_prefix_len("hello", "help"), 3);
        assert_eq!(common_prefix_len("", "help"), 0);
        assert_eq!(common_prefix_len("help", "help"), 4);
        assert_eq!(common_prefix_len("héllo", "hél"), 3);
    }

    #[test]
    fn the_decision_follows_the_baseline_and_the_floor() {
        assert_eq!(decide(Some(0.9), Some(0.5)), Decision::Promoted);
        assert_eq!(decide(Some(0.5), Some(0.5)), Decision::Promoted);
        assert_eq!(decide(Some(0.2), Some(0.5)), Decision::Regression);
        assert_eq!(decide(Some(0.0), Some(0.5)), Decision::Regression);
        assert_eq!(decide(Some(0.0), Some(0.0)), Decision::BelowMinimum);
        assert_eq!(decide(Some(0.0), None), Decision::TrainedNotPromoted);
        assert_eq!(decide(Some(0.7), None), Decision::Promoted);
        assert_eq!(decide(None, Some(0.5)), Decision::NotEvaluated);
    }

    #[test]
    fn a_leak_is_never_promoted() {
        assert!(!Decision::ValidationLeak.promotes());
        assert_eq!(Decision::ValidationLeak.as_str(), "validation_leak");
    }

    #[test]
    fn the_probe_seeds_are_the_first_character_of_each_prompt() {
        let pairs = [
            Pair {
                id: "a".into(),
                prompt: "alpha".into(),
                response: "one".into(),
            },
            Pair {
                id: "b".into(),
                prompt: "again".into(),
                response: "two".into(),
            },
            Pair {
                id: "c".into(),
                prompt: "bravo".into(),
                response: "three".into(),
            },
            Pair {
                id: "d".into(),
                prompt: String::new(),
                response: "four".into(),
            },
        ];
        let refs: Vec<&Pair> = pairs.iter().collect();
        assert_eq!(probe_seeds(&refs), vec!['a', 'b']);
    }

    #[test]
    fn a_leaked_validation_pair_is_found() {
        let pairs = [
            Pair {
                id: "leaky".into(),
                prompt: "q".into(),
                response: "a".into(),
            },
            Pair {
                id: "clean".into(),
                prompt: "other".into(),
                response: "answer".into(),
            },
        ];
        let refs: Vec<&Pair> = pairs.iter().collect();
        let report = leak_check("q\na\nsecond\nrow", &refs);
        assert!(!report.is_clean());
        assert_eq!(report.leaked, vec!["leaky".to_string()]);
        assert_eq!(report.checked, 2);
        let clean = leak_check("second\nrow", &refs);
        assert!(clean.is_clean());
        assert_eq!(clean.summary(), "none");
    }

    #[test]
    fn the_wrapped_slice_wraps_at_the_end_of_the_corpus() {
        let bytes = b"abc";
        assert_eq!(wrapped_slice(bytes, 0, 5), b"abcab");
        assert_eq!(wrapped_slice(bytes, 2, 3), b"cab");
        assert_eq!(wrapped_slice(bytes, 0, 0), b"");
    }
}
