//! Doctor, debug, persona/role/memory/init CLI helpers.

use crate::agent::{self, AgentConfig, run_resilient};
use crate::build_info;
use crate::cli::{MemoryCmd, MemoryFilterArgs, VERSION};
use crate::config;
use crate::gitops;
use crate::init;
use crate::memory::{self, MemoryFilter, MemoryRecord};
use crate::output;
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::session::open_memory;
use crate::state::AbbeyState;
use anyhow::{Result, bail};

pub fn cmd_init(
    cfg: &AgentConfig,
    state: &AbbeyState,
    force: bool,
    print_only: bool,
    agent: bool,
) -> Result<i32> {
    let cwd = std::env::current_dir()?;
    let opts = init::InitOpts {
        force,
        print_only,
        agent,
    };
    let (status_or_md, agent_prompt) = init::run_init(&cwd, opts)?;
    if print_only && !agent {
        let mut body = status_or_md;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        let _ = output::print(&body);
        return Ok(0);
    }
    if !print_only {
        let _ = output::println(&status_or_md);
    } else {
        // print + agent: draft on stderr so stdout stays for agent capture paths
        eprint!("{status_or_md}");
        if !status_or_md.ends_with('\n') {
            eprintln!();
        }
    }
    if let Some(prompt) = agent_prompt {
        eprintln!("abbey: refining AGENTS.md with agent…");
        let mut cfg = cfg.clone();
        cfg.print = true;
        return run_resilient(&cfg, state, false, &[prompt]);
    }
    Ok(0)
}

pub fn cmd_persona(name: Option<&str>) -> Result<i32> {
    match name {
        None | Some("auto") | Some("status") => {
            for line in persona::persona_status_lines("") {
                println!("{line}");
            }
            Ok(0)
        }
        Some(n) => {
            let Some(p) = persona::parse_persona(n) else {
                bail!("unknown persona '{n}' (abbey|aviva|abi)");
            };
            // Persist via env-file style: write to state isn't available; print export hint
            println!("{}", p.label());
            eprintln!(
                "abbey: set ABBEY_PERSONA={} for this shell, or use explicit @{} in prompts",
                p.label(),
                p.label()
            );
            Ok(0)
        }
    }
}

pub fn cmd_role(name: Option<&str>) -> Result<i32> {
    let abbey_cfg = config::AbbeyConfig::load().unwrap_or_default();
    match name {
        None | Some("status") => {
            for line in roles::role_status_lines(&abbey_cfg.roles.max, &abbey_cfg.roles.gemma) {
                println!("{line}");
            }
            println!("default_role: {}", abbey_cfg.default_role);
            Ok(0)
        }
        Some(n) => {
            let Some(r) = WorkerRole::parse(n) else {
                bail!("unknown role '{n}' (max|gemma|auto)");
            };
            println!("{}", r.label());
            eprintln!("abbey: set ABBEY_ROLE={} for this shell", r.label());
            Ok(0)
        }
    }
}

pub fn cmd_memory_store(state: &AbbeyState, cmd: MemoryCmd) -> Result<i32> {
    let mem = open_memory(state)?;
    match cmd {
        MemoryCmd::Chat => unreachable!(),
        MemoryCmd::Put {
            summary,
            retention,
            payload,
            provenance,
            tags,
            source,
            source_ref,
            project,
            timestamp,
        } => {
            let mut rec = MemoryRecord::new_stm(summary, payload);
            rec.retention = retention;
            rec.provenance = provenance;
            rec.origin = "user".into();
            rec.tags.extend(tags);
            rec.source_type = source;
            rec.source_ref = source_ref.unwrap_or_default();
            if let Some(project) = project {
                rec.project = project;
            }
            if let Some(timestamp) = timestamp {
                rec.timestamp = MemoryFilter::normalize_timestamp(&timestamp)?;
            }
            let id = rec.id.clone();
            store_record_with_selected_embedding(mem.as_ref(), rec)?;
            println!("{id}");
            Ok(0)
        }
        MemoryCmd::Get { id } => match mem.get(&id)? {
            Some(r) => {
                println!("{}", serde_json::to_string_pretty(&r)?);
                Ok(0)
            }
            None => {
                eprintln!("abbey: not found");
                Ok(1)
            }
        },
        MemoryCmd::Search {
            query,
            limit,
            filter,
        } => {
            let filter = query_filter(filter, None)?;
            for r in mem.search_keyword_with(&query, &filter, limit)? {
                println!("{}\t{}\t{}", r.id, r.retention, r.summary);
            }
            Ok(0)
        }
        MemoryCmd::Promote { id, retention } => {
            mem.promote(&id, &retention)?;
            println!("promoted {id} → {retention}");
            Ok(0)
        }
        MemoryCmd::Invalidate { id } => {
            mem.invalidate(&id)?;
            println!("invalidated {id} (marked obsolete, not deleted)");
            Ok(0)
        }
        MemoryCmd::Supersede {
            old_id,
            summary,
            retention,
            payload,
            provenance,
            tags,
            source,
            source_ref,
            project,
            timestamp,
        } => {
            let mut rec = MemoryRecord::new_stm(summary, payload);
            rec.retention = retention;
            rec.provenance = provenance;
            rec.origin = "user".into();
            rec.tags.extend(tags);
            rec.source_type = source;
            rec.source_ref = source_ref.unwrap_or_default();
            if let Some(project) = project {
                rec.project = project;
            }
            if let Some(timestamp) = timestamp {
                rec.timestamp = MemoryFilter::normalize_timestamp(&timestamp)?;
            }
            let new_id = rec.id.clone();
            mem.supersede(&old_id, rec)?;
            embed_existing_with_selected_provider(mem.as_ref(), &new_id)?;
            println!("superseded {old_id} → {new_id} (old marked obsolete)");
            Ok(0)
        }
        MemoryCmd::Reflect => {
            let report = mem.reflect()?;
            println!("low_confidence: {}", report.low_confidence.len());
            for id in &report.low_confidence {
                println!("  low\t{id}");
            }
            println!("superseded: {}", report.superseded.len());
            for id in &report.superseded {
                println!("  superseded\t{id}");
            }
            println!("duplicate_pairs: {}", report.duplicate_summaries.len());
            for (a, b) in &report.duplicate_summaries {
                println!("  dup\t{a}\t{b}");
            }
            Ok(0)
        }
        MemoryCmd::Map {
            limit,
            layer,
            filter,
        } => {
            let filter = query_filter(filter, layer)?;
            let records = mem.filter_with(&filter, limit)?;
            if records.is_empty() {
                eprintln!(
                    "abbey: no memories yet — teach her with `abbey memory put` or `abbey learn`"
                );
                return Ok(0);
            }
            println!("     x       y      z  subject          memory");
            println!(" topic recency  depth");
            for r in &records {
                let p = memory::coordinates(r);
                println!(
                    "{:>6.0} {:>7.2} {:>6.2}  {:<16} {}",
                    p.x,
                    p.y,
                    p.z,
                    memory::primary_topic(r),
                    r.summary
                );
            }
            Ok(0)
        }
        MemoryCmd::Near { id, limit, filter } => {
            let Some(anchor) = mem.get(&id)? else {
                bail!("memory id not found: {id}");
            };
            let target = memory::coordinates(&anchor);
            let filter = query_filter(filter, None)?;
            let records = mem.filter_with(&filter, 1_000)?;
            println!(
                "anchor  ({:.0}, {:.2}, {:.2})  {}",
                target.x, target.y, target.z, anchor.summary
            );
            for (dist, r) in memory::nearest(&records, target, limit.saturating_add(1))
                .into_iter()
                .filter(|(_, record)| record.id != id)
                .take(limit)
            {
                println!(
                    "{dist:>7.2}  {:<16} {}",
                    memory::primary_topic(r),
                    r.summary
                );
            }
            Ok(0)
        }
        MemoryCmd::Similar {
            query,
            id,
            limit,
            filter,
        } => {
            let filter = query_filter(filter, None)?;
            let hits = match id {
                Some(anchor) => {
                    let Some(rec) = mem.get(&anchor)? else {
                        bail!("memory id not found: {anchor}");
                    };
                    println!("anchor  {}", rec.summary);
                    memory::similar_to_id_filtered(mem.as_ref(), &anchor, &filter, limit)?
                }
                None => {
                    if query.trim().is_empty() {
                        bail!("usage: abbey memory similar <query> | --id <id>");
                    }
                    memory::similar_to_text_filtered(mem.as_ref(), &query, &filter, limit)?
                }
            };
            if hits.is_empty() {
                eprintln!(
                    "abbey: no memories yet — teach her with `abbey memory put` or `abbey learn`"
                );
                return Ok(0);
            }
            println!("  cos  subject          memory");
            for (score, r) in hits {
                println!(
                    "{score:>5.2}  {:<16} {}",
                    memory::primary_topic(&r),
                    r.summary
                );
            }
            println!(
                "note: lexical n-gram cosine — use `memory semantic` for the explicitly configured learned space"
            );
            Ok(0)
        }
        MemoryCmd::Semantic {
            query,
            limit,
            filter,
        } => {
            let cfg = config::AbbeyConfig::load()?;
            let embedder = memory::build_embedder(&cfg.embeddings)?;
            let filter = query_filter(filter, None)?;
            let hits =
                memory::search_semantic(mem.as_ref(), embedder.as_ref(), &query, &filter, limit)?;
            for hit in hits {
                println!(
                    "{:.6}\t{}\t{}",
                    hit.score, hit.record.id, hit.record.summary
                );
            }
            Ok(0)
        }
        MemoryCmd::Export { layer, filter } => {
            let filter = query_filter(filter, Some(layer))?;
            for r in mem.filter_with(&filter, 10_000)? {
                println!("{}", serde_json::to_string(&r)?);
            }
            Ok(0)
        }
        MemoryCmd::Embed { id, all, force } => {
            let cfg = config::AbbeyConfig::load()?;
            let embedder = memory::build_embedder(&cfg.embeddings)?;
            if id.as_deref() == Some("status") && !all && !force {
                let status = memory::embedding_status(mem.as_ref(), embedder.as_ref())?;
                println!("provider:  {}", embedder.space().provider);
                println!("model:     {}", embedder.space().model);
                println!("space_id:  {}", embedder.space().space_id);
                println!("dimension: {}", embedder.space().dimension);
                println!("total:     {}", status.total);
                println!("ready:     {}", status.ready);
                println!("missing:   {}", status.missing);
                println!("stale:     {}", status.stale);
                println!("pending:   {}", status.pending());
                return Ok(0);
            }
            if all {
                let status = memory::embedding_status(mem.as_ref(), embedder.as_ref())?;
                let report = memory::backfill_embeddings_with_force(
                    mem.as_ref(),
                    embedder.as_ref(),
                    if force {
                        status.total
                    } else {
                        status.pending()
                    },
                    force,
                )?;
                println!("attempted: {}", report.attempted);
                println!("embedded:  {}", report.embedded);
                println!("failed:    {}", report.failed);
                for error in report.errors {
                    eprintln!("abbey: embedding warning: {}", concise_error(&error));
                }
                return Ok(i32::from(report.failed > 0));
            }
            let id = id.ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: abbey memory embed status | <id> [--force] | --all [--force]"
                )
            })?;
            let embedded =
                memory::embed_one_if_needed(mem.as_ref(), &id, embedder.as_ref(), force)?;
            if embedded {
                println!("embedded {id} into {}", embedder.space().space_id);
            } else {
                println!("already current: {id} ({})", embedder.space().space_id);
            }
            Ok(0)
        }
    }
}

fn store_record_with_selected_embedding(
    store: &dyn memory::MemoryStore,
    record: MemoryRecord,
) -> Result<()> {
    let cfg = config::AbbeyConfig::load()?;
    if cfg.embeddings.provider.eq_ignore_ascii_case("none") {
        return store.store(record);
    }
    match memory::build_embedder(&cfg.embeddings) {
        Ok(embedder) => {
            let outcome = memory::semantic::store_with_embedding(store, record, embedder.as_ref())?;
            if let Some(error) = outcome.embedding_error {
                eprintln!(
                    "abbey: memory {} stored; embedding pending: {}",
                    outcome.memory_id,
                    concise_error(&error)
                );
            }
        }
        Err(error) => {
            let id = record.id.clone();
            store.store(record)?;
            eprintln!(
                "abbey: memory {id} stored; embedding pending: {}",
                concise_error(&format!("{error:#}"))
            );
        }
    }
    Ok(())
}

fn embed_existing_with_selected_provider(store: &dyn memory::MemoryStore, id: &str) -> Result<()> {
    let cfg = config::AbbeyConfig::load()?;
    if cfg.embeddings.provider.eq_ignore_ascii_case("none") {
        return Ok(());
    }
    let result = memory::build_embedder(&cfg.embeddings)
        .and_then(|embedder| memory::semantic::embed_one(store, id, embedder.as_ref()));
    if let Err(error) = result {
        eprintln!(
            "abbey: memory {id} stored; embedding pending: {}",
            concise_error(&format!("{error:#}"))
        );
    }
    Ok(())
}

fn concise_error(error: &str) -> String {
    error
        .lines()
        .next()
        .unwrap_or("embedding failed")
        .chars()
        .take(240)
        .collect()
}

fn query_filter(args: MemoryFilterArgs, retention: Option<String>) -> Result<MemoryFilter> {
    MemoryFilter::new(
        retention,
        args.tag,
        args.source,
        args.source_ref,
        args.project,
        args.since,
        args.until,
    )
}

pub fn print_permissions(cfg: &AgentConfig) {
    println!("trust:        {}", cfg.trust);
    println!("auto-review:  {}", cfg.auto_review);
    println!("force/yolo:   {}", cfg.force);
    println!("no-resume:    {}", cfg.no_resume);
    println!(
        "sandbox:      {}",
        cfg.sandbox.as_deref().unwrap_or("(default)")
    );
    println!(
        "mode:         {}",
        cfg.mode.as_deref().unwrap_or("(interactive)")
    );
    println!("model:        {}", cfg.model);
}

pub fn print_config(state: &AbbeyState, cfg: &AgentConfig) {
    println!("abbey {VERSION}");
    println!("agent:     {}", cfg.agent_path.display());
    println!("model:     {}", cfg.model);
    println!("state:     {}", state.state_dir.display());
    println!("per-cwd:   {}", state.per_cwd);
    println!("cwd:       {}", state.cwd.display());
    println!(
        "ABBEY_MODEL={}",
        std::env::var("ABBEY_MODEL").unwrap_or_default()
    );
    println!(
        "ABBEY_FORCE={}",
        std::env::var("ABBEY_FORCE").unwrap_or_default()
    );
    println!(
        "ABBEY_PER_CWD={}",
        std::env::var("ABBEY_PER_CWD").unwrap_or_else(|_| "1".into())
    );
    print_permissions(cfg);
}

/// Where the active backend choice came from — env, config key, or default.
///
/// Printing `ABBEY_BACKEND=<label>` unconditionally used to lie whenever the
/// backend came from the config key (or was the plain default).
fn backend_source() -> String {
    if let Ok(v) = std::env::var("ABBEY_BACKEND") {
        if !v.trim().is_empty() {
            return format!("ABBEY_BACKEND={}", v.trim());
        }
    }
    match config::AbbeyConfig::load()
        .ok()
        .and_then(|c| c.backend.clone())
    {
        Some(v) => format!("config backend={v}"),
        None => "default".into(),
    }
}

pub fn cmd_doctor(state: &AbbeyState, cfg: &AgentConfig) -> Result<i32> {
    let chat = state
        .read_chat_for(cfg.backend)
        .unwrap_or_else(|| "(none)".into());
    for line in build_info::lines() {
        let _ = output::println(line);
    }
    let lines = [
        format!("agent:     {}", cfg.agent_path.display()),
        format!("agent ver: {}", cfg.agent_version()),
        format!("model:     {}", cfg.model),
        format!("chat:      {chat}"),
        format!("chat file: {}", state.active_chat_file().display()),
        format!("per-cwd:   {}", state.per_cwd),
        format!("cwd:       {}", state.cwd.display()),
        format!("state:     {}", state.state_dir.display()),
        format!("auto-review: {}", cfg.auto_review),
        format!("trust:     {}", cfg.trust),
        format!("force:     {}", cfg.force),
        format!("no-resume: {}", cfg.no_resume),
        "tui:       ratatui (rust nightly, edition 2024)".into(),
        "parity:    Grok · Codex · Claude · ABI personas · parallel lanes · OS control".into(),
        format!(
            "backend:   {} (from {})",
            agent::AgentBackend::from_env().label(),
            backend_source()
        ),
    ];
    for line in &lines {
        let _ = output::println(line);
    }
    let abbey_cfg = config::AbbeyConfig::load().unwrap_or_default();
    for line in persona::persona_status_lines("") {
        let _ = output::println(line);
    }
    for line in roles::role_status_lines(&abbey_cfg.roles.max, &abbey_cfg.roles.gemma) {
        let _ = output::println(line);
    }
    let _ = output::println(
        "routing:    confidence/alternate/fallback on route.jsonl (audit only — no auto second agent)",
    );
    let _ =
        output::println("learn:      review|stats (+ learn-review/learn-stats aliases; LoRA OOS)");
    let _ = output::println(
        "os:         allowlist + dry-run; execute --confirm only (`abbey allowlist`)",
    );
    let _ = output::println(
        "media:      --image/--video/--media or /image|/video attach paths (workspace read; no local vision)",
    );
    let _ = output::println(
        "generate:   abbey imagine|generate video|/imagine|/gen-video via cursor-agent tools (not local models)",
    );
    let _ = output::println(
        "reasoning:  abbey reason|/reason + --thinking|/think → Cursor *-thinking-* (structured wrap)",
    );
    let _ = output::println(
        "tools/mcp:  abbey mcp status|paths|view + explicit provider management; not an MCP host",
    );
    let _ = output::println(
        "acp:        abbey acp list|run gemini|opencode (peer ACP servers; Abbey is not an ACP host)",
    );
    if let Ok(groups) = crate::protocols::load_mcp_servers(&state.cwd) {
        let n: usize = groups.iter().map(|(_, s)| s.len()).sum();
        let files = groups.len();
        let _ = output::println(format!(
            "mcp cfg:    {n} server(s) across {files} config file(s)"
        ));
    }
    let acp_n = crate::protocols::acp_peers()
        .iter()
        .filter(|p| p.path.is_some() && !p.acp_args.is_empty())
        .count();
    let _ = output::println(format!(
        "acp peers:  {acp_n} ACP-capable binary(ies) on PATH"
    ));
    let _ = output::println(crate::highlight::status_line());
    let _ = output::println(crate::subagents::status_line());
    let _ = output::println(crate::improve::status_line());
    let _ = output::println(crate::claims::status_line());
    let _ = output::println(crate::platform::status_line());
    for line in crate::surfaces::status_lines() {
        let _ = output::println(line);
    }
    for line in crate::deferred::status_lines() {
        let _ = output::println(line);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = output::println(
            "voice:      abbey voice speak|listen|ask — Premium/Enhanced say TTS + on-device Speech STT",
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = output::println("voice:      macOS only (say + Speech.framework)");
    }
    let _ = output::println(memory::backend_status(
        &state.state_dir,
        &abbey_cfg.memory_backend,
    ));
    let _ = output::println(memory::feature_status());
    let embedding_line = match memory::build_embedder(&abbey_cfg.embeddings) {
        Ok(embedder) if embedder.space().provider == "none" => {
            "semantic:  disabled (embedding provider none; lexical search remains available)"
                .to_string()
        }
        Ok(embedder) => match open_memory(state)
            .and_then(|store| memory::embedding_status(store.as_ref(), embedder.as_ref()))
        {
            Ok(status) => format!(
                "semantic:  {} {}d ready={} pending={} space={}",
                embedder.space().provider,
                embedder.space().dimension,
                status.ready,
                status.pending(),
                embedder.space().space_id
            ),
            Err(error) => format!("semantic:  configured but index unavailable: {error}"),
        },
        Err(error) => format!("semantic:  configured but provider unavailable: {error}"),
    };
    let _ = output::println(embedding_line);
    let mesh = crate::mesh::status(&abbey_cfg);
    let _ = output::println(format!(
        "mesh proof: {} (authenticated Unix local multi-process only; not production multi-host)",
        if mesh.available {
            "available"
        } else {
            "unavailable — set ABBEY_ABI_BIN"
        }
    ));
    let backend = agent::AgentBackend::from_env();
    if backend.is_on_device() {
        let _ = output::println(format!("on-device: {}", cfg.fm_availability()));
        let _ =
            output::println("on-device: no cursor-agent and no network required for generation");
    } else {
        let _ = output::println(format!(
            "on-device: available via ABBEY_BACKEND=fm ({})",
            if agent::which_bin("fm").is_some() {
                "fm present"
            } else {
                "fm not installed — needs macOS 26+"
            }
        ));
    }
    let _ = output::println(match config::resolve_abi_bin(&abbey_cfg) {
        // Name the real source: this line used to hardcode `ABBEY_BACKEND=abi`
        // and so lied whenever the backend came from the config key — the same
        // defect `backend_source()` was written to fix one line above.
        Some(p) if backend == agent::AgentBackend::Abi => format!(
            "abi backend: active (from {} → {}) — local persona-template; \
             claude-*/live models use abi's Anthropic transport; Abbey-side transcript continuity",
            backend_source(),
            p.display()
        ),
        Some(p) => format!(
            "abi backend: available via ABBEY_BACKEND=abi ({})",
            p.display()
        ),
        None if backend == agent::AgentBackend::Abi => {
            "abi backend: ACTIVE BUT NO BINARY — build with `cargo build -p abi-cli` \
             in ../abi, then set ABBEY_ABI_BIN"
                .into()
        }
        None => "abi backend: unavailable (no `abi` binary; alias won't do)".into(),
    });
    let _ = output::println(config::wdbx_cli_status(&abbey_cfg));
    let hist = state.history(5);
    if !hist.is_empty() {
        println!("recent chats:");
        for e in hist {
            println!("  {}\t{}\t{}", e.timestamp, e.chat_id, e.cwd);
        }
    }
    Ok(0)
}

pub fn cmd_debug(state: &AbbeyState, cfg: &AgentConfig) -> Result<i32> {
    cmd_doctor(state, cfg)?;
    println!("--- debug ---");
    println!("PATH agent candidates:");
    for name in [
        "cursor-agent",
        "agent",
        "grok",
        "codex",
        "claude",
        "abi",
        "fm",
    ] {
        match agent::which_bin(name) {
            Some(p) => println!("  {name}: {}", p.display()),
            None => println!("  {name}: (not found)"),
        }
    }
    println!("git repo: {}", gitops::is_repo());
    Ok(0)
}
