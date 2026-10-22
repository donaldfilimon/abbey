//! Free-function helpers for the bounded, single-worker run manager.
//!
//! These helpers are the worker loop and the lifecycle transitions that the
//! parent module's run manager calls into. The split is a pure code motion:
//! every helper keeps its name, body, and reachability, and the parent
//! module imports the ones it uses.

use super::super::NewRunEvent;
use super::super::executor::{
    ExecutionAttempt, ExecutionErrorKind, execute_catching_panics, execute_v4_catching_panics,
};
use super::*;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::sync::MutexGuard;
use std::sync::mpsc::Receiver;

pub(super) fn worker_entry<E: Executor, C: Clock>(
    shared: Arc<Shared<C>>,
    executor: Arc<E>,
    receiver: Receiver<WorkItem>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<(), ManagerError> {
            while let Ok(item) = receiver.recv() {
                if shared.shutting_down.load(Ordering::Acquire) {
                    cancel_queued(&shared, &item.run_id, CancellationReason::ManagerShutdown)?;
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

pub(super) fn execute_item<E: Executor, C: Clock>(
    shared: &Shared<C>,
    executor: &E,
    item: WorkItem,
) -> Result<(), ManagerError> {
    let is_v4 = matches!(item.payload, WorkPayload::V4(_));
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
        if is_v4 {
            finalize_v4_turn(shared, &item.run_id, RunState::Cancelled)?;
        }
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
            if is_v4 {
                finalize_v4_turn(shared, &item.run_id, RunState::Cancelled)?;
            }
            finish_admission(shared, &item.run_id);
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }

    let attempt = match item.payload {
        WorkPayload::Legacy(request) => {
            execute_catching_panics(executor, &item.run_id, request, &token)
        }
        WorkPayload::V4(request) => {
            execute_v4_catching_panics(executor, &shared.store, &item.run_id, request, &token)
        }
    };
    settle_attempt(shared, &item.run_id, &token, &attempt)?;
    if is_v4 {
        let terminal = shared
            .store
            .get_run(&item.run_id)?
            .map(|run| run.status)
            .unwrap_or(RunState::Failed);
        if terminal.is_terminal() {
            finalize_v4_turn(shared, &item.run_id, terminal)?;
        }
    }
    finish_admission(shared, &item.run_id);
    Ok(())
}

pub(super) fn finalize_v4_turn<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
    state: RunState,
) -> Result<(), ManagerError> {
    match shared
        .store
        .v4_finalize_turn(run_id, state, shared.clock.now_millis())
    {
        Ok(()) | Err(StoreError::AssistantNotFound) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn settle_attempt<C: Clock>(
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
                        cancellation_event(shared, cancellation_reason(shared, record.status)),
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

pub(super) const fn execution_failure_evidence(
    kind: ExecutionErrorKind,
) -> (&'static str, &'static str) {
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

pub(super) fn finish_cancelled<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
) -> Result<(), ManagerError> {
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
            cancellation_event(shared, cancellation_reason(shared, record.status)),
        ) {
            Ok(_) => return Ok(()),
            Err(StoreError::UnexpectedStatus { .. }) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(StoreError::CorruptData("cancellation transition did not converge").into())
}

pub(super) fn cancel_queued<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
    reason: CancellationReason,
) -> Result<(), ManagerError> {
    let transition = shared.store.transition_run(
        run_id,
        RunState::Queued,
        RunState::Cancelled,
        cancellation_event(shared, reason),
    );
    match transition {
        Ok(_) | Err(StoreError::TerminalRun { .. }) => {}
        Err(StoreError::UnexpectedStatus { .. }) => finish_cancelled(shared, run_id)?,
        Err(error) => return Err(error.into()),
    }
    finish_admission(shared, run_id);
    Ok(())
}

pub(super) fn finish_admission<C>(shared: &Shared<C>, run_id: &RunId) {
    lock(&shared.active).remove(run_id);
    lock(&shared.admitted).remove(run_id);
    notify(shared);
}

pub(super) fn settle_start_conflict<C: Clock>(
    shared: &Shared<C>,
    run_id: &RunId,
) -> Result<(), ManagerError> {
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

pub(super) fn reject_admission<C: Clock>(
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

pub(super) fn canonical_request_digest(request: &RunRequest) -> Result<String, ManagerError> {
    let encoded = serde_json::to_vec(request)
        .map_err(|_| ManagerError::InvalidRequest("request is not serializable".into()))?;
    let digest = Sha256::digest(encoded);
    let mut hexadecimal = String::with_capacity(64);
    for byte in digest {
        write!(&mut hexadecimal, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(hexadecimal)
}

pub(super) fn transition_with_failure<C: Clock>(
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

pub(super) fn event<C: Clock>(
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

pub(super) fn failure_event<C: Clock>(
    shared: &Shared<C>,
    code: &str,
    message: &str,
) -> NewRunEvent {
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

#[derive(Clone, Copy)]
pub(super) enum CancellationReason {
    Requested,
    ManagerShutdown,
}

impl CancellationReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::ManagerShutdown => "manager_shutdown",
        }
    }
}

pub(super) fn cancellation_reason<C: Clock>(
    shared: &Shared<C>,
    state: RunState,
) -> CancellationReason {
    if shared.shutting_down.load(Ordering::Acquire) && state != RunState::CancelRequested {
        CancellationReason::ManagerShutdown
    } else {
        CancellationReason::Requested
    }
}

pub(super) fn cancellation_event<C: Clock>(
    shared: &Shared<C>,
    reason: CancellationReason,
) -> NewRunEvent {
    event(
        shared,
        "run_cancelled",
        Some("reason"),
        Some(reason.as_str()),
    )
}

pub(super) fn interrupt_admitted<C: Clock>(shared: &Shared<C>) -> Result<(), ManagerError> {
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

pub(super) fn notify<C>(shared: &Shared<C>) {
    let (generation, progress) = &shared.progress;
    let mut generation = lock(generation);
    *generation = generation.wrapping_add(1);
    progress.notify_all();
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
