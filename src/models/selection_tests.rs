use crate::agent::{AgentBackend, AgentConfig};
use crate::cli::Cli;
use crate::state::AbbeyState;
use clap::Parser;

fn state() -> AbbeyState {
    let dir = std::env::temp_dir().join(format!("abbey-model-parity-{}", uuid::Uuid::new_v4()));
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

#[test]
fn current_model_inspection_reports_live_selection_not_persisted_preference() {
    use crate::stream::{StreamEvent, StreamTap};
    let state = state();
    state.save_model_literal("pcc").unwrap();
    for (backend, selected) in [
        (AgentBackend::Fm, "system"),
        (AgentBackend::Abi, "claude-opus-5"),
    ] {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut cfg = AgentConfig {
            backend,
            model: selected.into(),
            stream: Some(StreamTap::new(sender)),
            ..AgentConfig::default()
        };
        assert_eq!(
            crate::slash_dispatch::dispatch_slash("/model", &state, &mut cfg).unwrap(),
            0
        );
        assert!(
            matches!(receiver.try_recv().unwrap(), StreamEvent::Notice(text) if text == selected)
        );
        assert_eq!(
            crate::commands::run_cli(
                Cli::parse_from(["abbey", "model"]),
                state.clone(),
                cfg.clone()
            )
            .unwrap(),
            0
        );
        assert!(
            matches!(receiver.try_recv().unwrap(), StreamEvent::Notice(text) if text == selected)
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(cfg.model, selected);
        assert_eq!(
            std::fs::read_to_string(&state.model_file).unwrap().trim(),
            "pcc"
        );
    }
    std::fs::remove_dir_all(state.state_dir).unwrap();
}

#[test]
fn cli_and_slash_model_selection_persist_identical_models_and_argv() {
    for (backend, input, expected) in [
        (AgentBackend::Abi, "claude-opus-5", "claude-opus-5"),
        (AgentBackend::Abi, "claude-fable-5", "claude-fable-5"),
        (AgentBackend::Abi, "opus", "local"),
        (AgentBackend::Abi, "gemma", "local"),
        (AgentBackend::Abi, "max", "local"),
        (AgentBackend::Ollama, "custom:tag", "custom:tag"),
        (AgentBackend::Ollama, "gemma", super::OLLAMA_DEFAULT_MODEL),
        (AgentBackend::Fm, "pcc", "pcc"),
        (AgentBackend::Fm, "system", "system"),
        (AgentBackend::Fm, "private-cloud-compute", "pcc"),
        (AgentBackend::Fm, "private_cloud_compute", "pcc"),
        (AgentBackend::Fm, "opus", "system"),
        (AgentBackend::Fm, "unknown-cloud-model", "system"),
    ] {
        let state = state();
        let mut cfg = AgentConfig {
            backend,
            agent_path: state.state_dir.join("missing-executor"),
            ..AgentConfig::default()
        };
        let cli = Cli::parse_from(["abbey", "model", input]);
        assert_eq!(
            crate::commands::run_cli(cli, state.clone(), cfg.clone()).unwrap(),
            0
        );
        let persisted = std::fs::read_to_string(&state.model_file).unwrap();
        assert_eq!(persisted.trim(), expected);
        let mut cli_cfg = cfg.clone();
        cli_cfg.model = persisted.trim().into();
        assert_eq!(
            crate::slash_dispatch::dispatch_slash(&format!("/model {input}"), &state, &mut cfg)
                .unwrap(),
            0
        );
        assert_eq!(cfg.model, expected);
        assert_eq!(
            std::fs::read_to_string(&state.model_file).unwrap(),
            persisted
        );
        let args = cfg.build_args(None, &["hello".into()]);
        assert_eq!(args, cli_cfg.build_args(None, &["hello".into()]));
        if backend == AgentBackend::Abi {
            assert_eq!(
                args.iter().any(|a| a == "--live"),
                expected.starts_with("claude-")
            );
            assert!(!args.iter().any(|a| a.contains("thinking")));
        }
        std::fs::remove_dir_all(state.state_dir).unwrap();
    }
}

#[test]
fn slash_inventory_and_status_follow_live_non_account_backend_without_spawn() {
    let state = state();
    let mut cfg = AgentConfig {
        agent_path: state.state_dir.join("missing-executor"),
        ..AgentConfig::default()
    };
    for backend in [
        AgentBackend::Fm,
        AgentBackend::Ollama,
        AgentBackend::Abi,
        AgentBackend::Claude,
    ] {
        cfg.backend = backend;
        assert_eq!(
            crate::slash_dispatch::dispatch_slash("/models", &state, &mut cfg).unwrap(),
            0
        );
        assert_eq!(
            crate::slash_dispatch::dispatch_slash("/status", &state, &mut cfg).unwrap(),
            2
        );
    }
    std::fs::remove_dir_all(state.state_dir).unwrap();
}

#[cfg(unix)]
#[test]
fn cursor_and_grok_inventory_status_keep_passthrough_grammar() {
    use std::os::unix::fs::PermissionsExt;
    let state = state();
    let executable = state.state_dir.join("executor");
    let recorded = state.state_dir.join("argv");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            recorded.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut cfg = AgentConfig {
        agent_path: executable,
        ..AgentConfig::default()
    };
    for backend in [AgentBackend::Cursor, AgentBackend::Grok] {
        cfg.backend = backend;
        for verb in ["status", "models"] {
            assert_eq!(
                crate::commands::run_cli(
                    Cli::parse_from(["abbey", verb]),
                    state.clone(),
                    cfg.clone()
                )
                .unwrap(),
                0
            );
            assert_eq!(
                std::fs::read_to_string(&recorded).unwrap(),
                format!("{verb}\n")
            );
            assert_eq!(
                crate::slash_dispatch::dispatch_slash(&format!("/{verb}"), &state, &mut cfg)
                    .unwrap(),
                0
            );
            assert_eq!(
                std::fs::read_to_string(&recorded).unwrap(),
                format!("{verb}\n")
            );
        }
    }
    std::fs::remove_dir_all(state.state_dir).unwrap();
}
