use super::*;
use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// `terminate_grace` bounds a poll-until-the-group-is-gone loop, not a sleep:
/// `ChildGuard::terminate` rechecks `group_exists` every `poll_interval` and
/// returns the instant the group disappears, so a generous grace costs an idle
/// machine nothing and only decides how long teardown may wait before failing
/// closed.
///
/// It has to out-wait *reaping*, which is not ours to schedule. `group_exists`
/// is `killpg(pgid, 0)`, and on Darwin an unreaped zombie still answers it —
/// with success, or with the EPERM that `signal_group_until` deliberately
/// retries. A group member orphaned by its dying leader is reparented to
/// launchd and reaped asynchronously, so under parallel load the group can
/// stay observable for far longer than the processes are actually alive.
///
/// At 100ms this raced launchd and failed two ways in one 12-run baseline:
/// `Teardown("send SIGTERM to process group was not permitted")` (EPERM
/// retries exhausted) and `Teardown("process group survived SIGKILL grace")`.
/// Neither was the property under test. Keep this comfortably above the
/// reaper and below `MAX_TERMINATE_GRACE` (5s).
const TEARDOWN_GRACE: Duration = Duration::from_secs(1);

/// Upper bound for "the supervisor returned instead of waiting out a child's
/// own 30-second sleep". It must clear the fixture's worst-case teardown —
/// `terminate` spends up to `3 * TEARDOWN_GRACE` (SIGTERM, SIGKILL, gone) and
/// `collect_readers` one more — or the teardown race simply moves into this
/// assertion.
const LIVENESS_BOUND: Duration = Duration::from_secs(10);

fn limits(stdout_bytes: usize, stderr_bytes: usize) -> SupervisorLimits {
    SupervisorLimits {
        timeout: Duration::from_secs(2),
        terminate_grace: TEARDOWN_GRACE,
        stdout_bytes,
        stderr_bytes,
        poll_interval: Duration::from_millis(2),
    }
}

#[cfg(unix)]
fn shell(script: impl Into<OsString>) -> ProcessSpec {
    ProcessSpec::inherited(
        PathBuf::from("/bin/sh"),
        vec![OsString::from("-c"), script.into()],
    )
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "abbey-supervisor-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn debug_output_redacts_program_arguments_environment_and_captured_bytes() {
    let spec = ProcessSpec {
        program: PathBuf::from("/secret/program"),
        args: vec![OsString::from("secret-prompt")],
        current_dir: Some(PathBuf::from("/secret/workspace")),
        environment: ProcessEnvironment::ClearAndSet(vec![(
            OsString::from("TOKEN"),
            OsString::from("secret-token"),
        )]),
    };
    let rendered = format!("{spec:?}");
    assert!(!rendered.contains("secret"), "{rendered}");
    assert!(rendered.contains("argument_count"), "{rendered}");
    assert!(rendered.contains("entries: 1"), "{rendered}");

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        let outcome = SupervisorOutcome::Exited {
            status: ExitStatus::from_raw(0),
            stdout: b"secret-stdout".to_vec(),
            stderr: b"secret-stderr".to_vec(),
        };
        let rendered = format!("{outcome:?}");
        assert!(!rendered.contains("secret"), "{rendered}");
        assert!(rendered.contains("stdout_bytes: 13"), "{rendered}");
    }
}

#[cfg(unix)]
#[test]
fn exact_cap_succeeds_and_cap_plus_one_fails_closed_for_both_streams() {
    // Exercise the short-lived-child interleaving repeatedly: on Darwin the
    // leader may exit between the group observation and the teardown signal.
    for _ in 0..16 {
        for (script, expected) in [
            ("printf 1234", None),
            ("printf 12345", Some(StreamName::Stdout)),
            ("printf 12345 >&2", Some(StreamName::Stderr)),
        ] {
            let outcome =
                run(&shell(script), &limits(4, 4), &CancellationToken::default()).unwrap();
            match expected {
                None => assert!(matches!(outcome, SupervisorOutcome::Exited { .. })),
                Some(StreamName::Stdout) => {
                    assert!(matches!(outcome, SupervisorOutcome::StdoutLimit))
                }
                Some(StreamName::Stderr) => {
                    assert!(matches!(outcome, SupervisorOutcome::StderrLimit))
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn timeout_and_precancelled_execution_teardown_the_group() {
    // Only the timeout is shortened. It is what the test drives; the teardown
    // grace is not, and overriding it to 30ms made the fixture race launchd's
    // reaper rather than assert anything about timeout classification.
    let mut short = limits(64, 64);
    short.timeout = Duration::from_millis(30);
    let started = Instant::now();
    assert!(matches!(
        run(
            &shell("trap '' TERM; sleep 30"),
            &short,
            &CancellationToken::default()
        )
        .unwrap(),
        SupervisorOutcome::TimedOut
    ));
    assert!(started.elapsed() < LIVENESS_BOUND);

    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        run(&shell("sleep 30"), &short, &cancellation).unwrap(),
        SupervisorOutcome::Cancelled
    ));
}

#[cfg(unix)]
#[test]
fn child_guard_reaps_a_dead_leader_before_post_kill_group_liveness() {
    use nix::sys::signal::{Signal, killpg};
    use std::io::Read as _;
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};

    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "trap '' TERM; printf ready; exec sleep 30"])
        .stdout(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut ready = [0_u8; 5];
    stdout.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"ready");

    let mut guard = super::unix::ChildGuard::new(child).unwrap();
    let (process_group, reaped, disarmed) = guard.test_state();
    assert!(!reaped && !disarmed);
    killpg(process_group, Signal::SIGKILL).unwrap();
    let mut eof = Vec::new();
    stdout.read_to_end(&mut eof).unwrap();

    // EOF proves the leader is dead, but until `wait` reaps it the process
    // group probe still reports it as present. This is the deterministic false
    // liveness state that previously became a teardown error.
    assert!(super::unix::group_exists(process_group).unwrap());
    guard
        .terminate(Duration::from_secs(1), Duration::from_millis(2))
        .unwrap();
    let (_, reaped, disarmed) = guard.test_state();
    assert!(reaped && disarmed);
    assert!(!super::unix::group_exists(process_group).unwrap());
}

#[cfg(unix)]
#[test]
fn leader_exit_with_descendant_holding_pipes_is_bounded_and_kills_descendant() {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let scratch = ScratchDir::new("leader-exits-first");
    let pid_file = scratch.0.join("descendant.pid");
    let script = format!(
        "sleep 30 & descendant=$!; echo $descendant > '{}'; printf ok; exit 0",
        pid_file.display()
    );
    let started = Instant::now();
    let outcome = run(
        &shell(script),
        &limits(64, 64),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(started.elapsed() < LIVENESS_BOUND);
    let SupervisorOutcome::Exited { status, stdout, .. } = outcome else {
        panic!("leader should retain its exit outcome")
    };
    assert!(status.success());
    assert_eq!(stdout, b"ok");

    let descendant = std::fs::read_to_string(pid_file)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while kill(Pid::from_raw(descendant), None).is_ok() && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(
        kill(Pid::from_raw(descendant), None).is_err(),
        "descendant survived supervised teardown"
    );
}

#[cfg(unix)]
#[test]
fn environment_modes_and_current_directory_are_explicit() {
    let inherited = run(
        &shell("[ -n \"$PATH\" ] && printf inherited"),
        &limits(64, 64),
        &CancellationToken::default(),
    )
    .unwrap();
    let SupervisorOutcome::Exited { stdout, .. } = inherited else {
        panic!("inherited environment fixture failed")
    };
    assert_eq!(stdout, b"inherited");

    let scratch = ScratchDir::new("explicit-environment");
    let mut explicit = shell("printf '%s:%s' \"$ONLY\" \"$PWD\"");
    explicit.current_dir = Some(scratch.0.clone());
    explicit.environment =
        ProcessEnvironment::ClearAndSet(vec![(OsString::from("ONLY"), OsString::from("bounded"))]);
    let outcome = run(&explicit, &limits(512, 64), &CancellationToken::default()).unwrap();
    let SupervisorOutcome::Exited { stdout, .. } = outcome else {
        panic!("explicit environment fixture failed")
    };
    let stdout = String::from_utf8(stdout).unwrap();
    assert_eq!(
        stdout,
        format!(
            "bounded:{}",
            std::fs::canonicalize(&scratch.0).unwrap().display()
        )
    );
}

#[cfg(unix)]
#[test]
fn invalid_specs_and_limits_fail_before_spawn_without_secret_errors() {
    let mut invalid = shell("printf should-not-run");
    invalid.args.push(OsString::from("bad\0argument"));
    let error = run(&invalid, &limits(64, 64), &CancellationToken::default()).unwrap_err();
    assert!(matches!(error, SupervisorError::Invalid(_)));
    assert!(!format!("{error:?}").contains("should-not-run"));

    let mut invalid_limits = limits(64, 64);
    invalid_limits.stdout_bytes = 0;
    assert!(matches!(
        run(
            &shell("printf should-not-run"),
            &invalid_limits,
            &CancellationToken::default()
        ),
        Err(SupervisorError::Invalid(_))
    ));
}

#[cfg(unix)]
#[test]
fn argv_environment_path_and_limit_bounds_are_validated() {
    let valid = shell("printf ok");

    let mut too_many_args = valid.clone();
    too_many_args.args = vec![OsString::from("x"); MAX_ARGS + 1];
    assert!(matches!(
        too_many_args.validate(),
        Err(SupervisorError::Invalid(_))
    ));

    let mut oversized_arg = valid.clone();
    oversized_arg.args = vec![OsString::from("x".repeat(MAX_ARG_BYTES + 1))];
    assert!(matches!(
        oversized_arg.validate(),
        Err(SupervisorError::Invalid(_))
    ));

    let mut oversized_argv = valid.clone();
    oversized_argv.args = vec![
        OsString::from("x".repeat(MAX_ARG_BYTES)),
        OsString::from("y".repeat(MAX_ARG_BYTES)),
        OsString::from("z"),
    ];
    assert!(matches!(
        oversized_argv.validate(),
        Err(SupervisorError::Invalid(_))
    ));

    let mut too_many_environment_entries = valid.clone();
    too_many_environment_entries.environment = ProcessEnvironment::ClearAndSet(
        (0..=MAX_ENVIRONMENT)
            .map(|index| (OsString::from(format!("K{index}")), OsString::from("v")))
            .collect(),
    );
    assert!(matches!(
        too_many_environment_entries.validate(),
        Err(SupervisorError::Invalid(_))
    ));

    let mut invalid_environment = valid.clone();
    invalid_environment.environment = ProcessEnvironment::ClearAndSet(vec![(
        OsString::from("TOKEN"),
        OsString::from("secret\nvalue"),
    )]);
    assert!(matches!(
        invalid_environment.validate(),
        Err(SupervisorError::Invalid(_))
    ));

    let scratch = ScratchDir::new("path-bounds");
    let regular_file = scratch.0.join("file");
    std::fs::write(&regular_file, b"not a directory").unwrap();
    let mut invalid_program = valid.clone();
    invalid_program.program = scratch.0.clone();
    assert!(matches!(
        invalid_program.validate(),
        Err(SupervisorError::Invalid(_))
    ));
    let mut invalid_current_dir = valid;
    invalid_current_dir.current_dir = Some(regular_file);
    assert!(matches!(
        invalid_current_dir.validate(),
        Err(SupervisorError::Invalid(_))
    ));

    for invalid in [
        SupervisorLimits {
            timeout: Duration::ZERO,
            ..limits(64, 64)
        },
        SupervisorLimits {
            timeout: MAX_TIMEOUT + Duration::from_nanos(1),
            ..limits(64, 64)
        },
        SupervisorLimits {
            terminate_grace: MAX_TERMINATE_GRACE + Duration::from_nanos(1),
            ..limits(64, 64)
        },
        SupervisorLimits {
            poll_interval: Duration::ZERO,
            ..limits(64, 64)
        },
        SupervisorLimits {
            stdout_bytes: MAX_STREAM_BYTES + 1,
            ..limits(64, 64)
        },
    ] {
        assert!(matches!(
            invalid.validate(),
            Err(SupervisorError::Invalid(_))
        ));
    }
}

#[cfg(not(unix))]
#[test]
fn non_unix_fails_closed() {
    let spec = ProcessSpec::inherited(PathBuf::from("program"), Vec::new());
    assert!(matches!(
        run(&spec, &limits(64, 64), &CancellationToken::default()),
        Err(SupervisorError::Unsupported)
    ));
}
