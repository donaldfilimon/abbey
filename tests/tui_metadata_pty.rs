//! Real /doctor metadata teardown under a selected private ABI fixture.

#[cfg(unix)]
fn metadata_pty(mode: &str) {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_metadata_pty.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .arg(mode)
        .arg("--state-env")
        .arg(abbey::edition::ACTIVE.state_dir_env())
        .arg("--config-env")
        .arg(abbey::edition::ACTIVE.config_path_env())
        .output()
        .expect("python3 owned metadata PTY fixture is required");
    assert!(
        output.status.success(),
        "metadata PTY {mode} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("metadata PTY evidence JSON");
    assert_eq!(proof["status"], "passed");
    assert_eq!(proof["scenarios"].as_array().unwrap().len(), 1);
    assert!(proof["failures"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn doctor_metadata_control_executes_exact_selected_version_probe() {
    metadata_pty("control");
}

#[cfg(unix)]
#[test]
fn doctor_metadata_cancel_joins_nested_probe_before_queue_reuse() {
    metadata_pty("cancel");
}

#[cfg(unix)]
#[test]
fn doctor_metadata_quit_joins_nested_probe_before_terminal_restore() {
    metadata_pty("quit");
}
