//! Hybrid-loop stage prompts and correlated route logging.
//!
//! Orchestration lives in [`crate::session::hybrid_loop_run`] so persona wrap,
//! preference injection, activity memory, and the `fm` model guard stay on the
//! one canonical path. This module only owns the stage text and route linkage.

use crate::roles::WorkerRole;
use crate::route_log::{self, RouteRecord};
use anyhow::Result;
use std::path::Path;

pub const STAGE_INTERPRET: &str = "interpret";
pub const STAGE_IMPLEMENT: &str = "implement";

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
}
