//! `abbey train` — legacy delegated training plus a native Rust model family.
//!
//! `abbey train family` enters `abbey-learning`: native collection, LoRA,
//! evaluation and exports. The documentation below describes only the retained
//! legacy verbs and their ABI demo artifact; its fidelity score is not used to
//! qualify a native family release.
//!
//! ## What this is
//!
//! A curation-to-artifact pipeline. It assembles a corpus from two bounded
//! sources, redacts it deterministically, splits it deterministically, hands
//! the training half to an external trainer across a process boundary,
//! scores the produced artifact offline for how faithfully it reproduces the
//! corpus it was trained on — probed with seed characters drawn from the
//! held-out half — and then either installs that artifact or keeps the
//! previous one according to a recorded rule. Every step lands in an
//! append-only ledger.
//!
//! ## What the score is
//!
//! The score is a **training-fidelity** measure, not a held-out quality
//! measure and not a generalisation measure. The delegated trainer exposes no
//! loss on unseen text, so no held-out quality metric is available at all.
//! The pipeline names the score for what it measures everywhere it reports
//! it, and the leak check that keeps the held-out half out of the training
//! corpus is recorded beside it.
//!
//! ## What this is not
//!
//! Abbey owns no trainer, no optimizer, no loss, and no weight update. The
//! weight update belongs to the external `abi` binary's demo-grade
//! character-level trainer, and this module only bounds, records, and audits
//! it. Nothing here turns a corpus into a production model, and no quality
//! claim is made about the artifact.
//!
//! ## The consent gate
//!
//! Any verb that writes a dataset or invokes a trainer requires
//! `--consent <path>`, naming a readable file that contains the exact line
//! `abbey-train-consent: v1`. There is no environment variable that can stand
//! in for it. Training is an explicit human act, so the gate is one too.

mod args;
mod consent;
mod dataset;
mod digest;
mod evaluate;
mod ledger;
mod process;
mod redaction;
mod runs;
mod specs;
mod split;
mod trainer;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod metric_tests;
#[cfg(all(test, unix))]
mod review_regressions;
#[cfg(test)]
mod tests;

use crate::agent::{AgentBackend, resolve_agent_for};
use crate::state::AbbeyState;
use anyhow::Result;
use args::{Options, Verb};
use dataset::{Corpus, Source};
use std::path::{Path, PathBuf};

/// The usage text.
pub const USAGE: &str = "usage: abbey train <verb> [options]\n\
     verbs:\n\
     \x20  family <verb>              native Rust model family (family --help)\n\
     \x20  plan                       read-only preview; writes nothing\n\
     \x20  prepare --consent <path>   write dataset.jsonl + manifest.json\n\
     \x20  run --consent <path>       train, evaluate, decide, record\n\
     \x20  evaluate                   re-score the current artifact; writes nothing\n\
     \x20  rollback --consent <path> [--to <run-id>]   restore a previous artifact\n\
     \x20  status                     ledger summary and the current artifact\n\
     options:\n\
     \x20  --source memory|specs      corpus source (default: memory)\n\
     \x20  --consent <path>           consent file holding `abbey-train-consent: v1`\n\
     \x20  --to <run-id>              the retained run to restore (rollback only)\n\
     note: the weight update is delegated to the external `abi` trainer; Abbey\n\
     \x20      owns no trainer, optimizer, or model weights";

/// What one `abbey train` invocation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The process exit code.
    pub code: i32,
    /// The lines to print on standard output.
    pub out: Vec<String>,
    /// The lines to print on standard error.
    pub err: Vec<String>,
}

impl Outcome {
    /// A successful outcome.
    fn ok(out: Vec<String>) -> Self {
        Self {
            code: 0,
            out,
            err: Vec::new(),
        }
    }

    /// A refusal.
    fn error(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            out: Vec::new(),
            err: vec![message.into()],
        }
    }

    /// A usage failure: the message, then the usage, on standard error.
    fn usage_error(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            out: Vec::new(),
            err: vec![message.into(), USAGE.to_string()],
        }
    }
}

/// Run `abbey train` and print its output.
pub fn run(state: &AbbeyState, args: &[String]) -> Result<i32> {
    if args.first().is_some_and(|arg| arg == "family") {
        let prefix = vec!["train".to_string(), "family".to_string()];
        let value = match abbey_learning::run(&args[1..], &prefix) {
            Ok(value) => value,
            Err(error) => {
                if let Some(help) = error.downcast_ref::<clap::Error>()
                    && matches!(
                        help.kind(),
                        clap::error::ErrorKind::DisplayHelp
                            | clap::error::ErrorKind::DisplayVersion
                    )
                {
                    crate::output::println(help.to_string())?;
                    return Ok(0);
                }
                return Err(error);
            }
        };
        crate::output::println(&serde_json::to_string(&value)?)?;
        return Ok(0);
    }
    let outcome = dispatch(state, args)?;
    for line in &outcome.out {
        let _ = crate::output::println(line);
    }
    for line in &outcome.err {
        eprintln!("{line}");
    }
    Ok(outcome.code)
}

/// Run `abbey train` and return its outcome without printing.
pub fn dispatch(state: &AbbeyState, args: &[String]) -> Result<Outcome> {
    dispatch_with(state, args, None)
}

/// Run `abbey train` with an explicit trainer binary.
///
/// `trainer` of `None` resolves the external trainer through the existing
/// `abi` resolution path. The override exists so the pipeline can be driven
/// against a fixture trainer without touching the environment.
pub(crate) fn dispatch_with(
    state: &AbbeyState,
    args: &[String],
    trainer: Option<&Path>,
) -> Result<Outcome> {
    let options = match args::parse(args) {
        Ok(options) => options,
        Err(error) => return Ok(Outcome::usage_error(2, format!("abbey train: {error:#}"))),
    };
    match options.verb {
        Verb::Help => Ok(Outcome::ok(vec![USAGE.to_string()])),
        Verb::Plan => cmd_plan(state, &options),
        Verb::Prepare => cmd_prepare(state, &options),
        Verb::Run => cmd_run(state, &options, trainer),
        Verb::Evaluate => cmd_evaluate(state, &options, trainer),
        Verb::Rollback => cmd_rollback(state, &options),
        Verb::Status => cmd_status(state, &options),
    }
}

/// `abbey train plan` — a read-only preview. It writes nothing.
fn cmd_plan(state: &AbbeyState, options: &Options) -> Result<Outcome> {
    let mut out = vec!["abbey train plan — read-only preview (no file is written)".to_string()];
    out.push(format!("source:        {}", options.source.label()));
    let corpus = build_corpus(state, options.source, &configured_backend())?;
    if corpus.pairs.is_empty() {
        out.push("corpus:        no curated pairs — nothing to train".to_string());
    } else {
        out.push(format!("corpus:        {} pair(s)", corpus.pairs.len()));
        out.push(format!("redacted:      {} pair(s)", corpus.redacted_pairs));
        out.push(format!(
            "split:         {} · {}/{} train/validation",
            digest::SPLIT_RULE,
            split::TRAIN_PERCENT,
            100 - split::TRAIN_PERCENT
        ));
        out.push(format!(
            "train pairs:   {}",
            corpus.train(split::TRAIN_PERCENT).len()
        ));
        out.push(format!(
            "validation:    {}",
            corpus.validation(split::TRAIN_PERCENT).len()
        ));
        out.push(format!(
            "dataset:       {}",
            dataset::dataset_digest(&corpus.pairs, split::TRAIN_PERCENT)
        ));
    }
    for reason in [
        dataset::SkipReason::Obsolete,
        dataset::SkipReason::MissingProvenance,
        dataset::SkipReason::BelowConfidence,
        dataset::SkipReason::NotAPair,
        dataset::SkipReason::Blank,
    ] {
        let count = corpus.skipped_count(reason);
        if count > 0 {
            out.push(format!(
                "skipped:       {count} candidate(s) — {}",
                reason.label()
            ));
        }
    }
    if let Some(report) = &corpus.specs {
        out.push(format!(
            "specs:         {} claim(s), {} goal(s) from {}",
            report.claims,
            report.goals,
            report
                .goals_path
                .clone()
                .unwrap_or_else(|| "no goal ledger found".to_string())
        ));
        if report.skipped_goal_blocks > 0 {
            out.push(format!(
                "skipped:       {} malformed goal block(s)",
                report.skipped_goal_blocks
            ));
        }
    }
    out.push(format!("consent:       {}", consent_state(options)));
    out.push(format!("trainer:       {}", trainer_state()));
    out.push("metric:        corpus reproduction, probed from the held-out split".to_string());
    Ok(Outcome::ok(out))
}

/// `abbey train prepare` — write the dataset and the manifest.
fn cmd_prepare(state: &AbbeyState, options: &Options) -> Result<Outcome> {
    let Some(consent) = require_consent(options) else {
        return Ok(Outcome::usage_error(
            2,
            "abbey train prepare requires --consent <path>",
        ));
    };
    let consent = match consent {
        Ok(consent) => consent,
        Err(error) => return Ok(Outcome::error(2, format!("{error:#}"))),
    };
    let corpus = build_corpus(state, options.source, &configured_backend())?;
    if corpus.pairs.is_empty() {
        return Ok(Outcome::error(
            1,
            "no curated pairs — there is nothing to prepare",
        ));
    }
    let run_id = runs::new_run_id();
    let prepared = runs::write_run(
        &state.state_dir,
        &run_id,
        &corpus,
        options.source.label(),
        &consent.digest,
    )?;
    let mut row = ledger::LedgerRow::new("prepare");
    row.run_id = Some(run_id.clone());
    row.source = Some(options.source.label().to_string());
    row.dataset_digest = Some(prepared.manifest.dataset_digest.clone());
    row.dataset_path = Some(prepared.dataset.display().to_string());
    row.manifest_digest = Some(prepared.manifest_digest.clone());
    row.consent_digest = Some(consent.digest.clone());
    row.detail = format!(
        "{} training pair(s), {} validation pair(s)",
        prepared.manifest.train_pairs, prepared.manifest.validation_pairs
    );
    ledger::append(&state.state_dir, &mut row)?;
    Ok(Outcome::ok(vec![
        format!("abbey train prepare — run {run_id}"),
        format!("source:      {}", options.source.label()),
        format!("dataset:     {}", prepared.dataset.display()),
        format!("manifest:    {}", prepared.manifest_path.display()),
        format!(
            "pairs:       {} training, {} validation",
            prepared.manifest.train_pairs, prepared.manifest.validation_pairs
        ),
        format!("digest:      {}", prepared.manifest.dataset_digest),
        format!("consent:     {}", consent.digest),
    ]))
}

/// `abbey train run` — train, evaluate, decide, and record.
fn cmd_run(state: &AbbeyState, options: &Options, trainer: Option<&Path>) -> Result<Outcome> {
    let Some(consent) = require_consent(options) else {
        return Ok(Outcome::usage_error(
            2,
            "abbey train run requires --consent <path>",
        ));
    };
    let consent = match consent {
        Ok(consent) => consent,
        Err(error) => return Ok(Outcome::error(2, format!("{error:#}"))),
    };
    // The trainer is resolved before anything is written: a missing trainer
    // must not leave a half-prepared run behind.
    let binary = match trainer {
        Some(binary) => binary.to_path_buf(),
        None => match resolve_trainer() {
            Ok(path) => path,
            Err(error) => return Ok(Outcome::error(1, format!("{error:#}"))),
        },
    };
    let corpus = build_corpus(state, options.source, &configured_backend())?;
    if corpus.pairs.is_empty() {
        return Ok(Outcome::error(
            1,
            "no curated pairs — there is nothing to train",
        ));
    }
    let run_id = runs::new_run_id();
    let prepared = runs::write_run(
        &state.state_dir,
        &run_id,
        &corpus,
        options.source.label(),
        &consent.digest,
    )?;
    let artifact = prepared.run_dir.join(trainer::ARTIFACT_FILE_NAME);
    trainer::ensure_artifact_inside(&prepared.run_dir, &artifact)?;
    // A trainer failure leaves no artifact and no ledger row: the run did not
    // happen, so the ledger does not say it did.
    let trained = match trainer::run_trainer(
        &binary,
        &prepared.dataset,
        &artifact,
        trainer::TRAIN_TIMEOUT,
    ) {
        Ok(trained) => trained,
        Err(error) => return Ok(Outcome::error(1, format!("{error:#}"))),
    };

    let validation = corpus.validation(split::TRAIN_PERCENT);
    // The corpus the trainer consumed, read back from the file it consumed —
    // never from an in-memory copy — so the scored corpus is exactly the
    // text the artifact was trained on.
    let corpus_text = trainer::read_corpus(&prepared.dataset)?;
    let seeds = evaluate::probe_seeds(&validation);
    let leak = evaluate::leak_check(&corpus_text, &validation);
    let score = evaluate::score_artifact(
        &binary,
        &trained.artifact,
        &corpus_text,
        &seeds,
        trainer::SAMPLE_TIMEOUT,
    )?;
    let current = ledger::read_current(&state.state_dir)?;
    // The baseline is scored with the same probes and against the same corpus
    // text, so the two artifacts are directly comparable.
    let (baseline, baseline_note) = match &current {
        Some(current) => match evaluate::score_artifact(
            &binary,
            Path::new(&current.artifact_path),
            &corpus_text,
            &seeds,
            trainer::SAMPLE_TIMEOUT,
        ) {
            Ok(baseline) => (Some(baseline.score), String::new()),
            Err(error) => (None, format!("baseline unavailable: {error:#}")),
        },
        None => (None, String::new()),
    };
    // A holdout that leaked into the training corpus is not a holdout, so
    // nothing is promoted — whatever the score says.
    let decision = if current.is_some() && baseline.is_none() {
        evaluate::Decision::NotEvaluated
    } else if leak.is_clean() {
        evaluate::decide((score.probes > 0).then_some(score.score), baseline)
    } else {
        evaluate::Decision::ValidationLeak
    };

    let mut out = vec![format!("abbey train run — run {run_id}")];
    out.push(format!("source:      {}", options.source.label()));
    out.push(format!("dataset:     {}", prepared.manifest.dataset_digest));
    out.push(format!("consent:     {}", consent.digest));
    out.push(format!("artifact:    {}", trained.artifact.display()));
    out.push(format!("digest:      {}", trained.artifact_digest));
    out.push(format!(
        "reproduction: {:.4} ({} probe(s))",
        score.score, score.probes
    ));
    match baseline {
        Some(baseline) => out.push(format!("baseline:    {baseline:.4}")),
        None => out.push("baseline:    none".to_string()),
    }
    if !baseline_note.is_empty() {
        out.push(format!("note:        {baseline_note}"));
    }
    out.push("metric:      corpus reproduction (not held-out quality)".to_string());
    out.push(format!("leak:        {}", leak.summary()));
    out.push(format!("decision:    {}", decision.label()));

    if decision.promotes() {
        let target = ledger::current_artifact_path(&state.state_dir);
        std::fs::copy(&trained.artifact, &target)?;
        let current = ledger::CurrentArtifact {
            run_id: run_id.clone(),
            artifact_path: trained.artifact.display().to_string(),
            artifact_digest: trained.artifact_digest.clone(),
            reproduction_score: Some(score.score),
            updated_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        };
        ledger::write_current(&state.state_dir, &current)?;
        out.push(format!(
            "current:     {}",
            ledger::current_path(&state.state_dir).display()
        ));
    } else {
        out.push("current:     unchanged (the previous artifact is kept)".to_string());
    }

    let mut row = ledger::LedgerRow::new("run");
    row.run_id = Some(run_id.clone());
    row.source = Some(options.source.label().to_string());
    row.dataset_digest = Some(prepared.manifest.dataset_digest.clone());
    row.dataset_path = Some(prepared.dataset.display().to_string());
    row.manifest_digest = Some(prepared.manifest_digest.clone());
    row.consent_digest = Some(consent.digest.clone());
    row.artifact_path = Some(trained.artifact.display().to_string());
    row.artifact_digest = Some(trained.artifact_digest.clone());
    row.reproduction_score = Some(score.score);
    row.baseline_reproduction_score = baseline;
    row.metric = Some(evaluate::METRIC.to_string());
    row.validation_leak = Some(leak.summary());
    row.decision = Some(decision.as_str().to_string());
    row.detail = if baseline_note.is_empty() {
        format!(
            "{} probe(s); context window {} character(s)",
            score.probes, score.context_length
        )
    } else {
        format!(
            "{} probe(s); context window {} character(s); {baseline_note}",
            score.probes, score.context_length
        )
    };
    ledger::append(&state.state_dir, &mut row)?;
    Ok(Outcome::ok(out))
}

/// `abbey train evaluate` — re-score the current artifact. It writes nothing.
fn cmd_evaluate(state: &AbbeyState, options: &Options, trainer: Option<&Path>) -> Result<Outcome> {
    let Some(current) = ledger::read_current(&state.state_dir)? else {
        return Ok(Outcome::error(
            1,
            "no current artifact — `abbey train run --consent <path>` installs one",
        ));
    };
    let binary = match trainer {
        Some(binary) => binary.to_path_buf(),
        None => match resolve_trainer() {
            Ok(path) => path,
            Err(error) => return Ok(Outcome::error(1, format!("{error:#}"))),
        },
    };
    // The run that produced the current artifact owns the corpus it was
    // trained on and the held-out split it held out, so the evaluation reads
    // the run's own dataset and manifest rather than whatever the corpus
    // looks like now.
    let run_dir = Path::new(&current.artifact_path).parent().ok_or_else(|| {
        anyhow::anyhow!(
            "the recorded artifact path names no run directory: {}",
            current.artifact_path
        )
    })?;
    let manifest = runs::read_manifest(&run_dir.join(runs::MANIFEST_FILE_NAME))?;
    let validation = manifest.validation_pairs();
    let validation_refs: Vec<&dataset::Pair> = validation.iter().collect();
    let corpus_text = trainer::read_corpus(&run_dir.join(runs::DATASET_FILE_NAME))?;
    let seeds = evaluate::probe_seeds(&validation_refs);
    let leak = evaluate::leak_check(&corpus_text, &validation_refs);
    let score = evaluate::score_artifact(
        &binary,
        Path::new(&current.artifact_path),
        &corpus_text,
        &seeds,
        trainer::SAMPLE_TIMEOUT,
    )?;
    let previous = previous_promoted(state, &current.run_id)?;
    let baseline = match previous {
        Some(previous) => match &previous.artifact_path {
            Some(path) => evaluate::score_artifact(
                &binary,
                Path::new(path),
                &corpus_text,
                &seeds,
                trainer::SAMPLE_TIMEOUT,
            )
            .ok()
            .map(|baseline| baseline.score),
            None => None,
        },
        None => None,
    };
    // The same rule the run path applies: a leaked holdout is never a
    // promotion, whatever the score says.
    let decision = if leak.is_clean() {
        evaluate::decide((score.probes > 0).then_some(score.score), baseline)
    } else {
        evaluate::Decision::ValidationLeak
    };
    let mut out = vec!["abbey train evaluate — read-only (no file is written)".to_string()];
    out.push(format!("current:     {}", current.run_id));
    out.push(format!("artifact:    {}", current.artifact_path));
    out.push(format!(
        "reproduction: {:.4} ({} probe(s))",
        score.score, score.probes
    ));
    match baseline {
        Some(baseline) => out.push(format!("baseline:    {baseline:.4}")),
        None => out.push("baseline:    none".to_string()),
    }
    out.push("metric:      corpus reproduction (not held-out quality)".to_string());
    out.push(format!("leak:        {}", leak.summary()));
    if options.source.label() != manifest.source {
        out.push(format!(
            "note:        the run was prepared from the {} source; its own dataset is scored",
            manifest.source
        ));
    }
    out.push(format!("decision:    {}", decision.label()));
    Ok(Outcome::ok(out))
}

/// `abbey train rollback` — restore a previous artifact.
fn cmd_rollback(state: &AbbeyState, options: &Options) -> Result<Outcome> {
    let Some(consent) = require_consent(options) else {
        return Ok(Outcome::usage_error(
            2,
            "abbey train rollback requires --consent <path>",
        ));
    };
    let consent = match consent {
        Ok(consent) => consent,
        Err(error) => return Ok(Outcome::error(2, format!("{error:#}"))),
    };
    let rows = ledger::read(&state.state_dir)?;
    let current = ledger::read_current(&state.state_dir)?;
    let target: Option<ledger::LedgerRow> = match &options.to {
        Some(run_id) => rows
            .iter()
            .find(|row| row.op == "run" && row.run_id.as_deref() == Some(run_id.as_str()))
            .cloned(),
        None => {
            let promoted = ledger::promoted_runs(&rows);
            match current.as_ref().and_then(|current| {
                promoted
                    .iter()
                    .position(|row| row.run_id.as_deref() == Some(current.run_id.as_str()))
            }) {
                Some(0) => None,
                Some(index) => promoted.get(index - 1).map(|row| (*row).clone()),
                None => promoted.last().map(|row| (*row).clone()),
            }
        }
    };
    let Some(target) = target else {
        return Ok(Outcome::error(
            1,
            "no previous artifact to restore — every retained run is already current",
        ));
    };
    let Some(artifact_path) = target.artifact_path.clone() else {
        return Ok(Outcome::error(1, "the recorded run produced no artifact"));
    };
    let artifact = PathBuf::from(&artifact_path);
    if !artifact.is_file() {
        return Ok(Outcome::error(
            1,
            format!("the retained artifact is missing: {artifact_path}"),
        ));
    }
    let digest = digest::digest_file(digest::ARTIFACT_DOMAIN, &artifact)?;
    let copied = std::fs::copy(&artifact, ledger::current_artifact_path(&state.state_dir))?;
    let run_id = target.run_id.clone().unwrap_or_default();
    let current = ledger::CurrentArtifact {
        run_id: run_id.clone(),
        artifact_path: artifact_path.clone(),
        artifact_digest: digest.clone(),
        reproduction_score: target.reproduction_score,
        updated_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    ledger::write_current(&state.state_dir, &current)?;
    let mut row = ledger::LedgerRow::new("rollback");
    row.consent_digest = Some(consent.digest);
    row.run_id = Some(run_id.clone());
    row.artifact_path = Some(artifact_path.clone());
    row.artifact_digest = Some(digest.clone());
    row.reproduction_score = target.reproduction_score;
    row.metric = target.metric.clone();
    row.detail = format!("restored {copied} byte(s) of the retained artifact");
    ledger::append(&state.state_dir, &mut row)?;
    Ok(Outcome::ok(vec![
        format!("abbey train rollback — restored run {run_id}"),
        format!("artifact:    {artifact_path}"),
        format!("digest:      {digest}"),
        format!(
            "current:     {}",
            ledger::current_path(&state.state_dir).display()
        ),
        "note:        every artifact is retained, so this rollback is itself reversible"
            .to_string(),
    ]))
}

/// `abbey train status` — the ledger summary and the current artifact.
fn cmd_status(state: &AbbeyState, options: &Options) -> Result<Outcome> {
    let rows = ledger::read(&state.state_dir)?;
    let current = ledger::read_current(&state.state_dir)?;
    let mismatches = rows.iter().filter(|row| !row.digest_matches()).count();
    let mut out = vec!["abbey train status".to_string()];
    out.push(format!("source:      {}", options.source.label()));
    out.push(format!(
        "ledger:      {}",
        ledger::ledger_path(&state.state_dir).display()
    ));
    out.push(format!("rows:        {}", rows.len()));
    out.push(format!(
        "digest:      {mismatches} row(s) whose digest does not match"
    ));
    let mut prepare = 0usize;
    let mut run = 0usize;
    let mut rollback = 0usize;
    for row in &rows {
        match row.op.as_str() {
            "prepare" => prepare += 1,
            "run" => run += 1,
            "rollback" => rollback += 1,
            _ => {}
        }
    }
    out.push(format!(
        "operations:  prepare={prepare} run={run} rollback={rollback}"
    ));
    match &current {
        Some(current) => {
            out.push(format!("current:     {}", current.run_id));
            out.push(format!("artifact:    {}", current.artifact_path));
            out.push(format!("digest:      {}", current.artifact_digest));
            out.push(format!(
                "reproduction: {}",
                current
                    .reproduction_score
                    .map(|score| format!("{score:.4}"))
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
        None => out.push("current:     none — no artifact has been promoted yet".to_string()),
    }
    let retained = rows
        .iter()
        .filter(|row| row.op == "run" && row.artifact_path.is_some())
        .count();
    out.push(format!("artifacts:   {retained} retained run artifact(s)"));
    Ok(Outcome::ok(out))
}

/// The consent file's state, for the read-only plan.
fn consent_state(options: &Options) -> String {
    let Some(path) = &options.consent else {
        return "not supplied — no dataset will be written and no trainer will run".to_string();
    };
    match consent::read(path) {
        Ok(consent) => format!("granted: {} ({})", path.display(), consent.digest),
        Err(error) => format!("not granted: {error:#}"),
    }
}

/// Whether the external trainer resolves.
fn trainer_state() -> String {
    match resolve_trainer() {
        Ok(path) => format!("resolved: {}", path.display()),
        Err(error) => format!("not resolved: {error:#}"),
    }
}

/// The consent gate, when the option names a file.
fn require_consent(options: &Options) -> Option<Result<consent::Consent>> {
    options.consent.as_ref().map(|path| consent::read(path))
}

/// The external trainer binary, through the existing `abi` resolution path.
fn resolve_trainer() -> Result<PathBuf> {
    resolve_agent_for(AgentBackend::Abi).map_err(|error| {
        anyhow::anyhow!(
            "the external trainer could not be resolved — {error:#}\n\
             the weight update is delegated to the `abi` binary; install it, or set \
             ABBEY_ABI_BIN / `abi_bin` in config.toml"
        )
    })
}

/// The corpus for a source.
fn build_corpus(state: &AbbeyState, source: Source, backend: &str) -> Result<Corpus> {
    match source {
        Source::Memory => {
            // Read-only callers must not create the store.
            if !crate::memory::backend_path(&state.state_dir, backend).exists() {
                return Ok(Corpus {
                    source: Some(Source::Memory),
                    ..Corpus::default()
                });
            }
            let mem = crate::memory::open_backend(&state.state_dir, backend)?;
            dataset::memory_corpus(mem.as_ref())
        }
        Source::Specs => Ok(dataset::specs_corpus(goals_path(&state.cwd).as_deref())),
    }
}

/// The configured memory backend name.
fn configured_backend() -> String {
    crate::config::AbbeyConfig::load()
        .unwrap_or_default()
        .memory_backend
}

/// The goal ledger, found by walking up from the working directory.
fn goals_path(cwd: &Path) -> Option<PathBuf> {
    let mut current = Some(cwd);
    for _ in 0..16 {
        let directory = current?;
        let candidate = directory.join("tasks/goals.md");
        if candidate.is_file() {
            return Some(candidate);
        }
        current = directory.parent();
    }
    None
}

/// The promoted run before `run_id`, when there is one.
fn previous_promoted(state: &AbbeyState, run_id: &str) -> Result<Option<ledger::LedgerRow>> {
    let rows = ledger::read(&state.state_dir)?;
    let promoted = ledger::promoted_runs(&rows);
    let position = promoted
        .iter()
        .position(|row| row.run_id.as_deref() == Some(run_id));
    Ok(match position {
        Some(0) => None,
        Some(index) => promoted.get(index - 1).map(|row| (*row).clone()),
        None => None,
    })
}
