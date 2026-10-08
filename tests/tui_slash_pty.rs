//! Real CLI/PTY slash ownership; only private HOME/PATH response fixtures.
//! No production testing hook.
#[cfg(unix)]
fn slash_pty(mode: &str) {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_slash_pty.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .arg(mode)
        .arg("--state-env")
        .arg(abbey::edition::ACTIVE.state_dir_env())
        .arg("--config-env")
        .arg(abbey::edition::ACTIVE.config_path_env())
        .output()
        .expect("python3 owned PTY fixture required by source gate");
    assert!(
        output.status.success(),
        "owned slash PTY {mode} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("owned PTY evidence JSON");
    assert_eq!(proof["status"], "passed");
    assert!(!proof["scenarios"].as_array().unwrap().is_empty());
    assert!(proof["failures"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn slash_models_status_commit_output_survives_real_terminal_redraw() {
    slash_pty("output");
}

#[cfg(unix)]
#[test]
fn slash_models_status_commit_cancel_observes_join_before_queue_reuse() {
    slash_pty("cancel");
}

#[cfg(unix)]
#[test]
fn slash_models_status_commit_quit_joins_before_terminal_restore() {
    slash_pty("quit");
}

#[cfg(unix)]
#[test]
fn local_slash_uses_actual_selected_backend_and_conversation_owner() {
    slash_pty("backend");
}

#[cfg(unix)]
#[test]
fn nested_stateless_teacher_cancel_and_quit_close_all_owned_groups() {
    slash_pty("teacher");
}
