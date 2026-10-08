//! Captured-context warnings use actual native TUI entries and redraw.

#[cfg(unix)]
fn notice_case(mode: &str) {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_please_fix_notice.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .arg(mode)
        .arg("--state-env")
        .arg(abbey::edition::ACTIVE.state_dir_env())
        .arg("--config-env")
        .arg(abbey::edition::ACTIVE.config_path_env())
        .output()
        .expect("python3 owned please-fix notice fixture required");
    assert!(
        output.status.success(),
        "please-fix notice {mode} failed: {}{}",
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
fn please_fix_cli_control_preserves_captured_warning_and_answer() {
    notice_case("control");
}

#[cfg(unix)]
#[test]
fn native_ctrl_p_capture_notice_survives_real_terminal_redraw() {
    notice_case("ctrl-p");
}

#[cfg(unix)]
#[test]
fn slash_please_fix_capture_notice_survives_real_terminal_redraw() {
    notice_case("slash");
}
