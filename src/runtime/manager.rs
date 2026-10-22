//! Bounded, single-worker lifecycle manager for durable Abbey runs.
//!
//! This is a scheduling foundation, not a provider runtime. It persists every
//! state transition through [`RuntimeStore`] and delegates execution through an
//! injected [`Executor`].

use self::helpers::{
    CancellationReason, cancellation_event, canonical_request_digest, event, interrupt_admitted,
    lock, notify, reject_admission, worker_entry,
};
use super::executor::{CancellationToken, Executor};
use super::{NewRun, RunRecord, RuntimeStore, StoreError};
use crate::app_core::{RunId, RunRequest, RunState, V4SubmitTurn, V4TurnSubmission};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use self::helpers::{execute_item, execution_failure_evidence};
#[cfg(test)]
use super::executor::ExecutionErrorKind;

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

enum WorkPayload {
    Legacy(RunRequest),
    V4(V4SubmitTurn),
}

struct WorkItem {
    run_id: RunId,
    payload: WorkPayload,
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
            payload: WorkPayload::Legacy(request),
        };
        match self.enqueue(item)? {
            EnqueueOutcome::Enqueued => Ok(SubmitResult {
                run,
                disposition: SubmitDisposition::Enqueued,
            }),
            EnqueueOutcome::QueueFull(failed) => Ok(SubmitResult {
                run: failed,
                disposition: SubmitDisposition::QueueFull,
            }),
        }
    }

    /// Admit a protocol-v4 turn and enqueue it on the shared worker.
    ///
    /// Admission is the store's atomic `v4_admit_turn`. Execution uses
    /// [`Executor::execute_v4`]; the default refuses, so an unbound conversational
    /// route fails through the ordinary lifecycle instead of leaving a stuck queue.
    pub fn submit_v4(&self, request: V4SubmitTurn) -> Result<V4TurnSubmission, ManagerError> {
        if self.shared.shutting_down.load(Ordering::Acquire) {
            return Err(ManagerError::ShuttingDown);
        }
        request
            .validate()
            .map_err(|error| ManagerError::InvalidRequest(error.to_string()))?;
        let now_ms = self.shared.clock.now_millis();
        let admitted = self.shared.store.v4_admit_turn(&request, now_ms)?;
        if admitted.duplicate || admitted.state != RunState::Queued {
            return Ok(admitted);
        }

        {
            let mut admitted_ids = lock(&self.shared.admitted);
            if !admitted_ids.insert(admitted.run_id.clone()) {
                return Ok(admitted);
            }
        }

        let item = WorkItem {
            run_id: admitted.run_id.clone(),
            payload: WorkPayload::V4(request),
        };
        match self.enqueue(item)? {
            EnqueueOutcome::Enqueued => Ok(admitted),
            EnqueueOutcome::QueueFull(_) => {
                // Admission already failed the run; re-read the turn receipt.
                self.shared
                    .store
                    .v4_fail_unadopted_turn(&admitted.run_id)
                    .map_err(Into::into)
            }
        }
    }

    fn enqueue(&self, item: WorkItem) -> Result<EnqueueOutcome, ManagerError> {
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
        match sender.try_send(item) {
            Ok(()) => Ok(EnqueueOutcome::Enqueued),
            Err(TrySendError::Full(item)) => {
                let failed = reject_admission(
                    &self.shared,
                    &item.run_id,
                    "queue_full",
                    "bounded run queue is full",
                )?;
                Ok(EnqueueOutcome::QueueFull(failed))
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
                    cancellation_event(&self.shared, CancellationReason::Requested),
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

enum EnqueueOutcome {
    Enqueued,
    QueueFull(RunRecord),
}

mod helpers;

#[cfg(test)]
mod tests;
