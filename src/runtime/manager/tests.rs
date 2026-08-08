use super::*;
use crate::app_core::{BackendSelection, IdempotencyKey, RunMode};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};

#[derive(Default)]
struct FakeClock(AtomicU64);

impl Clock for FakeClock {
    fn now_millis(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Default)]
struct TestExecutor {
    calls: Mutex<HashMap<String, usize>>,
    entered: (Mutex<HashSet<String>>, Condvar),
    release_block: AtomicBool,
    observed_cancellation: AtomicBool,
}

impl TestExecutor {
    fn wait_until_entered(&self, input: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let (entered, changed) = &self.entered;
        let mut entered = lock(entered);
        while !entered.contains(input) {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("executor did not start before deadline");
            let waited = changed
                .wait_timeout(entered, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            entered = waited.0;
            assert!(!waited.1.timed_out(), "executor did not start");
        }
    }

    fn release(&self) {
        self.release_block.store(true, Ordering::Release);
    }

    fn calls_for(&self, input: &str) -> usize {
        *lock(&self.calls).get(input).unwrap_or(&0)
    }
}

impl Executor for TestExecutor {
    fn execute(
        &self,
        _run_id: &RunId,
        request: RunRequest,
        cancellation: &CancellationToken,
    ) -> Result<(), super::super::executor::ExecutionError> {
        *lock(&self.calls).entry(request.input.clone()).or_default() += 1;
        let (entered, changed) = &self.entered;
        lock(entered).insert(request.input.clone());
        changed.notify_all();

        match request.input.as_str() {
            "block" => {
                while !self.release_block.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                Ok(())
            }
            "cancel" => {
                while !cancellation.is_cancelled() {
                    thread::yield_now();
                }
                self.observed_cancellation.store(true, Ordering::Release);
                Ok(())
            }
            "fail" => Err(super::super::executor::ExecutionError::new(
                "deterministic fixture failure",
            )),
            "panic" => panic!("deterministic fixture panic"),
            _ => Ok(()),
        }
    }
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "abbey-manager-{label}-{}-{}",
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

fn manager(
    label: &str,
    capacity: usize,
) -> (
    ScratchDir,
    Arc<RuntimeStore>,
    Arc<TestExecutor>,
    RunManager<TestExecutor, FakeClock>,
) {
    let scratch = ScratchDir::new(label);
    let store = Arc::new(RuntimeStore::open(&scratch.0.join("runtime.sqlite")).unwrap());
    let executor = Arc::new(TestExecutor::default());
    let manager = RunManager::start(
        Arc::clone(&store),
        Arc::clone(&executor),
        Arc::new(FakeClock::default()),
        RunManagerConfig {
            queue_capacity: capacity,
        },
    );
    (scratch, store, executor, manager)
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

fn terminal<E: Executor, C: Clock>(manager: &RunManager<E, C>, id: &RunId) -> RunRecord {
    manager
        .wait_for_terminal(id, Duration::from_secs(2))
        .unwrap()
        .expect("run exists")
}

#[test]
fn queue_capacity_is_clamped_small() {
    assert_eq!(
        RunManagerConfig { queue_capacity: 0 }.effective_queue_capacity(),
        MIN_QUEUE_CAPACITY
    );
    assert_eq!(
        RunManagerConfig {
            queue_capacity: usize::MAX
        }
        .effective_queue_capacity(),
        MAX_QUEUE_CAPACITY
    );
}

#[test]
fn duplicate_idempotent_submit_executes_once() {
    let (_scratch, _store, executor, manager) = manager("duplicate", 2);
    let first = manager.submit(request("duplicate", "success")).unwrap();
    let second = manager.submit(request("duplicate", "success")).unwrap();
    assert_eq!(second.run.id, first.run.id);
    assert_eq!(second.disposition, SubmitDisposition::Existing);
    assert_eq!(
        terminal(&manager, &first.run.id).status,
        RunState::Succeeded
    );
    assert_eq!(executor.calls_for("success"), 1);
    manager.shutdown().unwrap();
}

#[test]
fn idempotency_digest_is_computed_from_the_validated_request() {
    let (_scratch, _store, _executor, manager) = manager("digest-binding", 2);
    manager.submit(request("same-key", "first")).unwrap();
    let error = manager
        .submit(request("same-key", "different"))
        .expect_err("different requests must not share one idempotency identity");
    assert!(matches!(
        error,
        ManagerError::Store(StoreError::IdempotencyConflict)
    ));
    manager.shutdown().unwrap();
}

#[test]
fn queued_cancellation_is_terminal_without_execution() {
    let (_scratch, _store, executor, manager) = manager("queued-cancel", 2);
    let blocker = manager.submit(request("blocker", "block")).unwrap();
    executor.wait_until_entered("block");
    let queued = manager.submit(request("queued", "never-execute")).unwrap();
    let cancelled = manager.cancel(&queued.run.id).unwrap();
    assert_eq!(cancelled.status, RunState::Cancelled);
    executor.release();
    assert_eq!(
        terminal(&manager, &blocker.run.id).status,
        RunState::Succeeded
    );
    assert_eq!(executor.calls_for("never-execute"), 0);
    manager.shutdown().unwrap();
}

#[test]
fn running_cancellation_reaches_executor_and_persists_cancelled() {
    let (_scratch, _store, executor, manager) = manager("running-cancel", 1);
    let run = manager.submit(request("running", "cancel")).unwrap();
    executor.wait_until_entered("cancel");
    let requested = manager.cancel(&run.run.id).unwrap();
    assert!(matches!(
        requested.status,
        RunState::CancelRequested | RunState::Cancelled
    ));
    assert_eq!(terminal(&manager, &run.run.id).status, RunState::Cancelled);
    assert!(executor.observed_cancellation.load(Ordering::Acquire));
    manager.shutdown().unwrap();
}

#[test]
fn failures_and_panics_become_explicit_durable_failures() {
    let (_scratch, store, _executor, manager) = manager("failures", 2);
    let failed = manager.submit(request("failure", "fail")).unwrap();
    let panicked = manager.submit(request("panic", "panic")).unwrap();
    assert_eq!(terminal(&manager, &failed.run.id).status, RunState::Failed);
    assert_eq!(
        terminal(&manager, &panicked.run.id).status,
        RunState::Failed
    );
    let failed_events = store.run_events(&failed.run.id).unwrap();
    let panic_events = store.run_events(&panicked.run.id).unwrap();
    assert_eq!(
        failed_events.last().unwrap().payload["code"],
        "executor_failed"
    );
    assert_eq!(
        failed_events.last().unwrap().payload["message"],
        "executor returned a failure"
    );
    assert_eq!(
        panic_events.last().unwrap().payload["code"],
        "executor_panicked"
    );
    assert_eq!(
        panic_events.last().unwrap().payload["message"],
        "executor panicked"
    );
    manager.shutdown().unwrap();
}

#[test]
fn queue_full_is_an_explicit_failure_and_shutdown_has_no_running_state() {
    let (_scratch, store, executor, manager) = manager("queue-full", 1);
    let active = manager.submit(request("active", "block")).unwrap();
    executor.wait_until_entered("block");
    let queued = manager
        .submit(request("queued-capacity", "success"))
        .unwrap();
    let rejected = manager.submit(request("rejected", "success")).unwrap();
    assert_eq!(rejected.disposition, SubmitDisposition::QueueFull);
    assert_eq!(rejected.run.status, RunState::Failed);
    executor.release();
    manager.shutdown().unwrap();
    for id in [&active.run.id, &queued.run.id, &rejected.run.id] {
        let status = store.get_run(id).unwrap().unwrap().status;
        assert!(!matches!(
            status,
            RunState::Starting | RunState::Running | RunState::CancelRequested
        ));
    }
}

#[test]
fn shutdown_cancels_running_and_queued_work_before_returning() {
    let (_scratch, store, executor, manager) = manager("shutdown", 1);
    let active = manager
        .submit(request("shutdown-active", "cancel"))
        .unwrap();
    executor.wait_until_entered("cancel");
    let queued = manager
        .submit(request("shutdown-queued", "never-execute"))
        .unwrap();

    manager.shutdown().unwrap();

    assert!(executor.observed_cancellation.load(Ordering::Acquire));
    assert_eq!(
        store.get_run(&active.run.id).unwrap().unwrap().status,
        RunState::Cancelled
    );
    assert_eq!(
        store.get_run(&queued.run.id).unwrap().unwrap().status,
        RunState::Cancelled
    );
    assert_eq!(executor.calls_for("never-execute"), 0);
}
