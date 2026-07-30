//! Self-learning from user corrections, routes, and activity → memory layers.

use crate::config;
use crate::memory::{self, MemoryRecord, MemoryStore};
use crate::route_log;
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::path::Path;

fn configured_backend() -> String {
    config::AbbeyConfig::load()
        .unwrap_or_default()
        .memory_backend
}

pub fn open_mem(state: &AbbeyState) -> Result<Box<dyn MemoryStore>> {
    memory::open_backend(&state.state_dir, &configured_backend())
}

/// Capture an explicit user correction into LTM (+ optional train_candidate).
pub fn capture_correction(
    state: &AbbeyState,
    summary: &str,
    detail: &str,
    as_train: bool,
) -> Result<String> {
    let mem = open_mem(state)?;
    let mut rec = MemoryRecord::new_stm(summary, detail);
    rec.origin = "user".into();
    rec.source_type = "correction".into();
    rec.retention = "ltm".into();
    rec.tags = vec!["ltm".into(), "correction".into(), "self-learn".into()];
    rec.confidence = 0.95;
    rec.provenance = format!("user correction @ {}", state.cwd.display());
    if as_train {
        rec.retention = "train_candidate".into();
        rec.tags.push("train_candidate".into());
    }
    let id = rec.id.clone();
    mem.store(rec)?;
    Ok(id)
}

/// Activity payload for one route record (confidence / alt / fb preserved).
fn route_activity_payload(r: &route_log::RouteRecord) -> String {
    format!(
        "{}\nreason={}\nconfidence={:.2}\nalternate={}\nfallback={}",
        r.cwd,
        r.reason,
        r.confidence,
        r.alternate.as_deref().unwrap_or("-"),
        r.fallback.as_deref().unwrap_or("-"),
    )
}

/// Promote recent high-signal route records into activity/LTM digest.
pub fn learn_from_routes(state: &AbbeyState, n: usize) -> Result<usize> {
    let mem = open_mem(state)?;
    let routes = route_log::recent_routes(&state.state_dir, n)?;
    let mut stored = 0;
    for r in routes {
        let mut rec = MemoryRecord::new_stm(
            format!("route {}/{} → {}", r.persona, r.role, r.model),
            route_activity_payload(&r),
        );
        rec.source_type = "route".into();
        rec.retention = "activity".into();
        rec.tags = vec!["activity".into(), "self-learn".into(), r.role.clone()];
        rec.confidence = r.confidence;
        rec.provenance = format!("route.jsonl @ {}", r.ts);
        mem.store(rec)?;
        stored += 1;
    }
    Ok(stored)
}

/// Reflect + auto-promote duplicate-free high-confidence activity → ltm.
pub fn learn_digest(state: &AbbeyState) -> Result<String> {
    let mem = open_mem(state)?;
    let report = mem.reflect()?;
    let activity = mem.filter(Some("activity"), Some("self-learn"), 100)?;
    let mut promoted = 0;
    for r in activity {
        if r.confidence >= 0.8 && !report.low_confidence.contains(&r.id) {
            let _ = mem.promote(&r.id, "ltm");
            promoted += 1;
        }
    }
    Ok(format!(
        "digest: promoted={promoted} low_confidence={} dups={} superseded={}",
        report.low_confidence.len(),
        report.duplicate_summaries.len(),
        report.superseded.len()
    ))
}

/// Preference standing directive into LTM.
pub fn learn_preference(state: &AbbeyState, preference: &str) -> Result<String> {
    let mem = open_mem(state)?;
    let mut rec = MemoryRecord::new_stm(
        format!(
            "preference: {}",
            preference.chars().take(80).collect::<String>()
        ),
        preference,
    );
    rec.origin = "user".into();
    rec.source_type = "preference".into();
    rec.retention = "ltm".into();
    rec.tags = vec!["ltm".into(), "preference".into(), "self-learn".into()];
    rec.confidence = 0.99;
    rec.provenance = "abbey learn preference".into();
    let id = rec.id.clone();
    mem.store(rec)?;
    Ok(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainStats {
    pub total: usize,
    pub with_provenance: usize,
    pub high_confidence: usize,
}

impl TrainStats {
    pub fn missing_provenance(self) -> usize {
        self.total.saturating_sub(self.with_provenance)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrainCuration {
    stats: TrainStats,
    ready: usize,
}

fn collect_train_curation(mem: &dyn MemoryStore) -> Result<TrainCuration> {
    let rows = mem.filter(Some("train_candidate"), None, 10_000)?;
    let with_provenance = rows
        .iter()
        .filter(|r| !r.provenance.trim().is_empty())
        .count();
    let high_confidence = rows.iter().filter(|r| r.confidence >= 0.9).count();
    let ready = rows
        .iter()
        .filter(|r| !r.provenance.trim().is_empty() && r.confidence >= 0.9)
        .count();
    Ok(TrainCuration {
        stats: TrainStats {
            total: rows.len(),
            with_provenance,
            high_confidence,
        },
        ready,
    })
}

pub fn status(state: &AbbeyState) -> Result<()> {
    let backend = configured_backend();
    let path = memory::backend_path(&state.state_dir, &backend);
    println!("abbey learn — self-learn + train_candidate curation (not a LoRA runner)\n");
    println!("store: {}", path.display());
    // `status` is read-only: opening a backend would create the store.
    if !path.exists() {
        println!("(empty — capture corrections / run learn routes)");
        println!();
        print_learn_usage();
        return Ok(());
    }
    let mem = open_mem(state)?;
    for layer in ["stm", "ltm", "activity", "train_candidate"] {
        let n = mem.filter(Some(layer), Some("self-learn"), 500)?.len();
        println!("  {layer:<16} self-learn={n}");
    }
    let report = mem.reflect()?;
    println!(
        "reflect: low={} dups={} superseded={}",
        report.low_confidence.len(),
        report.duplicate_summaries.len(),
        report.superseded.len()
    );
    let cur = collect_train_curation(mem.as_ref())?;
    println!(
        "train_candidate: total={} prov_ok={} missing={} high_conf={} ready={}",
        cur.stats.total,
        cur.stats.with_provenance,
        cur.stats.missing_provenance(),
        cur.stats.high_confidence,
        cur.ready
    );
    println!();
    println!("curate:  abbey learn review · abbey learn stats · abbey learn export");
    println!("refuse:  abbey learn lora · abbey claims refuse lora");
    Ok(())
}

fn print_learn_usage() {
    println!(
        "usage:\n\
         \x20  abbey learn                    # status + train_candidate summary\n\
         \x20  abbey learn review [n]         # list candidates (provenance gate)\n\
         \x20  abbey learn stats              # curation counts\n\
         \x20  abbey learn train <text>       # add train_candidate with provenance\n\
         \x20  abbey learn correction <text>  # LTM correction\n\
         \x20  abbey learn preference <text>  # LTM standing directive\n\
         \x20  abbey learn routes [n]         # route.jsonl → activity\n\
         \x20  abbey learn digest|export …\n\
         note:  LoRA / fine-tune is Out of scope — curation only"
    );
}

/// Human-review listing of `train_candidate` rows (no LoRA / export-to-weights).
pub fn review_train(state: &AbbeyState, limit: usize) -> Result<()> {
    println!("abbey learn review — train_candidate curation (no weight updates)\n");
    let path = memory::backend_path(&state.state_dir, &configured_backend());
    if !path.exists() {
        println!("(no store yet — `abbey learn train <text>` or `abbey learn correction …`)");
        return Ok(());
    }
    let mem = open_mem(state)?;
    let rows = mem.filter(Some("train_candidate"), None, limit)?;
    if rows.is_empty() {
        println!("(no train_candidate records — `abbey learn train <text>`)");
        return Ok(());
    }
    let mut missing_prov = 0usize;
    let mut ready = 0usize;
    for r in &rows {
        let prov_ok = !r.provenance.trim().is_empty();
        if !prov_ok {
            missing_prov += 1;
        }
        let is_ready = prov_ok && r.confidence >= 0.9;
        if is_ready {
            ready += 1;
        }
        let preview: String = r.payload.chars().take(120).collect();
        println!(
            "{}\tconf={:.2}\tprov={}\tready={}\t{}",
            r.id,
            r.confidence,
            if prov_ok { "ok" } else { "MISSING" },
            if is_ready { "yes" } else { "no" },
            preview
        );
    }
    println!(
        "\nreview: {} candidate(s); {} missing provenance; {} ready (prov+conf≥0.9)\n\
         next:   abbey learn stats · abbey learn export train_candidate\n\
         oos:    LoRA runners — `abbey lora refuse`",
        rows.len(),
        missing_prov,
        ready
    );
    Ok(())
}

/// Counts for train_candidate curation — evaluation substrate, not a trainer.
pub fn train_stats(state: &AbbeyState) -> Result<()> {
    println!("abbey learn stats — train_candidate curation counts\n");
    let path = memory::backend_path(&state.state_dir, &configured_backend());
    if !path.exists() {
        println!("train_candidate: total=0 (no store)");
        println!("note: export via `abbey learn export train_candidate`; LoRA is out of scope");
        return Ok(());
    }
    let mem = open_mem(state)?;
    let cur = collect_train_curation(mem.as_ref())?;
    println!("train_candidate: total={}", cur.stats.total);
    println!("  with_provenance={}", cur.stats.with_provenance);
    println!("  missing_provenance={}", cur.stats.missing_provenance());
    println!("  high_confidence(>=0.9)={}", cur.stats.high_confidence);
    println!("  curation_ready(prov+conf>=0.9)={}", cur.ready);
    println!(
        "\nnext:  abbey learn review · abbey learn export train_candidate\n\
         oos:   LoRA / fine-tune — `abbey lora refuse` (exit 2)"
    );
    Ok(())
}

pub fn dispatch(state: &AbbeyState, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        status(state)?;
        return Ok(0);
    }
    match args[0].as_str() {
        "status" | "show" => {
            status(state)?;
            Ok(0)
        }
        "help" | "-h" | "--help" => {
            print_learn_usage();
            Ok(0)
        }
        "correction" | "fix" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                bail!("usage: abbey learn correction <what was wrong / preferred behavior>");
            }
            let id = capture_correction(state, "user correction", &text, false)?;
            println!("{id}");
            Ok(0)
        }
        "train" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                bail!("usage: abbey learn train <curated example with provenance>");
            }
            let id = capture_correction(state, "train candidate", &text, true)?;
            println!("{id}");
            Ok(0)
        }
        "preference" | "pref" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                bail!("usage: abbey learn preference <standing directive>");
            }
            let id = learn_preference(state, &text)?;
            println!("{id}");
            Ok(0)
        }
        "routes" => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
            let n = learn_from_routes(state, n)?;
            println!("learned {n} route records into activity");
            Ok(0)
        }
        "digest" => {
            println!("{}", learn_digest(state)?);
            Ok(0)
        }
        "export" => {
            let layer = args.get(1).map(|s| s.as_str()).unwrap_or("train_candidate");
            let mem = open_mem(state)?;
            for r in mem.filter(Some(layer), None, 10_000)? {
                println!("{}", serde_json::to_string(&r)?);
            }
            Ok(0)
        }
        "review" => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50);
            review_train(state, n)?;
            Ok(0)
        }
        "stats" => {
            train_stats(state)?;
            Ok(0)
        }
        "lora" | "finetune" | "fine-tune" | "fine_tune" => crate::claims::refuse("lora"),
        other => bail!(
            "unknown learn subcommand `{other}`\n\
             usage: abbey learn [status|correction|train|preference|routes|digest|export|review|stats]\n\
             (LoRA/fine-tune is Out of scope — see `abbey claims oos`)"
        ),
    }
}

/// Inject top LTM preferences into a prompt (self-learning context).
pub fn preference_context(state_dir: &Path, limit: usize) -> String {
    let Ok(mem) = memory::open_backend(state_dir, &configured_backend()) else {
        return String::new();
    };
    let Ok(prefs) = mem.filter(Some("ltm"), Some("preference"), limit) else {
        return String::new();
    };
    if prefs.is_empty() {
        return String::new();
    }
    let mut out = String::from("Standing user preferences (from Abbey self-learn LTM):\n");
    for p in prefs {
        out.push_str("- ");
        out.push_str(&p.payload);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_log::RouteRecord;
    use std::fs;
    use std::path::PathBuf;

    fn temp_state(tag: &str) -> AbbeyState {
        let state_dir =
            std::env::temp_dir().join(format!("abbey-learn-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state_dir);
        fs::create_dir_all(&state_dir).unwrap();
        let cwd_dir = state_dir.join("by-cwd");
        fs::create_dir_all(&cwd_dir).unwrap();
        AbbeyState {
            chat_file: state_dir.join("chat-id"),
            model_file: state_dir.join("model"),
            history_file: state_dir.join("history.log"),
            cwd_dir,
            per_cwd: false,
            cwd: PathBuf::from("."),
            state_dir,
        }
    }

    #[test]
    fn learn_from_routes_keeps_alternate_and_fallback() {
        // Force SQLite so the test is independent of the user's config.toml.
        unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
        let state = temp_state("routes");
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.7)
            .with_routing(Some("gemma".into()), Some("prefer hybrid-loop".into()));
        route_log::append_route_record(&state.state_dir, &rec).unwrap();
        let n = learn_from_routes(&state, 5).unwrap();
        assert_eq!(n, 1);
        let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
        let rows = mem
            .filter(Some("activity"), Some("self-learn"), 10)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].payload.contains("alternate=gemma"));
        assert!(rows[0].payload.contains("fallback=prefer hybrid-loop"));
        assert!(rows[0].payload.contains("confidence=0.70"));
        let _ = fs::remove_dir_all(&state.state_dir);
    }

    #[test]
    fn route_activity_payload_includes_routing_fields() {
        let r = RouteRecord::new(".", "abbey", "max", "fable", "other", 0.55)
            .with_routing(Some("gemma".into()), Some("low conf".into()));
        let p = route_activity_payload(&r);
        assert!(p.contains("alternate=gemma"));
        assert!(p.contains("fallback=low conf"));
        assert!(p.contains("confidence=0.55"));
    }

    #[test]
    fn train_stats_counts_provenance_and_ready() {
        unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
        let state = temp_state("stats");
        let id = capture_correction(&state, "train candidate", "prefer small diffs", true).unwrap();
        assert!(!id.is_empty());
        let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
        let cur = collect_train_curation(mem.as_ref()).unwrap();
        assert_eq!(cur.stats.total, 1);
        assert_eq!(cur.stats.with_provenance, 1);
        assert_eq!(cur.stats.missing_provenance(), 0);
        assert_eq!(cur.ready, 1); // capture_correction uses conf 0.95
        let _ = fs::remove_dir_all(&state.state_dir);
    }
}
