//! Process-level checks for the read-only "effective model" summary.
//!
//! The summary answers exactly one question — *which model would a turn use
//! right now?* — and it has to answer it the same way in the CLI doctor
//! output and in the TUI doctor panel. These tests drive the real binary, so
//! the claim is checked against the process that actually runs rather than
//! against a helper in isolation.
//!
//! Every case runs hermetically: no executor on PATH, a scratch HOME, a
//! scratch config, and a scratch state dir, so nothing here can reach the
//! developer's real chat id, model selection, or memory store.

use std::path::PathBuf;
use std::process::Command;

/// The binary under test, provided by cargo for integration tests.
const BIN: &str = env!("CARGO_BIN_EXE_abbey");

/// A per-test state directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-effective-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch state dir");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `abbey` against a hermetic environment: no executor on PATH, a scratch
/// HOME and config, and only the backend/model knobs the case names.
fn run(
    scratch: &Scratch,
    backend: &str,
    model: Option<&str>,
    args: &[&str],
) -> (i32, String, String) {
    let empty_path = scratch.0.join("empty-path");
    std::fs::create_dir_all(&empty_path).expect("create empty path dir");
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env_clear()
        .env(abbey::edition::ACTIVE.state_dir_env(), &scratch.0)
        .env(
            abbey::edition::ACTIVE.config_path_env(),
            scratch.0.join("config.toml"),
        )
        .env("HOME", &scratch.0)
        .env("PATH", &empty_path)
        // Absolute candidate probing (/opt/homebrew, /usr/bin) would find a
        // real executor on the host and defeat the hermetic staging.
        .env("ABBEY_TEST_HOME_AGENTS_ONLY", "1")
        .env("ABBEY_BACKEND", backend);
    if let Some(model) = model {
        command.env("ABBEY_MODEL", model);
    }
    let out = command.output().expect("spawn abbey");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The `effective model:` line from a doctor run.
fn effective_line(stdout: &str) -> String {
    stdout
        .lines()
        .find(|line| line.starts_with("effective model:"))
        .unwrap_or_else(|| panic!("no effective model line in:\n{stdout}"))
        .to_string()
}

/// Every backend reports the model a turn would use *now*, resolved with that
/// backend's own vocabulary — and never with a foreign one.
#[test]
fn doctor_names_the_effective_model_for_each_backend() {
    let s = Scratch::new("backends");
    for (backend, expected) in [
        (
            "ollama",
            "effective model: gemma4:26b-mlx  (live: gemma4:26b-mlx, backend: ollama)",
        ),
        ("fm", "effective model: system  (live: auto, backend: fm)"),
        ("abi", "effective model: local  (live: local, backend: abi)"),
        (
            "cursor",
            "effective model: auto  (live: auto, backend: cursor-agent)",
        ),
        ("grok", "effective model: auto  (live: auto, backend: grok)"),
        (
            "claude",
            "effective model: auto  (live: auto, backend: claude)",
        ),
    ] {
        let (code, out, err) = run(&s, backend, None, &["doctor"]);
        assert_eq!(code, 0, "{backend} doctor should exit 0: {err}");
        assert_eq!(effective_line(&out), expected, "{backend} doctor line");
    }
}

/// A cursor-shaped id must never survive as the effective model of a backend
/// that does not speak it: `fm` has two models, and `abi` expands nothing
/// into something that would look like an explicit Anthropic live request.
#[test]
fn fm_and_abi_never_report_a_cursor_model_id() {
    let s = Scratch::new("no-cursor-id");

    // A cursor alias under `fm` collapses onto fm's own vocabulary.
    let (code, out, err) = run(&s, "fm", Some("opus"), &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        effective_line(&out),
        "effective model: system  (live: claude-opus-5-thinking-high, backend: fm)"
    );

    // Under `abi` the same alias stays the local persona template.
    let (code, out, err) = run(&s, "abi", Some("fable"), &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        effective_line(&out),
        "effective model: local  (live: local, backend: abi)"
    );

    // A bare Anthropic catalog id is the one literal `abi` selection: it stays
    // literal, because it *is* the opt-in live transport.
    let (code, out, err) = run(&s, "abi", Some("claude-sonnet-4"), &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        effective_line(&out),
        "effective model: claude-sonnet-4  (live: claude-sonnet-4, backend: abi)"
    );
}

/// The local backend reports a real local tag, never an alias or a cursor id.
#[test]
fn ollama_reports_the_normalized_local_tag() {
    let s = Scratch::new("ollama");

    let (code, out, err) = run(&s, "ollama", Some("gemma"), &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        effective_line(&out),
        "effective model: gemma4:26b-mlx  (live: gemma4:26b-mlx, backend: ollama)"
    );

    // An explicit local tag passes through untouched.
    let (code, out, err) = run(&s, "ollama", Some("gemma4:12b-mlx"), &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        effective_line(&out),
        "effective model: gemma4:12b-mlx  (live: gemma4:12b-mlx, backend: ollama)"
    );
}

/// The summary is a *summary*: it must not persist a model selection, change
/// routing, or otherwise write the model preference the user did not choose.
#[test]
fn the_summary_is_read_only() {
    let s = Scratch::new("read-only");
    let (code, out, err) = run(&s, "ollama", Some("gemma"), &["doctor"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("effective model:"));

    // The model file is the writable selection surface; a doctor run must
    // leave it alone.
    assert!(
        !s.0.join("model").exists(),
        "doctor must not persist a model selection"
    );

    // And the route log — the audit of what actually ran — stays empty.
    assert!(
        !s.0.join("route.jsonl").exists(),
        "a doctor run must not append a route row"
    );
}

/// The doctor surface is the same one the TUI's `/doctor` uses, so the line
/// exists in the CLI output the TUI prints too.
#[test]
fn the_effective_model_line_is_part_of_the_doctor_surface() {
    let s = Scratch::new("surface");
    let (code, out, err) = run(&s, "cursor", None, &["doctor"]);
    assert_eq!(code, 0, "{err}");
    // The live model line stays: the effective line explains it, it does not
    // replace it.
    assert!(out.contains("model:     auto"), "{out}");
    assert!(out.contains("effective model:"), "{out}");
}
