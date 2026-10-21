//! Process-level proof that the Ollama backend never pulls a model.
//!
//! Abbey enforces a strict no-auto-pull policy: before `ollama run <tag>` is
//! spawned, the tag must already be present in the local `ollama list`. A
//! missing tag has to fail the turn, not reach the network.
//!
//! The stub `ollama` used here lists no model row and leaves a marker behind
//! if `run` is ever invoked, so the assertions cover the whole chain: the
//! preflight probe ran (`ollama list` was called) and the model run did not
//! (`ollama run` was never called).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::Command;

/// The binary under test, provided by cargo for integration tests.
const BIN: &str = env!("CARGO_BIN_EXE_abbey");

/// The default local Ollama tag Abbey selects when no model is requested.
const DEFAULT_TAG: &str = "gemma4:26b-mlx";

/// A per-test scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-ollama-no-pull-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).expect("create scratch dir");
        Self(dir)
    }

    /// Marker written by the stub when `ollama list` was called.
    fn list_marker(&self) -> PathBuf {
        self.0.join("ollama-listed")
    }

    /// Marker written by the stub when `ollama run` was called.
    fn run_marker(&self) -> PathBuf {
        self.0.join("ollama-ran")
    }

    /// Install a stub `ollama` on the isolated PATH.
    ///
    /// `listing` is the body of the `ollama list` branch — the model rows the
    /// stub claims to have locally. `run` always leaves the run marker behind,
    /// so a spawned model run is visible to the assertions.
    fn stub_ollama(&self, listing: &str) {
        let path = self.0.join("bin").join("ollama");
        std::fs::write(
            &path,
            format!(
                r#"#!/bin/sh
case "$1" in
  list)
    printf x > "$ABBEY_TEST_OLLAMA_LIST_MARKER"
{listing}
    ;;
  run)
    printf x > "$ABBEY_TEST_OLLAMA_RUN_MARKER"
    printf 'stubbed local answer\n'
    ;;
  *) exit 9 ;;
esac
"#
            ),
        )
        .unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o700);
        std::fs::set_permissions(&path, perm).unwrap();
    }

    /// Run `abbey` hermetically: only the stub bin dir on PATH, a scratch HOME,
    /// state dir, and config, with the Ollama backend selected explicitly.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(BIN)
            .args(args)
            .env_clear()
            .env(abbey::edition::ACTIVE.state_dir_env(), &self.0)
            .env(
                abbey::edition::ACTIVE.config_path_env(),
                self.0.join("config.toml"),
            )
            .env("HOME", &self.0)
            .env("PATH", self.0.join("bin"))
            // Absolute candidate probing (/opt/homebrew, /usr/local) would find
            // a real ollama on the host and defeat the hermetic staging.
            .env("ABBEY_TEST_HOME_AGENTS_ONLY", "1")
            .env("ABBEY_BACKEND", "ollama")
            .env("ABBEY_MEMORY_BACKEND", "sqlite")
            .env("ABBEY_TEST_OLLAMA_LIST_MARKER", self.list_marker())
            .env("ABBEY_TEST_OLLAMA_RUN_MARKER", self.run_marker())
            .output()
            .expect("spawn abbey");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `ollama list` with a header and no model row: nothing is present locally.
const LISTED_NONE: &str = "    printf 'NAME\\tID\\tSIZE\\tMODIFIED\\n'";

/// `ollama list` with a header and the default tag present locally.
const LISTED_DEFAULT: &str =
    "    printf 'NAME\\tID\\tSIZE\\tMODIFIED\\ngemma4:26b-mlx\\tabc\\t1GB\\tnow\\n'";

/// `abbey print` is the documented headless capture surface. It must refuse a
/// tag that is not present locally instead of letting `ollama run` pull it.
#[test]
fn print_refuses_a_missing_local_model_instead_of_pulling_it() {
    let s = Scratch::new("print");
    s.stub_ollama(LISTED_NONE);

    let (code, out, err) = s.run(&["print", "hello"]);

    assert_ne!(code, 0, "a missing local model must fail the turn: {out}");
    assert!(err.contains("no-auto-pull"), "{err}");
    assert!(err.contains(DEFAULT_TAG), "{err}");
    assert!(
        s.list_marker().exists(),
        "the preflight must ask `ollama list`"
    );
    assert!(
        !s.run_marker().exists(),
        "`ollama run` must never be invoked for a missing local model"
    );
}

/// The global `--print` flag with a prompt runs through the persona/role wrap
/// and the same captured spawn, so it must refuse the missing tag too.
#[test]
fn prompted_print_refuses_a_missing_local_model_instead_of_pulling_it() {
    let s = Scratch::new("prompt");
    s.stub_ollama(LISTED_NONE);

    let (code, out, err) = s.run(&["--print", "hello"]);

    assert_ne!(code, 0, "a missing local model must fail the turn: {out}");
    assert!(err.contains("no-auto-pull"), "{err}");
    assert!(err.contains(DEFAULT_TAG), "{err}");
    assert!(
        s.list_marker().exists(),
        "the preflight must ask `ollama list`"
    );
    assert!(
        !s.run_marker().exists(),
        "`ollama run` must never be invoked for a missing local model"
    );
}

/// The counterpart: a tag that *is* present locally still runs. Without this,
/// a preflight that always refuses would satisfy the no-pull tests above.
#[test]
fn print_runs_when_the_model_is_present_locally() {
    let s = Scratch::new("present");
    s.stub_ollama(LISTED_DEFAULT);

    let (code, out, err) = s.run(&["print", "hello"]);

    assert_eq!(code, 0, "a locally present model must run: {err}");
    assert!(out.contains("stubbed local answer"), "{out}");
    assert!(
        s.list_marker().exists(),
        "the preflight must ask `ollama list`"
    );
    assert!(
        s.run_marker().exists(),
        "a locally present model must reach `ollama run`"
    );
}
