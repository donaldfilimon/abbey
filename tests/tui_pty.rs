//! Real terminal acceptance; fixtures never use provider services or user state.
#[cfg(unix)]
#[test]
fn isolated_pty_streaming_editor_interrupt_and_quit_restore_terminal() {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_pty.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .output()
        .expect("python3 PTY harness (required by the production gate)");
    assert!(
        output.status.success(),
        "PTY acceptance failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("PTY evidence JSON");
    assert_eq!(proof["status"], "passed");
    assert_eq!(proof["scenarios"].as_array().unwrap().len(), 9);
}
