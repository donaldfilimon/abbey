use super::*;
use crate::tui::app::{PROVIDERS, ProviderChoice};

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
    let before = std::fs::read_to_string(&calls).unwrap();
    app.overlay = Overlay::ModelPicker { idx: 0 };
    for _ in 0..5 {
        let _ = screen(&app, 120, 24);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Up);
    }
    assert_eq!(std::fs::read_to_string(calls).unwrap(), before);
    assert!(before.lines().all(|line| line == "--version"));
}
