//! Backend-aware agent capture shared by print-oriented CLI surfaces.

use crate::agent::AgentConfig;
use crate::state::AbbeyState;
use anyhow::Result;
use std::process::ExitStatus;

/// Captured child process result.
pub struct CapturedRun {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Capture one run while resuming only the active backend's conversation.
pub fn capture_chat(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    prompt: &[String],
) -> Result<CapturedRun> {
    cfg.print = true;
    let chat = state.read_chat_for(cfg.backend);
    let (status, stdout, stderr) = cfg.run_capture(chat.as_deref(), prompt)?;
    Ok(CapturedRun {
        status,
        stdout,
        stderr,
    })
}

/// Capture and emit a print-mode run with consistent stderr/highlighting.
pub fn run_print(cfg: &mut AgentConfig, state: &AbbeyState, prompt: &[String]) -> Result<i32> {
    let captured = capture_chat(cfg, state, prompt)?;
    eprint!("{}", captured.stderr);
    crate::highlight::emit_agent_stdout(&captured.stdout);
    Ok(captured.status.code().unwrap_or(1))
}
