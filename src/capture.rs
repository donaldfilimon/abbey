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
    let chat = state.resolve_chat_for(cfg.backend)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentBackend;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)]
    fn capture_uses_the_live_backend_when_resolving_resume_state() {
        use std::os::unix::fs::PermissionsExt as _;

        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var_os("CURSOR_AGENT_CHAT_ID");
        // SAFETY: this test serializes access through ENV_LOCK and restores the
        // process environment before returning.
        unsafe { std::env::set_var("CURSOR_AGENT_CHAT_ID", "cursor-session") };

        let dir =
            std::env::temp_dir().join(format!("abbey-capture-backend-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = dir.join("agent");
        std::fs::write(&agent, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        let mut permissions = std::fs::metadata(&agent).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&agent, permissions).unwrap();
        let state = AbbeyState {
            state_dir: dir.clone(),
            chat_file: dir.join("chat-id"),
            model_file: dir.join("model"),
            history_file: dir.join("history.log"),
            cwd_dir: dir.join("by-cwd"),
            per_cwd: false,
            cwd: dir.clone(),
        };
        std::fs::create_dir_all(&state.cwd_dir).unwrap();
        state.save_chat("local-abi-session").unwrap();

        let mut cursor = AgentConfig {
            agent_path: agent.clone(),
            backend: AgentBackend::Cursor,
            auto_review: false,
            trust: false,
            ..AgentConfig::default()
        };
        let captured = capture_chat(&mut cursor, &state, &["prompt".into()]).unwrap();
        assert!(captured.stdout.contains("cursor-session"));

        let transcripts = dir.join("abi");
        std::fs::create_dir_all(&transcripts).unwrap();
        std::fs::write(
            transcripts.join("local-abi-session.transcript"),
            "LOCAL ABI CONTINUITY MARKER",
        )
        .unwrap();
        let mut abi = AgentConfig {
            agent_path: agent,
            backend: AgentBackend::Abi,
            model: "local".into(),
            transcript_dir: Some(transcripts),
            ..AgentConfig::default()
        };
        let captured = capture_chat(&mut abi, &state, &["prompt".into()]).unwrap();
        assert!(captured.stdout.contains("LOCAL ABI CONTINUITY MARKER"));
        assert!(!captured.stdout.contains("cursor-session"));

        match original {
            Some(value) => unsafe { std::env::set_var("CURSOR_AGENT_CHAT_ID", value) },
            None => unsafe { std::env::remove_var("CURSOR_AGENT_CHAT_ID") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
