//! Hybrid-loop stage prompts and correlated route logging.
//!
//! Orchestration lives in [`crate::session::hybrid_loop_run`] so persona wrap,
//! preference injection, activity memory, and the `fm` model guard stay on the
//! one canonical path. This module only owns the stage text and route linkage.

use crate::agent::{AgentBackend, AgentConfig};
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::route_log::{self, RouteRecord};
use crate::state::AbbeyState;
use abi_ai::AgentProfile;
use anyhow::{Result, bail};
use std::path::Path;
use std::process::ExitStatus;

pub const STAGE_INTERPRET: &str = "interpret";
pub const STAGE_IMPLEMENT: &str = "implement";

pub struct StageRequest<'a> {
    pub persona: AgentProfile,
    pub role: WorkerRole,
    pub requested_model: &'a str,
    pub body: &'a str,
    pub prefs: &'a str,
    pub cwd: &'a str,
    pub correlation: &'a str,
    pub stage: &'a str,
}

pub struct StageResult {
    pub status: ExitStatus,
    pub output: String,
    pub model: String,
}

/// Execute one correlated hybrid stage through the common backend-aware path.
///
/// Foundation Models and ABI use role text instead of Cursor model bindings;
/// stderr is preserved and every completed invocation receives a route record.
pub fn run_stage(
    base: &AgentConfig,
    state: &AbbeyState,
    request: StageRequest<'_>,
) -> Result<StageResult> {
    let mut stage_cfg = base.clone();
    stage_cfg.print = true;
    if stage_accepts_model_binding(stage_cfg.backend) {
        stage_cfg.model = request.requested_model.to_string();
    }
    let prompt = assemble_stage_prompt(request.persona, request.role, request.body, request.prefs);
    let (status, output, stderr) = stage_cfg.run_capture(None, &[prompt])?;
    if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }
    log_stage(
        &state.state_dir,
        request.cwd,
        request.correlation,
        request.stage,
        request.role,
        &stage_cfg.model,
        request.persona.label(),
    )?;
    Ok(StageResult {
        status,
        output,
        model: stage_cfg.model,
    })
}

fn stage_accepts_model_binding(backend: AgentBackend) -> bool {
    !matches!(backend, AgentBackend::Fm | AgentBackend::Abi)
}

fn assemble_stage_prompt(
    profile: AgentProfile,
    role: WorkerRole,
    body: &str,
    prefs: &str,
) -> String {
    let wrapped = persona::wrap_prompt(profile, body);
    let note = roles::role_system_note(role);
    if prefs.is_empty() {
        format!("{note}\n\n{wrapped}")
    } else {
        format!("{note}\n\n{prefs}\n{wrapped}")
    }
}

/// Keep the interpret-stage empty-output failure consistent across callers.
pub fn require_interpretation(result: &StageResult) -> Result<&str> {
    if result.output.trim().is_empty() {
        bail!(
            "hybrid-loop: interpret stage produced no output (exit {})",
            result.status.code().unwrap_or(1)
        );
    }
    Ok(&result.output)
}

/// Stage-1 body (no role note — [`crate::session`] adds it via assemble_prompt).
pub fn interpret_body(user: &str) -> String {
    format!(
        "Stage 1 of 2 (interpret). Do NOT write the implementation.\n\
         Restate the request precisely, list what is visually or behaviourally\n\
         implied, name the ambiguities, and state the acceptance criteria a\n\
         reviewer would check.\n\nRequest:\n{user}"
    )
}

/// Stage-2 body (no role note — session assemble_prompt adds it).
pub fn implement_body(user: &str, interpretation: &str) -> String {
    format!(
        "Stage 2 of 2 (implement). Stage 1 (Gemma) produced the\n\
         interpretation below. Implement against it. If it conflicts with the\n\
         original request, say so and follow the original request.\n\n\
         --- interpretation ---\n{interpretation}\n--- end interpretation ---\n\n\
         Original request:\n{user}"
    )
}

/// Full interpret prompt including the Gemma role note (tests / direct callers).
#[cfg(test)]
pub fn interpret_prompt(user: &str) -> String {
    let note = crate::roles::role_system_note(WorkerRole::Gemma);
    format!("{note}\n\n{}", interpret_body(user))
}

/// Full implement prompt including the Max role note (tests / direct callers).
#[cfg(test)]
pub fn implement_prompt(user: &str, interpretation: &str) -> String {
    let note = crate::roles::role_system_note(WorkerRole::Max);
    format!("{note}\n\n{}", implement_body(user, interpretation))
}

/// Paired role recorded as alternate for a hybrid-loop stage (audit only).
fn paired_alternate(stage: &str) -> Option<String> {
    match stage {
        STAGE_INTERPRET => Some(WorkerRole::Max.label().into()),
        STAGE_IMPLEMENT => Some(WorkerRole::Gemma.label().into()),
        _ => None,
    }
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
        0.85,
    )
    .in_stage(correlation, stage)
    .with_routing(
        paired_alternate(stage),
        Some("hybrid-loop paired stage (audit only)".into()),
    );
    route_log::append_route_record(state_dir, &rec)
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
        assert_eq!(linked[0].alternate.as_deref(), Some("max"));
        assert_eq!(linked[1].stage.as_deref(), Some(STAGE_IMPLEMENT));
        assert_eq!(linked[1].role, "max");
        assert_eq!(linked[1].alternate.as_deref(), Some("gemma"));
        assert!(
            linked[0]
                .fallback
                .as_deref()
                .is_some_and(|s| s.contains("hybrid-loop"))
        );

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

    #[test]
    fn stage_bodies_omit_role_notes_so_session_can_assemble() {
        let body = interpret_body("x");
        assert!(!body.contains("Gemma"));
        assert!(body.contains("Stage 1 of 2"));
    }

    #[test]
    fn fm_and_abi_keep_their_backend_model_vocabulary() {
        assert!(!stage_accepts_model_binding(AgentBackend::Fm));
        assert!(!stage_accepts_model_binding(AgentBackend::Abi));
        assert!(stage_accepts_model_binding(AgentBackend::Cursor));
        assert!(stage_accepts_model_binding(AgentBackend::Grok));
        assert!(stage_accepts_model_binding(AgentBackend::Ollama));
    }

    #[test]
    fn empty_interpretation_remains_a_hard_failure() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt as _;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt as _;

        #[cfg(unix)]
        let status = ExitStatus::from_raw(7 << 8);
        #[cfg(windows)]
        let status = ExitStatus::from_raw(7);
        let result = StageResult {
            status,
            output: " \n".into(),
            model: "test".into(),
        };
        let error = require_interpretation(&result).unwrap_err().to_string();
        assert!(error.contains("produced no output"));
        assert!(error.contains("exit 7"));
    }
}
