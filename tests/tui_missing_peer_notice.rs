//! Missing-peer warnings use the real owned PATH and survive TUI redraw.

#[cfg(unix)]
fn missing_peer_case(mode: &str) {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_missing_peer_notice.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .arg(mode)
        .arg("--state-env")
        .arg(abbey::edition::ACTIVE.state_dir_env())
        .arg("--config-env")
        .arg(abbey::edition::ACTIVE.config_path_env())
        .output()
        .expect("python3 owned missing-peer notice fixture required");
    assert!(
        output.status.success(),
        "missing-peer notice {mode} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value = serde_json::from_slice(&output.stdout).expect("PTY proof JSON");
    assert_eq!(proof["status"], "passed");
    assert_eq!(proof["scenarios"].as_array().unwrap().len(), 1);
    assert!(proof["failures"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn missing_peer_cli_control_keeps_warning_and_all_missing_refusal() {
    missing_peer_case("control");
}

#[cfg(unix)]
#[test]
fn missing_peer_notice_survives_real_terminal_redraw() {
    missing_peer_case("tui");
}
