use super::*;
use crate::app_core::{
    BackendSelection, IdempotencyKey, RunCancellationReason, RunLifecycleEvent, RunMode,
};
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
struct CompletionRaceState {
    entered: bool,
    released: bool,
}

#[derive(Default)]
struct CompletionRaceClock {
    calls: AtomicU64,
    state: (Mutex<CompletionRaceState>, Condvar),
}

impl CompletionRaceClock {
    fn wait_until_terminal_transition_is_staged(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let (state, changed) = &self.state;
        let mut state = lock(state);
        while !state.entered {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("terminal transition was not staged before deadline");
            let waited = changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = waited.0;
            assert!(!waited.1.timed_out(), "terminal transition was not staged");
        }
    }

    fn release_terminal_transition(&self) {
        let (state, changed) = &self.state;
        lock(state).released = true;
        changed.notify_all();
    }
}

impl Clock for CompletionRaceClock {
    fn now_millis(&self) -> u64 {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        if call == 2 {
            let (state, changed) = &self.state;
            let mut state = lock(state);
            state.entered = true;
            changed.notify_all();
            while !state.released {
                state = changed
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
        call
    }
}

struct CompletionRaceRelease(Arc<CompletionRaceClock>);

impl Drop for CompletionRaceRelease {
    fn drop(&mut self) {
        self.0.release_terminal_transition();
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

fn projected_cancellation_reason(store: &RuntimeStore, id: &RunId) -> RunCancellationReason {
    let page = store.run_events_page(id, 0, None, 16).unwrap();
    match page.events.last().map(|record| &record.event) {
        Some(RunLifecycleEvent::Cancelled { reason }) => *reason,
        other => panic!("expected projected cancellation event, found {other:?}"),
    }
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
fn execution_failure_kinds_map_to_stable_non_secret_evidence() {
    let cases = [
        (ExecutionErrorKind::General, "executor_failed"),
        (ExecutionErrorKind::Unsupported, "executor_unsupported"),
        (ExecutionErrorKind::Spawn, "executor_spawn_failed"),
        (ExecutionErrorKind::TimedOut, "executor_timed_out"),
        (ExecutionErrorKind::OutputLimit, "executor_output_limit"),
        (ExecutionErrorKind::ProviderExit, "executor_provider_exit"),
        (ExecutionErrorKind::Teardown, "executor_teardown_failed"),
    ];
    for (kind, expected_code) in cases {
        let (code, message) = execution_failure_evidence(kind);
        assert_eq!(code, expected_code);
        assert!(message.starts_with("executor "));
        assert!(!message.contains("fixture"));
    }
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
    let (_scratch, store, executor, manager) = manager("queued-cancel", 2);
    let blocker = manager.submit(request("blocker", "block")).unwrap();
    executor.wait_until_entered("block");
    let queued = manager.submit(request("queued", "never-execute")).unwrap();
    let cancelled = manager.cancel(&queued.run.id).unwrap();
    assert_eq!(cancelled.status, RunState::Cancelled);
    assert_eq!(
        projected_cancellation_reason(&store, &queued.run.id),
        RunCancellationReason::Requested
    );
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
fn cancellation_winning_during_terminal_persistence_converges_to_cancelled() {
    let scratch = ScratchDir::new("completion-cancel-race");
    let store = Arc::new(RuntimeStore::open(&scratch.0.join("runtime.sqlite")).unwrap());
    let executor = Arc::new(TestExecutor::default());
    let clock = Arc::new(CompletionRaceClock::default());
    let manager = RunManager::start(
        Arc::clone(&store),
        executor,
        Arc::clone(&clock),
        RunManagerConfig { queue_capacity: 1 },
    );
    let release = CompletionRaceRelease(Arc::clone(&clock));

    let run = manager
        .submit(request("completion-race", "success"))
        .unwrap();
    clock.wait_until_terminal_transition_is_staged();
    let cancellation = manager.cancel(&run.run.id).unwrap();
    release.0.release_terminal_transition();
    assert_eq!(cancellation.status, RunState::CancelRequested);

    assert_eq!(terminal(&manager, &run.run.id).status, RunState::Cancelled);
    manager.shutdown().unwrap();
    assert!(
        store
            .get_run(&run.run.id)
            .unwrap()
            .unwrap()
            .status
            .is_terminal()
    );
}

fn assert_cancel_race_preserves_failure(input: &str, expected_code: &str) {
    let scratch = ScratchDir::new(expected_code);
    let store = Arc::new(RuntimeStore::open(&scratch.0.join("runtime.sqlite")).unwrap());
    let executor = Arc::new(TestExecutor::default());
    let clock = Arc::new(CompletionRaceClock::default());
    let manager = RunManager::start(
        Arc::clone(&store),
        executor,
        Arc::clone(&clock),
        RunManagerConfig { queue_capacity: 1 },
    );
    let release = CompletionRaceRelease(Arc::clone(&clock));

    let run = manager
        .submit(request(expected_code, input))
        .expect("failure fixture should be admitted");
    clock.wait_until_terminal_transition_is_staged();
    let cancellation = manager.cancel(&run.run.id).unwrap();
    release.0.release_terminal_transition();
    assert_eq!(cancellation.status, RunState::CancelRequested);

    assert_eq!(terminal(&manager, &run.run.id).status, RunState::Failed);
    let events = store.run_events(&run.run.id).unwrap();
    assert_eq!(events.last().unwrap().payload["code"], expected_code);
    manager.shutdown().unwrap();
}

#[test]
fn cancellation_racing_with_failure_preserves_failure_record() {
    assert_cancel_race_preserves_failure("fail", "executor_failed");
}

#[test]
fn cancellation_racing_with_panic_preserves_panic_record() {
    assert_cancel_race_preserves_failure("panic", "executor_panicked");
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
    assert_eq!(
        projected_cancellation_reason(&store, &active.run.id),
        RunCancellationReason::ManagerShutdown
    );
    assert_eq!(
        projected_cancellation_reason(&store, &queued.run.id),
        RunCancellationReason::ManagerShutdown
    );
    assert_eq!(executor.calls_for("never-execute"), 0);
}

#[test]
fn rejected_admission_is_released_only_after_terminal_persistence() {
    let (_scratch, store, _executor, manager) = manager("rejected-admission", 1);
    let request = request("rejected-admission", "success");
    let run = store
        .create_or_get_run(NewRun {
            conversation_id: None,
            idempotency_key: request.idempotency_key.clone(),
            request_digest: canonical_request_digest(&request).unwrap(),
        })
        .unwrap();
    lock(&manager.shared.admitted).insert(run.id.clone());

    let terminal = reject_admission(
        &manager.shared,
        &run.id,
        "fixture_rejection",
        "fixture rejection",
    )
    .unwrap();

    assert_eq!(terminal.status, RunState::Failed);
    assert!(!lock(&manager.shared.admitted).contains(&run.id));
    manager.shutdown().unwrap();
}

#[test]
fn rejected_admission_retains_recovery_ownership_on_nonterminal_conflict() {
    let (_scratch, store, _executor, manager) = manager("rejection-conflict", 1);
    let request = request("rejection-conflict", "success");
    let run = store
        .create_or_get_run(NewRun {
            conversation_id: None,
            idempotency_key: request.idempotency_key.clone(),
            request_digest: canonical_request_digest(&request).unwrap(),
        })
        .unwrap();
    lock(&manager.shared.admitted).insert(run.id.clone());
    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Starting,
            event(&manager.shared, "fixture_starting", None, None),
        )
        .unwrap();

    assert!(matches!(
        reject_admission(
            &manager.shared,
            &run.id,
            "fixture_rejection",
            "fixture rejection"
        ),
        Err(ManagerError::Store(StoreError::UnexpectedStatus {
            found: RunState::Starting,
            ..
        }))
    ));
    assert!(lock(&manager.shared.admitted).contains(&run.id));
    manager.shutdown().unwrap();
    assert_eq!(
        store.get_run(&run.id).unwrap().unwrap().status,
        RunState::Interrupted
    );
}

#[test]
fn start_conflict_settles_cancel_requested_before_releasing_admission() {
    let (_scratch, store, executor, manager) = manager("start-conflict", 1);
    let request = request("start-conflict", "never-execute");
    let run = store
        .create_or_get_run(NewRun {
            conversation_id: None,
            idempotency_key: request.idempotency_key.clone(),
            request_digest: canonical_request_digest(&request).unwrap(),
        })
        .unwrap();
    lock(&manager.shared.admitted).insert(run.id.clone());
    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::CancelRequested,
            event(&manager.shared, "fixture_cancel_requested", None, None),
        )
        .unwrap();

    execute_item(
        &manager.shared,
        executor.as_ref(),
        WorkItem {
            run_id: run.id.clone(),
            request,
        },
    )
    .unwrap();

    assert_eq!(
        store.get_run(&run.id).unwrap().unwrap().status,
        RunState::Cancelled
    );
    assert!(!lock(&manager.shared.admitted).contains(&run.id));
    assert_eq!(executor.calls_for("never-execute"), 0);
    manager.shutdown().unwrap();
}
