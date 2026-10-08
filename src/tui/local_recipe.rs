//! Private complete live-context envelope for finite captured local commands.

use crate::agent::AgentConfig;
use crate::state::AbbeyState;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

pub(crate) const ENV: &str = "ABBEY_INTERNAL_TUI_LOCAL_RECIPE";
const MAX_BYTES: usize = 48 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    version: u8,
    edition: String,
    cwd: std::path::PathBuf,
    cfg: AgentConfig,
    state: AbbeyState,
}

pub(super) fn owned_slash(input: &str) -> bool {
    let Some((name, rest)) = crate::slash::parse_slash(input) else {
        return false;
    };
    let name = crate::slash_alias::resolve_name(name).unwrap_or(name);
    let args = crate::slash::split_args(rest);
    let first = args.first().map(String::as_str);
    match name {
        "doctor" | "debug" => true,
        "cot" => first == Some("run"),
        "learn" => {
            matches!(first, Some("distill" | "teach"))
                && !args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
        }
        "subagents" | "swarm" | "distribute" => {
            !matches!(
                first,
                Some("list" | "ls" | "catalog" | "status" | "-h" | "--help")
            ) && crate::subagents::parse_args(&args).is_ok_and(|opts| !opts.prompt.is_empty())
        }
        "improve" | "smart-improve" | "prod-ready" => {
            !matches!(
                first,
                None | Some("status" | "plan" | "help" | "-h" | "--help")
            ) && !args
                .iter()
                .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
        }
        "init" => crate::init::parse_init_args(rest).agent,
        _ => false,
    }
}

fn finite_args(args: &[String]) -> bool {
    if args.len() == 1 && args[0].starts_with('/') {
        let Some((name, rest)) = crate::slash::parse_slash(&args[0]) else {
            return false;
        };
        let name = crate::slash_alias::resolve_name(name).unwrap_or(name);
        let subcommand = crate::slash::split_args(rest).first().cloned();
        if (name == "mcp" && matches!(subcommand.as_deref(), Some("serve" | "server" | "stdio")))
            || (name == "acp" && matches!(subcommand.as_deref(), Some("run" | "serve" | "server")))
        {
            return false;
        }
        return matches!(
            super::worker::route_slash(&args[0]),
            super::worker::SlashRoute::Local
        ) && !owned_slash(&args[0]);
    }
    // The TUI's confirmed OS allowlist adapter is its only non-slash caller.
    args.len() >= 4 && args[0] == "os" && args[1] == "execute" && args[2] == "--confirm"
}

pub(super) fn encode(cfg: &AgentConfig, state: &AbbeyState, args: &[String]) -> Result<String> {
    ensure!(
        finite_args(args),
        "this foreground command requires the ordinary CLI; local TUI children must be finite"
    );
    let recipe = Recipe {
        version: 1,
        edition: crate::edition::ACTIVE.slug().into(),
        cwd: std::env::current_dir()?,
        cfg: cfg.clone(),
        state: state.clone(),
    };
    let text = serde_json::to_string(&recipe)?;
    ensure!(
        text.len() <= MAX_BYTES,
        "local TUI recipe exceeds its bound"
    );
    Ok(text)
}

pub(crate) fn from_environment() -> Result<Option<(AgentConfig, AbbeyState)>> {
    let Some(text) = std::env::var_os(ENV) else {
        return Ok(None);
    };
    let Some(text) = text.to_str().filter(|text| text.len() <= MAX_BYTES) else {
        bail!("invalid local TUI recipe");
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    decode(text, &args, &std::env::current_dir()?).map(Some)
}

fn decode(text: &str, args: &[String], cwd: &std::path::Path) -> Result<(AgentConfig, AbbeyState)> {
    ensure!(
        text.len() <= MAX_BYTES,
        "local TUI recipe exceeds its bound"
    );
    ensure!(
        finite_args(args),
        "local TUI recipe cannot admit this command"
    );
    // Reject missing Option fields too: a recipe must be a complete current
    // configuration, rather than a partial ambient-default override.
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| anyhow::anyhow!("invalid local TUI recipe"))?;
    let recipe: Recipe =
        serde_json::from_str(text).map_err(|_| anyhow::anyhow!("invalid local TUI recipe"))?;
    let canonical = serde_json::to_value(&recipe)?;
    ensure!(
        value == canonical
            && recipe.version == 1
            && recipe.edition == crate::edition::ACTIVE.slug()
            && recipe.cwd == cwd,
        "local TUI recipe identity mismatch"
    );
    ensure!(
        recipe.cfg.add_dirs.len() <= 128 && recipe.cfg.extra_args.len() <= 128,
        "local TUI recipe collections exceed their bounds"
    );
    ensure!(
        recipe.state.cwd.is_absolute() && recipe.state.state_dir.is_absolute(),
        "local TUI recipe state must be absolute"
    );
    Ok((recipe.cfg, recipe.state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentBackend;

    fn recipe() -> (String, AgentConfig, AbbeyState) {
        let cwd = std::env::current_dir().unwrap();
        let root = std::env::temp_dir().join("abbey-private-recipe-fixture");
        let state = AbbeyState {
            state_dir: root.clone(),
            chat_file: root.join("chat"),
            model_file: root.join("model"),
            history_file: root.join("history"),
            cwd_dir: root.join("by-cwd"),
            per_cwd: false,
            cwd,
        };
        let mut cfg = AgentConfig::fixed_provider_recipe(
            "/owned/abi".into(),
            AgentBackend::Abi,
            "selected-local".into(),
        );
        cfg.permission_mode = Some("plan".into());
        cfg.transcript_dir = Some(root.join("selected-transcripts"));
        cfg.add_dirs = vec![root.join("allowed")];
        cfg.extra_args = vec!["selected-option".into()];
        (
            encode(&cfg, &state, &["/memory".into()]).unwrap(),
            cfg,
            state,
        )
    }

    #[test]
    fn complete_recipe_preserves_actual_backend_knobs_and_state_identity() {
        let (text, cfg, state) = recipe();
        let (restored, restored_state) = decode(
            &text,
            &["/memory".into()],
            &std::env::current_dir().unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&cfg).unwrap()
        );
        assert_eq!(
            serde_json::to_value(restored_state).unwrap(),
            serde_json::to_value(state).unwrap()
        );
        assert!(restored.stream.is_none());
    }

    #[test]
    fn duplicate_missing_and_unknown_fields_fail_closed() {
        let (text, _, _) = recipe();
        let cwd = std::env::current_dir().unwrap();
        let args = ["/memory".into()];
        assert!(
            decode(
                &text.replacen("\"version\":1", "\"version\":2,\"version\":1", 1),
                &args,
                &cwd
            )
            .is_err()
        );
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["cfg"]
            .as_object_mut()
            .unwrap()
            .remove("permission_mode");
        assert!(decode(&value.to_string(), &args, &cwd).is_err());
        value["cfg"]["unexpected"] = true.into();
        assert!(decode(&value.to_string(), &args, &cwd).is_err());
    }

    #[test]
    fn generation_foreground_servers_and_identity_changes_cannot_adopt_a_recipe() {
        let (text, _, _) = recipe();
        let cwd = std::env::current_dir().unwrap();
        for command in [
            "/ask private prompt",
            "/cot run prompt",
            "/learn teach prompt",
            "/subagents run prompt",
            "/improve run",
            "/init --agent",
            "/mcp serve http",
            "/mcp server",
            "/mcp stdio",
            "/acp run gemini",
        ] {
            assert!(decode(&text, &[command.into()], &cwd).is_err(), "{command}");
        }
        assert!(decode(&text, &["/memory".into()], &cwd.join("different")).is_err());
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["edition"] = "other-edition".into();
        assert!(decode(&value.to_string(), &["/memory".into()], &cwd).is_err());
    }
}
