//! Bounded, single-worker lifecycle manager for durable Abbey runs.
//!
//! This is a scheduling foundation, not a provider runtime. It persists every
//! state transition through [`RuntimeStore`] and delegates execution through an
//! injected [`Executor`].

use super::executor::{
    CancellationToken, ExecutionAttempt, ExecutionErrorKind, Executor, execute_catching_panics,
};
use super::{NewRun, NewRunEvent, RunRecord, RuntimeStore, StoreError};
use crate::app_core::{RunId, RunRequest, RunState};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIN_QUEUE_CAPACITY: usize = 1;
const MAX_QUEUE_CAPACITY: usize = 32;

/// Configurable scheduling limits. Capacity is clamped to `1..=32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunManagerConfig {
    pub queue_capacity: usize,
}

impl Default for RunManagerConfig {
    fn default() -> Self {
        Self { queue_capacity: 8 }
    }
}

impl RunManagerConfig {
    #[must_use]
    pub fn effective_queue_capacity(self) -> usize {
        self.queue_capacity
            .clamp(MIN_QUEUE_CAPACITY, MAX_QUEUE_CAPACITY)
    }
}

/// Clock seam used in deterministic lifecycle evidence payloads.
pub trait Clock: Send + Sync + 'static {
    fn now_millis(&self) -> u64;
}

/// Wall clock used outside tests.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

/// Whether a submit call admitted new work or returned an existing identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitDisposition {
    Enqueued,
    Existing,
    QueueFull,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitResult {
    pub run: RunRecord,
    pub disposition: SubmitDisposition,
}

#[derive(Debug)]
pub enum ManagerError {
    InvalidRequest(String),
    ShuttingDown,
    Store(StoreError),
    WorkerPanicked,
}

impl fmt::Display for ManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid run request: {message}"),
            Self::ShuttingDown => formatter.write_str("run manager is shutting down"),
            Self::Store(error) => write!(formatter, "runtime store error: {error}"),
            Self::WorkerPanicked => formatter.write_str("run manager worker panicked"),
        }
    }
}

impl std::error::Error for ManagerError {}

impl From<StoreError> for ManagerError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

struct WorkItem {
    run_id: RunId,
    request: RunRequest,
}

struct Shared<C> {
    store: Arc<RuntimeStore>,
    clock: Arc<C>,
    admitted: Mutex<HashSet<RunId>>,
    active: Mutex<HashMap<RunId, CancellationToken>>,
    shutting_down: AtomicBool,
    worker_failed: AtomicBool,
    progress: (Mutex<u64>, Condvar),
}

/// One deterministic worker over a bounded synchronous queue.
pub struct RunManager<E: Executor, C: Clock = SystemClock> {
    shared: Arc<Shared<C>>,
    sender: Mutex<Option<SyncSender<WorkItem>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    _executor: std::marker::PhantomData<E>,
}

impl<E: Executor, C: Clock> RunManager<E, C> {
    #[must_use]
    pub fn start(
        store: Arc<RuntimeStore>,
        executor: Arc<E>,
        clock: Arc<C>,
        config: RunManagerConfig,
    ) -> Self {
        let capacity = config.effective_queue_capacity();
        let (sender, receiver) = sync_channel(capacity);
        let shared = Arc::new(Shared {
            store,
            clock,
            admitted: Mutex::new(HashSet::new()),
            active: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
            worker_failed: AtomicBool::new(false),
            progress: (Mutex::new(0), Condvar::new()),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("abbey-run-manager".into())
            .spawn(move || worker_entry(worker_shared, executor, receiver))
            .expect("failed to spawn Abbey run manager worker");
        Self {
            shared,
            sender: Mutex::new(Some(sender)),
            worker: Mutex::new(Some(worker)),
            _executor: std::marker::PhantomData,
        }
    }

    /// Persist and enqueue a validated request.
    pub fn submit(&self, request: RunRequest) -> Result<SubmitResult, ManagerError> {
        if self.shared.shutting_down.load(Ordering::Acquire) {
            return Err(ManagerError::ShuttingDown);
        }
        request
            .validate()
            .map_err(|error| ManagerError::InvalidRequest(error.to_string()))?;
        let request_digest = canonical_request_digest(&request)?;
        if let Some(conversation_id) = &request.conversation_id {
            self.shared.store.create_conversation(conversation_id)?;
        }
        let run = self.shared.store.create_or_get_run(NewRun {
            conversation_id: request.conversation_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_digest,
        })?;
        if run.status != RunState::Queued {
            return Ok(SubmitResult {
                run,
                disposition: SubmitDisposition::Existing,
            });
        }

        {
            let mut admitted = lock(&self.shared.admitted);
            if !admitted.insert(run.id.clone()) {
                return Ok(SubmitResult {
                    run,
                    disposition: SubmitDisposition::Existing,
                });
            }
        }

        let item = WorkItem {
            run_id: run.id.clone(),
            request,
        };
        let sender = lock(&self.sender).as_ref().cloned();
        let Some(sender) = sender else {
            reject_admission(
                &self.shared,
                &item.run_id,
                "worker_unavailable",
                "run worker is unavailable",
            )?;
            return Err(ManagerError::ShuttingDown);
        };
        let send_result = sender.try_send(item);
        match send_result {
            Ok(()) => Ok(SubmitResult {
                run,
                disposition: SubmitDisposition::Enqueued,
            }),
            Err(TrySendError::Full(item)) => {
                let failed = reject_admission(
                    &self.shared,
                    &item.run_id,
                    "queue_full",
                    "bounded run queue is full",
                )?;
                Ok(SubmitResult {
                    run: failed,
                    disposition: SubmitDisposition::QueueFull,
                })
            }
            Err(TrySendError::Disconnected(item)) => {
                reject_admission(
                    &self.shared,
                    &item.run_id,
                    "worker_unavailable",
                    "run worker is unavailable",
                )?;
                Err(ManagerError::ShuttingDown)
            }
        }
    }

    /// Cooperatively cancel a queued or running request.
    pub fn cancel(&self, run_id: &RunId) -> Result<RunRecord, ManagerError> {
        for _ in 0..8 {
            let record = self
                .shared
                .store
                .get_run(run_id)?
                .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
            if record.status.is_terminal() {
                return Ok(record);
            }
            if let Some(token) = lock(&self.shared.active).get(run_id).cloned() {
                token.cancel();
            }
            let transition = match record.status {
                RunState::Queued => self.shared.store.transition_run(
                    run_id,
                    RunState::Queued,
                    RunState::Cancelled,
                    event(&self.shared, "run_cancelled", None, None),
                ),
                RunState::Starting | RunState::Running => self.shared.store.transition_run(
                    run_id,
                    record.status,
                    RunState::CancelRequested,
                    event(&self.shared, "run_cancel_requested", None, None),
                ),
                RunState::CancelRequested => return Ok(record),
                RunState::Succeeded
                | RunState::Failed
                | RunState::Cancelled
                | RunState::Interrupted => unreachable!("terminal status handled above"),
            };
            match transition {
                Ok(_) => {
                    notify(&self.shared);
                    return self
                        .shared
                        .store
                        .get_run(run_id)?
                        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()).into());
                }
                Err(StoreError::UnexpectedStatus { .. }) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(ManagerError::Store(StoreError::CorruptData(
            "cancellation transition did not converge",
        )))
    }

    /// Wait for a run to reach a terminal state, using worker notifications.
    pub fn wait_for_terminal(
        &self,
        run_id: &RunId,
        timeout: Duration,
    ) -> Result<Option<RunRecord>, ManagerError> {
        let deadline = std::time::Instant::now() + timeout;
        let (generation_lock, progress) = &self.shared.progress;
        let mut generation = lock(generation_lock);
        loop {
            let record = self.shared.store.get_run(run_id)?;
            if record.as_ref().is_some_and(|run| run.status.is_terminal()) {
                return Ok(record);
            }
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return Ok(record);
            };
            let observed = *generation;
            let waited = progress
                .wait_timeout_while(generation, remaining, |current| *current == observed)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            generation = waited.0;
            if waited.1.timed_out() {
                return self.shared.store.get_run(run_id).map_err(Into::into);
            }
        }
    }

    /// Cancel active work, drain queued work to terminal cancellation, and join.
    pub fn shutdown(&self) -> Result<(), ManagerError> {
        if !self.shared.shutting_down.swap(true, Ordering::AcqRel) {
            for token in lock(&self.shared.active).values() {
                token.cancel();
            }
            lock(&self.sender).take();
        }
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker
            && worker.join().is_err()
        {
            let _ = interrupt_admitted(&self.shared);
            return Err(ManagerError::WorkerPanicked);
        }
        interrupt_admitted(&self.shared)?;
        if self.shared.worker_failed.load(Ordering::Acquire) {
            return Err(ManagerError::WorkerPanicked);
        }
        Ok(())
    }
}

impl<E: Executor, C: Clock> Drop for RunManager<E, C> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_entry<E: Executor, C: Clock>(
    shared: Arc<Shared<C>>,
    executor: Arc<E>,
    receiver: Receiver<WorkItem>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<(), ManagerError> {
            while let Ok(item) = receiver.recv() {
                if shared.shutting_down.load(Ordering::Acquire) {
                    cancel_queued(&shared, &item.run_id, "manager_shutdown")?;
                    continue;
                }
                execute_item(&shared, executor.as_ref(), item)?;
            }
            Ok(())
        },
    ));
    if !matches!(result, Ok(Ok(()))) {
        shared.shutting_down.store(true, Ordering::Release);
        shared.worker_failed.store(true, Ordering::Release);
        let _ = interrupt_admitted(&shared);
    }
}

fn execute_item<E: Executor, C: Clock>(
    shared: &Shared<C>,
    executor: &E,
    item: WorkItem,
) -> Result<(), ManagerError> {
    let started = shared.store.transition_run(
        &item.run_id,
        RunState::Queued,
        RunState::Starting,
        event(shared, "run_starting", None, None),
    );
    match started {
        Ok(_) => {}
        Err(StoreError::UnexpectedStatus { .. } | StoreError::TerminalRun { .. }) => {
            settle_start_conflict(shared, &item.run_id)?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }

    let token = CancellationToken::default();
    lock(&shared.active).insert(item.run_id.clone(), token.clone());
    if shared.shutting_down.load(Ordering::Acquire) {
        token.cancel();
    }

    let record = shared
        .store
        .get_run(&item.run_id)?
        .ok_or_else(|| StoreError::RunNotFound(item.run_id.to_string()))?;
    if record.status == RunState::CancelRequested || token.is_cancelled() {
        token.cancel();
        finish_cancelled(shared, &item.run_id)?;
        finish_admission(shared, &item.run_id);
        return Ok(());
    }
    let running = shared.store.transition_run(
        &item.run_id,
        RunState::Starting,
        RunState::Running,
        event(shared, "run_started", None, None),
    );
    match running {
        Ok(_) => {}
        Err(StoreError::UnexpectedStatus { .. }) => {
            token.cancel();
            finish_cancelled(shared, &item.run_id)?;
            finish_admission(shared, &item.run_id);
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }

    let attempt = execute_catching_panics(executor, &item.run_id, item.request, &token);
    settle_attempt(shared, &item.run_id, &token, &attempt)?;
    finish_admission(shared, &item.run_id);
    Ok(())
}

fn settle_attempt<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
    token: &CancellationToken,
    attempt: &ExecutionAttempt,
) -> Result<(), ManagerError> {
    for _ in 0..8 {
        let record = shared
            .store
            .get_run(run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        if record.status.is_terminal() {
            return Ok(());
        }
        let (next, terminal_event) = match attempt {
            ExecutionAttempt::Failed(error) => {
                let (code, message) = execution_failure_evidence(error.kind());
                (RunState::Failed, failure_event(shared, code, message))
            }
            ExecutionAttempt::Panicked(_) => (
                RunState::Failed,
                failure_event(shared, "executor_panicked", "executor panicked"),
            ),
            ExecutionAttempt::Completed => {
                let cancelling = token.is_cancelled()
                    || record.status == RunState::CancelRequested
                    || shared.shutting_down.load(Ordering::Acquire);
                if cancelling {
                    (
                        RunState::Cancelled,
                        event(shared, "run_cancelled", None, None),
                    )
                } else {
                    (
                        RunState::Succeeded,
                        event(shared, "run_succeeded", None, None),
                    )
                }
            }
        };
        match shared
            .store
            .transition_run(run_id, record.status, next, terminal_event)
        {
            Ok(_) => return Ok(()),
            Err(StoreError::UnexpectedStatus { .. }) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(StoreError::CorruptData("terminal run transition did not converge").into())
}

const fn execution_failure_evidence(kind: ExecutionErrorKind) -> (&'static str, &'static str) {
    match kind {
        ExecutionErrorKind::General => ("executor_failed", "executor returned a failure"),
        ExecutionErrorKind::Unsupported => (
            "executor_unsupported",
            "executor does not support this request",
        ),
        ExecutionErrorKind::Spawn => ("executor_spawn_failed", "executor process failed to start"),
        ExecutionErrorKind::TimedOut => ("executor_timed_out", "executor exceeded its deadline"),
        ExecutionErrorKind::OutputLimit => (
            "executor_output_limit",
            "executor output exceeded its limit",
        ),
        ExecutionErrorKind::ProviderExit => (
            "executor_provider_exit",
            "executor process exited unsuccessfully",
        ),
        ExecutionErrorKind::Teardown => (
            "executor_teardown_failed",
            "executor process teardown failed",
        ),
    }
}

fn finish_cancelled<C: Clock>(shared: &Shared<C>, run_id: &RunId) -> Result<(), ManagerError> {
    for _ in 0..8 {
        let record = shared
            .store
            .get_run(run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        if record.status.is_terminal() {
            return Ok(());
        }
        match shared.store.transition_run(
            run_id,
            record.status,
            RunState::Cancelled,
            event(shared, "run_cancelled", None, None),
        ) {
            Ok(_) => return Ok(()),
            Err(StoreError::UnexpectedStatus { .. }) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(StoreError::CorruptData("cancellation transition did not converge").into())
}

fn cancel_queued<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
    reason: &str,
) -> Result<(), ManagerError> {
    let transition = shared.store.transition_run(
        run_id,
        RunState::Queued,
        RunState::Cancelled,
        event(shared, "run_cancelled", Some("reason"), Some(reason)),
    );
    match transition {
        Ok(_) | Err(StoreError::TerminalRun { .. }) => {}
        Err(StoreError::UnexpectedStatus { .. }) => finish_cancelled(shared, run_id)?,
        Err(error) => return Err(error.into()),
    }
    finish_admission(shared, run_id);
    Ok(())
}

fn finish_admission<C>(shared: &Shared<C>, run_id: &RunId) {
    lock(&shared.active).remove(run_id);
    lock(&shared.admitted).remove(run_id);
    notify(shared);
}

fn settle_start_conflict<C: Clock>(shared: &Shared<C>, run_id: &RunId) -> Result<(), ManagerError> {
    let record = shared
        .store
        .get_run(run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    if record.status.is_terminal() {
        finish_admission(shared, run_id);
        return Ok(());
    }
    if record.status == RunState::CancelRequested {
        finish_cancelled(shared, run_id)?;
        finish_admission(shared, run_id);
        return Ok(());
    }
    Err(StoreError::UnexpectedStatus {
        expected: RunState::Queued,
        found: record.status,
    }
    .into())
}

fn reject_admission<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
    code: &str,
    message: &str,
) -> Result<RunRecord, ManagerError> {
    for _ in 0..8 {
        let record = shared
            .store
            .get_run(run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        if record.status.is_terminal() {
            finish_admission(shared, run_id);
            return Ok(record);
        }
        if record.status != RunState::Queued {
            return Err(StoreError::UnexpectedStatus {
                expected: RunState::Queued,
                found: record.status,
            }
            .into());
        }
        match transition_with_failure(shared, run_id, RunState::Queued, code, message) {
            Ok(terminal) => {
                finish_admission(shared, run_id);
                return Ok(terminal);
            }
            Err(ManagerError::Store(StoreError::UnexpectedStatus { .. })) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(StoreError::CorruptData("rejected admission did not converge").into())
}

fn canonical_request_digest(request: &RunRequest) -> Result<String, ManagerError> {
    let encoded = serde_json::to_vec(request)
        .map_err(|_| ManagerError::InvalidRequest("request is not serializable".into()))?;
    let digest = Sha256::digest(encoded);
    let mut hexadecimal = String::with_capacity(64);
    for byte in digest {
        write!(&mut hexadecimal, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(hexadecimal)
}

fn transition_with_failure<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
    expected: RunState,
    code: &str,
    message: &str,
) -> Result<RunRecord, ManagerError> {
    shared.store.transition_run(
        run_id,
        expected,
        RunState::Failed,
        failure_event(shared, code, message),
    )?;
    shared
        .store
        .get_run(run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()).into())
}

fn event<C: Clock>(
    shared: &Shared<C>,
    kind: &str,
    detail_key: Option<&str>,
    detail_value: Option<&str>,
) -> NewRunEvent {
    let mut payload = Map::new();
    payload.insert(
        "manager_clock_ms".into(),
        Value::from(shared.clock.now_millis()),
    );
    if let (Some(key), Some(value)) = (detail_key, detail_value) {
        payload.insert(key.to_owned(), Value::from(value));
    }
    NewRunEvent {
        kind: kind.to_owned(),
        payload: Value::Object(payload),
    }
}

fn failure_event<C: Clock>(shared: &Shared<C>, code: &str, message: &str) -> NewRunEvent {
    let mut payload = Map::new();
    payload.insert(
        "manager_clock_ms".into(),
        Value::from(shared.clock.now_millis()),
    );
    payload.insert("code".into(), Value::from(code));
    payload.insert("message".into(), Value::from(message));
    NewRunEvent {
        kind: "run_failed".into(),
        payload: Value::Object(payload),
    }
}

fn interrupt_admitted<C: Clock>(shared: &Shared<C>) -> Result<(), ManagerError> {
    let ids = lock(&shared.admitted).iter().cloned().collect::<Vec<_>>();
    for id in ids {
        let Some(record) = shared.store.get_run(&id)? else {
            continue;
        };
        let next = match record.status {
            RunState::Queued => RunState::Cancelled,
            RunState::Starting | RunState::Running | RunState::CancelRequested => {
                RunState::Interrupted
            }
            RunState::Succeeded
            | RunState::Failed
            | RunState::Cancelled
            | RunState::Interrupted => continue,
        };
        shared.store.transition_run(
            &id,
            record.status,
            next,
            event(shared, "run_manager_stopped", None, None),
        )?;
    }
    lock(&shared.admitted).clear();
    lock(&shared.active).clear();
    notify(shared);
    Ok(())
}

fn notify<C>(shared: &Shared<C>) {
    let (generation, progress) = &shared.progress;
    let mut generation = lock(generation);
    *generation = generation.wrapping_add(1);
    progress.notify_all();
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests;
