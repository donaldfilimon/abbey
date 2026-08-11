use super::*;
use serde_json::Value;

pub(super) fn validate_event_snapshot(
    conn: &Connection,
    id: &RunId,
    current_watermark: u64,
) -> Result<(), StoreError> {
    let (count, minimum, maximum): (i64, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT COUNT(*), MIN(sequence), MAX(sequence)
         FROM run_events WHERE run_id=?1",
        [id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let expected = sql_sequence(current_watermark)?;
    if count != expected || minimum != Some(1) || maximum != Some(expected) {
        return Err(StoreError::CorruptData(
            "run event snapshot contains a sequence gap",
        ));
    }
    Ok(())
}

pub(super) fn project_run_snapshot(
    conn: &Connection,
    run: RunRecord,
) -> Result<RunSnapshot, StoreError> {
    let event_count = run
        .next_event_sequence
        .checked_sub(1)
        .filter(|count| *count > 0)
        .ok_or(StoreError::CorruptData("run event watermark is missing"))?;
    validate_event_snapshot(conn, &run.id, event_count)?;
    let sequence = sql_sequence(event_count)?;
    let latest = conn.query_row(
        "SELECT run_id, sequence, kind, payload_json, created_at
         FROM run_events WHERE run_id=?1 AND sequence=?2",
        params![run.id.as_str(), sequence],
        row_to_event,
    )?;
    let latest = project_run_event(latest, Some(run.status))?;
    let failure = match latest.event {
        RunLifecycleEvent::Failed { failure } => Some(failure),
        _ => None,
    };
    let snapshot = RunSnapshot {
        run_id: run.id,
        conversation_id: run.conversation_id,
        idempotency_key: run.idempotency_key,
        state: run.status,
        created_at: run.created_at,
        updated_at: run.updated_at,
        failure,
        event_count,
    };
    snapshot
        .validate()
        .map_err(|_| StoreError::CorruptData("run snapshot projection is invalid"))?;
    Ok(snapshot)
}

pub(super) fn project_run_event(
    event: RunEvent,
    current_state: Option<RunState>,
) -> Result<RunEventRecord, StoreError> {
    let projected = match event.kind.as_str() {
        "run_queued" => RunLifecycleEvent::Queued,
        "run_starting" => RunLifecycleEvent::Starting,
        "run_started" => RunLifecycleEvent::Running,
        "run_cancel_requested" => RunLifecycleEvent::CancelRequested,
        "run_succeeded" => RunLifecycleEvent::Succeeded,
        "run_failed" => RunLifecycleEvent::Failed {
            failure: project_failure(&event.payload)?,
        },
        "run_cancelled" => RunLifecycleEvent::Cancelled {
            reason: match event.payload.get("reason").and_then(Value::as_str) {
                None | Some("requested") => RunCancellationReason::Requested,
                Some("manager_shutdown") => RunCancellationReason::ManagerShutdown,
                Some(_) => {
                    return Err(StoreError::CorruptData(
                        "run cancellation reason is invalid",
                    ));
                }
            },
        },
        "run_manager_stopped" => match current_state {
            Some(RunState::Cancelled) => RunLifecycleEvent::Cancelled {
                reason: RunCancellationReason::ManagerShutdown,
            },
            Some(RunState::Interrupted) => RunLifecycleEvent::Interrupted {
                reason: RunInterruptionReason::ManagerShutdown,
            },
            _ => {
                return Err(StoreError::CorruptData(
                    "manager shutdown event does not match the run state",
                ));
            }
        },
        "run_recovered_interrupted" => {
            if event.payload.get("reason").and_then(Value::as_str) != Some("daemon_restart") {
                return Err(StoreError::CorruptData(
                    "run interruption reason is invalid",
                ));
            }
            RunLifecycleEvent::Interrupted {
                reason: RunInterruptionReason::DaemonRestart,
            }
        }
        _ => {
            return Err(StoreError::CorruptData(
                "run event kind cannot cross the application boundary",
            ));
        }
    };
    if current_state.is_some_and(|state| lifecycle_state(&projected) != state) {
        return Err(StoreError::CorruptData(
            "latest run event does not match the run state",
        ));
    }
    let record = RunEventRecord {
        run_id: event.run_id,
        sequence: event.sequence,
        recorded_at: event.created_at,
        event: projected,
    };
    record
        .validate()
        .map_err(|_| StoreError::CorruptData("run event projection is invalid"))?;
    Ok(record)
}

fn lifecycle_state(event: &RunLifecycleEvent) -> RunState {
    match event {
        RunLifecycleEvent::Queued => RunState::Queued,
        RunLifecycleEvent::Starting => RunState::Starting,
        RunLifecycleEvent::Running => RunState::Running,
        RunLifecycleEvent::CancelRequested => RunState::CancelRequested,
        RunLifecycleEvent::Succeeded => RunState::Succeeded,
        RunLifecycleEvent::Failed { .. } => RunState::Failed,
        RunLifecycleEvent::Cancelled { .. } => RunState::Cancelled,
        RunLifecycleEvent::Interrupted { .. } => RunState::Interrupted,
    }
}

fn project_failure(payload: &Value) -> Result<RunFailure, StoreError> {
    let Some(code) = payload.get("code").and_then(Value::as_str) else {
        return Err(StoreError::CorruptData("run failure code is missing"));
    };
    let (message, retryable) = match code {
        "worker_unavailable" => ("run worker is unavailable", true),
        "queue_full" => ("bounded run queue is full", true),
        "executor_failed" => ("executor returned a failure", false),
        "executor_unsupported" => ("executor does not support this request", false),
        "executor_spawn_failed" => ("executor process failed to start", true),
        "executor_timed_out" => ("executor exceeded its deadline", true),
        "executor_output_limit" => ("executor output exceeded its limit", false),
        "executor_provider_exit" => ("executor process exited unsuccessfully", true),
        "executor_teardown_failed" => ("executor process teardown failed", false),
        "executor_panicked" => ("executor panicked", false),
        _ => return Err(StoreError::CorruptData("run failure code is invalid")),
    };
    if payload.get("message").and_then(Value::as_str) != Some(message) {
        return Err(StoreError::CorruptData("run failure message is invalid"));
    }
    Ok(RunFailure {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    })
}
