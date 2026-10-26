//! Explicit preview contracts. Every source/config/state is a synthetic scratch
//! fixture; no backend, live memory store, or real private directory is opened.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("abbey-knowledge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        // macOS temp_dir may begin with the /var symlink. The fixture itself
        // must select its actual path; the product never canonicalizes input.
        Self(path.canonicalize().unwrap())
    }

    fn run(&self, config: &str, args: &[&str]) -> Output {
        std::fs::write(self.0.join("config.toml"), config).unwrap();
        let output = self.command(args);
        assert_eq!(
            std::fs::read_to_string(self.0.join("config.toml")).unwrap(),
            config
        );
        output
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_abbey"))
            .env_clear()
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .env("ABBEY_CONFIG", self.0.join("config.toml"))
            .env("ABBEY_PERSONAL_CONFIG", self.0.join("config.toml"))
            .env("ABBEY_STATE_DIR", self.0.join("absent-state"))
            .env("ABBEY_PERSONAL_STATE_DIR", self.0.join("absent-state"))
            .stdin(Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    #[cfg(unix)]
    fn folder_config(&self) -> String {
        format!(
            "[knowledge]\nsource = 'folder'\nfolder = '{}'\n",
            self.0.join("selected").display()
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn status_and_missing_folder_search_create_no_state_and_infer_no_source() {
    let scratch = Scratch::new();
    let status = json(scratch.command(&["knowledge", "status"]));
    assert!(!scratch.0.join("config.toml").exists());
    assert_eq!(status["source"], "folder");
    assert_eq!(status["folder_configured"], false);
    let output = scratch.run("", &["knowledge", "search", "fixture"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not configured"));
    assert!(!scratch.0.join("absent-state").exists());
}

#[test]
fn synthetic_is_explicit_deterministic_and_never_opens_memory() {
    let scratch = Scratch::new();
    let config = "[knowledge]\nsource = 'synthetic'";
    let first = json(scratch.run(config, &["knowledge", "search", "fixture", "--limit", "1"]));
    let second = json(scratch.run(config, &["knowledge", "search", "fixture", "--limit", "1"]));
    assert_eq!(first, second);
    assert_eq!(first["results"].as_array().unwrap().len(), 1);
    assert!(!scratch.0.join("absent-state").exists());
    for config in [
        "[knowledge]\nsource = 'memory'",
        "[knowledge]\nsource = 'invalid'",
        "[knowledge]\nsource = 'synthetic",
    ] {
        assert!(
            !scratch
                .run(config, &["knowledge", "search", "fixture"])
                .status
                .success()
        );
    }
    assert!(!scratch.0.join("absent-state").exists());
}

#[test]
fn configured_status_does_not_inspect_missing_or_unsafe_path() {
    let scratch = Scratch::new();
    let config = "[knowledge]\nsource = 'folder'\nfolder = '/missing/.hidden/credentials'";
    let status = json(scratch.run(config, &["knowledge", "status"]));
    assert_eq!(status["folder_configured"], true);
    assert!(!status.to_string().contains("/missing"));
    assert!(!scratch.0.join("absent-state").exists());
}

#[cfg(unix)]
#[test]
fn folder_reads_only_visible_plain_text_nonrecursively_without_paths_or_state() {
    let scratch = Scratch::new();
    let selected = scratch.0.join("selected");
    std::fs::create_dir(&selected).unwrap();
    std::fs::write(selected.join("notes.md"), "A visible juniper project note.").unwrap();
    for name in [
        ".env",
        ".hidden.md",
        "credentials.txt",
        "secrets.md",
        "image.png",
    ] {
        std::fs::write(selected.join(name), "FORBIDDEN marker").unwrap();
    }
    std::fs::create_dir(selected.join("nested")).unwrap();
    std::fs::write(selected.join("nested/deep.txt"), "FORBIDDEN marker").unwrap();
    let config = scratch.folder_config();
    let result = json(scratch.run(&config, &["knowledge", "search", "JUNIPER"]));
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"][0]["sha256"].as_str().unwrap().len(), 64);
    assert!(!result.to_string().contains("notes.md"));
    assert!(!result.to_string().contains(selected.to_str().unwrap()));
    assert!(
        json(scratch.run(&config, &["knowledge", "search", "FORBIDDEN"]))["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!scratch.0.join("absent-state").exists());
}

#[cfg(unix)]
#[test]
fn folder_refuses_symlink_roots_ancestors_entries_and_special_files() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("note.txt"), "FORBIDDEN marker").unwrap();
    let selected = scratch.0.join("selected");
    symlink(&outside, &selected).unwrap();
    let config = scratch.folder_config();
    assert!(
        !scratch
            .run(&config, &["knowledge", "search", "marker"])
            .status
            .success()
    );
    let ancestor_config = format!(
        "[knowledge]\nfolder = '{}'",
        selected.join("child").display()
    );
    std::fs::create_dir(outside.join("child")).unwrap();
    assert!(
        !scratch
            .run(&ancestor_config, &["knowledge", "search", "marker"])
            .status
            .success()
    );
    std::fs::remove_file(&selected).unwrap();
    std::fs::create_dir(&selected).unwrap();
    symlink(outside.join("note.txt"), selected.join("note.txt")).unwrap();
    let output = scratch.run(&config, &["knowledge", "search", "marker"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    std::fs::remove_file(selected.join("note.txt")).unwrap();
    nix::unistd::mkfifo(&selected.join("pipe.txt"), nix::sys::stat::Mode::S_IRUSR).unwrap();
    assert!(
        !scratch
            .run(&config, &["knowledge", "search", "marker"])
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn folder_fails_closed_on_file_total_entry_and_text_bounds() {
    let scratch = Scratch::new();
    let selected = scratch.0.join("selected");
    std::fs::create_dir(&selected).unwrap();
    let config = scratch.folder_config();
    let note = selected.join("note.txt");
    for bytes in [vec![b'x'; 65537], vec![0, 1, 2], vec![0xff]] {
        std::fs::write(&note, bytes).unwrap();
        assert!(
            !scratch
                .run(&config, &["knowledge", "search", "x"])
                .status
                .success()
        );
    }
    std::fs::remove_file(&note).unwrap();
    for i in 0..9 {
        std::fs::write(selected.join(format!("{i}.txt")), vec![b'x'; 65536]).unwrap();
    }
    assert!(
        !scratch
            .run(&config, &["knowledge", "search", "x"])
            .status
            .success()
    );
    for i in 0..9 {
        std::fs::remove_file(selected.join(format!("{i}.txt"))).unwrap();
    }
    for i in 0..65 {
        std::fs::write(selected.join(format!("{i}.txt")), "x").unwrap();
    }
    assert!(
        !scratch
            .run(&config, &["knowledge", "search", "x"])
            .status
            .success()
    );
    for i in 0..65 {
        std::fs::remove_file(selected.join(format!("{i}.txt"))).unwrap();
    }
    for i in 0..129 {
        std::fs::write(selected.join(format!("{i}.bin")), "x").unwrap();
    }
    assert!(
        !scratch
            .run(&config, &["knowledge", "search", "x"])
            .status
            .success()
    );
}
