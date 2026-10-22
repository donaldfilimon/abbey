//! Domain-separated digests for the training pipeline.
//!
//! Every digest mixes a distinct ASCII domain prefix into the hash, following
//! the style already used by `src/learn.rs`. Two digests that cover the same
//! bytes therefore never collide across roles (a dataset digest can never be
//! mistaken for an artifact digest), and a digest computed under one rule can
//! never be replayed under another.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// The split-assignment rule. Also the exact byte prefix the split hash mixes
/// in — see [`crate::train::split::bucket`].
pub const SPLIT_RULE: &str = "abbey.train.split.v1";
/// Corpus digest domain.
pub const DATASET_DOMAIN: &str = "abbey.train.dataset.v1";
/// Consent-file digest domain.
pub const CONSENT_DOMAIN: &str = "abbey.train.consent.v1";
/// Manifest digest domain.
pub const MANIFEST_DOMAIN: &str = "abbey.train.manifest.v1";
/// Trainer artifact digest domain.
pub const ARTIFACT_DOMAIN: &str = "abbey.train.artifact.v1";
/// Ledger row digest domain.
pub const LEDGER_DOMAIN: &str = "abbey.train.ledger.v1";

/// `sha256:<hex>` over `domain || 0x00 || payload`.
#[must_use]
pub fn digest(domain: &str, payload: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain.as_bytes());
    hash.update([0u8]);
    hash.update(payload);
    format!("sha256:{:x}", hash.finalize())
}

/// Digest of a file's bytes under `domain`.
pub fn digest_file(domain: &str, path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(digest(domain, &bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_are_distinct_and_digests_are_stable() {
        let a = digest(DATASET_DOMAIN, b"payload");
        let b = digest(ARTIFACT_DOMAIN, b"payload");
        assert_ne!(a, b, "two domains over the same bytes must differ");
        assert_eq!(a, digest(DATASET_DOMAIN, b"payload"));
        assert!(a.starts_with("sha256:"));
        assert_eq!(a.len(), "sha256:".len() + 64);
    }
}
