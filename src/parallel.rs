//! Parallel agentic lanes — thin alias over [`crate::subagents`].
//!
//! Historical Max/Gemma/Aviva fan-out. Prefer `abbey subagents` for the full
//! catalog (reviewer/security/planner + local peer CLIs).

use crate::agent::AgentConfig;
use crate::state::AbbeyState;
use crate::subagents;
use anyhow::Result;

pub fn run_parallel_cli(
    cfg: &AgentConfig,
    state: &AbbeyState,
    prompt: &[String],
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    subagents::run_parallel_compat(cfg, state, prompt, max_model, gemma_model)
}
