//! Session orchestration: flags, hybrid persona/role run, history compact.

use crate::agent::{AgentBackend, AgentConfig, Worktree, abi_normalize_model, run_resilient};
use crate::cli::{Cli, ExecMode};
use crate::config;
use crate::hybrid_loop::{self, STAGE_IMPLEMENT, STAGE_INTERPRET};
use crate::learn;
use crate::media;
use crate::memory::{self, MemoryRecord, MemoryStore};
use crate::models::resolve_model;
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::route_log;
use crate::state::AbbeyState;
use abi_ai::AgentProfile;
use anyhow::{Result, bail};

pub fn apply_global_flags(cli: &Cli, state: &AbbeyState, cfg: &mut AgentConfig) -> Result<()> {
    // `fm` and `abi` keep conversations in transcript files under the state
    // dir — `fm` natively (--resume/--save-transcript), `abi` Abbey-side
    // (bounded context prefix; `abi complete` itself is a stateless one-shot).
    cfg.transcript_dir = Some(state.state_dir.join(match cfg.backend {
        AgentBackend::Abi => "abi",
        _ => "fm",
    }));
    if cfg.backend == AgentBackend::Abi {
        // Do not expand through cursor aliases: `fable` → claude-*-thinking-*
        // would look like an explicit Anthropic live request under abi.
        // Also skip `read_model()` (which itself resolve_models) so a persisted
        // bare `claude-*` catalog id survives as live, not as a thinking binding.
        if let Some(level) = &cli.thinking {
            eprintln!(
                "abbey: --thinking is a cursor-agent model alias; under ABBEY_BACKEND=abi \
                 it has no effect (local persona-template). Requested level={level}"
            );
            cfg.model = "local".into();
        } else if let Some(m) = &cli.model {
            cfg.model = abi_normalize_model(m);
        } else {
            cfg.model = abi_normalize_model(&state.read_model_raw());
        }
    } else if let Some(level) = &cli.thinking {
        cfg.model = resolve_model(&format!("fable-thinking-{level}"));
    } else if let Some(m) = &cli.model {
        cfg.model = resolve_model(m);
    } else {
        cfg.model = state.read_model();
    }
    if cli.force {
        cfg.force = true;
    }
    if cli.print {
        cfg.print = true;
    }
    if let Some(fmt) = &cli.output_format {
        cfg.output_format = Some(fmt.clone());
        cfg.print = true;
    }
    if cli.plan {
        cfg.mode = Some("plan".into());
    }
    if cli.ask {
        cfg.mode = Some("ask".into());
    }
    if let Some(mode) = cli.mode {
        cfg.mode = Some(match mode {
            ExecMode::Ask => "ask".into(),
            ExecMode::Plan => "plan".into(),
        });
    }
    if let Some(wt) = &cli.worktree {
        cfg.worktree = Some(if wt.is_empty() {
            Worktree::Auto
        } else {
            Worktree::Named(wt.clone())
        });
    }
    if let Some(ws) = &cli.workspace {
        cfg.workspace = Some(ws.clone());
    }
    cfg.add_dirs = cli.add_dirs.clone();
    if let Some(sb) = &cli.sandbox {
        cfg.sandbox = Some(match sb.as_str() {
            "on" | "enable" | "enabled" => "enabled".into(),
            "off" | "disable" | "disabled" => "disabled".into(),
            other => other.into(),
        });
    }
    if cli.debug {
        cfg.extra_args.push("--debug".into());
    }
    if cli.approve_mcps {
        cfg.extra_args.push("--approve-mcps".into());
    }
    if let Some(n) = cli.max_turns {
        // cursor-agent may ignore unknown flags; keep for Grok-parity surface
        cfg.extra_args.push("--max-turns".into());
        cfg.extra_args.push(n.to_string());
    }

    let attach = media::collect(&cli.images, &cli.videos, &cli.media)?;
    apply_media_attach(cfg, &attach);
    Ok(())
}

/// Apply a media attach set onto the agent config (add-dir + prompt note).
pub fn apply_media_attach(cfg: &mut AgentConfig, attach: &media::MediaAttach) {
    if attach.is_empty() {
        return;
    }
    attach.apply_add_dirs(cfg);
    let note = attach.prompt_note();
    cfg.media_note = Some(match cfg.media_note.take() {
        Some(prev) => format!("{prev}{note}"),
        None => note,
    });
    cfg.media_prefers_gemma = true;
}

/// Open the configured memory backend (sqlite by default; wdbx under `--features wdbx`).
pub fn open_memory(state: &AbbeyState) -> Result<Box<dyn MemoryStore>> {
    let backend = config::AbbeyConfig::load()
        .unwrap_or_default()
        .memory_backend;
    memory::open_backend(&state.state_dir, &backend)
}

/// Shared persona × role × prefs assembly used by single-shot and hybrid-loop.
fn assemble_prompt(
    persona: AgentProfile,
    role: WorkerRole,
    user_body: &str,
    prefs: &str,
) -> String {
    let wrapped = persona::wrap_prompt(persona, user_body);
    let note = roles::role_system_note(role);
    if prefs.is_empty() {
        format!("{note}\n\n{wrapped}")
    } else {
        format!("{note}\n\n{prefs}\n{wrapped}")
    }
}

fn maybe_inject_role_model(cfg: &mut AgentConfig, state: &AbbeyState, model_alias: &str) {
    // Never under `fm` (vocabulary is system|pcc) or `abi` (a cursor id like
    // `claude-*` would read as an explicit live-transport request there —
    // injection would silently turn a local run into a network call). Role
    // distinction is prompt-only on both.
    if !matches!(cfg.backend, AgentBackend::Fm | AgentBackend::Abi)
        && std::env::var("ABBEY_MODEL").is_err()
        && cfg.model == state.read_model()
    {
        cfg.model = resolve_model(model_alias);
    }
}

fn record_activity(
    state: &AbbeyState,
    persona: AgentProfile,
    role: WorkerRole,
    joined: &str,
    model: &str,
) {
    if let Ok(mem) = open_memory(state) {
        let mut m = MemoryRecord::new_stm(
            format!("route {}/{}", persona.label(), role.label()),
            format!("{joined}\n→ model {model}"),
        );
        m.tags.push("activity".into());
        m.retention = "activity".into();
        let _ = mem.store(m);
    }
}

pub fn hybrid_run(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    fresh: bool,
    prompt: &[String],
    role_override: Option<WorkerRole>,
) -> Result<i32> {
    // Prompt-token media paths (e.g. `abbey describe ./shot.png`) → add-dir + note.
    let discovered = media::discover_in_prompt(prompt);
    apply_media_attach(cfg, &discovered);

    let joined = match &cfg.media_note {
        Some(note) => format!("{note}{}", prompt.join(" ")),
        None => prompt.join(" "),
    };
    let abbey_cfg = config::AbbeyConfig::load().unwrap_or_default();

    let persona = persona::select_persona(&joined);
    let role_override = match role_override {
        Some(r) => Some(r),
        None if cfg.media_prefers_gemma => Some(WorkerRole::Gemma),
        None => WorkerRole::parse(&abbey_cfg.default_role),
    };
    let decision = roles::route_decision(&joined, role_override);
    let role = decision.primary;
    let model_alias = match role {
        WorkerRole::Gemma => abbey_cfg.roles.gemma.as_str(),
        WorkerRole::Max | WorkerRole::Auto => {
            if abbey_cfg.roles.max.is_empty() {
                roles::default_model_for_role(role)
            } else {
                abbey_cfg.roles.max.as_str()
            }
        }
    };
    maybe_inject_role_model(cfg, state, model_alias);

    let prefs = learn::preference_context(&state.state_dir, 8);
    let final_prompt = assemble_prompt(persona, role, &joined, &prefs);

    let reason = format!(
        "persona={} role={} class={:?}",
        persona.label(),
        role.label(),
        decision.class
    );
    let mut rec = route_log::RouteRecord::new(
        state.cwd.display().to_string(),
        persona.label(),
        role.label(),
        cfg.model.clone(),
        reason,
        decision.confidence,
    )
    .with_routing(
        decision.alternate.map(|r| r.label().to_string()),
        decision.fallback,
    );
    if cfg.media_prefers_gemma {
        rec.tools.push("media".into());
    }
    if cfg.extra_args.iter().any(|a| a == "--approve-mcps") {
        rec.tools.push("mcp".into());
    }
    let _ = route_log::append_route_record(&state.state_dir, &rec);
    record_activity(state, persona, role, &joined, &cfg.model);

    run_resilient(cfg, state, fresh, &[final_prompt])
}

/// Gemma interpret → Max implement through the same wrap as [`hybrid_run`].
pub fn hybrid_loop_run(
    cfg: &AgentConfig,
    state: &AbbeyState,
    prompt: &[String],
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    // Allow `abbey hybrid-loop describe ./ui.png` to attach paths like hybrid_run.
    let discovered = media::discover_in_prompt(prompt);
    // hybrid_loop takes &AgentConfig — clone to apply add-dirs for both stages.
    let mut cfg_media = cfg.clone();
    apply_media_attach(&mut cfg_media, &discovered);
    let user = match &cfg_media.media_note {
        Some(note) => format!("{note}{}", prompt.join(" ")),
        None => prompt.join(" "),
    };
    if user.trim().is_empty() {
        bail!("usage: abbey hybrid-loop <prompt…>");
    }
    let cfg = &cfg_media;

    let correlation = uuid::Uuid::new_v4().to_string();
    let cwd = state.cwd.display().to_string();
    let persona = persona::select_persona(&user);
    let prefs = learn::preference_context(&state.state_dir, 8);
    let gemma = resolve_model(gemma_model);
    let max = resolve_model(max_model);

    eprintln!(
        "abbey: hybrid-loop {correlation}\n  stage 1 interpret → gemma({gemma})\n  \
         stage 2 implement → max({max})"
    );

    record_activity(state, persona, WorkerRole::Gemma, &user, &gemma);

    // ---- stage 1: Gemma interprets ----
    let mut g_cfg = cfg.clone();
    g_cfg.print = true;
    if !matches!(g_cfg.backend, AgentBackend::Fm | AgentBackend::Abi) {
        g_cfg.model = gemma.clone();
    }
    let stage1 = assemble_prompt(
        persona,
        WorkerRole::Gemma,
        &hybrid_loop::interpret_body(&user),
        &prefs,
    );
    let (g_status, interpretation, g_err) = g_cfg.run_capture(None, &[stage1])?;
    if !g_err.trim().is_empty() {
        eprint!("{g_err}");
    }
    if interpretation.trim().is_empty() {
        bail!(
            "hybrid-loop: interpret stage produced no output (exit {})",
            g_status.code().unwrap_or(1)
        );
    }
    hybrid_loop::log_stage(
        &state.state_dir,
        &cwd,
        &correlation,
        STAGE_INTERPRET,
        WorkerRole::Gemma,
        &g_cfg.model,
        persona.label(),
    )?;
    println!(
        "===== stage:interpret role:gemma model:{} =====",
        g_cfg.model
    );
    crate::highlight::emit_agent_stdout(interpretation.trim_end());
    println!();

    // ---- stage 2: Max implements ----
    let mut m_cfg = cfg.clone();
    m_cfg.print = true;
    if !matches!(m_cfg.backend, AgentBackend::Fm | AgentBackend::Abi) {
        m_cfg.model = max.clone();
    }
    let stage2 = assemble_prompt(
        persona,
        WorkerRole::Max,
        &hybrid_loop::implement_body(&user, &interpretation),
        &prefs,
    );
    let (m_status, implementation, m_err) = m_cfg.run_capture(None, &[stage2])?;
    if !m_err.trim().is_empty() {
        eprint!("{m_err}");
    }
    hybrid_loop::log_stage(
        &state.state_dir,
        &cwd,
        &correlation,
        STAGE_IMPLEMENT,
        WorkerRole::Max,
        &m_cfg.model,
        persona.label(),
    )?;
    println!(
        "\n===== stage:implement role:max model:{} =====",
        m_cfg.model
    );
    crate::highlight::emit_agent_stdout(implementation.trim_end());
    println!();
    println!("\n===== route link =====");
    println!("correlation {correlation} — `abbey routes --correlation` shows both stages");

    Ok(m_status.code().unwrap_or(1))
}

pub fn compact_history(state: &AbbeyState, keep: usize) -> Result<usize> {
    let path = &state.history_file;
    if !path.exists() {
        return Ok(0);
    }
    let text = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let keep = keep.max(1);
    let start = lines.len().saturating_sub(keep);
    let kept = &lines[start..];
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(kept.len())
}
