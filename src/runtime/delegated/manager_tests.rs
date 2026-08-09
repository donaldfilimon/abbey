use super::*;
use crate::app_core::{IdempotencyKey, RunMode, RunState};
use crate::runtime::{RunManager, RunManagerConfig, RuntimeStore, SubmitDisposition, SystemClock};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "abbey-delegated-manager-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    #[cfg(unix)]
    fn script(&self, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = self.0.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(key: &str, input: &str) -> RunRequest {
    RunRequest {
        idempotency_key: key.parse::<IdempotencyKey>().unwrap(),
        conversation_id: None,
        mode: RunMode::Background,
        backend: BackendSelection::Abi,
        input: input.into(),
        labels: Vec::new(),
    }
}

#[cfg(unix)]
fn manager(
    scratch: &ScratchDir,
    program: &Path,
    limits: DelegatedLimits,
) -> (
    Arc<RuntimeStore>,
    RunManager<DelegatedExecutor, SystemClock>,
) {
    let store = Arc::new(RuntimeStore::open(&scratch.0.join("runtime.sqlite")).unwrap());
    let config = DelegatedExecutorConfig::new(&scratch.0)
        .unwrap()
        .bind_abi_local(program)
        .unwrap()
        .with_limits(limits)
        .unwrap();
    let executor = Arc::new(DelegatedExecutor::new(config));
    let manager = RunManager::start(
        Arc::clone(&store),
        executor,
        Arc::new(SystemClock),
        RunManagerConfig { queue_capacity: 2 },
    );
    (store, manager)
}

#[cfg(unix)]
fn terminal_event_code(store: &RuntimeStore, run_id: &RunId) -> Option<String> {
    let events = store.run_events(run_id).unwrap();
    events
        .last()
        .and_then(|event| event.payload.get("code"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

#[cfg(unix)]
#[test]
fn successful_delegated_process_becomes_durable_success() {
    let scratch = ScratchDir::new("success");
    let program = scratch.script("abi", "exit 0");
    let (store, manager) = manager(&scratch, &program, DelegatedLimits::default());
    let submitted = manager.submit(request("success", "input")).unwrap();
    let terminal = manager
        .wait_for_terminal(&submitted.run.id, Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(terminal.status, RunState::Succeeded);
    assert_eq!(terminal_event_code(&store, &terminal.id), None);
    manager.shutdown().unwrap();
}

#[cfg(unix)]
#[test]
fn timeout_and_output_limit_persist_stable_failure_codes() {
    for (label, body, expected) in [
        ("timeout", "exec /bin/sleep 30", "executor_timed_out"),
        (
            "output",
            "printf 12345678901234567\nexec /bin/sleep 30",
            "executor_output_limit",
        ),
    ] {
        let scratch = ScratchDir::new(label);
        let program = scratch.script("abi", body);
        let limits = DelegatedLimits {
            timeout: if label == "timeout" {
                Duration::from_millis(80)
            } else {
                Duration::from_secs(2)
            },
            terminate_grace: Duration::from_millis(500),
            stdout_bytes: 16,
            stderr_bytes: 16,
            poll_interval: Duration::from_millis(5),
        };
        let (store, manager) = manager(&scratch, &program, limits);
        let submitted = manager.submit(request(label, "private input")).unwrap();
        let terminal = manager
            .wait_for_terminal(&submitted.run.id, Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(terminal.status, RunState::Failed, "{label}");
        assert_eq!(
            terminal_event_code(&store, &terminal.id),
            Some(expected.to_owned())
        );
        manager.shutdown().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn duplicate_idempotent_submission_launches_one_process() {
    let scratch = ScratchDir::new("duplicate");
    let count = scratch.0.join("launch-count");
    let program = scratch.script("abi", &format!("printf x >> '{}'\nexit 0", count.display()));
    let (_store, manager) = manager(&scratch, &program, DelegatedLimits::default());
    let first = manager.submit(request("duplicate", "same input")).unwrap();
    let second = manager.submit(request("duplicate", "same input")).unwrap();
    assert_eq!(first.run.id, second.run.id);
    assert_eq!(second.disposition, SubmitDisposition::Existing);
    let terminal = manager
        .wait_for_terminal(&first.run.id, Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(terminal.status, RunState::Succeeded);
    manager.shutdown().unwrap();
    assert_eq!(fs::read(&count).unwrap(), b"x");
}
