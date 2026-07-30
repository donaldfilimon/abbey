use std::process::Command;

fn abbey() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_abbey"));
    c.env("CURSOR_AGENT_SHELL", "0");
    c
}

#[test]
fn version_prints() {
    let out = abbey().arg("--version").output().expect("run");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("abbey"), "{s}");
}

#[test]
fn slash_help_lists_commands() {
    let out = abbey().arg("slash-help").output().expect("run");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("/plan"));
    assert!(s.contains("/diff"));
    assert!(s.contains("/pr"));
}

#[test]
fn doctor_exits_zero() {
    let out = abbey().arg("doctor").output().expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("parity") || s.contains("agent"));
}

#[test]
fn cost_is_honest() {
    let out = abbey().arg("cost").output().expect("run");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("N/A") || s.contains("not exposed") || s.contains("dashboard"));
}

#[test]
fn completion_zsh_emits_compdef() {
    let out = abbey().args(["completion", "zsh"]).output().expect("run");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("compdef") || s.contains("_abbey"));
}

#[test]
fn init_print_scans_this_repo() {
    let out = abbey()
        .args(["init", "--print"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("# AGENTS.md"), "{s}");
    assert!(s.contains("Rust") || s.contains("cargo"), "{s}");
    assert!(s.contains("abbey") || s.contains("Commands"), "{s}");
}

#[test]
fn init_force_writes_agents_md() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "init-it"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let out = abbey()
        .args(["init", "--force"])
        .current_dir(&dir)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let md = std::fs::read_to_string(dir.join("AGENTS.md")).expect("AGENTS.md");
    assert!(md.contains("init-it"));
    assert!(md.contains("cargo test"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "abbey-it-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
