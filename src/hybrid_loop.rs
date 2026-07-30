//! Two-stage hybrid loop: **Gemma interprets → Max implements**, with both
//! stages linked in the route log by a shared correlation id (Abi's audit view).
//!
//! Only the sequencing, prompt assembly, and route linkage live here. The stage
//! bodies are ordinary `cursor-agent --print` captures, so a live run's *content*
//! is not deterministic — the tests cover the parts that are.

use crate::agent::AgentConfig;
use crate::models::resolve_model;
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::route_log::{self, RouteRecord};
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::path::Path;

pub const STAGE_INTERPRET: &str = "interpret";
pub const STAGE_IMPLEMENT: &str = "implement";

/// Gemma stage: turn the raw request into an explicit, human-facing reading of
/// what is being asked — including anything visual or ambiguous.
pub fn interpret_prompt(user: &str) -> String {
    let note = roles::role_system_note(WorkerRole::Gemma);
    format!(
        "{note}\n\nStage 1 of 2 (interpret). Do NOT write the implementation.\n\
         Restate the request precisely, list what is visually or behaviourally\n\
         implied, name the ambiguities, and state the acceptance criteria a\n\
         reviewer would check.\n\nRequest:\n{user}"
    )
}

/// Max stage: implement against the interpretation, not the raw request.
pub fn implement_prompt(user: &str, interpretation: &str) -> String {
    let note = roles::role_system_note(WorkerRole::Max);
    format!(
        "{note}\n\nStage 2 of 2 (implement). Stage 1 (Gemma) produced the\n\
         interpretation below. Implement against it. If it conflicts with the\n\
         original request, say so and follow the original request.\n\n\
         --- interpretation ---\n{interpretation}\n--- end interpretation ---\n\n\
         Original request:\n{user}"
    )
}

/// Append one stage's route record under a shared correlation id.
pub fn log_stage(
    state_dir: &Path,
    cwd: &str,
    correlation: &str,
    stage: &str,
    role: WorkerRole,
    model: &str,
    persona_label: &str,
) -> Result<()> {
    let rec = RouteRecord::new(
        cwd,
        persona_label,
        role.label(),
        model,
        format!("hybrid-loop stage={stage}"),
        0.8,
    )
    .in_stage(correlation, stage);
    route_log::append_route_record(state_dir, &rec)
}

/// Run both stages. Returns the exit code of the implement stage.
pub fn run_hybrid_loop(
    cfg: &AgentConfig,
    state: &AbbeyState,
    prompt: &[String],
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    let user = prompt.join(" ");
    if user.trim().is_empty() {
        bail!("usage: abbey hybrid-loop <prompt…>");
    }

    let correlation = uuid::Uuid::new_v4().to_string();
    let cwd = state.cwd.display().to_string();
    let persona_label = persona::select_persona(&user).label().to_string();
    let gemma = resolve_model(gemma_model);
    let max = resolve_model(max_model);

    eprintln!(
        "abbey: hybrid-loop {correlation}\n  stage 1 interpret → gemma({gemma})\n  \
         stage 2 implement → max({max})"
    );

    // ---- stage 1: Gemma interprets ----
    log_stage(
        &state.state_dir,
        &cwd,
        &correlation,
        STAGE_INTERPRET,
        WorkerRole::Gemma,
        &gemma,
        &persona_label,
    )?;
    let mut g_cfg = cfg.clone();
    g_cfg.model = gemma.clone();
    g_cfg.print = true;
    let (g_status, interpretation, g_err) = g_cfg.run_capture(None, &[interpret_prompt(&user)])?;
    if !g_err.trim().is_empty() {
        eprint!("{g_err}");
    }
    if interpretation.trim().is_empty() {
        bail!(
            "hybrid-loop: interpret stage produced no output (exit {})",
            g_status.code().unwrap_or(1)
        );
    }
    println!("===== stage:interpret role:gemma model:{gemma} =====");
    println!("{}", interpretation.trim_end());

    // ---- stage 2: Max implements ----
    log_stage(
        &state.state_dir,
        &cwd,
        &correlation,
        STAGE_IMPLEMENT,
        WorkerRole::Max,
        &max,
        &persona_label,
    )?;
    let mut m_cfg = cfg.clone();
    m_cfg.model = max.clone();
    m_cfg.print = true;
    let (m_status, implementation, m_err) =
        m_cfg.run_capture(None, &[implement_prompt(&user, &interpretation)])?;
    if !m_err.trim().is_empty() {
        eprint!("{m_err}");
    }
    println!("\n===== stage:implement role:max model:{max} =====");
    println!("{}", implementation.trim_end());
    println!("\n===== route link =====");
    println!("correlation {correlation} — `abbey routes` shows both stages");

    Ok(m_status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("abbey-loop-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn both_stages_share_one_correlation_id() {
        let dir = tmp("link");
        let correlation = "corr-test-1";

        log_stage(
            &dir,
            ".",
            correlation,
            STAGE_INTERPRET,
            WorkerRole::Gemma,
            "composer",
            "abbey",
        )
        .unwrap();
        log_stage(
            &dir,
            ".",
            correlation,
            STAGE_IMPLEMENT,
            WorkerRole::Max,
            "fable",
            "abbey",
        )
        .unwrap();

        let linked = route_log::correlated_routes(&dir, correlation).unwrap();
        assert_eq!(linked.len(), 2, "both stages are linked by correlation");
        assert_eq!(linked[0].stage.as_deref(), Some(STAGE_INTERPRET));
        assert_eq!(linked[0].role, "gemma");
        assert_eq!(linked[1].stage.as_deref(), Some(STAGE_IMPLEMENT));
        assert_eq!(linked[1].role, "max");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unrelated_runs_are_not_linked() {
        let dir = tmp("unrelated");
        log_stage(
            &dir,
            ".",
            "corr-a",
            STAGE_INTERPRET,
            WorkerRole::Gemma,
            "composer",
            "abbey",
        )
        .unwrap();
        log_stage(
            &dir,
            ".",
            "corr-b",
            STAGE_IMPLEMENT,
            WorkerRole::Max,
            "fable",
            "abbey",
        )
        .unwrap();
        assert_eq!(
            route_log::correlated_routes(&dir, "corr-a").unwrap().len(),
            1
        );
        assert_eq!(
            route_log::correlated_routes(&dir, "corr-b").unwrap().len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn implement_stage_carries_the_interpretation_and_original_request() {
        let p = implement_prompt(
            "add a dark mode toggle",
            "User wants a persisted theme switch.",
        );
        assert!(p.contains("User wants a persisted theme switch."));
        assert!(p.contains("add a dark mode toggle"));
        assert!(p.contains("Stage 2 of 2"));
    }

    #[test]
    fn interpret_stage_forbids_implementation() {
        let p = interpret_prompt("add a dark mode toggle");
        assert!(p.contains("Do NOT write the implementation"));
        assert!(p.contains("add a dark mode toggle"));
    }
}
