//! Self-learning from user corrections, routes, and activity → memory layers.
//!
//! Visibility enhancements: `abbey doctor` and TUI Doctor panel now surface
//! `train_candidate` curation counts; `abbey learn stats|review|improve` (and
//! aliases) are TUI-owned for direct execution (output lands cleanly in
//! transcript); stats output includes readiness and next-action guidance.
//! All via safe extension of dispatch/review/stats/improve to carry optional
//! `&AgentConfig` for stream-aware output. Curation only — no weights.

use crate::config;
use crate::memory::{self, MemoryFilter, MemoryRecord, MemoryStore};
use crate::route_log;
use crate::state::AbbeyState;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::agent::AgentConfig;
use crate::output;

#[path = "learn/args.rs"]
mod args;

fn configured_backend() -> String {
    config::AbbeyConfig::load()
        .unwrap_or_default()
        .memory_backend
}

/// Stream-aware or plain output for TUI-owned learn commands (review/stats/improve/status)
/// vs CLI. When a live TUI stream is present, routes through cfg.output_line so the line
/// appears in the transcript without polluting the alternate screen.
fn emit(cfg: Option<&AgentConfig>, line: &str) {
    if let Some(c) = cfg
        && c.stream.is_some()
    {
        c.output_line(line);
        return;
    }
    let _ = output::println(line);
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
    if detail.trim().is_empty() {
        bail!("correction / train candidate text must not be blank");
    }
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
    let routes = route_log::recent_routes(&state.state_dir, n)?;
    apply_routes(state, &routes)
}

fn apply_routes(state: &AbbeyState, routes: &[route_log::RouteRecord]) -> Result<usize> {
    if routes.is_empty() {
        return Ok(0);
    }
    let mem = open_mem(state)?;
    store_route_activity(mem.as_ref(), routes)
}

fn route_provenance(r: &route_log::RouteRecord) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"abbey.learn.route.v1\0");
    hash.update(serde_json::to_vec(r)?);
    Ok(format!(
        "route.jsonl @ {} sha256:{:x}",
        r.ts,
        hash.finalize()
    ))
}

fn store_route_activity(mem: &dyn MemoryStore, routes: &[route_log::RouteRecord]) -> Result<usize> {
    let existing = mem.filter_with(
        &MemoryFilter {
            source_type: Some("route".into()),
            ..MemoryFilter::default()
        },
        10_000,
    )?;
    let mut already: std::collections::HashSet<String> =
        existing.iter().map(|rec| rec.provenance.clone()).collect();
    // Legacy rows have only timestamp provenance and a lossy route projection.
    // Match the stored evidence, never the timestamp alone; each legacy row
    // accounts for at most one distinct identity in this tail. Routes carrying
    // tools/stage/correlation cannot be identified by that old projection: keep
    // them rather than silently suppressing evidence the legacy row never held.
    // Preserve legacy rows as-is.
    let mut legacy = existing;
    let mut stored = 0;
    for r in routes {
        let provenance = route_provenance(r)?;
        if already.contains(&provenance) {
            continue;
        }
        let summary = format!("route {}/{} → {}", r.persona, r.role, r.model);
        let payload = route_activity_payload(r);
        let legacy_provenance = format!("route.jsonl @ {}", r.ts);
        if let Some(index) = legacy.iter().position(|rec| {
            r.tools.is_empty()
                && r.correlation.is_none()
                && r.stage.is_none()
                && rec.provenance == legacy_provenance
                && rec.summary == summary
                && rec.payload == payload
                && rec.confidence == r.confidence
        }) {
            legacy.swap_remove(index);
            already.insert(provenance);
            continue;
        }
        let mut rec = MemoryRecord::new_stm(summary, payload);
        rec.source_type = "route".into();
        rec.retention = "activity".into();
        rec.tags = vec!["activity".into(), "self-learn".into(), r.role.clone()];
        rec.confidence = r.confidence;
        rec.provenance = provenance.clone();
        mem.store(rec).with_context(|| {
            format!(
                "route promotion stopped after {stored} record(s) stored; earlier writes retained"
            )
        })?;
        already.insert(provenance);
        stored += 1;
    }
    Ok(stored)
}

/// Reflect + auto-promote duplicate-free high-confidence activity → ltm.
pub fn learn_digest(state: &AbbeyState) -> Result<String> {
    let mem = open_mem(state)?;
    digest_memory(mem.as_ref())
}

fn digest_memory(mem: &dyn MemoryStore) -> Result<String> {
    let report = mem.reflect()?;
    let activity = mem.filter(Some("activity"), Some("self-learn"), 100)?;
    let mut promoted = 0;
    for r in activity {
        if r.confidence >= 0.8 && !report.low_confidence.contains(&r.id) {
            mem.promote(&r.id, "ltm").with_context(|| {
                format!("digest stopped after {promoted} promotion(s); earlier writes retained")
            })?;
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
    if preference.trim().is_empty() {
        bail!("preference text must not be blank");
    }
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
    let high_confidence = rows
        .iter()
        .filter(|r| r.confidence >= READY_CONFIDENCE)
        .count();
    let ready = rows
        .iter()
        .filter(|r| !r.provenance.trim().is_empty() && r.confidence >= READY_CONFIDENCE)
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

pub fn status(state: &AbbeyState, cfg: Option<&AgentConfig>) -> Result<()> {
    let backend = configured_backend();
    let path = memory::backend_path(&state.state_dir, &backend);
    emit(
        cfg,
        "abbey learn — self-learn + train_candidate curation (not a LoRA runner)\n",
    );
    emit(cfg, &format!("store: {}", path.display()));
    // `status` is read-only: opening a backend would create the store.
    if !path.exists() {
        emit(cfg, "(empty — capture corrections / run learn routes)");
        emit(cfg, "");
        print_learn_usage_with(cfg);
        return Ok(());
    }
    let mem = open_mem(state)?;
    for layer in ["stm", "ltm", "activity", "train_candidate"] {
        let n = mem.filter(Some(layer), Some("self-learn"), 500)?.len();
        emit(cfg, &format!("  {layer:<16} self-learn={n}"));
    }
    let report = mem.reflect()?;
    emit(
        cfg,
        &format!(
            "reflect: low={} dups={} superseded={}",
            report.low_confidence.len(),
            report.duplicate_summaries.len(),
            report.superseded.len()
        ),
    );
    let cur = collect_train_curation(mem.as_ref())?;
    emit(
        cfg,
        &format!(
            "train_candidate: total={} prov_ok={} missing={} high_conf={} ready={}",
            cur.stats.total,
            cur.stats.with_provenance,
            cur.stats.missing_provenance(),
            cur.stats.high_confidence,
            cur.ready
        ),
    );
    emit(cfg, "");
    emit(
        cfg,
        "curate:  abbey learn review · abbey learn stats · abbey learn export",
    );
    emit(cfg, "refuse:  abbey learn lora · abbey claims refuse lora");
    Ok(())
}

fn print_learn_usage_with(cfg: Option<&AgentConfig>) {
    emit(
        cfg,
        "usage:\n\
         \x20  abbey learn                    # status + train_candidate summary\n\
         \x20  abbey learn review [n]         # list candidates (provenance gate)\n\
         \x20  abbey learn stats              # curation counts\n\
         \x20  abbey learn train <text>       # add train_candidate with provenance\n\
         \x20  abbey learn promote <id>       # mark a distilled candidate curated (ready)\n\
         \x20  abbey learn distill <prompt>   # fm:system → ollama → fm:pcc teacher pairs\n\
         \x20  abbey learn sft                # distill pairs as chat-messages JSONL\n\
         \x20  abbey learn correction <text>  # LTM correction\n\
         \x20  abbey learn preference <text>  # LTM standing directive\n\
         \x20  abbey learn routes [n]         # route.jsonl → activity\n\
         \x20  abbey learn improve [n] [--apply] # propose; apply additive steps only\n\
         \x20  abbey learn digest|export …\n\
         note:  LoRA / fine-tune is Proposed but unavailable — curation only\n\
         \x20  abbey train plan|prepare|run|evaluate|rollback  # consented self-training orchestration",
    );
}

/// Human-review listing of `train_candidate` rows (no LoRA / export-to-weights).
pub fn review_train(state: &AbbeyState, limit: usize, cfg: Option<&AgentConfig>) -> Result<()> {
    emit(
        cfg,
        "abbey learn review — train_candidate curation (no weight updates)\n",
    );
    let path = memory::backend_path(&state.state_dir, &configured_backend());
    if !path.exists() {
        emit(
            cfg,
            "(no store yet — `abbey learn train <text>` or `abbey learn correction …`)",
        );
        return Ok(());
    }
    let mem = open_mem(state)?;
    let rows = mem.filter(Some("train_candidate"), None, limit)?;
    if rows.is_empty() {
        emit(
            cfg,
            "(no train_candidate records — `abbey learn train <text>`)",
        );
        return Ok(());
    }
    let mut missing_prov = 0usize;
    let mut ready = 0usize;
    for r in &rows {
        let prov_ok = !r.provenance.trim().is_empty();
        if !prov_ok {
            missing_prov += 1;
        }
        let is_ready = prov_ok && r.confidence >= READY_CONFIDENCE;
        if is_ready {
            ready += 1;
        }
        let preview: String = r.payload.chars().take(120).collect();
        emit(
            cfg,
            &format!(
                "{}\tconf={:.2}\tprov={}\tready={}\t{}",
                r.id,
                r.confidence,
                if prov_ok { "ok" } else { "MISSING" },
                if is_ready { "yes" } else { "no" },
                preview
            ),
        );
    }
    emit(
        cfg,
        &format!(
            "\nreview: {} candidate(s); {} missing provenance; {} ready (prov+conf≥{READY_CONFIDENCE})\n\
             next:   abbey learn promote <id> · abbey learn stats · abbey learn export train_candidate\n\
             proposed: LoRA pipeline unavailable — `abbey lora refuse`",
            rows.len(),
            missing_prov,
            ready
        ),
    );
    Ok(())
}

/// Counts for train_candidate curation — evaluation substrate, not a trainer.
/// Enhanced output for visibility: includes ready percent and TUI/doctor guidance.
pub fn train_stats(state: &AbbeyState, cfg: Option<&AgentConfig>) -> Result<()> {
    emit(cfg, "abbey learn stats — train_candidate curation counts\n");
    let path = memory::backend_path(&state.state_dir, &configured_backend());
    if !path.exists() {
        emit(cfg, "train_candidate: total=0 (no store)");
        emit(
            cfg,
            "note: export via `abbey learn export train_candidate`; LoRA is Proposed, not implemented",
        );
        return Ok(());
    }
    let mem = open_mem(state)?;
    let cur = collect_train_curation(mem.as_ref())?;
    let ready_pct = if cur.stats.total > 0 {
        (cur.ready as f64 * 100.0 / cur.stats.total as f64) as u32
    } else {
        0
    };
    emit(cfg, &format!("train_candidate: total={}", cur.stats.total));
    emit(
        cfg,
        &format!("  with_provenance={}", cur.stats.with_provenance),
    );
    emit(
        cfg,
        &format!("  missing_provenance={}", cur.stats.missing_provenance()),
    );
    emit(
        cfg,
        &format!(
            "  high_confidence(>={READY_CONFIDENCE})={}",
            cur.stats.high_confidence
        ),
    );
    emit(
        cfg,
        &format!(
            "  curation_ready(prov+conf>={READY_CONFIDENCE})={}",
            cur.ready
        ),
    );
    emit(cfg, &format!("  ready_pct={} %", ready_pct));
    emit(
        cfg,
        "  self_learn_cycle: review → promote → improve → export (curation only; no weights; distinct from specimen proposals and ABI --learn)",
    );
    emit(
        cfg,
        "\nnext:  abbey learn review · abbey learn promote <id> · abbey learn export train_candidate\n\
         TUI: /learn-stats (owned) · doctor panel · F2 memory; visible in `abbey doctor`",
    );
    emit(
        cfg,
        "proposed: LoRA / fine-tune unavailable — `abbey lora refuse` (exit 2)",
    );
    Ok(())
}

/// Short single-line curation summary for doctor integration and TUI visibility.
/// Read-only (never creates the store). Enhances self-learn story in `abbey doctor`.
pub fn curation_summary(state: &AbbeyState) -> Result<String> {
    let path = memory::backend_path(&state.state_dir, &configured_backend());
    if !path.exists() {
        return Ok("train_candidate: 0 (no store yet)".to_string());
    }
    let mem = open_mem(state)?;
    let cur = collect_train_curation(mem.as_ref())?;
    let ready_pct = if cur.stats.total > 0 {
        (cur.ready as f64 * 100.0 / cur.stats.total as f64) as u32
    } else {
        0
    };
    Ok(format!(
        "train_candidate={} prov={} miss={} ready={} ({}%)",
        cur.stats.total,
        cur.stats.with_provenance,
        cur.stats.missing_provenance(),
        cur.ready,
        ready_pct
    ))
}

/// Route confidence below which a routing decision is proposed for review.
pub const IMPROVE_LOW_ROUTE_CONFIDENCE: f32 = 0.6;

/// One reviewable proposal from `abbey learn improve`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImprovementProposal {
    /// Stable kind label (`promote-routes`, `review-route`, `dedupe`, …).
    pub kind: &'static str,
    /// Human-readable detail.
    pub detail: String,
    /// Whether `--apply` performs this step (only additive steps are applied).
    pub applies: bool,
}

/// Group duplicate-summary pairs into connected clusters of memory ids.
/// Clusters and their members keep first-seen order, so output is stable.
fn duplicate_clusters(pairs: &[(String, String)]) -> Vec<Vec<String>> {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut ids: Vec<String> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    for (a, b) in pairs {
        let mut slot = |id: &String| -> usize {
            if let Some(&i) = index.get(id) {
                return i;
            }
            let i = ids.len();
            ids.push(id.clone());
            parent.push(i);
            index.insert(id.clone(), i);
            i
        };
        let (ia, ib) = (slot(a), slot(b));
        let (ra, rb) = (root(&mut parent, ia), root(&mut parent, ib));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut clusters: Vec<(usize, Vec<String>)> = Vec::new();
    for (i, id) in ids.into_iter().enumerate() {
        let r = root(&mut parent, i);
        match clusters.iter_mut().find(|c| c.0 == r) {
            Some(cluster) => cluster.1.push(id),
            None => clusters.push((r, vec![id])),
        }
    }
    clusters.into_iter().map(|(_, members)| members).collect()
}

/// Build proposals from existing self-learn signals. Pure: no I/O.
pub fn plan_improvements(
    reflect: &memory::ReflectReport,
    routes: &[route_log::RouteRecord],
    curation: Option<TrainStats>,
) -> Vec<ImprovementProposal> {
    let mut out = Vec::new();
    if !routes.is_empty() {
        out.push(ImprovementProposal {
            kind: "promote-routes",
            detail: format!(
                "promote {} recent route record(s) into activity",
                routes.len()
            ),
            applies: true,
        });
    }
    // One row per repeated low-confidence routing pattern, not per record:
    // a fallback route logs the same (persona, role, model, reason) many times.
    let mut route_groups: Vec<(String, usize, String, String)> = Vec::new();
    for r in routes
        .iter()
        .filter(|r| r.confidence < IMPROVE_LOW_ROUTE_CONFIDENCE)
    {
        let key = format!(
            "{}/{} -> {} at conf {:.2} ({})",
            r.persona, r.role, r.model, r.confidence, r.reason
        );
        match route_groups.iter_mut().find(|g| g.0 == key) {
            Some(group) => {
                group.1 += 1;
                group.3.clone_from(&r.ts);
            }
            None => route_groups.push((key, 1, r.ts.clone(), r.ts.clone())),
        }
    }
    for (key, count, first, last) in route_groups {
        let when = if count == 1 {
            first
        } else {
            format!("x{count} {first}..{last}")
        };
        out.push(ImprovementProposal {
            kind: "review-route",
            detail: format!("{when} {key}; consider `abbey learn preference`"),
            applies: false,
        });
    }
    // Duplicate pairs are reported pairwise by reflect; cluster them so one
    // shared summary yields one row however many copies exist.
    for cluster in duplicate_clusters(&reflect.duplicate_summaries) {
        let shown: Vec<&str> = cluster.iter().take(3).map(String::as_str).collect();
        let more = cluster.len().saturating_sub(shown.len());
        let tail = if more > 0 {
            format!(" (+{more} more)")
        } else {
            String::new()
        };
        out.push(ImprovementProposal {
            kind: "dedupe",
            detail: format!(
                "{} memories share one summary: {}{tail}",
                cluster.len(),
                shown.join(", ")
            ),
            applies: false,
        });
    }
    for id in &reflect.low_confidence {
        out.push(ImprovementProposal {
            kind: "low-confidence",
            detail: format!("memory {id} is low confidence; confirm or correct it"),
            applies: false,
        });
    }
    for id in &reflect.superseded {
        out.push(ImprovementProposal {
            kind: "superseded",
            detail: format!("memory {id} is superseded; retire it by hand if stale"),
            applies: false,
        });
    }
    if let Some(stats) = curation
        && stats.missing_provenance() > 0
    {
        out.push(ImprovementProposal {
            kind: "provenance",
            detail: format!(
                "{} train_candidate record(s) lack provenance; see `abbey learn review`",
                stats.missing_provenance()
            ),
            applies: false,
        });
    }
    out
}

/// `abbey learn improve [n] [--apply]`: dry-run by default. `--apply` runs only
/// the additive steps (route promotion); every other proposal stays for a human.
pub fn improve(
    state: &AbbeyState,
    n: usize,
    apply: bool,
    cfg: Option<&AgentConfig>,
) -> Result<usize> {
    let mode = if apply { "apply" } else { "dry-run" };
    emit(
        cfg,
        &format!("abbey learn improve ({mode}) — propose, then apply only additive steps\n"),
    );
    let routes = route_log::recent_routes(&state.state_dir, n)?;
    let path = memory::backend_path(&state.state_dir, &configured_backend());
    // Dry-run must stay read-only: opening a backend would create the store.
    let (reflect, curation) = if path.exists() {
        let mem = open_mem(state)?;
        let cur = collect_train_curation(mem.as_ref())?;
        (mem.reflect()?, Some(cur.stats))
    } else {
        (memory::ReflectReport::default(), None)
    };
    let plan = plan_improvements(&reflect, &routes, curation);
    if plan.is_empty() {
        emit(
            cfg,
            "(no proposals — no routes, reflect issues, or curation gaps)",
        );
        return Ok(0);
    }
    for p in &plan {
        let tag = if p.applies { "auto" } else { "review" };
        emit(cfg, &format!("[{tag}] {:<15} {}", p.kind, p.detail));
    }
    let mut applied = 0;
    if apply {
        if plan.iter().any(|p| p.kind == "promote-routes") {
            // Apply exactly the snapshot used to build and display this plan.
            applied += apply_routes(state, &routes)?;
        }
        emit(
            cfg,
            &format!("\napplied: {applied} record(s) promoted; review items unchanged"),
        );
    } else {
        emit(
            cfg,
            &format!("\nnext: `abbey learn improve {n} --apply` runs the [auto] steps only"),
        );
    }
    Ok(applied)
}

/// The confidence a `train_candidate` must reach to count as curated.
///
/// This is the bar `abbey learn review` calls "ready" and the bar the
/// self-training corpus requires, so it is declared once here: two
/// different numbers for "curated" would make a review and a training run
/// disagree about the same record.
pub const READY_CONFIDENCE: f32 = 0.9;

/// Promote one `train_candidate` record to the curated confidence bar.
///
/// A distilled pair is written at teacher confidence (0.6), so it is not
/// "ready" until a human says so. This is that human step: it raises the
/// confidence of one record to [`READY_CONFIDENCE`] and records the
/// promotion in the provenance, so the record still names its teacher and
/// now also names the promotion.
///
/// Nothing is deleted and no payload is rewritten: the pair is the same
/// pair, marked curated.
pub fn promote_candidate(state: &AbbeyState, id: &str) -> Result<String> {
    let mem = open_mem(state)?;
    let Some(mut rec) = mem.get(id)? else {
        bail!("no train candidate with id {id}");
    };
    if rec.retention != "train_candidate" {
        bail!("{id} is a {} record, not a train candidate", rec.retention);
    }
    if rec.obsolete {
        bail!("{id} is obsolete; it is kept as provenance and is not promoted");
    }
    if rec.provenance.trim().is_empty() {
        bail!("{id} has no provenance; a candidate without provenance is not promotable");
    }
    if rec.confidence >= READY_CONFIDENCE {
        return Ok(format!(
            "{id} is already curated (confidence {:.2})",
            rec.confidence
        ));
    }
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    rec.confidence = READY_CONFIDENCE;
    if !rec.tags.iter().any(|t| t == "promoted") {
        rec.tags.push("promoted".into());
    }
    rec.provenance = format!("{} · promoted @ {stamp}", rec.provenance);
    mem.update(rec)?;
    Ok(format!(
        "promoted {id} to confidence {READY_CONFIDENCE:.2} (curated)"
    ))
}

pub fn dispatch(state: &AbbeyState, args: &[String], cfg: Option<&AgentConfig>) -> Result<i32> {
    if args.is_empty() {
        status(state, cfg)?;
        return Ok(0);
    }
    args::validate(args)?;
    match args[0].as_str() {
        "status" | "show" => {
            status(state, cfg)?;
            Ok(0)
        }
        "help" | "-h" | "--help" => {
            print_learn_usage_with(cfg);
            Ok(0)
        }
        "correction" | "fix" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                bail!("usage: abbey learn correction <what was wrong / preferred behavior>");
            }
            let id = capture_correction(state, "user correction", &text, false)?;
            emit(cfg, &id);
            Ok(0)
        }
        "train" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                bail!("usage: abbey learn train <curated example with provenance>");
            }
            let id = capture_correction(state, "train candidate", &text, true)?;
            emit(cfg, &id);
            Ok(0)
        }
        "preference" | "pref" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                bail!("usage: abbey learn preference <standing directive>");
            }
            let id = learn_preference(state, &text)?;
            emit(cfg, &id);
            Ok(0)
        }
        "routes" => {
            let n = args::count(&args[1..], 20)?;
            let n = learn_from_routes(state, n)?;
            emit(cfg, &format!("learned {n} route records into activity"));
            Ok(0)
        }
        "digest" => {
            emit(cfg, &learn_digest(state)?);
            Ok(0)
        }
        "export" => {
            let layer = args.get(1).map(|s| s.as_str()).unwrap_or("train_candidate");
            if !memory::backend_path(&state.state_dir, &configured_backend()).exists() {
                return Ok(0);
            }
            let mem = open_mem(state)?;
            for r in mem.filter(Some(layer), None, 10_000)? {
                emit(cfg, &serde_json::to_string(&r)?);
            }
            Ok(0)
        }
        "review" => {
            let n = args::count(&args[1..], 50)?;
            review_train(state, n, cfg)?;
            Ok(0)
        }
        "stats" => {
            train_stats(state, cfg)?;
            Ok(0)
        }
        "improve" => {
            let (n, apply) = args::improve(&args[1..])?;
            improve(state, n, apply, cfg)?;
            Ok(0)
        }
        "promote" | "curate" => {
            let id = args[1..].join(" ");
            if id.is_empty() {
                bail!("usage: abbey learn promote <train-candidate id>");
            }
            emit(cfg, &promote_candidate(state, &id)?);
            Ok(0)
        }
        "distill" | "teach" => crate::distill::dispatch(state, &args[1..]),
        "sft" => crate::distill::export_sft(state),
        "lora" | "finetune" | "fine-tune" | "fine_tune" => crate::claims::refuse("lora"),
        other => bail!(
            "unknown learn subcommand `{other}`\n\
             usage: abbey learn [status|correction|train|promote|distill|sft|preference|routes|digest|export|review|stats|improve]\n\
             (LoRA/fine-tune is Proposed — see `abbey claims proposed`)"
        ),
    }
}

/// Inject top LTM preferences into a prompt (self-learning context).
pub fn preference_context(state_dir: &Path, limit: usize) -> String {
    if !memory::backend_path(state_dir, &configured_backend()).exists() {
        return String::new();
    }
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
#[path = "learn_tests.rs"]
mod reliability_tests;

#[cfg(test)]
#[path = "learn/safety_tests.rs"]
mod safety_tests;

#[cfg(test)]
// `std::env::set_var` is unsafe in edition 2024 (it races other threads), and
// these tests pin the memory backend so they do not depend on the developer's
// config.toml. Test-only: the crate otherwise denies `unsafe_code`.
#[allow(unsafe_code)]
mod tests;
