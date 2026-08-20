//! Canonical agent run specs — one path for CLI and slash (no dual orchestration).

use crate::agent::AgentConfig;
use crate::config::AbbeyConfig;
use crate::prompts::{build_commit_prompt, build_review_prompt};
use crate::roles::WorkerRole;
use crate::session::{hybrid_loop_run, hybrid_run};
use crate::state::AbbeyState;
use anyhow::Result;

/// How to invoke the hybrid session.
#[derive(Debug, Clone, Default)]
pub struct RunSpec {
    pub fresh: bool,
    pub mode: Option<&'static str>,
    pub role: Option<WorkerRole>,
    pub print: bool,
    /// Gemma interpret → Max implement, correlated in the route log.
    pub hybrid_loop: bool,
}

impl RunSpec {
    pub fn resume() -> Self {
        Self::default()
    }

    pub fn fresh() -> Self {
        Self {
            fresh: true,
            ..Self::default()
        }
    }

    pub fn max() -> Self {
        Self {
            role: Some(WorkerRole::Max),
            ..Self::default()
        }
    }

    pub fn gemma() -> Self {
        Self {
            role: Some(WorkerRole::Gemma),
            ..Self::default()
        }
    }

    pub fn ask() -> Self {
        Self {
            mode: Some("ask"),
            role: Some(WorkerRole::Gemma),
            ..Self::default()
        }
    }

    pub fn plan() -> Self {
        Self {
            mode: Some("plan"),
            role: Some(WorkerRole::Max),
            ..Self::default()
        }
    }

    pub fn hybrid_loop() -> Self {
        Self {
            hybrid_loop: true,
            print: true,
            ..Self::default()
        }
    }
}

/// Single entry used by clap + slash. Always goes through `hybrid_run`
/// (including empty prompts) so persona/role/learn wrap stays consistent.
pub fn run_agent(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    prompt: &[String],
    spec: RunSpec,
) -> Result<i32> {
    if let Some(mode) = spec.mode {
        cfg.mode = Some(mode.into());
    }
    if spec.print {
        cfg.print = true;
    }
    if spec.hybrid_loop {
        let ac = AbbeyConfig::load().unwrap_or_default();
        return hybrid_loop_run(cfg, state, prompt, &ac.roles.max, &ac.roles.gemma);
    }
    hybrid_run(cfg, state, spec.fresh, prompt, spec.role)
}

pub fn run_review(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    staged: bool,
    note: &[String],
    security: bool,
) -> Result<i32> {
    let prompt = build_review_prompt(staged, note, security)?;
    run_agent(cfg, state, &[prompt], RunSpec::max())
}

pub fn run_commit(cfg: &mut AgentConfig, state: &AbbeyState) -> Result<i32> {
    let prompt = build_commit_prompt()?;
    crate::capture::run_print(cfg, state, &[prompt])
}

pub fn run_pr(cfg: &mut AgentConfig, state: &AbbeyState) -> Result<i32> {
    let ctx = crate::gitops::pr_context()?;
    let prompt = format!(
        "Write a GitHub pull request title and body for this branch. \
         Use complete sentences. Output markdown with a short title then body.\n\n{ctx}"
    );
    run_agent(cfg, state, &[prompt], RunSpec::max())
}

pub fn fork_prompt(extra: &[String]) -> Vec<String> {
    let mut p = vec![
        "Continue from the previous Abbey session context for this project. Prior chat was cleared; use repo state and git history."
            .into(),
    ];
    p.extend(extra.iter().cloned());
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_specs_encode_the_surface_contracts() {
        // Every CLI/slash surface funnels through one of these constructors;
        // a drifted binding here silently reroutes a whole verb.
        assert!(!RunSpec::resume().fresh);
        assert!(RunSpec::fresh().fresh);

        assert_eq!(RunSpec::max().role, Some(WorkerRole::Max));
        assert_eq!(RunSpec::gemma().role, Some(WorkerRole::Gemma));

        let ask = RunSpec::ask();
        assert_eq!(ask.mode, Some("ask"));
        assert_eq!(ask.role, Some(WorkerRole::Gemma));

        let plan = RunSpec::plan();
        assert_eq!(plan.mode, Some("plan"));
        assert_eq!(plan.role, Some(WorkerRole::Max));

        let looped = RunSpec::hybrid_loop();
        assert!(looped.hybrid_loop);
        assert!(
            looped.print,
            "hybrid loop stages are captured, not streamed"
        );
        assert!(!looped.fresh);
    }

    #[test]
    fn fork_prompt_keeps_the_context_note_first() {
        let extra = vec!["and fix the tests".to_string()];
        let p = fork_prompt(&extra);
        assert_eq!(p.len(), 2);
        assert!(p[0].contains("Prior chat was cleared"));
        assert_eq!(p[1], "and fix the tests");

        let bare = fork_prompt(&[]);
        assert_eq!(bare.len(), 1, "no extra args still forks with the note");
    }
}
