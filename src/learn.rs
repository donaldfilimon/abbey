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

/// Promote recent high-signal route records into activity/LTM digest.
pub fn learn_from_routes(state: &AbbeyState, n: usize) -> Result<usize> {
    let mem = open_mem(state)?;
    let routes = route_log::recent_routes(&state.state_dir, n)?;
    let mut stored = 0;
    for r in routes {
        let mut rec = MemoryRecord::new_stm(
            format!("route {}/{} → {}", r.persona, r.role, r.model),
            format!("{}\nreason={}", r.cwd, r.reason),
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

pub fn status(state: &AbbeyState) -> Result<()> {
    println!(
        "learn store: {}",
        memory::backend_status(&state.state_dir, &configured_backend())
    );
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
    Ok(())
}

pub fn dispatch(state: &AbbeyState, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        status(state)?;
        return Ok(0);
    }
    match args[0].as_str() {
        "status" => {
            status(state)?;
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
        other => bail!(
            "unknown learn subcommand `{other}`\n\
             usage: abbey learn [status|correction|train|preference|routes|digest|export]"
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
