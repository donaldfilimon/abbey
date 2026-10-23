//! Regressions for the completion-program review's training boundaries.
use super::fixture::{Scratch, consent_file, state_in, store_corpus, write_trainer};
use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

fn script(scratch: &Scratch, body: &str) -> PathBuf {
    let path = scratch.dir.join("process-fixture");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn review_descendant_pipe_retention_cannot_escape_the_deadline() {
    let scratch = Scratch::new("review-pipe");
    let binary = script(&scratch, "sleep 2 &\nexit 0");
    let started = Instant::now();
    let result = process::run_bounded(&binary, &[], Duration::from_millis(100), 100, 100);
    assert_eq!(result, Err(process::RunError::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn review_timeout_removes_an_artifact_written_before_the_hang() {
    let scratch = Scratch::new("review-partial");
    let binary = script(
        &scratch,
        r#"while [ "$1" != "--out" ]; do shift; done
shift
printf partial > "$1"
exec sleep 2"#,
    );
    let artifact = scratch.dir.join("artifact.json");
    let result = trainer::run_trainer(
        &binary,
        &scratch.dir.join("dataset"),
        &artifact,
        Duration::from_millis(100),
    );
    assert!(result.is_err());
    assert!(!artifact.exists());
}

#[test]
fn review_pem_redaction_survives_a_split_utf8_window_boundary() {
    let key = "-----BEGIN PRIVATE KEY-----\nsecret-material\n-----END PRIVATE KEY-----";
    let input = format!("{key}{}é trailing", " ".repeat(511 - key.len()));
    let result = redaction::redact(&input);
    assert!(!result.text.contains("secret-material"));
    assert!(result.text.ends_with("é trailing"));
}

#[test]
fn review_failed_baseline_scoring_preserves_current_and_records_refusal() {
    let scratch = Scratch::new("review-baseline");
    let state = state_in(&scratch.dir);
    let consent = consent_file(&scratch);
    let args = vec![
        "run".into(),
        "--consent".into(),
        consent.display().to_string(),
    ];
    let binary = write_trainer(&scratch, "ok");
    store_corpus(&scratch.dir, 5, 2);
    dispatch_with(&state, &args, Some(&binary)).unwrap();
    let current = fs::read(ledger::current_path(&scratch.dir)).unwrap();
    let installed = fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap();
    let first = ledger::read_current(&scratch.dir).unwrap().unwrap();
    fs::remove_file(&first.artifact_path).unwrap();
    dispatch_with(&state, &args, Some(&binary)).unwrap();
    assert_eq!(
        fs::read(ledger::current_path(&scratch.dir)).unwrap(),
        current
    );
    assert_eq!(
        fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap(),
        installed
    );
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[1].decision.as_deref(), Some("promoted"));
    assert!(rows[1].detail.contains("baseline unavailable"));
}

#[test]
fn review_sampler_failure_preserves_current_and_records_refusal() {
    let scratch = Scratch::new("review-baseline");
    let state = state_in(&scratch.dir);
    let consent = consent_file(&scratch);
    let args = vec![
        "run".into(),
        "--consent".into(),
        consent.display().to_string(),
    ];
    let binary = write_trainer(&scratch, "ok");
    store_corpus(&scratch.dir, 5, 2);
    dispatch_with(&state, &args, Some(&binary)).unwrap();
    let current = fs::read(ledger::current_path(&scratch.dir)).unwrap();
    let installed = fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap();
    let first = ledger::read_current(&scratch.dir).unwrap().unwrap();
    let wrapper = script(
        &scratch,
        &format!(
            r#"for arg in "$@"; do
if [ "$arg" = "{}" ]; then exit 3; fi
done
exec "{}" "$@""#,
            first.artifact_path,
            binary.display()
        ),
    );
    dispatch_with(&state, &args, Some(&wrapper)).unwrap();
    assert_eq!(
        fs::read(ledger::current_path(&scratch.dir)).unwrap(),
        current
    );
    assert_eq!(
        fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap(),
        installed
    );
    let rows = ledger::read(&scratch.dir).unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[1].decision.as_deref(), Some("promoted"));
    assert!(rows[1].detail.contains("baseline unavailable"));
}

#[test]
fn review_rollback_requires_valid_consent_before_any_state_change() {
    let scratch = Scratch::new("review-consent");
    let state = state_in(&scratch.dir);
    let consent = consent_file(&scratch);
    let binary = write_trainer(&scratch, "ok");
    store_corpus(&scratch.dir, 5, 2);
    let args = vec![
        "run".into(),
        "--consent".into(),
        consent.display().to_string(),
    ];
    dispatch_with(&state, &args, Some(&binary)).unwrap();
    dispatch_with(&state, &args, Some(&binary)).unwrap();
    let current = fs::read(ledger::current_path(&scratch.dir)).unwrap();
    let installed = fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap();
    let wrong = scratch.dir.join("wrong-consent");
    fs::write(&wrong, "no").unwrap();
    for args in [
        vec!["rollback".into()],
        vec![
            "rollback".into(),
            "--consent".into(),
            wrong.display().to_string(),
        ],
    ] {
        let outcome = dispatch_with(&state, &args, Some(&binary)).unwrap();
        assert_eq!(outcome.code, 2);
        assert_eq!(
            fs::read(ledger::current_path(&scratch.dir)).unwrap(),
            current
        );
        assert_eq!(
            fs::read(ledger::current_artifact_path(&scratch.dir)).unwrap(),
            installed
        );
        assert_eq!(ledger::read(&scratch.dir).unwrap().len(), 2);
    }
    let outcome = dispatch_with(
        &state,
        &[
            "rollback".into(),
            "--consent".into(),
            consent.display().to_string(),
        ],
        Some(&binary),
    )
    .unwrap();
    assert_eq!(outcome.code, 0);
    assert!(
        ledger::read(&scratch.dir).unwrap()[2]
            .consent_digest
            .is_some()
    );
}

#[test]
fn review_group_leader_is_not_reaped_while_descendants_hold_pipes() {
    let scratch = Scratch::new("review-owned-group");
    let pid_file = scratch.dir.join("leader.pid");
    let binary = script(&scratch, "echo $$ > \"$1\"\nsleep 2 &\nexit 0");
    let arg = pid_file.clone().into_os_string();
    let worker = std::thread::spawn(move || {
        process::run_bounded(&binary, &[arg], Duration::from_millis(700), 100, 100)
    });
    let deadline = Instant::now() + Duration::from_millis(400);
    // Creation is not readiness: the shell may have opened the file but not
    // written its PID yet. Observe the complete value before testing ownership.
    let pid = loop {
        if let Some(pid) = fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 1)
        {
            break pid;
        }
        assert!(Instant::now() < deadline, "fixture did not publish its PID");
        std::thread::sleep(Duration::from_millis(5));
    };
    std::thread::sleep(Duration::from_millis(100));
    let owned = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok();
    let result = worker.join().unwrap();
    assert!(owned, "the leader PID must remain reserved until teardown");
    assert_eq!(result, Err(process::RunError::TimedOut));
}
