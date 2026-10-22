//! The explicit, file-based consent gate.
//!
//! Training is a human act, so the gate is a human act too: a file the
//! operator created, holding one exact line. There is **no** environment
//! variable that can stand in for it, and no flag that skips it. A verb that
//! writes a dataset or invokes a trainer refuses without this file.

use crate::train::digest::{CONSENT_DOMAIN, digest_file};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The exact line a consent file must contain.
pub const CONSENT_LINE: &str = "abbey-train-consent: v1";

/// A consent grant, with the digest that is recorded in the manifest and the
/// ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consent {
    /// The file the grant came from.
    pub path: PathBuf,
    /// `sha256:<hex>` over the consent file's bytes.
    pub digest: String,
}

/// Read and check a consent file.
///
/// The file must exist, be readable, and contain the exact line
/// [`CONSENT_LINE`]. Anything else is a refusal, never a warning.
pub fn read(path: &Path) -> Result<Consent> {
    if !path.exists() {
        bail!(
            "consent file not found: {}\n\
             expected a readable file containing the exact line `{CONSENT_LINE}`",
            path.display()
        );
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read consent file {}", path.display()))?;
    if !text.lines().any(|line| line.trim() == CONSENT_LINE) {
        bail!(
            "consent file {} does not grant consent\n\
             expected the exact line `{CONSENT_LINE}` on a line of its own",
            path.display()
        );
    }
    let digest = digest_file(CONSENT_DOMAIN, path)?;
    Ok(Consent {
        path: path.to_path_buf(),
        digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("abbey-train-consent-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_is_refused() {
        let dir = scratch("missing");
        let error = read(&dir.join("nope")).unwrap_err().to_string();
        assert!(error.contains("consent file not found"), "{error}");
        assert!(error.contains(CONSENT_LINE), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_wrong_content_is_refused_and_says_what_is_expected() {
        let dir = scratch("wrong");
        let path = dir.join("consent.txt");
        fs::write(&path, "abbey-train-consent: v2\n").unwrap();
        let error = read(&path).unwrap_err().to_string();
        assert!(error.contains("does not grant consent"), "{error}");
        assert!(error.contains(CONSENT_LINE), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_exact_line_is_accepted_and_digested() {
        let dir = scratch("ok");
        let path = dir.join("consent.txt");
        fs::write(&path, format!("{CONSENT_LINE}\n")).unwrap();
        let consent = read(&path).unwrap();
        assert!(consent.digest.starts_with("sha256:"));
        assert_eq!(consent.digest, read(&path).unwrap().digest);
        let _ = fs::remove_dir_all(&dir);
    }
}
