//! Shift-Tab permission mode: shown in the header, forwarded as executor flags.

use crate::agent::{AgentBackend, AgentConfig};

const CLAUDE_MODES: [&str; 4] = ["default", "acceptEdits", "plan", "bypassPermissions"];

pub(crate) fn label(cfg: &AgentConfig) -> String {
    match cfg.backend {
        AgentBackend::Claude if cfg.force => "bypassPermissions".into(),
        AgentBackend::Claude => cfg
            .permission_mode
            .clone()
            .unwrap_or_else(|| "default".into()),
        AgentBackend::Cursor => if cfg.force { "force" } else { "ask" }.into(),
        _ => "n/a (executor-managed)".into(),
    }
}

pub(crate) fn cycle(cfg: &mut AgentConfig) -> String {
    match cfg.backend {
        AgentBackend::Claude => {
            let cur = label(cfg);
            let i = CLAUDE_MODES.iter().position(|m| *m == cur).unwrap_or(0);
            let next = CLAUDE_MODES[(i + 1) % CLAUDE_MODES.len()];
            cfg.force = next == "bypassPermissions";
            cfg.permission_mode =
                (next != "default" && next != "bypassPermissions").then(|| next.to_string());
        }
        AgentBackend::Cursor => cfg.force = !cfg.force,
        _ => {}
    }
    label(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentBackend, AgentConfig};

    #[test]
    fn claude_cycles_four_modes_and_force_tracks_bypass() {
        let mut cfg = AgentConfig {
            backend: AgentBackend::Claude,
            ..AgentConfig::default()
        };
        let seen: Vec<String> = (0..4).map(|_| cycle(&mut cfg)).collect();
        assert_eq!(
            seen,
            ["acceptEdits", "plan", "bypassPermissions", "default"]
        );
        assert!(!cfg.force);
    }

    #[test]
    fn cursor_toggles_force_and_others_are_executor_managed() {
        let mut cursor = AgentConfig {
            backend: AgentBackend::Cursor,
            ..AgentConfig::default()
        };
        assert_eq!(cycle(&mut cursor), "force");
        assert!(cursor.force);
        assert_eq!(cycle(&mut cursor), "ask");
        let mut ollama = AgentConfig {
            backend: AgentBackend::Ollama,
            ..AgentConfig::default()
        };
        assert_eq!(cycle(&mut ollama), "n/a (executor-managed)");
    }
}
