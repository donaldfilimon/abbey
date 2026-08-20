//! Session orchestration: flags, hybrid persona/role run, history compact.

use crate::agent::{AgentBackend, AgentConfig, Worktree, abi_normalize_model, run_resilient};
use crate::cli::{Cli, ExecMode};
use crate::config;
use crate::hybrid_loop::{self, STAGE_IMPLEMENT, STAGE_INTERPRET, StageRequest};
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
    // dir; Claude stores its transcript itself and Abbey keeps only a local
    // marker that distinguishes a new session id from one safe to resume.
    cfg.transcript_dir = Some(state.state_dir.join(cfg.backend.transcript_subdir()));
    if cfg.backend == AgentBackend::Abi {
        // Do not expand through cursor aliases: `opus` → claude-*-thinking-*
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
        cfg.model = resolve_model(&format!("opus-thinking-{level}"));
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
    let stage1_body = hybrid_loop::interpret_body(&user);
    let stage1 = hybrid_loop::run_stage(
        cfg,
        state,
        StageRequest {
            persona,
            role: WorkerRole::Gemma,
            requested_model: &gemma,
            body: &stage1_body,
            prefs: &prefs,
            cwd: &cwd,
            correlation: &correlation,
            stage: STAGE_INTERPRET,
        },
    )?;
    hybrid_loop::require_interpretation(&stage1)?;
    println!(
        "===== stage:interpret role:gemma model:{} =====",
        stage1.model
    );
    crate::highlight::emit_agent_stdout(stage1.output.trim_end());
    println!();

    // ---- stage 2: Max implements ----
    let stage2_body = hybrid_loop::implement_body(&user, &stage1.output);
    let stage2 = hybrid_loop::run_stage(
        cfg,
        state,
        StageRequest {
            persona,
            role: WorkerRole::Max,
            requested_model: &max,
            body: &stage2_body,
            prefs: &prefs,
            cwd: &cwd,
            correlation: &correlation,
            stage: STAGE_IMPLEMENT,
        },
    )?;
    println!(
        "\n===== stage:implement role:max model:{} =====",
        stage2.model
    );
    crate::highlight::emit_agent_stdout(stage2.output.trim_end());
    println!();
    println!("\n===== route link =====");
    println!("correlation {correlation} — `abbey routes --correlation` shows both stages");

    Ok(stage2.status.code().unwrap_or(1))
}

pub fn compact_history(state: &AbbeyState, keep: usize) -> Result<usize> {
    state.compact_history(keep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::resolve_model;
    use clap::Parser as _;
    use std::sync::Mutex;

    // `Cli` reads `ABBEY_MODEL` through clap and `maybe_inject_role_model`
    // reads it directly, so every test that parses flags or exercises
    // injection serializes here and runs with the variable cleared.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        #[allow(unsafe_code)]
        fn clear_abbey_model() -> Self {
            let original = std::env::var_os("ABBEY_MODEL");
            // SAFETY: callers hold ENV_LOCK for the guard's whole life, and the
            // original value is restored on drop.
            unsafe { std::env::remove_var("ABBEY_MODEL") };
            Self { original }
        }
    }

    impl Drop for EnvGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            if let Some(value) = self.original.take() {
                // SAFETY: still under the caller's ENV_LOCK.
                unsafe { std::env::set_var("ABBEY_MODEL", value) };
            }
        }
    }

    fn scratch_state(tag: &str) -> AbbeyState {
        let dir = std::env::temp_dir().join(format!(
            "abbey-session-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("by-cwd")).unwrap();
        AbbeyState {
            state_dir: dir.clone(),
            chat_file: dir.join("chat-id"),
            model_file: dir.join("model"),
            history_file: dir.join("history.log"),
            cwd_dir: dir.join("by-cwd"),
            per_cwd: false,
            cwd: dir,
        }
    }

    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    #[test]
    fn transcript_dir_follows_the_live_backend_not_the_process_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::clear_abbey_model();
        let state = scratch_state("transcripts");
        let cli = parse(&["abbey"]);

        // The same flags applied to two configs land each backend's turns in
        // its own transcript directory — the invariant behind the Ctrl-B
        // switch (a cached `from_env()` here silently killed abi continuity).
        let mut abi = AgentConfig {
            backend: AgentBackend::Abi,
            ..AgentConfig::default()
        };
        apply_global_flags(&cli, &state, &mut abi).unwrap();
        assert_eq!(
            abi.transcript_dir.as_deref(),
            Some(state.state_dir.join("abi").as_path())
        );

        let mut fm = AgentConfig {
            backend: AgentBackend::Fm,
            ..AgentConfig::default()
        };
        apply_global_flags(&cli, &state, &mut fm).unwrap();
        assert_eq!(
            fm.transcript_dir.as_deref(),
            Some(state.state_dir.join("fm").as_path())
        );

        let mut claude = AgentConfig {
            backend: AgentBackend::Claude,
            ..AgentConfig::default()
        };
        apply_global_flags(&cli, &state, &mut claude).unwrap();
        assert_eq!(
            claude.transcript_dir.as_deref(),
            Some(state.state_dir.join("claude").as_path())
        );
    }

    #[test]
    fn thinking_is_a_cursor_alias_that_stays_local_under_abi() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::clear_abbey_model();
        let state = scratch_state("thinking");
        let cli = parse(&["abbey", "--thinking", "high"]);

        let mut cursor = AgentConfig {
            backend: AgentBackend::Cursor,
            ..AgentConfig::default()
        };
        apply_global_flags(&cli, &state, &mut cursor).unwrap();
        assert_eq!(cursor.model, resolve_model("opus-thinking-high"));

        // Under abi the alias would read as an explicit Anthropic live request;
        // it must collapse to the local persona-template instead.
        let mut abi = AgentConfig {
            backend: AgentBackend::Abi,
            ..AgentConfig::default()
        };
        apply_global_flags(&cli, &state, &mut abi).unwrap();
        assert_eq!(abi.model, "local");
    }

    #[test]
    fn sandbox_normalizes_and_output_format_implies_print() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::clear_abbey_model();
        let state = scratch_state("flags");

        for (given, stored) in [
            ("on", "enabled"),
            ("enable", "enabled"),
            ("off", "disabled"),
            ("disable", "disabled"),
            ("permissive", "permissive"),
        ] {
            let cli = parse(&["abbey", "--sandbox", given]);
            let mut cfg = AgentConfig::default();
            apply_global_flags(&cli, &state, &mut cfg).unwrap();
            assert_eq!(cfg.sandbox.as_deref(), Some(stored), "sandbox {given}");
        }

        let cli = parse(&["abbey", "--output-format", "json"]);
        let mut cfg = AgentConfig::default();
        apply_global_flags(&cli, &state, &mut cfg).unwrap();
        assert!(cfg.print, "--output-format must imply print mode");
        assert_eq!(cfg.output_format.as_deref(), Some("json"));

        let cli = parse(&["abbey", "--max-turns", "5"]);
        let mut cfg = AgentConfig::default();
        apply_global_flags(&cli, &state, &mut cfg).unwrap();
        assert!(
            cfg.extra_args
                .windows(2)
                .any(|w| w[0] == "--max-turns" && w[1] == "5"),
            "extra_args: {:?}",
            cfg.extra_args
        );
    }

    #[test]
    fn role_model_injection_never_fires_under_fm_or_abi() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::clear_abbey_model();
        let state = scratch_state("inject");

        // Injection on cursor turns the persisted default into the role alias…
        let mut cursor = AgentConfig {
            backend: AgentBackend::Cursor,
            model: state.read_model(),
            ..AgentConfig::default()
        };
        maybe_inject_role_model(&mut cursor, &state, "max");
        assert_eq!(cursor.model, resolve_model("max"));

        // …but an explicitly chosen model is never overridden…
        let mut chosen = AgentConfig {
            backend: AgentBackend::Cursor,
            model: "explicit-model".into(),
            ..AgentConfig::default()
        };
        maybe_inject_role_model(&mut chosen, &state, "max");
        assert_eq!(chosen.model, "explicit-model");

        // …and fm/abi never inject: a cursor id there would silently turn a
        // local run into a live-transport request.
        for backend in [AgentBackend::Fm, AgentBackend::Abi] {
            let mut cfg = AgentConfig {
                backend,
                model: state.read_model(),
                ..AgentConfig::default()
            };
            let before = cfg.model.clone();
            maybe_inject_role_model(&mut cfg, &state, "max");
            assert_eq!(cfg.model, before, "{backend:?} must not inject");
        }
    }
}
