//! Local configuration diagnostics never invoke an executor or modify settings.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "abbey-config-report-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir_all(path.join("state/by-cwd")).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn config(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    fn command(&self) -> Command {
        let edition = abbey::edition::ACTIVE;
        let mut command = Command::new(env!("CARGO_BIN_EXE_abbey"));
        command
            .env_clear()
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .env("APPDATA", &self.0)
            .env("LOCALAPPDATA", &self.0)
            .env("XDG_CONFIG_HOME", &self.0)
            .env("XDG_STATE_HOME", &self.0)
            .env("PATH", self.0.join("empty-path"))
            .env("ABBEY_BACKEND", "ollama")
            .env(edition.config_path_env(), self.config())
            .env(edition.state_dir_env(), self.0.join("state"))
            .current_dir(&self.0);
        command
    }

    fn run(&self, arg: &str) -> Output {
        self.command().arg(arg).output().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn slash_includes_cli_local_diagnostics_without_changing_config() {
    let scratch = Scratch::new();
    let text = "backend = 'claude'\npersona_policy = 'manual'\n[roles]\nmax = 'custom-max'\ngemma = 'custom-gemma'\n[embeddings]\nprovider = 'apple'\ndimension = 512\n";
    std::fs::write(scratch.config(), text).unwrap();
    let cli = scratch.run("config");
    let slash = scratch.run("/config");
    assert!(cli.status.success(), "{cli:?}");
    assert!(slash.status.success(), "{slash:?}");
    let cli = stdout(&cli);
    let slash = stdout(&slash);
    let local = slash.split_once("Local configuration").unwrap().1;
    assert!(local.contains("file/defaults + environment overrides"));
    for line in cli.lines().skip_while(|line| !line.starts_with("config:")) {
        assert!(local.lines().any(|candidate| candidate == line), "{line}");
    }
    assert!(local.contains("custom-max"));
    assert!(local.contains("custom-gemma"));
    assert!(local.contains("512"));
    assert_eq!(std::fs::read_to_string(scratch.config()).unwrap(), text);
    assert!(!scratch.0.join("state/memory.sqlite").exists());
    assert!(!scratch.0.join("state/route.jsonl").exists());
}

#[test]
fn malformed_config_reports_error_without_substitute_local_defaults() {
    let scratch = Scratch::new();
    let text = "[embeddings]\ndimension = not-an-integer\n";
    std::fs::write(scratch.config(), text).unwrap();
    let output = scratch.run("/config");
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("embedding dimension"), "{error}");
    assert!(
        error.contains(scratch.config().to_str().unwrap()),
        "{error}"
    );
    let report = stdout(&output);
    assert!(report.contains("Live session"));
    assert!(!report.contains("persona_policy:"), "{report}");
    assert!(!report.contains("embedding_dim:"), "{report}");
    assert_eq!(std::fs::read_to_string(scratch.config()).unwrap(), text);
}

#[test]
fn absent_config_reports_local_defaults_without_creating_file() {
    let scratch = Scratch::new();
    let output = scratch.run("/config");
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    assert!(report.contains("persona_policy:  auto"), "{report}");
    assert!(!scratch.config().exists());
}

#[test]
fn tui_recipe_reports_current_selection_not_ambient_backend_or_model() {
    let scratch = Scratch::new();
    let state = scratch.0.join("state");
    let recipe = serde_json::json!({
        "version": 1,
        "edition": abbey::edition::ACTIVE.slug(),
        "cwd": scratch.0,
        "cfg": {
            "agent_path": scratch.0.join("missing-live-claude"),
            "model": "live-model", "backend": "Claude",
            // The recipe carries a complete current configuration, so every
            // field of the config is named — including the flag that records
            // an explicitly selected model.
            "model_selected": true,
            "auto_review": false, "trust": false, "force": false,
            "no_resume": true, "mode": null, "print": false,
            "output_format": null, "worktree": null, "workspace": null,
            "add_dirs": [], "sandbox": null, "extra_args": [],
            "transcript_dir": null, "media_note": null,
            "media_prefers_gemma": false, "force_capture": false,
            "cot_path": null, "permission_mode": null
        },
        "state": {
            "state_dir": state, "chat_file": state.join("chat-id"),
            "model_file": state.join("model"),
            "history_file": state.join("history.log"),
            "cwd_dir": state.join("by-cwd"),
            "per_cwd": true, "cwd": scratch.0
        }
    });
    let output = scratch
        .command()
        .env("ABBEY_MODEL", "ambient-model")
        .env("ABBEY_INTERNAL_TUI_LOCAL_RECIPE", recipe.to_string())
        .arg("/config")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report = stdout(&output);
    let live = report.split_once("Local configuration").unwrap().0;
    assert!(live.contains("backend:   claude"), "{live}");
    assert!(live.contains("model:     live-model"), "{live}");
    assert!(!live.contains("model:     ambient-model"), "{live}");
    assert!(!scratch.config().exists());
    assert_eq!(std::fs::read_dir(state).unwrap().count(), 1);
}
