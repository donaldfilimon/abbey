//! Coverage for the corpus-reproduction metric.
//!
//! The metric is a **training-fidelity** measure: it asks how faithfully an
//! artifact reproduces the corpus it was trained on, probed with seed
//! characters drawn from the held-out split. These tests pin what that means
//! in practice — a faithful artifact scores one, an artifact that reproduces
//! nothing scores zero, the comparison against the installed artifact decides
//! promotion, and a held-out pair inside the training corpus is a leak that
//! is never promoted.

use super::*;
use crate::train::dataset::Pair;
use crate::train::evaluate::{Decision, METRIC};
use crate::train::fixture::{
    Scratch, consent_file, ids_with_splits, state_in, store_corpus, store_pair, write_trainer,
};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// The dataset the fixture trainer is given.
const DATASET: &str = "{\"text\":\"alpha\\none\"}\n{\"text\":\"bravo\\ntwo\"}\n";

/// Train the fixture and return the artifact it wrote.
fn train_fixture(scratch: &Scratch, binary: &Path) -> (std::path::PathBuf, String) {
    let dataset = scratch.dir.join("dataset.jsonl");
    fs::write(&dataset, DATASET).unwrap();
    let artifact = scratch.dir.join("artifact.json");
    trainer::run_trainer(binary, &dataset, &artifact, Duration::from_secs(30)).unwrap();
    let corpus = trainer::read_corpus(&dataset).unwrap();
    (artifact, corpus)
}

/// An artifact that reproduces its own corpus scores one.
#[test]
fn an_artifact_that_reproduces_its_corpus_scores_one() {
    let scratch = Scratch::new("metric-reproduces");
    let binary = write_trainer(&scratch, "ok");
    let (artifact, corpus) = train_fixture(&scratch, &binary);
    assert_eq!(corpus, "alpha\none\nbravo\ntwo");
    let score = evaluate::score_artifact(
        &binary,
        &artifact,
        &corpus,
        &['a', 'b'],
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(score.probes, 2);
    assert!(
        (score.score - 1.0).abs() < 1e-9,
        "a faithful artifact must score one, not {}",
        score.score
    );
    assert_eq!(score.context_length, 2);
}

/// An artifact that emits something unrelated scores zero.
#[test]
fn an_artifact_that_emits_something_unrelated_scores_zero() {
    let scratch = Scratch::new("metric-unrelated");
    let binary = write_trainer(&scratch, "unrelated");
    let (artifact, corpus) = train_fixture(&scratch, &binary);
    let score = evaluate::score_artifact(
        &binary,
        &artifact,
        &corpus,
        &['a', 'b'],
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(score.probes, 2);
    assert_eq!(score.score, 0.0, "an unrelated sample must score zero");
}

/// The same artifact and the same corpus score the same way twice.
#[test]
fn the_score_is_the_same_twice() {
    let scratch = Scratch::new("metric-twice");
    let binary = write_trainer(&scratch, "ok");
    let (artifact, corpus) = train_fixture(&scratch, &binary);
    let first =
        evaluate::score_artifact(&binary, &artifact, &corpus, &['a'], Duration::from_secs(10))
            .unwrap();
    let second =
        evaluate::score_artifact(&binary, &artifact, &corpus, &['a'], Duration::from_secs(10))
            .unwrap();
    assert_eq!(first, second);
}

/// The held-out pairs supply the probe seeds, and only the held-out ones.
#[test]
fn the_probe_seeds_come_from_the_held_out_pairs() {
    let scratch = Scratch::new("metric-seeds");
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
    let detail = &rows[0].detail;
    assert!(
        detail.contains("2 probe(s)"),
        "the two held-out pairs must supply two probes: {detail}"
    );
}

/// A run whose artifact reproduces the corpus is promoted, and the ledger
/// records the metric and the leak check beside the score.
#[test]
fn a_reproducing_artifact_is_promoted_and_recorded() {
    let scratch = Scratch::new("metric-promote");
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
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("decision:    promoted")),
        "{outcome:?}"
    );
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("metric:      corpus reproduction")),
        "the output must name the metric: {outcome:?}"
    );
    let current = ledger::read_current(&scratch.dir)
        .unwrap()
        .expect("a current artifact");
    assert!(Path::new(&current.artifact_path).is_file());
    assert!(current.artifact_digest.starts_with("sha256:"));
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].decision.as_deref(), Some("promoted"));
    assert_eq!(rows[0].op, "run");
    assert_eq!(rows[0].metric.as_deref(), Some(METRIC));
    assert_eq!(rows[0].validation_leak.as_deref(), Some("none"));
    let score = rows[0].reproduction_score.expect("a reproduction score");
    assert!(
        (score - 1.0).abs() < 1e-9,
        "a faithful artifact must score one, not {score}"
    );
    assert!(rows[0].row_digest.starts_with("sha256:"));
}

/// A run whose artifact reproduces nothing is not promoted.
#[test]
fn an_unreproducing_artifact_is_not_promoted() {
    let scratch = Scratch::new("metric-not-promoted");
    let state = state_in(&scratch.dir);
    let binary = write_trainer(&scratch, "unrelated");
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
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("trained, not promoted")),
        "{outcome:?}"
    );
    assert!(ledger::read_current(&scratch.dir).unwrap().is_none());
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].decision.as_deref(), Some("trained_not_promoted"));
    assert_eq!(rows[0].reproduction_score, Some(0.0));
}

/// A candidate that scores below the installed artifact is a regression, and
/// the previous artifact is kept.
#[test]
fn a_regression_keeps_the_previous_artifact() {
    let scratch = Scratch::new("metric-regression");
    let state = state_in(&scratch.dir);
    let consent = consent_file(&scratch);
    store_corpus(&scratch.dir, 5, 2);
    let run_args = vec![
        "run".to_string(),
        "--consent".to_string(),
        consent.display().to_string(),
    ];

    // The first run reproduces the corpus and is promoted.
    let good = write_trainer(&scratch, "ok");
    dispatch_with(&state, &run_args, Some(&good)).unwrap();
    let first = ledger::read_current(&scratch.dir).unwrap().unwrap();

    // The second run reproduces nothing: a regression, and the pointer stays.
    let bad = write_trainer(&scratch, "unrelated");
    dispatch_with(&state, &run_args, Some(&bad)).unwrap();
    let after_regression = ledger::read_current(&scratch.dir).unwrap().unwrap();
    assert_eq!(after_regression.run_id, first.run_id);
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].decision.as_deref(), Some("regression"));
    assert_eq!(rows[1].reproduction_score, Some(0.0));
    assert!(
        rows[1].baseline_reproduction_score.is_some(),
        "the baseline must be scored with the same probes: {:?}",
        rows[1]
    );
}

/// A rollback restores the previous artifact, and every row is kept.
#[test]
fn a_rollback_restores_the_previous_artifact_and_keeps_every_row() {
    let scratch = Scratch::new("metric-rollback");
    let state = state_in(&scratch.dir);
    let consent = consent_file(&scratch);
    store_corpus(&scratch.dir, 5, 2);
    let run_args = vec![
        "run".to_string(),
        "--consent".to_string(),
        consent.display().to_string(),
    ];

    let good = write_trainer(&scratch, "ok");
    dispatch_with(&state, &run_args, Some(&good)).unwrap();
    let first = ledger::read_current(&scratch.dir).unwrap().unwrap();

    // A worse artifact is a regression: the pointer must not move.
    let bad = write_trainer(&scratch, "unrelated");
    dispatch_with(&state, &run_args, Some(&bad)).unwrap();
    let after_regression = ledger::read_current(&scratch.dir).unwrap().unwrap();
    assert_eq!(after_regression.run_id, first.run_id);

    // A third run that reproduces the corpus again is promoted, and a
    // rollback returns to the artifact that was current before it.
    dispatch_with(&state, &run_args, Some(&good)).unwrap();
    let promoted = ledger::read_current(&scratch.dir).unwrap().unwrap();
    assert_ne!(promoted.run_id, first.run_id);

    let outcome = dispatch_with(&state, &["rollback".to_string()], Some(&good)).unwrap();
    assert_eq!(outcome.code, 0, "{outcome:?}");
    let restored = ledger::read_current(&scratch.dir).unwrap().unwrap();
    assert_eq!(restored.run_id, first.run_id);
    assert_eq!(restored.artifact_digest, first.artifact_digest);
    let materialized = fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap();
    assert_eq!(
        crate::train::digest::digest(crate::train::digest::ARTIFACT_DOMAIN, &materialized),
        first.artifact_digest,
        "the materialized copy must be the restored artifact"
    );
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 4, "the ledger retains every row");
    assert_eq!(rows[3].op, "rollback");
    assert!(rows.iter().all(|row| row.digest_matches()));
}

/// A held-out pair whose text is inside the training corpus is a leak: it is
/// detected, recorded, and never promoted.
#[test]
fn a_validation_pair_in_the_training_corpus_is_detected_and_reported() {
    let scratch = Scratch::new("metric-leak");
    let state = state_in(&scratch.dir);
    let binary = write_trainer(&scratch, "ok");
    let consent = consent_file(&scratch);
    // The same text under a training id and a held-out id: the held-out text
    // is inside the training corpus, so the holdout is not a holdout.
    let ids = ids_with_splits(1, 1);
    let train_id = ids
        .iter()
        .find(|id| {
            crate::train::split::assign(id, crate::train::split::TRAIN_PERCENT)
                == crate::train::split::Split::Train
        })
        .expect("a training id");
    let validation_id = ids
        .iter()
        .find(|id| {
            crate::train::split::assign(id, crate::train::split::TRAIN_PERCENT)
                == crate::train::split::Split::Validation
        })
        .expect("a held-out id");
    store_pair(&scratch.dir, train_id, "shared", "text", 0.95);
    store_pair(&scratch.dir, validation_id, "shared", "text", 0.95);

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
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("decision:    validation leak")),
        "{outcome:?}"
    );
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("leak:        1 validation pair(s)")),
        "{outcome:?}"
    );
    assert!(
        ledger::read_current(&scratch.dir).unwrap().is_none(),
        "a leaked holdout must never be promoted"
    );
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows[0].decision.as_deref(), Some("validation_leak"));
    assert_eq!(
        rows[0].validation_leak.as_deref(),
        Some("1 validation pair(s) present in the training corpus")
    );
}

/// The evaluation verb re-scores the current artifact against the corpus its
/// own run was trained on.
#[test]
fn the_evaluate_verb_re_scores_the_current_artifact() {
    let scratch = Scratch::new("metric-evaluate");
    let state = state_in(&scratch.dir);
    let binary = write_trainer(&scratch, "ok");
    let consent = consent_file(&scratch);
    store_corpus(&scratch.dir, 5, 2);
    let run_args = vec![
        "run".to_string(),
        "--consent".to_string(),
        consent.display().to_string(),
    ];
    dispatch_with(&state, &run_args, Some(&binary)).unwrap();

    let outcome = dispatch_with(&state, &["evaluate".to_string()], Some(&binary)).unwrap();
    assert_eq!(outcome.code, 0, "{outcome:?}");
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("reproduction: 1.0000")),
        "{outcome:?}"
    );
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("metric:      corpus reproduction")),
        "{outcome:?}"
    );
    assert!(
        outcome
            .out
            .iter()
            .any(|line| line.contains("leak:        none")),
        "{outcome:?}"
    );
    // It is read-only: no new ledger row.
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 1);
}

/// The leak check itself: it looks for the held-out pair's exact text.
#[test]
fn the_leak_check_finds_a_held_out_pair_in_the_corpus() {
    let held_out = Pair {
        id: "memory:held".to_string(),
        prompt: "alpha".to_string(),
        response: "one".to_string(),
    };
    let clean = Pair {
        id: "memory:clean".to_string(),
        prompt: "bravo".to_string(),
        response: "two".to_string(),
    };
    let refs = vec![&held_out, &clean];
    let report = evaluate::leak_check("alpha\none\nunrelated text", &refs);
    assert!(!report.is_clean());
    assert_eq!(report.leaked, vec!["memory:held".to_string()]);
    assert_eq!(report.checked, 2);
    assert!(report.summary().contains("1 validation pair(s)"));
    let clean_report = evaluate::leak_check("unrelated text", &refs);
    assert!(clean_report.is_clean());
    assert_eq!(clean_report.summary(), "none");
}

/// The decision rule: a leak is never a promotion, whatever the score says.
#[test]
fn a_leak_is_never_promoted() {
    assert!(!Decision::ValidationLeak.promotes());
    assert_eq!(Decision::ValidationLeak.as_str(), "validation_leak");
    assert_eq!(Decision::ValidationLeak.label(), "validation leak");
}
