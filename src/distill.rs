//! `abbey learn distill` — teacher-generated training data, local first.
//!
//! Apple's on-device Foundation Model (`fm --model system`), the local Ollama
//! model, and Apple Private Cloud Compute (`fm --model pcc`, off-device) answer
//! curated prompts; each answer is stored as a `train_candidate` record whose
//! provenance names the exact teacher and model. `abbey learn sft` exports
//! those pairs as chat-messages JSONL for an external fine-tuning run.
//!
//! This is data generation and curation only. Nothing here updates weights:
//! the LoRA/fine-tune pipeline stays Proposed (`abbey claims proposed`).
//! Teacher calls are headless one-shot captures through
//! [`crate::capture::capture_oneshot`] — no conversation resume, no persona
//! wrap, no route-log row.

use crate::agent::{AgentBackend, AgentConfig, resolve_agent_for};
use crate::memory::{MemoryRecord, MemoryStore};
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Most prompts one `--file` batch may carry.
const MAX_BATCH: usize = 256;
/// Summary length for review listings.
const SUMMARY_CHARS: usize = 120;
/// Confidence for unreviewed teacher output (below a user correction's 0.95).
const TEACHER_CONFIDENCE: f32 = 0.6;

/// One teacher model. The order of [`Teacher::LOCAL_FIRST`] is the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Teacher {
    /// On-device Apple Foundation Model (`fm respond --model system`).
    FmSystem,
    /// Local Ollama model (`ollama run gemma4:26b-mlx`), never auto-pulled.
    Ollama,
    /// Apple Foundation Model on Private Cloud Compute — off-device.
    FmPcc,
}

impl Teacher {
    /// Default chain: both on-device teachers before the off-device one.
    pub const LOCAL_FIRST: [Teacher; 3] = [Self::FmSystem, Self::Ollama, Self::FmPcc];

    /// Parse a `--teacher` value into the chain it selects.
    pub fn chain(value: &str) -> Option<Vec<Teacher>> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "all" => Self::LOCAL_FIRST.to_vec(),
            "local" | "on-device" => vec![Self::FmSystem, Self::Ollama],
            "fm" | "system" | "fm:system" => vec![Self::FmSystem],
            "pcc" | "fm:pcc" | "private-cloud-compute" => vec![Self::FmPcc],
            "ollama" | "gemma" | "local-model" => vec![Self::Ollama],
            _ => return None,
        })
    }

    pub fn backend(self) -> AgentBackend {
        match self {
            Self::FmSystem | Self::FmPcc => AgentBackend::Fm,
            Self::Ollama => AgentBackend::Ollama,
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Self::FmSystem => "system",
            Self::FmPcc => "pcc",
            Self::Ollama => crate::models::OLLAMA_DEFAULT_MODEL,
        }
    }

    pub fn label(self) -> String {
        format!("{}:{}", self.backend().label(), self.model())
    }

    /// False only for PCC, which leaves the device (Apple Private Cloud Compute).
    pub fn on_device(self) -> bool {
        !matches!(self, Self::FmPcc)
    }
}

/// The stored prompt/response pair (the record's `payload`, as JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistillPair {
    pub prompt: String,
    pub response: String,
    pub teacher: String,
    pub on_device: bool,
}

/// Why one teacher did not produce an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeacherMiss {
    pub teacher: Teacher,
    pub reason: String,
}

/// Outcome of asking a chain for one prompt.
#[derive(Debug)]
pub struct ChainResult {
    pub answers: Vec<(Teacher, String)>,
    pub misses: Vec<TeacherMiss>,
}

/// Walk `chain` in order. Without `all`, stop at the first non-empty answer;
/// with `all`, ask every teacher. `ask` is the only side effect.
pub fn ask_chain(
    chain: &[Teacher],
    all: bool,
    prompt: &str,
    ask: &mut dyn FnMut(Teacher, &str) -> Result<String>,
) -> ChainResult {
    let mut out = ChainResult {
        answers: Vec::new(),
        misses: Vec::new(),
    };
    for &teacher in chain {
        match ask(teacher, prompt) {
            Ok(text) if !text.trim().is_empty() => {
                out.answers.push((teacher, text.trim().to_string()));
                if !all {
                    break;
                }
            }
            Ok(_) => out.misses.push(TeacherMiss {
                teacher,
                reason: "empty response".into(),
            }),
            Err(err) => out.misses.push(TeacherMiss {
                teacher,
                reason: format!("{err:#}"),
            }),
        }
    }
    out
}

/// Ask one real teacher through the shared headless capture path.
fn ask_teacher(teacher: Teacher, prompt: &str, owner: Option<&AgentConfig>) -> Result<String> {
    if let Some(cfg) = owner {
        cfg.check_cancelled()?;
    }
    let backend = teacher.backend();
    let path = resolve_agent_for(backend)?;
    let ready = if teacher != Teacher::Ollama {
        true
    } else if let Some(cfg) = owner {
        let (status, stdout, _) = cfg.capture_command(&path, &["list".into()], None)?;
        status.success()
            && stdout.lines().skip(1).any(|line| {
                line.split_whitespace().next() == Some(crate::models::OLLAMA_DEFAULT_MODEL)
            })
    } else {
        crate::agent::ollama_lists_model(&path, crate::models::OLLAMA_DEFAULT_MODEL)
    };
    if !ready {
        // `ollama run` would pull a missing tag; a teacher never downloads.
        bail!(
            "{} is not pulled locally (run `ollama pull {}` first)",
            crate::models::OLLAMA_DEFAULT_MODEL,
            crate::models::OLLAMA_DEFAULT_MODEL
        );
    }
    let mut cfg = AgentConfig::fixed_provider_recipe(path, backend, teacher.model().into());
    cfg.stream = owner.and_then(|owner| owner.stream.clone());
    let run = crate::capture::capture_oneshot(&mut cfg, &[prompt.to_string()])?;
    if !run.status.success() {
        let tail = crate::agent::utf8_tail(run.stderr.trim(), 400);
        bail!("exited with {}: {tail}", run.status);
    }
    Ok(crate::agent::strip_ansi(&run.stdout))
}

/// Build the `train_candidate` record for one teacher answer.
pub fn candidate_record(teacher: Teacher, prompt: &str, response: &str) -> Result<MemoryRecord> {
    let pair = DistillPair {
        prompt: prompt.to_string(),
        response: response.to_string(),
        teacher: teacher.label(),
        on_device: teacher.on_device(),
    };
    let summary: String = prompt.chars().take(SUMMARY_CHARS).collect();
    let mut rec = MemoryRecord::new_stm(summary, serde_json::to_string(&pair)?);
    rec.origin = "teacher".into();
    rec.source_type = "distill".into();
    rec.source_ref = teacher.label();
    rec.retention = "train_candidate".into();
    rec.tags = vec![
        "train_candidate".into(),
        "distill".into(),
        format!("teacher:{}", teacher.label()),
        if teacher.on_device() {
            "on-device".into()
        } else {
            "off-device".into()
        },
    ];
    rec.confidence = TEACHER_CONFIDENCE;
    rec.provenance = format!("teacher {} @ {}", teacher.label(), rec.timestamp);
    Ok(rec)
}

/// Chat-messages JSONL line for one stored pair, or `None` when the record is
/// not a distill pair (hand-written `learn train` rows have no response).
pub fn sft_line(rec: &MemoryRecord) -> Option<String> {
    if rec.source_type != "distill" || rec.obsolete {
        return None;
    }
    let pair: DistillPair = serde_json::from_str(&rec.payload).ok()?;
    let line = serde_json::json!({
        "messages": [
            {"role": "user", "content": pair.prompt},
            {"role": "assistant", "content": pair.response},
        ],
        "teacher": pair.teacher,
        "on_device": pair.on_device,
        "id": rec.id,
    });
    Some(line.to_string())
}

struct DistillArgs {
    chain: Vec<Teacher>,
    all: bool,
    prompts: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<DistillArgs> {
    let mut chain = Teacher::LOCAL_FIRST.to_vec();
    let mut all = false;
    let mut prompts = Vec::new();
    let mut words = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--teacher" | "-t" => {
                let Some(v) = it.next() else {
                    bail!("--teacher needs a value: auto|local|fm|ollama|pcc");
                };
                let Some(c) = Teacher::chain(v) else {
                    bail!("unknown teacher `{v}` (auto|local|fm|ollama|pcc)");
                };
                all = all || v.eq_ignore_ascii_case("all");
                chain = c;
            }
            "--all" => all = true,
            "--file" | "-f" => {
                let Some(path) = it.next() else {
                    bail!("--file needs a path (one prompt per line)");
                };
                let text = std::fs::read_to_string(path)?;
                prompts.extend(
                    text.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        .map(String::from),
                );
            }
            "--" => words.extend(it.by_ref().cloned()),
            _ => words.push(arg.clone()),
        }
    }
    if !words.is_empty() {
        prompts.push(words.join(" "));
    }
    if prompts.is_empty() {
        bail!("{USAGE}");
    }
    if prompts.len() > MAX_BATCH {
        bail!(
            "{} prompts exceeds the {MAX_BATCH}-prompt batch limit",
            prompts.len()
        );
    }
    Ok(DistillArgs {
        chain,
        all,
        prompts,
    })
}

const USAGE: &str = "usage: abbey learn distill [--teacher auto|local|fm|ollama|pcc] [--all] \
    [--file prompts.txt] [prompt…]\n\
    default chain is local first: fm:system (on-device) → ollama (local) → fm:pcc (off-device)\n\
    curate: `abbey learn review` lists pairs; `abbey memory invalidate <id>` drops one from `abbey learn sft`";

/// `abbey learn distill …`: store teacher answers as `train_candidate` rows.
pub fn dispatch(state: &AbbeyState, args: &[String]) -> Result<i32> {
    dispatch_impl(state, args, None)
}

pub(crate) fn dispatch_owned(
    state: &AbbeyState,
    args: &[String],
    cfg: &AgentConfig,
) -> Result<i32> {
    dispatch_impl(state, args, Some(cfg))
}

fn dispatch_impl(state: &AbbeyState, args: &[String], owner: Option<&AgentConfig>) -> Result<i32> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    let parsed = parse_args(args)?;
    let mem = crate::learn::open_mem(state)?;
    run_impl(
        mem.as_ref(),
        &parsed,
        &mut |teacher, prompt| ask_teacher(teacher, prompt, owner),
        owner,
    )
}

#[cfg(test)]
fn run(
    mem: &dyn MemoryStore,
    parsed: &DistillArgs,
    ask: &mut dyn FnMut(Teacher, &str) -> Result<String>,
) -> Result<i32> {
    run_impl(mem, parsed, ask, None)
}

fn run_impl(
    mem: &dyn MemoryStore,
    parsed: &DistillArgs,
    ask: &mut dyn FnMut(Teacher, &str) -> Result<String>,
    owner: Option<&AgentConfig>,
) -> Result<i32> {
    let mut stored = 0usize;
    let mut failed = 0usize;
    for prompt in &parsed.prompts {
        if let Some(cfg) = owner {
            cfg.check_cancelled()?;
        }
        let result = ask_chain(&parsed.chain, parsed.all, prompt, &mut |teacher, prompt| {
            if let Some(cfg) = owner {
                cfg.check_cancelled()?;
            }
            ask(teacher, prompt)
        });
        if let Some(cfg) = owner {
            cfg.check_cancelled()?;
        }
        for miss in &result.misses {
            let text = format!("teacher {}: {}", miss.teacher.label(), miss.reason);
            if let Some(cfg) = owner {
                cfg.notice(text);
            } else {
                eprintln!("{text}");
            }
        }
        if result.answers.is_empty() {
            failed += 1;
            continue;
        }
        for (teacher, answer) in &result.answers {
            let rec = candidate_record(*teacher, prompt, answer)?;
            let id = rec.id.clone();
            if let Some(cfg) = owner {
                cfg.check_cancelled()?;
            }
            mem.store(rec)?;
            if let Some(cfg) = owner {
                cfg.output_line(format!("{id}\t{}", teacher.label()));
            } else {
                println!("{}\t{}", id, teacher.label());
            }
            stored += 1;
        }
    }
    let summary = format!(
        "distill: {stored} train_candidate record(s) stored, {failed} prompt(s) unanswered \
         — review with `abbey learn review`, drop bad pairs with `abbey memory invalidate <id>`, \
         export with `abbey learn sft` (no weights are updated)"
    );
    if let Some(cfg) = owner {
        cfg.notice(summary);
    } else {
        eprintln!("{summary}");
    }
    Ok(if stored == 0 { 1 } else { 0 })
}

/// `abbey learn sft`: chat-messages JSONL of every live distill pair.
pub fn export_sft(state: &AbbeyState) -> Result<i32> {
    let mem = crate::learn::open_mem(state)?;
    // Both backends collect actual matching rows without allocating by this limit.
    for rec in mem.filter(Some("train_candidate"), None, usize::MAX)? {
        if let Some(line) = sft_line(&rec) {
            println!("{line}");
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chain_is_local_first_with_pcc_last() {
        assert_eq!(
            Teacher::chain("auto").unwrap(),
            vec![Teacher::FmSystem, Teacher::Ollama, Teacher::FmPcc]
        );
        assert!(
            Teacher::chain("local")
                .unwrap()
                .iter()
                .all(|t| t.on_device())
        );
        assert!(!Teacher::FmPcc.on_device());
        assert_eq!(Teacher::FmPcc.label(), "fm:pcc");
        assert_eq!(Teacher::FmSystem.label(), "fm:system");
        assert_eq!(Teacher::chain("gpt"), None);
    }

    #[test]
    fn chain_stops_at_first_answer_and_records_misses() {
        let mut calls = Vec::new();
        let mut ask = |t: Teacher, _: &str| -> Result<String> {
            calls.push(t);
            match t {
                Teacher::FmSystem => bail!("model unavailable"),
                Teacher::Ollama => Ok("  local answer \n".into()),
                Teacher::FmPcc => Ok("cloud answer".into()),
            }
        };
        let r = ask_chain(&Teacher::LOCAL_FIRST, false, "q", &mut ask);
        assert_eq!(
            calls,
            vec![Teacher::FmSystem, Teacher::Ollama],
            "pcc never asked"
        );
        assert_eq!(
            r.answers,
            vec![(Teacher::Ollama, "local answer".to_string())]
        );
        assert_eq!(r.misses.len(), 1);
        assert!(r.misses[0].reason.contains("unavailable"));
    }

    #[test]
    fn all_asks_every_teacher_and_empty_output_is_a_miss() {
        let mut ask = |t: Teacher, _: &str| -> Result<String> {
            Ok(if t == Teacher::Ollama {
                "   ".into()
            } else {
                t.label()
            })
        };
        let r = ask_chain(&Teacher::LOCAL_FIRST, true, "q", &mut ask);
        assert_eq!(r.answers.len(), 2);
        assert_eq!(r.misses[0].teacher, Teacher::Ollama);
        assert_eq!(r.misses[0].reason, "empty response");
    }

    #[test]
    fn record_carries_teacher_provenance_and_round_trips_to_sft() {
        let rec = candidate_record(Teacher::FmPcc, "What is Swift?", "A language.").unwrap();
        assert_eq!(rec.retention, "train_candidate");
        assert_eq!(rec.source_type, "distill");
        assert!(rec.provenance.starts_with("teacher fm:pcc @ "));
        assert!(rec.tags.contains(&"off-device".to_string()));
        let line: serde_json::Value = serde_json::from_str(&sft_line(&rec).unwrap()).unwrap();
        assert_eq!(line["messages"][0]["content"], "What is Swift?");
        assert_eq!(line["messages"][1]["role"], "assistant");
        assert_eq!(line["teacher"], "fm:pcc");
        assert_eq!(line["on_device"], false);

        let mut gone = rec.clone();
        gone.obsolete = true;
        assert_eq!(sft_line(&gone), None, "obsolete pairs are not exported");
        let hand = MemoryRecord::new_stm("train candidate", "free text");
        assert_eq!(sft_line(&hand), None, "hand-written rows have no response");
    }

    #[test]
    fn args_parse_teacher_file_and_words() {
        let dir = std::env::temp_dir().join(format!("abbey-distill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("p.txt");
        std::fs::write(&file, "# comment\nfirst\n\n second \n").unwrap();
        let args: Vec<String> = [
            "--teacher",
            "pcc",
            "--file",
            file.to_str().unwrap(),
            "third",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let p = parse_args(&args).unwrap();
        assert_eq!(p.chain, vec![Teacher::FmPcc]);
        assert!(!p.all);
        assert_eq!(p.prompts, vec!["first", "second", "third"]);
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["--teacher".into(), "gpt".into(), "x".into()]).is_err());
        assert!(
            parse_args(&["--teacher".into(), "all".into(), "x".into()])
                .unwrap()
                .all
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // External draft only, uncompiled and unexecuted. Append inside the existing
    // #[cfg(test)] mod tests in src/distill.rs (which already imports super::*).
    // These call actual private run/parse_args and the actual owned SQLite store.
    // The subprocess is the Rust test runner for stdout observation, not a CLI
    // generation fixture. It selects exactly one probe and contacts no teacher.

    use crate::memory::SqliteMemory;
    use std::collections::BTreeSet;

    struct DistillReceiptScratch(std::path::PathBuf);

    impl DistillReceiptScratch {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "abbey-distill-receipt-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for DistillReceiptScratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn distill_receipt_probe(reject: Option<&str>, all: bool, expected_rows: usize) {
        let scratch = DistillReceiptScratch::new();
        let path = scratch.0.join("memory.sqlite");
        let mem = SqliteMemory::open(&path).unwrap();
        if let Some(condition) = reject {
            // Fixed test-owned conditions only; no user data or production path.
            let trigger = format!(
                "CREATE TRIGGER owned_distill_failure BEFORE INSERT ON memory \
             WHEN NEW.source_type = 'distill' AND ({condition}) \
             BEGIN SELECT RAISE(FAIL, 'owned fixture rejects teacher row'); END;"
            );
            rusqlite::Connection::open(&path)
                .unwrap()
                .execute_batch(&trigger)
                .unwrap();
        }
        let parsed = if all {
            DistillArgs {
                chain: vec![Teacher::FmSystem, Teacher::FmPcc],
                all: true,
                prompts: vec!["synthetic receipt fixture".into()],
            }
        } else {
            parse_args(&[
                "--teacher".into(),
                "fm".into(),
                "synthetic receipt fixture".into(),
            ])
            .unwrap()
        };
        let mut contacts = 0usize;
        let mut ask = |teacher: Teacher, _: &str| -> Result<String> {
            contacts += 1;
            Ok(format!("owned synthetic {} answer", teacher.label()))
        };
        let result = run(&mem, &parsed, &mut ask);
        if reject.is_some() {
            let error = result.expect_err("real SQLite trigger must reject the store");
            assert!(format!("{error:#}").contains("owned fixture rejects teacher row"));
        } else {
            assert_eq!(result.unwrap(), 0);
        }
        assert_eq!(contacts, if all { 2 } else { 1 });
        drop(mem);
        let reopened = SqliteMemory::open(&path).unwrap();
        let rows = reopened.filter(Some("train_candidate"), None, 10).unwrap();
        assert_eq!(
            rows.len(),
            expected_rows,
            "actual reopened durable row count"
        );
        for row in rows {
            assert_eq!(row.source_type, "distill");
            println!("OWNED-PERSISTED-ID={}", row.id);
        }
        println!("OWNED-DISTILL-PROBE-COMPLETED");
    }

    #[test]
    fn distill_receipt_first_store_failure_probe() {
        distill_receipt_probe(Some("1"), false, 0);
    }

    #[test]
    fn distill_receipt_partial_store_failure_probe() {
        distill_receipt_probe(Some("NEW.source_ref = 'fm:pcc'"), true, 1);
    }

    #[test]
    fn distill_receipt_success_probe() {
        distill_receipt_probe(None, false, 1);
    }

    fn captured_distill_probe(probe: &str) -> (BTreeSet<String>, BTreeSet<String>) {
        // Test harness names omit the crate prefix in module_path!(). This also
        // works if root keeps this draft in a registered test-only child module.
        let module = module_path!()
            .split_once("::")
            .expect("crate/module path")
            .1;
        let exact = format!("{module}::{probe}");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .output()
            .expect("owned exact test probe");
        assert!(
            output.status.success(),
            "fixture probe failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.len() < 16_384 && output.stderr.len() < 16_384);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("OWNED-DISTILL-PROBE-COMPLETED"),
            "exact probe selected zero tests"
        );
        let persisted = text
            .lines()
            .filter_map(|line| line.strip_prefix("OWNED-PERSISTED-ID=").map(str::to_string))
            .collect();
        let issued = text
            .lines()
            .filter_map(|line| {
                let (before_tab, teacher) = line.split_once('\t')?;
                // libtest may prefix the first println with "test <name> ... ".
                // Inspect the final token, so that prefix cannot hide the first ID.
                let id = before_tab.split_whitespace().last()?;
                (matches!(teacher, "fm:system" | "fm:pcc") && uuid::Uuid::parse_str(id).is_ok())
                    .then(|| id.to_string())
            })
            .collect();
        (issued, persisted)
    }

    #[test]
    fn first_store_failure_does_not_issue_a_success_shaped_record_receipt() {
        let (issued, persisted) =
            captured_distill_probe("distill_receipt_first_store_failure_probe");
        assert!(
            persisted.is_empty(),
            "fixture must have zero committed records"
        );
        assert!(
            issued.is_empty(),
            "failed store still published record ID(s): {issued:?}"
        );
    }

    #[test]
    fn partial_store_failure_issues_only_ids_that_reopen_as_committed_records() {
        let (issued, persisted) =
            captured_distill_probe("distill_receipt_partial_store_failure_probe");
        assert_eq!(
            persisted.len(),
            1,
            "fixture retains its first actual committed answer"
        );
        assert_eq!(
            issued, persisted,
            "failed later answer published an uncommitted record ID"
        );
    }

    #[test]
    fn successful_store_keeps_the_existing_record_id_teacher_receipt() {
        let (issued, persisted) = captured_distill_probe("distill_receipt_success_probe");
        assert_eq!(persisted.len(), 1);
        assert_eq!(issued, persisted);
    }
}
