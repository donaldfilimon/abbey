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

/// An ambient variable a child must never inherit.
///
/// `ABBEY_INTERNAL_TUI_LOCAL_RECIPE` carries the TUI's complete live-context
/// envelope to the TUI's own children. Inherited by an unrelated child it has
/// two possible effects, and neither is the one the child asked for: a command
/// the recipe cannot admit (`ask`, `print`) is *refused* outright
/// ("local TUI recipe cannot admit this command"), and a finite slash child
/// whose cwd and edition match has its state root — and therefore the
/// directory its routing audit is written to — replaced with the TUI's. A test
/// that pins the audit would then be reading a different session's evidence.
const AMBIENT_RECIPE_ENV: &str = "ABBEY_INTERNAL_TUI_LOCAL_RECIPE";

fn fixture_command(scratch: &Scratch, binary: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(binary);
    command.env_clear();
    // Compiler/native loader paths are required for built fixtures; credentials
    // and application overrides must not leak from the invoking shell.
    for key in [
        "PATH",
        "LANG",
        "LC_ALL",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "SYSTEMROOT",
        "WINDIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("HOME", &scratch.0)
        .env("USERPROFILE", &scratch.0)
        .env("APPDATA", &scratch.0)
        .env("LOCALAPPDATA", &scratch.0)
        .env("XDG_CONFIG_HOME", &scratch.0)
        .env("XDG_STATE_HOME", &scratch.0)
        .env("ABBEY_STATE_DIR", &scratch.0)
        .env("ABBEY_PERSONAL_STATE_DIR", &scratch.0)
        .env("ABBEY_CONFIG", scratch.0.join("config.toml"))
        .env("ABBEY_PERSONAL_CONFIG", scratch.0.join("config.toml"));
    command
}

#[cfg(unix)]
#[test]
fn fixture_environment_contains_both_editions_even_for_a_stale_binary() {
    let scratch = Scratch::new("both-editions-contained");
    let output = fixture_command(&scratch, "/bin/sh")
        .args(["-c", "printf '%s\\n' \"$HOME\" \"$ABBEY_STATE_DIR\" \"$ABBEY_PERSONAL_STATE_DIR\" \"$ABBEY_CONFIG\" \"$ABBEY_PERSONAL_CONFIG\""])
        .output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let paths: Vec<_> = text.lines().collect();
    assert_eq!(paths.len(), 5);
    assert!(
        paths
            .iter()
            .all(|value| std::path::Path::new(value).starts_with(&scratch.0)),
        "HOME and both editions' state/config must be owned even when the executable's edition differs"
    );
}

/// Run abbey with an isolated state dir; returns (exit code, stdout, stderr).
fn run(scratch: &Scratch, args: &[&str]) -> (i32, String, String) {
    let out = fixture_command(scratch, BIN)
        .args(args)
        .env(abbey::edition::ACTIVE.state_dir_env(), &scratch.0)
        .env_remove(AMBIENT_RECIPE_ENV)
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

    // Which branch applies is decided by what the binary *did*, not by this
    // test crate's own cfg — the two are compiled together today, but the
    // guarantee under test belongs to the process. The cfg is then used only as
    // a cross-check, so a build that silently lost the feature cannot pass by
    // quietly taking the refusal branch.
    assert_eq!(
        code == 2,
        !cfg!(feature = "accel"),
        "exit {code} disagrees with the feature this build was compiled with"
    );

    if code != 2 {
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

/// The capture-bypass inventory (`print`, `commit`, `voice ask`) deliberately
/// skips `hybrid_run` — no persona wrap and, load-bearing for the routing
/// audit, **no `route.jsonl` entry**. AGENTS.md (*Execution path*) documents this as verified by
/// hand; this encodes it, so a new bypass that starts routing (or a routed
/// verb that stops logging) fails the gate instead of drifting silently.
#[cfg(unix)]
fn write_stub_agent(s: &Scratch) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    // A stub agent stands in for cursor-agent: what these tests pin is Abbey's
    // own bookkeeping and output, never the backend's.
    let agent = s.0.join("stub-agent");
    std::fs::write(&agent, "#!/bin/sh\necho stub-reply\n").expect("write stub agent");
    let mut permissions = std::fs::metadata(&agent).expect("stat stub").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&agent, permissions).expect("chmod stub");
    agent
}

/// Like [`run`], but with `ABBEY_AGENT` pointing at the scratch stub so agent
/// verbs execute deterministically without a real backend on the host.
#[cfg(unix)]
fn run_stubbed(s: &Scratch, agent: &PathBuf, args: &[&str]) -> (i32, String, String) {
    let out = fixture_command(s, BIN)
        .args(args)
        .env(abbey::edition::ACTIVE.state_dir_env(), &s.0)
        .env("ABBEY_AGENT", agent)
        .env("ABBEY_BACKEND", "cursor")
        .env_remove("CURSOR_AGENT_CHAT_ID")
        .env_remove(AMBIENT_RECIPE_ENV)
        .output()
        .expect("spawn abbey");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[cfg(unix)]
#[test]
fn explicit_backend_cannot_receive_the_legacy_agent_executable() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = Scratch::new("backend-agent-collision");
    let legacy = scratch.0.join("legacy-agent");
    let legacy_marker = scratch.0.join("legacy-ran");
    std::fs::write(
        &legacy,
        format!(
            "#!/bin/sh\ntouch '{}'\necho wrong-backend\n",
            legacy_marker.display()
        ),
    )
    .unwrap();
    let abi = scratch.0.join("abi");
    let abi_args = scratch.0.join("abi-args");
    std::fs::write(
        &abi,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho abi-reply\n",
            abi_args.display()
        ),
    )
    .unwrap();
    for path in [&legacy, &abi] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let output = fixture_command(&scratch, BIN)
        .args(["print", "hello"])
        .env(abbey::edition::ACTIVE.state_dir_env(), &scratch.0)
        .env("ABBEY_BACKEND", "abi")
        .env("ABBEY_AGENT", &legacy)
        .env("ABBEY_ABI_BIN", &abi)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(!legacy_marker.exists(), "lower-precedence ABBEY_AGENT ran");
    let args = std::fs::read_to_string(abi_args).unwrap();
    assert!(
        args.lines().next().is_some_and(|arg| arg == "complete"),
        "{args}"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("abi-reply"));
}

/// What the scratch state dir holds, for a failure message.
///
/// A missing audit record is only diagnosable with the directory it should
/// have landed in: the record may be absent, written somewhere else, or
/// replaced by something the child created.
fn state_listing(s: &Scratch) -> String {
    let mut names: Vec<String> = std::fs::read_dir(&s.0)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    format!("{names:?}")
}

#[cfg(unix)]
#[test]
fn an_unwritable_route_log_fails_the_turn_instead_of_losing_the_record() {
    // The routing audit is evidence. A record that cannot be written must not
    // disappear silently: the audit would then under-report what actually
    // ran, and the turn would report success for a session whose audit trail
    // has a hole in it.
    let s = Scratch::new("route-log-unwritable");
    let agent = write_stub_agent(&s);
    // A directory where the route log belongs cannot be opened for append.
    std::fs::create_dir_all(s.0.join("route.jsonl")).expect("block the route log");

    let (code, out, err) = run_stubbed(&s, &agent, &["ask", "hello"]);
    assert_ne!(
        code,
        0,
        "a lost audit record must not report success: {err}; state dir: {}",
        state_listing(&s)
    );
    assert!(
        err.contains("routing audit record not written"),
        "the failure must name the audit record: {err}"
    );
    // The turn failed *before* the executor ran: the stub would have printed
    // its reply. A non-zero exit alone would also be satisfied by a turn that
    // ran the agent and then failed, and the audit record is written before
    // the run precisely so that a failed write can stop the turn.
    assert!(
        !out.contains("stub-reply"),
        "the turn must fail before the executor runs: {out}"
    );
}

#[cfg(unix)]
#[test]
fn print_bypasses_the_route_log_where_ask_appends() {
    let s = Scratch::new("route-bypass");
    let agent = write_stub_agent(&s);
    let route_rows = || {
        std::fs::read_to_string(s.0.join("route.jsonl"))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    };

    assert_eq!(route_rows(), 0, "scratch state must start unrouted");

    // `ask` goes through the canonical path: the routing audit sees it.
    let (code, _, err) = run_stubbed(&s, &agent, &["ask", "hello"]);
    assert_eq!(code, 0, "stubbed ask should succeed: {err}");
    let after_ask = route_rows();
    assert!(
        after_ask >= 1,
        "ask must append a route record: {err}; state dir: {}",
        state_listing(&s)
    );

    // `print` is a capture bypass: same prompt, no new route record.
    let (code, out, err) = run_stubbed(&s, &agent, &["print", "hello"]);
    assert_eq!(code, 0, "stubbed print should succeed: {err}");
    assert!(
        out.contains("stub-reply"),
        "print should emit capture: {out}"
    );
    assert_eq!(
        route_rows(),
        after_ask,
        "print must leave the route log unchanged"
    );
}

/// `abbey doctor` is the honesty instrument: it must report the state it is
/// actually running against, and exit 0. A doctor that silently misreports
/// would undercut the claims discipline everywhere else in the repo.
#[cfg(unix)]
#[test]
fn doctor_reports_the_real_state_it_runs_against() {
    let s = Scratch::new("doctor");
    let agent = write_stub_agent(&s);
    let (code, out, err) = run_stubbed(&s, &agent, &["doctor"]);
    assert_eq!(code, 0, "doctor should exit 0: {err}");

    for field in ["agent:", "model:", "chat:", "state:", "backend:", "parity:"] {
        assert!(
            out.contains(field),
            "doctor output missing `{field}`: {out}"
        );
    }
    // The paths reported must be the scratch state this process was given —
    // not the developer's real state dir.
    let scratch = s.0.to_string_lossy();
    assert!(
        out.contains(scratch.as_ref()),
        "doctor must report the state dir it was pointed at: {out}"
    );
    // The stub agent proves which executable would run.
    assert!(
        out.contains("stub-agent"),
        "doctor must name the resolved agent path: {out}"
    );
}

/// Every catalog entry that is safe to invoke headlessly must be wired in
/// `dispatch_slash`. The dispatcher has an explicit "registered … but not
/// wired yet" arm, which means catalog↔dispatch drift is a real failure mode;
/// this pins the read-only/local subset (agent-run and repo-mutating verbs are
/// deliberately not executed here).
#[cfg(unix)]
#[test]
fn registered_local_slash_commands_are_wired() {
    let s = Scratch::new("slash-wired");
    let agent = write_stub_agent(&s);
    for cmd in [
        "/help",
        "/doctor",
        "/debug",
        "/claims",
        "/platform",
        "/runtime",
        "/vision",
        "/oos",
        "/model",
        "/status",
        "/routes",
        "/skills",
        "/plugins",
        "/allowlist",
        "/cost",
        "/permissions",
        "/config",
        "/mcp",
        "/acp",
        "/memory",
        "/compact",
        "/daemon",
    ] {
        let (_, _, err) = run_stubbed(&s, &agent, &[cmd]);
        assert!(
            !err.contains("unknown slash"),
            "{cmd} fell through to the unknown-command arm: {err}"
        );
        assert!(
            !err.contains("not wired yet"),
            "{cmd} is in the catalog but not dispatched: {err}"
        );
    }
}

#[test]
fn closing_a_pipe_early_is_not_an_error() {
    // Rust sets SIGPIPE to SIG_IGN before main, which turns a closed reader into
    // an EPIPE that `println!` panics on. `restore_sigpipe()` undoes that, so
    // `abbey … | head` must exit cleanly like `cat` or `ls` would. Only shows up
    // once output exceeds the pipe buffer, so this uses a long listing.
    let s = Scratch::new("sigpipe");
    let mut producer = fixture_command(&s, BIN)
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

/// Run abbey in a fully isolated environment: cleared env, a scratch HOME (so
/// no real install candidates resolve), and a caller-chosen PATH. This is how
/// "a machine with no executor installed" is simulated deterministically even
/// on developer hosts that have cursor-agent.
#[cfg(unix)]
fn run_isolated(s: &Scratch, path: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = fixture_command(s, BIN)
        .args(args)
        .env("PATH", path)
        // Absolute candidate probing (/opt/homebrew, /usr/bin) would find a
        // real grok/fm on the host and defeat the "no executor" staging.
        .env("ABBEY_TEST_HOME_AGENTS_ONLY", "1")
        .output()
        .expect("spawn abbey");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// cursor-agent (or any executor) is preferred, never required: every local
/// verb must work on a machine where no backend binary exists at all.
#[cfg(unix)]
#[test]
fn local_verbs_need_no_executor_backend() {
    let s = Scratch::new("no-backend");
    let empty = s.0.join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();

    for (args, want) in [
        (&["claims"][..], 0),
        (&["claims", "refuse", "lora"][..], 2),
        (&["allowlist"][..], 0),
        (&["model"][..], 0),
        (&["doctor"][..], 0),
        (&["edition", "--name"][..], 0),
    ] {
        let (code, _, err) = run_isolated(&s, &empty, args);
        assert_eq!(code, want, "{args:?} must not need an executor: {err}");
        assert!(
            !err.contains("not found"),
            "{args:?} must not complain about a missing executor: {err}"
        );
    }

    // Doctor stays honest about the absence instead of erroring or lying.
    let (_, out, _) = run_isolated(&s, &empty, &["doctor"]);
    assert!(
        out.contains("none found"),
        "doctor should report the missing executor plainly: {out}"
    );
}

/// Generation is the one thing that really needs an executor — and the
/// failure must arrive at spawn time with the full remedy list, not as an
/// eager startup error that also breaks local verbs.
#[cfg(unix)]
#[test]
fn generation_without_any_backend_fails_with_guidance() {
    let s = Scratch::new("no-backend-gen");
    let empty = s.0.join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();

    let (code, _, err) = run_isolated(&s, &empty, &["print", "hi"]);
    assert_ne!(code, 0, "generation cannot succeed with no executor");
    assert!(
        err.contains("not found") && err.contains("ABBEY_BACKEND"),
        "the error must name the remedies: {err}"
    );
}

/// With cursor-agent absent but another executor installed, the unchosen
/// default falls back to it — the machine works out of the box, and doctor
/// says the choice was automatic.
#[cfg(unix)]
#[test]
fn default_backend_falls_back_to_an_installed_executor() {
    use std::os::unix::fs::PermissionsExt as _;

    let s = Scratch::new("fallback");
    let bin = s.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let abi = bin.join("abi");
    std::fs::write(&abi, "#!/bin/sh\necho fallback-serves\n").unwrap();
    let mut permissions = std::fs::metadata(&abi).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&abi, permissions).unwrap();

    let (code, out, err) = run_isolated(&s, &bin, &["print", "hi"]);
    assert_eq!(
        code, 0,
        "the installed abi stub should serve the run: {err}"
    );
    assert!(
        out.contains("fallback-serves"),
        "output should come from the fallback executor: {out}"
    );

    let (code, out, _) = run_isolated(&s, &bin, &["doctor"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("abi (from auto"),
        "doctor must show the automatic backend choice: {out}"
    );
}

#[test]
fn plain_help_retains_cli_text_without_stream_json() {
    let s = Scratch::new("plain-help-contract");
    let (code, out, err) = run(&s, &["/help"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("/help") && out.contains("/review"), "{out}");
    assert!(
        !out.contains("text_delta")
            && !out.contains("stream-json")
            && !out.contains("\u{1b}[?1049"),
        "CLI help adopted TUI framing: {out}"
    );
}

/// Compatibility UX acceptance matrix (run as part of gate).
/// Exercises the documented Grok/Codex/Claude surface parity aliases and
/// commands listed in README and cli surface (without claiming runtime parity
/// or live executor behavior). Local surfaces only; generation paths covered
/// by sibling tests.
#[test]
fn compatibility_ux_acceptance_matrix() {
    let s = Scratch::new("ux-matrix");

    // Grok Build documented surfaces (from README table + cli aliases):
    // -c/--continue, -w/--worktree, --cwd, model aliases, session resume
    // (continue subcommand + worktree flag are parseable; doctor is shared)
    // Use --help form so we do not attempt generation; surface parse is the matrix point.
    let (code, _, err) = run(&s, &["--continue", "--help"]);
    assert_eq!(
        code, 0,
        "grok --continue flag must be accepted by surface: {err}"
    );

    let (code, _, err) = run(&s, &["--worktree", "tmp", "--help"]);
    assert_eq!(code, 0, "grok --worktree flag must parse: {err}");

    // Codex documented: exec/print, review, doctor (which/info), login/logout (status),
    // mcp/plugin, completion, sandbox/force
    let (code, out, _) = run(&s, &["doctor"]);
    assert_eq!(code, 0, "codex doctor alias must succeed: {out}");
    let (code, _, _) = run(&s, &["which"]);
    assert_eq!(code, 0, "codex which alias");
    let (code, _, _) = run(&s, &["info"]);
    assert_eq!(code, 0, "codex info alias");

    let (code, _, _) = run(&s, &["-p", "--help"]);
    assert_eq!(code, 0, "codex -p / print surface");

    let (code, _, _) = run(&s, &["review", "--help"]);
    assert_eq!(code, 0, "codex review");

    let (code, _, _) = run(&s, &["completion", "--help"]);
    // completion subcmd exists for grok/codex parity
    assert_eq!(code, 0, "grok/codex completion surface");

    // Claude Code documented: slash (/plan /diff /pr /init /commit /clear …),
    // -p, ask/plan modes
    let (code, _, _) = run(&s, &["plan", "--help"]);
    assert_eq!(code, 0, "claude plan mode");

    let (code, _, _) = run(&s, &["ask", "--help"]);
    assert_eq!(code, 0, "claude ask");

    // -p is print alias
    let (code, _, _) = run(&s, &["-p", "--help"]);
    assert_eq!(code, 0, "claude -p (print)");

    // clear (claude /clear alias)
    let (code, _, _) = run(&s, &["clear", "--help"]);
    assert_eq!(code, 0, "claude clear");

    // diff, pr, init, commit, security-review
    for sub in ["diff", "pr", "init", "commit", "security-review"] {
        let (code, _, _) = run(&s, &[sub, "--help"]);
        assert_eq!(code, 0, "claude {sub} surface",);
    }

    // cross surface aliases resolve in slash too (tested more in unit)
    // ensure help mentions parity surfaces without overclaiming
    let (code, out, _) = run(&s, &["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("Grok Build") || out.contains("Codex") || out.contains("Claude Code"));
    assert!(out.contains("does not reimplement vendor runtimes"));
}
