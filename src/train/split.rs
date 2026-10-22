//! Deterministic, RNG-free train/validation split.
//!
//! The assignment of a record to the training or validation split is a pure
//! function of the record id and the ratio, so it is stable forever: the same
//! record lands in the same split in every run, on every host, with no RNG
//! seed to record and no ordering dependency.
//!
//! The rule is fixed by specification:
//! `sha256("abbey.train.split.v1" || record_id)`, the first 8 hex characters
//! read as a `u64`, and `value % 100 < train_percent` selects the training
//! split.

use sha2::{Digest, Sha256};

pub use crate::train::digest::SPLIT_RULE;

/// Percentage of the corpus that lands in the training split.
pub const TRAIN_PERCENT: u64 = 80;

/// Which split a record belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// The trainer sees these pairs.
    Train,
    /// Held out for scoring; never written into the trainer's dataset.
    Validation,
}

impl Split {
    /// Stable machine label used in manifests, digests, and ledger rows.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Train => "train",
            Self::Validation => "validation",
        }
    }

    /// Sort rank: the training split comes first.
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::Train => 0,
            Self::Validation => 1,
        }
    }
}

/// `sha256("abbey.train.split.v1" || record_id)`, first 8 hex chars as a `u64`.
#[must_use]
pub fn bucket(record_id: &str) -> u64 {
    let mut hash = Sha256::new();
    hash.update(SPLIT_RULE.as_bytes());
    hash.update(record_id.as_bytes());
    let bytes = hash.finalize();
    let mut value = 0u64;
    for byte in bytes.iter().take(4) {
        value = (value << 8) | u64::from(*byte);
    }
    value
}

/// The split a record id belongs to at `train_percent`.
#[must_use]
pub fn assign(record_id: &str, train_percent: u64) -> Split {
    if bucket(record_id) % 100 < train_percent {
        Split::Train
    } else {
        Split::Validation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_is_stable_for_a_record_id() {
        let first = assign("memory:6f1c", TRAIN_PERCENT);
        for _ in 0..8 {
            assert_eq!(assign("memory:6f1c", TRAIN_PERCENT), first);
        }
    }

    #[test]
    fn the_eighty_twenty_rule_is_the_specified_one() {
        for id in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            let expected = if bucket(id) % 100 < 80 {
                Split::Train
            } else {
                Split::Validation
            };
            assert_eq!(assign(id, TRAIN_PERCENT), expected);
        }
    }

    #[test]
    fn a_different_ratio_moves_records() {
        let moved = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
            .iter()
            .filter(|id| assign(id, TRAIN_PERCENT) != assign(id, 50))
            .count();
        assert!(moved > 0, "the ratio must be able to move a record");
    }
}
