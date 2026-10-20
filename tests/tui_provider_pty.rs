//! Provider/model selection must survive real local commands and queued execution.

#[cfg(unix)]
#[test]
fn on_device_selection_survives_ambient_pcc_and_local_help() {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_provider_pty.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .arg("--state-env")
        .arg(abbey::edition::ACTIVE.state_dir_env())
        .arg("--config-env")
        .arg(abbey::edition::ACTIVE.config_path_env())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("python3 provider PTY fixture is required");
    assert!(
        output.status.success(),
        "provider PTY failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(proof["status"], "passed");
    assert_eq!(proof["model"], "system");
    assert_eq!(proof["calls"], 6);
    assert_eq!(
        proof["scenarios"],
        serde_json::json!([
            "provider-switch-local-help",
            "explicit-startup-model",
            "queued-model-selection",
            "model-clear-ambient-pcc",
            "failed-queued-model-discards-prompt"
        ])
    );
    assert_eq!(proof["scenario_calls"], serde_json::json!([1, 1, 2, 1, 1]));
}
