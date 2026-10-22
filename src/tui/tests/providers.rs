use super::*;
use crate::tui::app::ModelMutation;
use crate::tui::app::{PROVIDERS, ProviderChoice};

fn finish_local(app: &mut App, command: &str, code: i32, cancelled: bool) {
    let (_events, done) = fixture_run(app);
    app.run.as_mut().unwrap().local = true;
    app.pending_model_mutation = ModelMutation::for_local(&[command.into()]);
    if cancelled {
        app.run.as_ref().unwrap().cancel.cancel();
    }
    done.send((code, None)).unwrap();
    app.pump();
    assert!(!app.is_running());
    assert!(app.pending_model_mutation.is_none());
}

#[test]
fn only_successful_explicit_model_mutations_update_the_live_model() {
    let mut app = isolated_app();
    app.cfg.backend = AgentBackend::Fm;
    app.cfg.model = "system".into();
    app.state.save_model_literal("pcc").unwrap();
    for command in ["/help", "/model", "/think", "/doctor"] {
        finish_local(&mut app, command, 0, false);
        assert_eq!(app.cfg.model, "system", "unrelated command {command}");
    }
    for (code, cancelled) in [(1, false), (130, false), (0, true)] {
        finish_local(&mut app, "/model pcc", code, cancelled);
        assert_eq!(app.cfg.model, "system");
    }
    finish_local(&mut app, "/model pcc", 0, false);
    assert_eq!(app.cfg.model, "pcc");
    app.state.save_model_literal("system").unwrap();
    finish_local(&mut app, "/model system", 0, false);
    assert_eq!(app.cfg.model, "system");
    app.cfg.backend = AgentBackend::Claude;
    app.state
        .save_model_literal("persisted-catalog-id")
        .unwrap();
    finish_local(&mut app, "/thinking high", 0, false);
    assert_eq!(
        app.cfg.model, "persisted-catalog-id",
        "use persisted result, not guessed argv"
    );
    app.state.clear_model().unwrap();
    finish_local(&mut app, "/model clear", 0, false);
    assert_eq!(app.cfg.model, "auto");
}

#[test]
fn ambient_pcc_does_not_override_live_selections_after_local_commands() {
    const CHILD: &str = "ABBEY_TEST_PROVIDER_MODEL_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tui::tests::providers::ambient_pcc_does_not_override_live_selections_after_local_commands", "--nocapture"])
            .env(CHILD, "1")
            .env("ABBEY_MODEL", "pcc")
            .env("ABBEY_BACKEND", "cursor")
            .stdin(std::process::Stdio::null())
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    assert_eq!(std::env::var("ABBEY_MODEL").unwrap(), "pcc");
    let mut app = isolated_app();
    app.select_provider(
        AgentBackend::Fm,
        app.state.state_dir.join("missing-fixture-fm"),
    );
    assert_eq!(app.cfg.model, "system");
    finish_local(&mut app, "/help", 0, false);
    assert_eq!(app.cfg.model, "system");
    app.overlay = Overlay::ModelPicker { idx: 1 };
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.model, "pcc");
    app.overlay = Overlay::ModelPicker { idx: 0 };
    press(&mut app, KeyCode::Enter);
    finish_local(&mut app, "/help", 0, false);
    assert_eq!(app.cfg.model, "system");
    app.state.save_model_literal("pcc").unwrap();
    finish_local(&mut app, "/model pcc", 0, false);
    assert_eq!(
        app.cfg.model, "pcc",
        "an explicit successful model mutation remains deliberate"
    );
    app.state.clear_model().unwrap();
    finish_local(&mut app, "/model clear", 0, false);
    assert_eq!(
        app.cfg.model, "system",
        "clear under FM must not select ambient PCC"
    );
}

fn isolated_app() -> App {
    let dir = std::env::temp_dir().join(format!("abbey-provider-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("by-cwd")).unwrap();
    let state = AbbeyState {
        state_dir: dir.clone(),
        chat_file: dir.join("chat-id"),
        model_file: dir.join("model"),
        history_file: dir.join("history.log"),
        cwd_dir: dir.join("by-cwd"),
        per_cwd: false,
        cwd: dir.clone(),
    };
    App::new(
        state,
        AgentConfig {
            backend: AgentBackend::Cursor,
            agent_path: dir.join("missing-cli"),
            ..AgentConfig::default()
        },
    )
    .unwrap()
}

#[test]
fn provider_shortcut_palette_and_help_are_discoverable() {
    let mut app = isolated_app();
    ctrl(&mut app, 's');
    assert!(matches!(app.overlay, Overlay::ProviderPicker { .. }));
    let Overlay::ProviderPicker { choices, .. } = &app.overlay else {
        panic!("provider picker");
    };
    assert_eq!(
        choices.iter().map(|p| p.backend).collect::<Vec<_>>(),
        PROVIDERS
    );
    press(&mut app, KeyCode::Esc);
    ctrl(&mut app, 'k');
    type_str(&mut app, "Choose provider");
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Overlay::ProviderPicker { .. }));
    assert!(
        crate::tui::overlays::help_lines()
            .iter()
            .any(|line| line.contains("Ctrl-S"))
    );
}

#[test]
fn unavailable_provider_stays_visible_and_does_not_mutate_active_backend() {
    let mut app = isolated_app();
    let choices = PROVIDERS
        .into_iter()
        .map(|backend| ProviderChoice {
            backend,
            executable: Err("fixture CLI not installed".into()),
        })
        .collect();
    app.overlay = Overlay::ProviderPicker { idx: 2, choices };
    let before = app.cfg.agent_path.clone();
    let rendered = screen(&app, 140, 30);
    assert!(rendered.contains("Apple Foundation Models"));
    assert!(rendered.contains("unavailable"));
    for _ in 0..3 {
        press(&mut app, KeyCode::Enter);
        assert!(
            app.status
                .contains("fm unavailable: fixture CLI not installed")
        );
    }
    assert_eq!(app.cfg.backend, AgentBackend::Cursor);
    assert_eq!(app.cfg.agent_path, before);
    assert!(matches!(
        app.overlay,
        Overlay::ProviderPicker { idx: 2, .. }
    ));
}

#[test]
fn fm_model_choices_remain_exact_without_live_inventory_or_executable() {
    let mut app = isolated_app();
    app.cfg.backend = AgentBackend::Fm;
    app.cfg.model = "system".into();
    app.live_models = vec!["foreign-model".into()];
    app.overlay = Overlay::ModelPicker { idx: 0 };
    let rendered = screen(&app, 140, 24);
    assert!(rendered.contains("system  Apple Foundation Models"));
    assert!(rendered.contains("pcc  Private Cloud Compute"));
    assert!(rendered.contains("OFF-DEVICE"));
    assert!(rendered.contains("readiness: /doctor"));
    assert!(!rendered.contains("foreign-model"));
    for _ in 0..10 {
        press(&mut app, KeyCode::Down);
    }
    assert!(matches!(app.overlay, Overlay::ModelPicker { idx: 1 }));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.model, "pcc");
    assert!(app.status.contains("OFF-DEVICE"));
    assert_eq!(
        std::fs::read_to_string(&app.state.model_file)
            .unwrap()
            .trim(),
        "pcc"
    );
    assert!(
        app.run.is_none(),
        "FM model selection must not spawn a child with the launch backend"
    );
    app.overlay = Overlay::ModelPicker { idx: 0 };
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.model, "system");
    assert!(app.status.contains("on-device"));
}

#[test]
fn provider_and_model_changes_are_guarded_while_running() {
    let mut app = isolated_app();
    let (_events, _done) = fixture_run(&mut app);
    ctrl(&mut app, 's');
    assert!(matches!(app.overlay, Overlay::None));
    assert!(app.status.contains("active run"));
    app.select_provider(AgentBackend::Fm, app.state.state_dir.join("fake-fm"));
    assert_eq!(app.cfg.backend, AgentBackend::Cursor);
    app.cfg.backend = AgentBackend::Fm;
    app.cfg.model = "system".into();
    app.overlay = Overlay::ModelPicker { idx: 1 };
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.model, "system");
    ctrl(&mut app, 'b');
    assert_eq!(app.cfg.backend, AgentBackend::Fm);
    assert!(app.status.contains("active run"));
}

#[test]
fn leaving_fm_resets_apple_models_to_persisted_auto_for_every_other_provider() {
    let mut app = isolated_app();
    let missing_cli = app.state.state_dir.join("missing-fixture-cli");
    for backend in PROVIDERS.into_iter().filter(|b| *b != AgentBackend::Fm) {
        for model in ["system", "pcc"] {
            app.cfg.backend = AgentBackend::Fm;
            app.cfg.model = model.into();
            app.state.save_model_literal(model).unwrap();
            app.select_provider(backend, missing_cli.clone());
            assert_eq!(app.cfg.backend, backend);
            assert_eq!(app.cfg.model, "auto");
            assert_eq!(
                std::fs::read_to_string(&app.state.model_file)
                    .unwrap()
                    .trim(),
                "auto"
            );
            assert_eq!(
                app.cfg.transcript_dir,
                Some(app.state.state_dir.join(backend.transcript_subdir()))
            );
        }
    }
}

#[test]
fn same_provider_selection_preserves_the_explicit_model() {
    let mut app = isolated_app();
    let missing_cli = app.state.state_dir.join("missing-fixture-cli");
    for (backend, model) in [
        (AgentBackend::Fm, "pcc"),
        (AgentBackend::Ollama, "custom:tag"),
    ] {
        app.cfg.backend = backend;
        app.cfg.model = model.into();
        app.state.save_model_literal(model).unwrap();
        app.select_provider(backend, missing_cli.clone());
        assert_eq!(app.cfg.model, model);
        assert_eq!(
            std::fs::read_to_string(&app.state.model_file)
                .unwrap()
                .trim(),
            model
        );
    }
}

#[test]
fn failed_transition_model_save_keeps_provider_model_and_identity_unchanged() {
    let mut app = isolated_app();
    // A directory at the model-file destination makes replacement fail without
    // permissions assumptions, environment changes, or external executables.
    std::fs::create_dir(&app.state.model_file).unwrap();
    for (from, model, to) in [
        (AgentBackend::Fm, "pcc", AgentBackend::Ollama),
        (AgentBackend::Claude, "sonnet", AgentBackend::Fm),
    ] {
        app.cfg.backend = from;
        app.cfg.model = model.into();
        let old_path = app.cfg.agent_path.clone();
        let old_transcripts = app.cfg.transcript_dir.clone();
        app.select_provider(to, app.state.state_dir.join("replacement-cli"));
        assert_eq!(app.cfg.backend, from);
        assert_eq!(app.cfg.model, model);
        assert_eq!(app.cfg.agent_path, old_path);
        assert_eq!(app.cfg.transcript_dir, old_transcripts);
        assert!(app.status.contains("provider unchanged: could not select"));
    }
}

#[cfg(unix)]
#[test]
fn selecting_fake_fm_resets_vendor_or_cloud_model_without_querying_services() {
    use std::os::unix::fs::PermissionsExt;
    let mut app = isolated_app();
    let fm = app.state.state_dir.join("fm");
    let calls = app.state.state_dir.join("calls");
    std::fs::write(&fm, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n--version) echo fixture-fm;;\n*) exit 1;;\nesac\n", calls.display())).unwrap();
    std::fs::set_permissions(&fm, std::fs::Permissions::from_mode(0o700)).unwrap();
    for old in ["claude-sonnet-4", "pcc"] {
        app.cfg.backend = AgentBackend::Claude;
        app.cfg.model = old.into();
        app.state.save_model_literal(old).unwrap();
        app.select_provider(AgentBackend::Fm, fm.clone());
        assert_eq!(app.cfg.model, "system");
        assert_eq!(
            app.cfg.transcript_dir,
            Some(
                app.state
                    .state_dir
                    .join(AgentBackend::Fm.transcript_subdir())
            )
        );
        assert!(app.status.contains("Apple Foundation Models"));
    }
    let before = wait_for_calls(&calls);
    app.overlay = Overlay::ModelPicker { idx: 0 };
    for _ in 0..5 {
        let _ = screen(&app, 120, 24);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Up);
    }
    let after = wait_for_calls(&calls);
    // The doctor panel's `--version` probe runs in the background, so the
    // number of calls may still grow; what must not happen is the picker
    // invoking the provider in any other way.
    assert!(
        before.lines().all(|line| line == "--version"),
        "only the doctor refresh may probe the provider: {before:?}"
    );
    assert!(
        after.lines().all(|line| line == "--version"),
        "the model picker must not invoke the provider: {after:?}"
    );
}

/// Read the fake provider's call log, waiting for the background probe.
///
/// The `--version` probe runs on the refresh thread, so the log appears a
/// moment after the selection. Waiting for it is not a race: the probe is
/// part of what the test observes.
fn wait_for_calls(calls: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(calls)
            && !text.is_empty()
        {
            return text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the provider probe never called the fake binary: {}",
            calls.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
