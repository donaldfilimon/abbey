//! End-to-end checks on the built binary for the guarantees Abbey advertises.
//!
//! These cover behaviour the unit tests cannot see, because it only exists once
//! the process runs: real exit codes, real stdout/stderr, and the SIGPIPE reset
//! that happens before `main`. Each guarantee here was previously confirmed by
//! hand; encoding it means a regression fails the gate instead of surviving
//! until someone re-checks manually.
//!
//! Every test runs against a throwaway state dir (this edition's own state
//! variable, `edition::ACTIVE.state_dir_env()`) so nothing touches the
//! developer's real chat id, memory store, or route log. Nothing here spawns
//! cursor-agent — only local surfaces are exercised.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The binary under test, provided by cargo for integration tests.
const BIN: &str = env!("CARGO_BIN_EXE_abbey");

/// A per-test state directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-cli-{tag}-{}-{:?}",
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

/// Run abbey with an isolated state dir; returns (exit code, stdout, stderr).
fn run(scratch: &Scratch, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .env(abbey::edition::ACTIVE.state_dir_env(), &scratch.0)
        .output()
        .expect("spawn abbey");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn out_of_scope_verbs_refuse_with_exit_2() {
    // The claims gate's core promise: a Proposed/OOS verb fails honestly rather
    // than pretending to succeed. Exit 2 distinguishes "refused on principle"
    // from an ordinary error (1).
    let s = Scratch::new("refuse");
    for topic in ["lora", "multinode"] {
        let (code, _, _) = run(&s, &["claims", "refuse", topic]);
        assert_eq!(code, 2, "claims refuse {topic} should exit 2");
    }
    assert_eq!(
        run(&s, &["claims", "refuse", "embeddings"]).0,
        0,
        "the legacy refuse topic must report that semantic embeddings are now Current"
    );
    assert_eq!(run(&s, &["runtime", "refuse"]).0, 2);
    assert_eq!(run(&s, &["vision", "refuse"]).0, 2);
    // The informational panels are not refusals.
    assert_eq!(run(&s, &["runtime"]).0, 0);
    assert_eq!(run(&s, &["claims"]).0, 0);
}

/// `abbey accel verify` at the process level, in whichever build produced it.
///
/// The two builds are different guarantees, so both are asserted here rather
/// than only whichever one CI happened to compile:
///
/// * without `--features accel` the verb must refuse with exit 2 and say so on
///   stderr — never exit 0 with an empty success;
/// * with the feature it must run the kernels and report the backend that
///   actually served them. Exit 0 is asserted only when the report says Metal
///   both ran and matched, because a host with no Metal device is an honest
///   `NOT VERIFIED` (exit 1), not a test failure.
#[test]
fn accel_verify_is_honest_about_what_this_build_can_do() {
    let s = Scratch::new("accel");
    let (code, out, err) = run(&s, &["accel", "verify"]);

    if cfg!(feature = "accel") {
        assert!(
            code == 0 || code == 1,
            "compiled bridge should report, not error: code={code} err={err}"
        );
        assert!(
            out.contains("backend used:"),
            "the report must name the backend that ran: {out}"
        );
        // The honesty boundary ships with the output, not just the docs.
        for boundary in [
            "gpu_npu_tpu_compilation",
            "training_or_model_inference",
            "device_residency_or_placement",
            "speedup_or_performance",
        ] {
            assert!(out.contains(boundary), "missing boundary {boundary}: {out}");
        }
        // Exit code and verdict may never disagree.
        if out.contains("backend used:   gpu-metal") && out.contains("VERIFIED — every kernel") {
            assert_eq!(code, 0, "a verified Metal run must exit 0: {out}");
        } else {
            assert_eq!(code, 1, "anything short of verified must exit 1: {out}");
        }
        // A CPU-served run must never describe itself as a Metal success.
        if out.contains("backend used:   cpu") {
            assert!(out.contains("NOT VERIFIED"), "{out}");
        }
    } else {
        assert_eq!(code, 2, "the unbuilt bridge must refuse with exit 2: {err}");
        assert!(
            err.contains("--features accel"),
            "the refusal must name the missing feature: {err}"
        );
        assert!(out.is_empty(), "a refusal must not print a report: {out}");
    }

    // Detection stays available and distinct in both builds — it is a different
    // (report-only) capability, and it is not a refusal.
    assert_eq!(run(&s, &["accel", "detect"]).0, 0);
    // The Proposed row is untouched: compile/train/infer still refuses.
    assert_eq!(run(&s, &["accel", "refuse"]).0, 2);
    assert_eq!(run(&s, &["claims", "refuse", "gpu"]).0, 2);
}

#[test]
fn os_control_requires_confirm_and_honours_the_allowlist() {
    let s = Scratch::new("os");

    // Execute without the safety gate is refused even for an allowed command.
    let (code, _, err) = run(&s, &["os", "execute", "whoami"]);
    assert_ne!(code, 0, "execute without --confirm must fail");
    assert!(
        err.contains("--confirm"),
        "stderr should name the gate: {err}"
    );

    // Allowlisted with the gate: runs.
    let (code, out, _) = run(&s, &["os", "execute", "--confirm", "whoami"]);
    assert_eq!(code, 0);
    assert!(!out.trim().is_empty(), "whoami should print a username");

    // Off-list commands are refused even with the gate.
    for denied in ["curl", "rm", "bash"] {
        let (code, _, err) = run(&s, &["os", "execute", "--confirm", denied]);
        assert_ne!(code, 0, "{denied} must not run");
        assert!(
            err.contains("allowlist"),
            "stderr should cite the allowlist"
        );
    }

    // Case variants are refused on Unix: `WHOAMI` opens the same inode on a
    // case-insensitive filesystem but dispatches on argv[0] and behaves as `id`,
    // so the program that ran would differ from the one the policy approved.
    #[cfg(unix)]
    {
        let (code, _, _) = run(&s, &["os", "execute", "--confirm", "WHOAMI"]);
        assert_ne!(code, 0, "case variant must not pass the allowlist");
    }
}

#[test]
fn missing_memory_ids_fail_rather_than_succeeding_quietly() {
    let s = Scratch::new("mem-missing");
    for args in [
        vec!["memory", "get", "no-such-id"],
        vec!["memory", "promote", "no-such-id", "ltm"],
        vec!["memory", "invalidate", "no-such-id"],
        vec!["memory", "supersede", "no-such-id", "replacement"],
    ] {
        let (code, _, _) = run(&s, &args);
        assert_eq!(code, 1, "{args:?} should exit 1, not report success");
    }
}

#[test]
fn invalidate_hides_from_search_but_keeps_the_record() {
    // The contract behind "mark obsolete, never delete".
    let s = Scratch::new("mem-invalidate");
    let (code, id, _) = run(
        &s,
        &[
            "memory",
            "put",
            "surface probe token",
            "--provenance",
            "test",
        ],
    );
    assert_eq!(code, 0);
    let id = id.trim().to_string();
    assert!(!id.is_empty(), "put should print the new id");

    let (_, found, _) = run(&s, &["memory", "search", "surface"]);
    assert!(found.contains(&id), "record should be searchable first");

    assert_eq!(run(&s, &["memory", "invalidate", &id]).0, 0);

    let (_, after, _) = run(&s, &["memory", "search", "surface"]);
    assert!(
        !after.contains(&id),
        "invalidated record must leave search results"
    );
    let (code, shown, _) = run(&s, &["memory", "get", &id]);
    assert_eq!(code, 0, "the record must still exist");
    assert!(shown.contains("\"obsolete\": true"), "got: {shown}");
}

#[test]
fn closing_a_pipe_early_is_not_an_error() {
    // Rust sets SIGPIPE to SIG_IGN before main, which turns a closed reader into
    // an EPIPE that `println!` panics on. `restore_sigpipe()` undoes that, so
    // `abbey … | head` must exit cleanly like `cat` or `ls` would. Only shows up
    // once output exceeds the pipe buffer, so this uses a long listing.
    let s = Scratch::new("sigpipe");
    let mut producer = Command::new(BIN)
        .args(["claims"])
        .env(abbey::edition::ACTIVE.state_dir_env(), &s.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn abbey");

    // Drop the read end immediately — the reader is gone before abbey finishes.
    drop(producer.stdout.take());
    let out = producer.wait_with_output().expect("wait");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.contains("panicked"),
        "closing the pipe should not panic: {err}"
    );
}
