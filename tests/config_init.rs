//! Config initialization preserves existing destinations and reports them honestly.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("abbey-config-init-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_abbey"))
            .args(["config", "--init"])
            .env_clear()
            .env("HOME", &self.0)
            .env("PATH", "/usr/bin:/bin")
            .env(
                abbey::edition::ACTIVE.config_path_env(),
                self.0.join("config.toml"),
            )
            .env(abbey::edition::ACTIVE.state_dir_env(), self.0.join("state"))
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn init_rejects_a_directory_without_reporting_a_write() {
    let scratch = Scratch::new();
    let path = scratch.0.join("config.toml");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("keep"), "untouched").unwrap();
    let output = scratch.run();
    assert!(!output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("wrote default config"));
    assert_eq!(fs::read_to_string(path.join("keep")).unwrap(), "untouched");
}

#[cfg(unix)]
#[test]
fn init_reports_a_preserved_dangling_symlink_as_existing() {
    let scratch = Scratch::new();
    let path = scratch.0.join("config.toml");
    let target = scratch.0.join("absent.toml");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let output = scratch.run();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("config already exists, left untouched"),
        "{stderr}"
    );
    assert!(!stderr.contains("wrote default config"));
    assert_eq!(fs::read_link(path).unwrap(), target);
    assert!(!target.exists());
}
