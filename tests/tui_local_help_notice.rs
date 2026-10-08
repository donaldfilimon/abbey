//! Real metadata-only Local slash help retains output under selected ABI.

#[cfg(unix)]
#[test]
fn local_help_and_empty_run_output_survive_real_terminal_redraw_without_generation() {
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/tests/smoke_tui_local_help_notice.py"
        ))
        .arg(env!("CARGO_BIN_EXE_abbey"))
        .arg("--state-env")
        .arg(abbey::edition::ACTIVE.state_dir_env())
        .arg("--config-env")
        .arg(abbey::edition::ACTIVE.config_path_env())
        .output()
        .expect("python3 owned Local help fixture required");
    assert!(
        output.status.success(),
        "local help fixture failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value = serde_json::from_slice(&output.stdout).expect("PTY proof JSON");
    assert_eq!(proof["status"], "passed");
    assert_eq!(proof["scenarios"].as_array().unwrap().len(), 4);
    assert!(proof["failures"].as_array().unwrap().is_empty());
}
