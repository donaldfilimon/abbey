//! Session orchestration: flags, hybrid persona/role run, history compact.

use crate::agent::{AgentBackend, AgentConfig, Worktree, run_resilient};
use crate::cli::{Cli, ExecMode};
use crate::config;
use crate::learn;
use crate::memory::{self, MemoryRecord, MemoryStore};
use crate::models::resolve_model;
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::route_log;
use crate::state::AbbeyState;
use anyhow::Result;

pub fn apply_global_flags(cli: &Cli, state: &AbbeyState, cfg: &mut AgentConfig) -> Result<()> {
    // `fm` keeps conversations in transcript files under the state dir.
    cfg.transcript_dir = Some(state.state_dir.join("fm"));
    if let Some(m) = &cli.model {
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
    if let Some(n) = cli.max_turns {
        // cursor-agent may ignore unknown flags; keep for Grok-parity surface
        cfg.extra_args.push("--max-turns".into());
        cfg.extra_args.push(n.to_string());
    }
    Ok(())
}

/// Open the configured memory backend (sqlite by default; wdbx under `--features wdbx`).
pub fn open_memory(state: &AbbeyState) -> Result<Box<dyn MemoryStore>> {
    let backend = config::AbbeyConfig::load()
        .unwrap_or_default()
        .memory_backend;
    memory::open_backend(&state.state_dir, &backend)
}

pub fn hybrid_run(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    fresh: bool,
    prompt: &[String],
    role_override: Option<WorkerRole>,
) -> Result<i32> {
    let joined = prompt.join(" ");
    let abbey_cfg = config::AbbeyConfig::load().unwrap_or_default();

    let persona = persona::select_persona(&joined);
    let role = roles::select_role(
        &joined,
        role_override.or_else(|| WorkerRole::parse(&abbey_cfg.default_role)),
    );
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
    // Only override model when user didn't set -m / ABBEY_MODEL explicitly this run.
    // Never under `fm`: its vocabulary is system|pcc, so injecting a cursor-agent
    // id here would hand the backend an argument it rejects. Under `fm` the role
    // distinction is carried by the prompt alone.
    if cfg.backend != AgentBackend::Fm
        && std::env::var("ABBEY_MODEL").is_err()
        && cfg.model == state.read_model()
    {
        cfg.model = resolve_model(model_alias);
    }

    let wrapped_user = persona::wrap_prompt(persona, &joined);
    let note = roles::role_system_note(role);
    let prefs = learn::preference_context(&state.state_dir, 8);
    let final_prompt = if prefs.is_empty() {
        format!("{note}\n\n{wrapped_user}")
    } else {
        format!("{note}\n\n{prefs}\n{wrapped_user}")
    };

    let reason = format!(
        "persona={} role={} class={:?}",
        persona.label(),
        role.label(),
        roles::classify_task(&joined)
    );
    let rec = route_log::RouteRecord::new(
        state.cwd.display().to_string(),
        persona.label(),
        role.label(),
        cfg.model.clone(),
        reason,
        0.75,
    );
    let _ = route_log::append_route_record(&state.state_dir, &rec);

    if let Ok(mem) = open_memory(state) {
        let mut m = MemoryRecord::new_stm(
            format!("route {}/{}", persona.label(), role.label()),
            format!("{joined}\n→ model {}", cfg.model),
        );
        m.tags.push("activity".into());
        m.retention = "activity".into();
        let _ = mem.store(m);
    }

    run_resilient(cfg, state, fresh, &[final_prompt])
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
