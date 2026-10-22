//! The bounded adapter over the external `abi` trainer.
//!
//! Abbey owns no trainer, optimizer, loss, or weight update. The weight
//! update is delegated across a process boundary to the sibling `abi` binary's
//! demo-grade character-level trainer, exactly the way the `abi` backend
//! already delegates generation. The adapter adds the boundaries Abbey
//! requires and nothing more:
//!
//! * a hard timeout,
//! * a bounded captured output,
//! * a fixed argv grammar (`nn train --jsonl … --field text --out …`),
//! * an artifact that must exist after a zero exit, and
//! * a refusal to write outside the run directory.
//!
//! The core takes the binary path as a parameter. It never reads the
//! environment, so it is deterministic inside a test binary.

use crate::train::digest::{ARTIFACT_DOMAIN, digest_file};
use crate::train::process::run_bounded;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use std::time::Duration;

/// Hard limit on one trainer invocation.
pub const TRAIN_TIMEOUT: Duration = Duration::from_secs(120);
/// Hard limit on one sampler invocation.
pub const SAMPLE_TIMEOUT: Duration = Duration::from_secs(10);
/// The artifact file name inside a run directory.
pub const ARTIFACT_FILE_NAME: &str = "artifact.json";
/// The JSONL field the trainer reads.
pub const DATASET_FIELD: &str = "text";
/// Captured trainer output cap.
const STDOUT_CAP: usize = 64 * 1024;
/// Captured trainer error cap.
const STDERR_CAP: usize = 16 * 1024;

/// What a successful trainer invocation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainOutcome {
    /// Where the artifact was written.
    pub artifact: PathBuf,
    /// `sha256:<hex>` over the artifact's bytes.
    pub artifact_digest: String,
    /// The trainer's captured standard output.
    pub stdout: String,
}

/// Refuse to write an artifact outside the run directory.
pub fn ensure_artifact_inside(run_dir: &Path, artifact: &Path) -> Result<()> {
    let parent = artifact.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "the artifact path must name a run directory: {}",
            artifact.display()
        )
    })?;
    if parent != run_dir {
        bail!(
            "refusing to write an artifact outside the run directory: {} (run directory is {})",
            artifact.display(),
            run_dir.display()
        );
    }
    Ok(())
}

/// The exact argv the trainer is invoked with.
#[must_use]
pub fn train_argv(dataset: &Path, artifact: &Path) -> Vec<OsString> {
    vec![
        OsString::from("nn"),
        OsString::from("train"),
        OsString::from("--jsonl"),
        dataset.as_os_str().to_owned(),
        OsString::from("--field"),
        OsString::from(DATASET_FIELD),
        OsString::from("--out"),
        artifact.as_os_str().to_owned(),
    ]
}

/// The exact argv the sampler is invoked with.
#[must_use]
pub fn sample_argv(checkpoint: &Path, seed: char, n: usize) -> Vec<OsString> {
    vec![
        OsString::from("nn"),
        OsString::from("sample"),
        OsString::from("--checkpoint"),
        checkpoint.as_os_str().to_owned(),
        OsString::from("--seed"),
        OsString::from(seed.to_string()),
        OsString::from("--n"),
        OsString::from(n.to_string()),
    ]
}

/// Run the trainer and return the artifact it produced.
///
/// Fails closed: a non-zero exit, a timeout, or a missing artifact is an
/// error, and no artifact is left behind.
pub fn run_trainer(
    binary: &Path,
    dataset: &Path,
    artifact: &Path,
    timeout: Duration,
) -> Result<TrainOutcome> {
    let outcome = run_bounded(
        binary,
        &train_argv(dataset, artifact),
        timeout,
        STDOUT_CAP,
        STDERR_CAP,
    )
    .map_err(|error| anyhow::anyhow!("trainer invocation failed: {error}"))?;
    if outcome.code != Some(0) {
        let _ = std::fs::remove_file(artifact);
        bail!(
            "the trainer exited with {}{}",
            outcome
                .code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "no exit code".into()),
            tail(&outcome.stderr, &outcome.stdout)
        );
    }
    if !artifact.is_file() {
        bail!(
            "the trainer exited zero but wrote no artifact at {}",
            artifact.display()
        );
    }
    let artifact_digest = digest_file(ARTIFACT_DOMAIN, artifact)?;
    Ok(TrainOutcome {
        artifact: artifact.to_path_buf(),
        artifact_digest,
        stdout: outcome.stdout,
    })
}

/// Sample `n` characters from a checkpoint, seeded with `seed`.
///
/// The sampler's own report line is stripped: only the sampled text is
/// returned.
pub fn run_sample(
    binary: &Path,
    checkpoint: &Path,
    seed: char,
    n: usize,
    timeout: Duration,
) -> Result<String> {
    let outcome = run_bounded(
        binary,
        &sample_argv(checkpoint, seed, n),
        timeout,
        STDOUT_CAP,
        STDERR_CAP,
    )
    .map_err(|error| anyhow::anyhow!("sampler invocation failed: {error}"))?;
    if outcome.code != Some(0) {
        bail!(
            "the sampler exited with {}{}",
            outcome
                .code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "no exit code".into()),
            tail(&outcome.stderr, &outcome.stdout)
        );
    }
    if outcome.truncated {
        // The sampled text is the data the score is computed from. A
        // truncated capture is a partial sample, and a score over a partial
        // sample is not a score: refuse instead of reporting one.
        bail!(
            "the sampler's output exceeded the capture cap; refusing to score a truncated sample"
        );
    }
    extract_sample(&outcome.stdout)
        .ok_or_else(|| anyhow::anyhow!("the sampler produced no `nn sample:` line in its output"))
}

/// Pull the sampled text out of the sampler's stdout.
///
/// The sampler prints its training report first and then one `nn sample:`
/// line. The **last** such line wins, so a report that happens to contain the
/// marker cannot shadow the real sample.
#[must_use]
pub fn extract_sample(stdout: &str) -> Option<String> {
    const MARKER: &str = "nn sample: ";
    let at = stdout.rfind(MARKER)?;
    let mut text = stdout[at + MARKER.len()..].to_string();
    // The CLI terminates the line it prints; strip exactly that one newline so
    // a sampled text that ends in a newline keeps it.
    if text.ends_with('\n') {
        text.pop();
    }
    Some(text)
}

/// Reconstruct the trainer's corpus from a dataset file's bytes.
///
/// This is the sibling trainer's own rule, reproduced exactly: the file is
/// split on `\n`; each line is trimmed of spaces, tabs, carriage returns and
/// newlines; a blank line is skipped; a line that is not a JSON object is
/// skipped; a line whose `text` field is missing or is not a string is
/// skipped; a line whose `text` field is empty is skipped; and the surviving
/// values are joined with a single `\n` between them.
///
/// Reading the corpus back from the file Abbey wrote — rather than from an
/// in-memory copy — is what makes the scored corpus exactly the text the
/// trainer consumed.
#[must_use]
pub fn extract_corpus(bytes: &str) -> String {
    let mut corpus = String::new();
    for raw_line in bytes.split('\n') {
        let line = raw_line.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        let Some(text) = object.get(DATASET_FIELD).and_then(|value| value.as_str()) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        if !corpus.is_empty() {
            corpus.push('\n');
        }
        corpus.push_str(text);
    }
    corpus
}

/// Read the dataset file and reconstruct the corpus the trainer consumed.
pub fn read_corpus(dataset: &Path) -> Result<String> {
    let bytes =
        std::fs::read_to_string(dataset).with_context(|| format!("read {}", dataset.display()))?;
    Ok(extract_corpus(&bytes))
}

/// A short tail of the child's diagnostics.
fn tail(stderr: &str, stdout: &str) -> String {
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let tail: String = trimmed.chars().rev().take(400).collect::<String>();
    format!(": {}", tail.chars().rev().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_train_argv_is_exactly_the_documented_grammar() {
        let argv = train_argv(Path::new("/d.jsonl"), Path::new("/a.json"));
        let argv: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            vec![
                "nn", "train", "--jsonl", "/d.jsonl", "--field", "text", "--out", "/a.json"
            ]
        );
    }

    #[test]
    fn the_sample_argv_is_exactly_the_documented_grammar() {
        let argv = sample_argv(Path::new("/a.json"), 'W', 12);
        let argv: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            vec![
                "nn",
                "sample",
                "--checkpoint",
                "/a.json",
                "--seed",
                "W",
                "--n",
                "12"
            ]
        );
    }

    #[test]
    fn the_sampled_text_is_taken_from_the_last_marker() {
        let stdout = "nn train: initial_loss=1.0\nnn sample: hello\n";
        assert_eq!(extract_sample(stdout).as_deref(), Some("hello"));
        let stdout = "nn sample: first\nnn train: x\nnn sample: second\n";
        assert_eq!(extract_sample(stdout).as_deref(), Some("second"));
        assert_eq!(extract_sample("nothing here"), None);
    }

    #[test]
    fn an_artifact_outside_the_run_directory_is_refused() {
        let run_dir = Path::new("/state/train/run-1");
        assert!(
            ensure_artifact_inside(run_dir, Path::new("/state/train/run-1/artifact.json")).is_ok()
        );
        let error = ensure_artifact_inside(run_dir, Path::new("/tmp/artifact.json"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside the run directory"), "{error}");
    }

    #[test]
    fn the_corpus_is_reconstructed_the_way_the_trainer_reads_it() {
        // The sibling's rule, case for case: a blank line, a line that is not
        // an object, a line with no `text` field, and a line with an empty
        // `text` field are all skipped; the survivors are joined with one
        // newline.
        let dataset = concat!(
            "{\"text\":\"hello\"}\n",
            "\n",
            "   \n",
            "not json at all\n",
            "[1,2,3]\n",
            "{\"other\":\"ignored\"}\n",
            "{\"text\":42}\n",
            "{\"text\":\"\"}\n",
            "{\"text\":\"world\"}\n",
        );
        assert_eq!(extract_corpus(dataset), "hello\nworld");
    }

    #[test]
    fn an_empty_dataset_reconstructs_to_an_empty_corpus() {
        assert_eq!(extract_corpus(""), "");
        assert_eq!(extract_corpus("\n\n"), "");
        assert_eq!(extract_corpus("{\"text\":\"\"}\n"), "");
    }
}
