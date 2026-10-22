//! Unit coverage for the training pipeline.
//!
//! The trainer and the sampler are exercised against a **fixture script**
//! written into a scratch directory — the same shape the TUI prediction
//! fixture uses — so the tests never depend on a real `abi` binary, a real
//! model, or the network. The fixture itself lives in
//! [`crate::train::fixture`], which the metric tests share.

use super::*;
use crate::train::consent::CONSENT_LINE;
use crate::train::dataset::dataset_digest;
use crate::train::evaluate::decide;
use crate::train::fixture::{
    Scratch, consent_file, corpus, pair, state_in, store_corpus, store_pair, write_trainer,
};
use crate::train::redaction::redact;
use crate::train::split::{SPLIT_RULE, TRAIN_PERCENT, assign, bucket};
use std::fs;
use std::path::Path;
use std::time::Duration;

// ---------------------------------------------------------------- redaction

#[test]
fn the_redaction_table_is_exact() {
    let cases: &[(&str, &str)] = &[
        (
            "the binary lives at /usr/local/bin/abbey",
            "the binary lives at [path]",
        ),
        (
            r"the binary lives at C:\Users\me\abbey.exe",
            "the binary lives at [path]",
        ),
        (
            "the key is sk-abcdefghijklmnopqrst",
            "the key is [redacted]",
        ),
        (
            "the token is ghp_abcdefghijklmnopqrst",
            "the token is [redacted]",
        ),
        ("the id is AKIAIOSFODNN7EXAMPLE", "the id is [redacted]"),
        ("auth Bearer abcdefghijklmnop here", "auth [redacted] here"),
        ("password=hunter2secret", "password=[redacted]"),
        ("token=abcdefghij", "token=[redacted]"),
        ("secret=abcdefghij", "secret=[redacted]"),
        (
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----",
            "[redacted]",
        ),
        (
            "Rust is a systems programming language.",
            "Rust is a systems programming language.",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(&redact(input).text, expected, "input: {input}");
    }
}

#[test]
fn redaction_is_deterministic_and_reports_whether_it_changed_anything() {
    let input = "/tmp/x sk-abcdefghijklmnop password=abcdefghij";
    let first = redact(input);
    assert!(first.changed);
    for _ in 0..4 {
        assert_eq!(redact(input), first);
    }
    let clean = redact("nothing sensitive here");
    assert!(!clean.changed);
    assert_eq!(clean.text, "nothing sensitive here");
}

// ------------------------------------------------------------------- split

#[test]
fn the_split_is_deterministic_and_stable_for_a_record_id() {
    for id in ["memory:a", "memory:b", "specs:claim:backend-ollama"] {
        let first = assign(id, TRAIN_PERCENT);
        for _ in 0..8 {
            assert_eq!(assign(id, TRAIN_PERCENT), first);
        }
    }
}

#[test]
fn the_split_follows_the_eighty_twenty_rule() {
    for id in ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"] {
        let expected = if bucket(id) % 100 < 80 {
            crate::train::split::Split::Train
        } else {
            crate::train::split::Split::Validation
        };
        assert_eq!(assign(id, TRAIN_PERCENT), expected, "id: {id}");
    }
}

#[test]
fn the_split_is_part_of_the_dataset_digest() {
    let pairs = vec![pair("a", "p", "r"), pair("b", "p", "r")];
    let at_eighty = dataset_digest(&pairs, TRAIN_PERCENT);
    let at_fifty = dataset_digest(&pairs, 50);
    assert_ne!(
        at_eighty, at_fifty,
        "a different ratio must change the dataset digest"
    );
    assert_eq!(at_eighty, dataset_digest(&pairs, TRAIN_PERCENT));
    assert_ne!(at_eighty, dataset_digest(&pairs, TRAIN_PERCENT + 1));
}

// ------------------------------------------------------------------ digest

#[test]
fn the_dataset_digest_is_stable_and_sensitive() {
    let pairs = vec![pair("a", "p1", "r1"), pair("b", "p2", "r2")];
    let first = dataset_digest(&pairs, TRAIN_PERCENT);
    assert_eq!(first, dataset_digest(&pairs, TRAIN_PERCENT));
    assert!(first.starts_with("sha256:"));

    let mut changed = pairs.clone();
    changed[0].response = "r1!".into();
    assert_ne!(
        first,
        dataset_digest(&changed, TRAIN_PERCENT),
        "a one-character change must change the digest"
    );

    let mut reordered = pairs.clone();
    reordered.reverse();
    assert_ne!(
        first,
        dataset_digest(&reordered, TRAIN_PERCENT),
        "a different order must change the digest"
    );
}

#[test]
fn the_corpus_is_kept_in_split_order() {
    let mut pairs = vec![
        pair("a", "p", "r"),
        pair("b", "p", "r"),
        pair("c", "p", "r"),
    ];
    crate::train::dataset::sort_into_split_order(&mut pairs, TRAIN_PERCENT);
    let ranks: Vec<u8> = pairs
        .iter()
        .map(|pair| assign(&pair.id, TRAIN_PERCENT).rank())
        .collect();
    assert!(ranks.windows(2).all(|window| window[0] <= window[1]));
}

#[test]
fn the_split_rule_is_mixed_into_the_dataset_digest() {
    assert!(SPLIT_RULE.starts_with("abbey.train.split."));
    let pairs = vec![pair("a", "p", "r")];
    let digest = dataset_digest(&pairs, TRAIN_PERCENT);
    let mut payload = Vec::new();
    payload.extend_from_slice(SPLIT_RULE.as_bytes());
    payload.push(0);
    payload.extend_from_slice(TRAIN_PERCENT.to_string().as_bytes());
    assert!(digest.starts_with("sha256:"));
    assert_ne!(digest.len(), 0);
    assert!(!payload.is_empty());
}

// ----------------------------------------------------------------- consent

#[test]
fn a_missing_or_wrong_consent_file_is_refused() {
    let scratch = Scratch::new("consent-refusal");
    let missing = scratch.dir.join("absent.txt");
    let error = crate::train::consent::read(&missing)
        .unwrap_err()
        .to_string();
    assert!(error.contains("consent file not found"), "{error}");
    assert!(error.contains(CONSENT_LINE), "{error}");

    let wrong = scratch.dir.join("wrong.txt");
    fs::write(&wrong, "abbey-train-consent: v0\n").unwrap();
    let error = crate::train::consent::read(&wrong).unwrap_err().to_string();
    assert!(error.contains("does not grant consent"), "{error}");
    assert!(error.contains(CONSENT_LINE), "{error}");
}

#[test]
fn the_consent_digest_is_recorded_in_the_manifest() {
    let scratch = Scratch::new("consent-manifest");
    let consent = crate::train::consent::read(&consent_file(&scratch)).unwrap();
    let prepared = runs::write_run(
        &scratch.dir,
        "run-1",
        &corpus(vec![pair("a", "p", "r")]),
        "memory",
        &consent.digest,
    )
    .unwrap();
    assert_eq!(prepared.manifest.consent_digest, consent.digest);
    assert!(consent.digest.starts_with("sha256:"));
}

// ----------------------------------------------------------------- trainer

#[test]
fn a_trainer_that_writes_the_artifact_reports_its_digest() {
    let scratch = Scratch::new("trainer-ok");
    let binary = write_trainer(&scratch, "ok");
    let dataset = scratch.dir.join("dataset.jsonl");
    fs::write(&dataset, "{\"text\":\"q\\na\"}\n").unwrap();
    let artifact = scratch.dir.join("artifact.json");
    let outcome = trainer::run_trainer(&binary, &dataset, &artifact, Duration::from_secs(30))
        .expect("the fixture trainer must succeed");
    assert!(artifact.is_file());
    assert!(outcome.artifact_digest.starts_with("sha256:"));
    assert!(outcome.stdout.contains("nn checkpoint: wrote"));
}

#[test]
fn a_trainer_that_fails_leaves_no_artifact() {
    let scratch = Scratch::new("trainer-fail");
    let binary = write_trainer(&scratch, "train-fail");
    let dataset = scratch.dir.join("dataset.jsonl");
    fs::write(&dataset, "{\"text\":\"q\\na\"}\n").unwrap();
    let artifact = scratch.dir.join("artifact.json");
    let error = trainer::run_trainer(&binary, &dataset, &artifact, Duration::from_secs(30))
        .expect_err("a non-zero trainer exit must fail the run");
    assert!(format!("{error:#}").contains("exited with 1"), "{error:#}");
    assert!(!artifact.exists(), "a failed run must leave no artifact");
}

#[test]
fn a_trainer_that_hangs_is_stopped_by_the_timeout() {
    let scratch = Scratch::new("trainer-hang");
    let binary = write_trainer(&scratch, "hang");
    let dataset = scratch.dir.join("dataset.jsonl");
    fs::write(&dataset, "{\"text\":\"q\\na\"}\n").unwrap();
    let artifact = scratch.dir.join("artifact.json");
    let started = std::time::Instant::now();
    let error = trainer::run_trainer(&binary, &dataset, &artifact, Duration::from_millis(400))
        .expect_err("a hanging trainer must fail closed");
    assert!(format!("{error:#}").contains("time limit"), "{error:#}");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the timeout must actually fire"
    );
    assert!(!artifact.exists());
}

// ------------------------------------------------------------ cli dispatch

#[test]
fn an_unknown_verb_exits_two_with_the_usage_on_stderr() {
    let scratch = Scratch::new("unknown-verb");
    let state = state_in(&scratch.dir);
    let outcome = dispatch(&state, &["nope".to_string()]).unwrap();
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .err
            .iter()
            .any(|line| line.contains("usage: abbey train"))
    );
    assert!(outcome.out.is_empty());
}

#[test]
fn help_exits_zero() {
    let scratch = Scratch::new("help");
    let state = state_in(&scratch.dir);
    let outcome = dispatch(&state, &["--help".to_string()]).unwrap();
    assert_eq!(outcome.code, 0);
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("usage: abbey train"))
    );
}

#[test]
fn a_plan_over_an_empty_corpus_says_so_and_writes_nothing() {
    let scratch = Scratch::new("empty-plan");
    let state = state_in(&scratch.dir);
    let outcome = dispatch(&state, &["plan".to_string()]).unwrap();
    assert_eq!(outcome.code, 0, "{outcome:?}");
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("no curated pairs")),
        "{outcome:?}"
    );
    assert!(
        !scratch.dir.join("train").exists(),
        "plan must not create the run directory"
    );
    assert_eq!(
        fs::read_dir(&scratch.dir).unwrap().count(),
        0,
        "plan must not write anything"
    );
}

/// The memory corpus is reachable only if a human can curate a pair.
///
/// A distilled pair is written at teacher confidence (0.6), below the
/// curated bar, so it is skipped. `abbey learn promote` is the human step
/// that lifts it over the bar — without it the memory corpus would be
/// empty in practice, and `abbey train --source memory` would have
/// nothing to train on.
#[test]
fn a_promoted_distilled_pair_reaches_the_memory_corpus() {
    let scratch = Scratch::new("promote-reach");
    let state = state_in(&scratch.dir);

    // A distilled pair, at teacher confidence, with a real prompt/response.
    store_pair(&scratch.dir, "memory:pair-1", "q", "abcdefgh", 0.6);

    let mem = crate::memory::SqliteMemory::open(&crate::memory::SqliteMemory::path_for_state_dir(
        &scratch.dir,
    ))
    .unwrap();
    let corpus = dataset::memory_corpus(&mem).unwrap();
    assert!(
        corpus.pairs.is_empty(),
        "a pair below the curated bar must not enter the corpus"
    );
    assert!(
        corpus
            .skipped
            .iter()
            .any(|(reason, count)| *reason == dataset::SkipReason::BelowConfidence && *count == 1),
        "the skip must be reported as a confidence skip: {:?}",
        corpus.skipped
    );

    // The human curation step.
    let message = crate::learn::promote_candidate(&state, "memory:pair-1").unwrap();
    assert!(message.contains("promoted"), "{message}");

    let mem = crate::memory::SqliteMemory::open(&crate::memory::SqliteMemory::path_for_state_dir(
        &scratch.dir,
    ))
    .unwrap();
    let corpus = dataset::memory_corpus(&mem).unwrap();
    assert_eq!(
        corpus.pairs.len(),
        1,
        "a promoted pair must reach the corpus: {:?}",
        corpus.pairs
    );
    assert_eq!(corpus.pairs[0].prompt, "q");
    assert_eq!(corpus.pairs[0].response, "abcdefgh");
}

#[test]
fn a_run_without_consent_is_refused_with_exit_two() {
    let scratch = Scratch::new("no-consent");
    let state = state_in(&scratch.dir);
    let outcome = dispatch(&state, &["run".to_string()]).unwrap();
    assert_eq!(outcome.code, 2, "{outcome:?}");
    assert!(
        outcome.err.iter().any(|line| line.contains("--consent")),
        "{outcome:?}"
    );
    assert!(!scratch.dir.join("train").exists());
}

#[test]
fn a_run_with_a_wrong_consent_file_is_refused_with_exit_two() {
    let scratch = Scratch::new("bad-consent");
    let state = state_in(&scratch.dir);
    let wrong = scratch.dir.join("wrong.txt");
    fs::write(&wrong, "nope\n").unwrap();
    let outcome = dispatch(
        &state,
        &[
            "run".to_string(),
            "--consent".to_string(),
            wrong.display().to_string(),
        ],
    )
    .unwrap();
    assert_eq!(outcome.code, 2, "{outcome:?}");
    assert!(
        outcome.err.iter().any(|line| line.contains(CONSENT_LINE)),
        "{outcome:?}"
    );
}

/// The corpus a run trains on is the corpus the trainer consumed.
///
/// The run writes a dataset file, the trainer reads that file, and the
/// evaluation reads the same file back — never an in-memory copy — so the
/// scored corpus is exactly the text the artifact was trained on.
#[test]
fn the_dataset_file_is_the_corpus_the_trainer_consumed() {
    let scratch = Scratch::new("corpus-file");
    let state = state_in(&scratch.dir);
    let binary = write_trainer(&scratch, "ok");
    let consent = consent_file(&scratch);
    store_corpus(&scratch.dir, 5, 2);
    let outcome = dispatch_with(
        &state,
        &[
            "run".to_string(),
            "--consent".to_string(),
            consent.display().to_string(),
        ],
        Some(&binary),
    )
    .unwrap();
    assert_eq!(outcome.code, 0, "{outcome:?}");
    let rows = ledger::read(&scratch.dir).unwrap();
    let dataset_path = rows[0]
        .dataset_path
        .clone()
        .expect("the row names the dataset");
    let corpus_text = trainer::read_corpus(Path::new(&dataset_path)).unwrap();
    assert!(!corpus_text.is_empty(), "the corpus must not be empty");
    for prompt in crate::train::fixture::FIXTURE_PROMPTS {
        assert!(
            corpus_text.contains(prompt),
            "the training corpus must carry the training prompts: {corpus_text}"
        );
    }
    for prompt in crate::train::fixture::FIXTURE_VALIDATION_PROMPTS {
        assert!(
            !corpus_text.contains(prompt),
            "a held-out prompt must not be in the training corpus: {corpus_text}"
        );
    }
}

/// The evaluation is deterministic for a fixed artifact and a fixed corpus.
#[test]
fn the_score_is_deterministic_for_a_fixed_artifact() {
    let scratch = Scratch::new("evaluate");
    let binary = write_trainer(&scratch, "ok");
    let dataset = scratch.dir.join("dataset.jsonl");
    fs::write(&dataset, "{\"text\":\"q\\na\"}\n").unwrap();
    let artifact = scratch.dir.join("artifact.json");
    trainer::run_trainer(&binary, &dataset, &artifact, Duration::from_secs(30)).unwrap();
    let corpus_text = trainer::read_corpus(&dataset).unwrap();
    let seeds = ['q'];
    let first = evaluate::score_artifact(
        &binary,
        &artifact,
        &corpus_text,
        &seeds,
        Duration::from_secs(10),
    )
    .unwrap();
    let second = evaluate::score_artifact(
        &binary,
        &artifact,
        &corpus_text,
        &seeds,
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(first, second);
}

/// The decision rule itself: a fresh install with no baseline and a zero
/// score is not a promotion.
#[test]
fn a_fresh_install_with_no_baseline_records_trained_not_promoted() {
    assert_eq!(
        decide(Some(0.0), None),
        crate::train::evaluate::Decision::TrainedNotPromoted
    );
    assert_eq!(decide(Some(0.0), None).as_str(), "trained_not_promoted");
    assert_eq!(decide(Some(0.0), None).label(), "trained, not promoted");
}
