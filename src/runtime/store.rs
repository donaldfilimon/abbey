//! SQLite-backed runtime lifecycle store.

use super::migrations;
use crate::app_core::{
    BackendSelection, ConversationId, MAX_RUN_EVENT_PAGE, RunCancellationReason, RunEventPage,
    RunEventRecord, RunFailure, RunId, RunInterruptionReason, RunLifecycleEvent, RunSnapshot,
    RunState,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use thiserror::Error;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_EVENT_KIND_BYTES: usize = 64;
const MAX_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;

mod audit;
mod identity;
mod model_operation;
#[cfg(unix)]
mod private;
mod projection;
mod records;
mod tool_approval;
mod tool_execution;

pub use audit::{AuditEvent, AuditMetadata, NewAuditEvent};
use audit::{row_to_audit, validate_audit_label};
pub use model_operation::{
    ModelOperationKind, ModelOperationRecord, ModelOperationState, NewModelOperation,
};
use projection::{project_run_event, project_run_snapshot, validate_event_snapshot};
pub use records::{ConversationBackend, NewRun, NewRunEvent, RunEvent, RunRecord};
pub use tool_approval::{
    MAX_TOOL_APPROVAL_TTL_MS, NewToolApproval, ToolApprovalDecision, ToolApprovalEvent,
    ToolApprovalRecord, ToolApprovalState,
};
pub use tool_execution::{
    ToolExecutionEvent, ToolExecutionOutcome, ToolExecutionPreparation, ToolExecutionRecord,
    ToolExecutionState,
};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("runtime store input is invalid: {0}")]
    InvalidInput(&'static str),
    #[error("runtime audit metadata is invalid: {0}")]
    InvalidAuditMetadata(&'static str),
    #[error("idempotency key already belongs to a different request")]
    IdempotencyConflict,
    #[error("run was not found: {0}")]
    RunNotFound(String),
    #[error("conversation was not found: {0}")]
    ConversationNotFound(String),
    #[error("run status changed concurrently: expected {expected:?}, found {found:?}")]
    UnexpectedStatus { expected: RunState, found: RunState },
    #[error("invalid run transition from {from:?} to {to:?}")]
    InvalidTransition { from: RunState, to: RunState },
    #[error("terminal run {run_id} cannot be modified")]
    TerminalRun { run_id: String },
    #[error("tool approval was not found: {0}")]
    ToolApprovalNotFound(String),
    #[error("tool approval transition conflicts with durable state")]
    ToolApprovalConflict,
    #[error("tool approval digest does not match the pending call")]
    ToolApprovalDigestMismatch,
    #[error("tool execution was not found: {0}")]
    ToolExecutionNotFound(String),
    #[error("tool execution transition conflicts with durable state")]
    ToolExecutionConflict,
    #[error("model operation was not found: {0}")]
    ModelOperationNotFound(String),
    #[error("model operation transition conflicts with durable state")]
    ModelOperationConflict,
    #[error("model operation ledger reached its bounded capacity")]
    ModelOperationCapacity,
    #[error("runtime database contains invalid data: {0}")]
    CorruptData(&'static str),
    #[error(transparent)]
    Migration(#[from] migrations::MigrationError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct RuntimeStore {
    conn: Mutex<Connection>,
    recovered_runs: usize,
    recovered_tool_executions: usize,
    recovered_model_operations: usize,
    legacy_imported: bool,
}

impl RuntimeStore {
    #[must_use]
    pub fn path_for_state_dir(state_dir: &Path) -> PathBuf {
        state_dir.join("runtime.sqlite")
    }

    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::open_internal(path, None, true)
    }

    /// Open only canonical metadata without changing interrupted run states.
    #[cfg(test)]
    pub(crate) fn open_metadata(path: &Path) -> Result<Self, StoreError> {
        Self::open_internal(path, None, false)
    }

    #[cfg(test)]
    pub(crate) fn open_with_legacy(
        path: &Path,
        legacy: Option<&super::legacy::PreparedLegacyImport>,
    ) -> Result<Self, StoreError> {
        Self::open_internal(path, legacy, true)
    }

    /// Open the edition-private runtime database for metadata-only writes.
    ///
    /// This path validates ownership, permissions, type, and symlink safety,
    /// and deliberately does not recover interrupted daemon runs.
    #[cfg(unix)]
    pub(crate) fn open_metadata_private(path: &Path) -> Result<Self, StoreError> {
        Self::finish_open(private::open(path)?, None, false)
    }

    /// Open the edition-private runtime database for daemon lifecycle work.
    #[cfg(unix)]
    pub(crate) fn open_with_legacy_private(
        path: &Path,
        legacy: Option<&super::legacy::PreparedLegacyImport>,
    ) -> Result<Self, StoreError> {
        Self::finish_open(private::open(path)?, legacy, true)
    }

    fn open_internal(
        path: &Path,
        legacy: Option<&super::legacy::PreparedLegacyImport>,
        recover_interrupted: bool,
    ) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::finish_open(Connection::open(path)?, legacy, recover_interrupted)
    }

    fn finish_open(
        mut conn: Connection,
        legacy: Option<&super::legacy::PreparedLegacyImport>,
        recover_interrupted: bool,
    ) -> Result<Self, StoreError> {
        configure(&conn)?;
        let timestamp = now();
        migrations::apply(&mut conn, &timestamp)?;
        let legacy_imported = legacy
            .map(|import| migrations::import_legacy(&mut conn, import, &timestamp))
            .transpose()?
            .unwrap_or(false);
        let recovered_runs = if recover_interrupted {
            recover_interrupted_on(&mut conn)?
        } else {
            0
        };
        let recovered_tool_executions = if recover_interrupted {
            tool_execution::recover_prepared_on(&mut conn, epoch_ms()?)?
        } else {
            0
        };
        let recovered_model_operations = if recover_interrupted {
            model_operation::recover_incomplete_on(&mut conn, epoch_ms()?)?
        } else {
            0
        };
        Ok(Self {
            conn: Mutex::new(conn),
            recovered_runs,
            recovered_tool_executions,
            recovered_model_operations,
            legacy_imported,
        })
    }

    #[must_use]
    pub const fn recovered_runs(&self) -> usize {
        self.recovered_runs
    }

    /// Number of prepared tool effects marked interrupted during this open.
    #[must_use]
    pub const fn recovered_tool_executions(&self) -> usize {
        self.recovered_tool_executions
    }

    /// Number of queued or running model operations failed during this open.
    #[must_use]
    pub const fn recovered_model_operations(&self) -> usize {
        self.recovered_model_operations
    }

    /// Whether this open committed a new legacy metadata snapshot.
    #[must_use]
    pub const fn legacy_imported(&self) -> bool {
        self.legacy_imported
    }

    pub fn create_conversation(&self, id: &ConversationId) -> Result<(), StoreError> {
        let timestamp = now();
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        conn.execute(
            "INSERT OR IGNORE INTO conversations(id, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![id.as_str(), timestamp],
        )?;
        Ok(())
    }

    pub fn set_conversation_backend(
        &self,
        conversation_id: &ConversationId,
        backend: BackendSelection,
        backend_conversation_id: Option<&str>,
    ) -> Result<ConversationBackend, StoreError> {
        if backend_conversation_id
            .is_some_and(|value| value.len() > 512 || value.chars().any(char::is_control))
        {
            return Err(StoreError::InvalidInput(
                "backend conversation id contains controls or exceeds 512 bytes",
            ));
        }
        let timestamp = now();
        let backend_name = backend_as_str(backend);
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let changed = conn.execute(
            "INSERT INTO conversation_backends(
                conversation_id, backend, backend_conversation_id, updated_at
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(conversation_id, backend) DO UPDATE SET
                backend_conversation_id=excluded.backend_conversation_id,
                updated_at=excluded.updated_at",
            params![
                conversation_id.as_str(),
                backend_name,
                backend_conversation_id,
                timestamp
            ],
        );
        match changed {
            Ok(_) => Ok(ConversationBackend {
                conversation_id: conversation_id.clone(),
                backend,
                backend_conversation_id: backend_conversation_id.map(str::to_owned),
                updated_at: timestamp,
            }),
            Err(error) if is_foreign_key(&error) => Err(StoreError::ConversationNotFound(
                conversation_id.to_string(),
            )),
            Err(error) => Err(error.into()),
        }
    }

    pub fn create_or_get_run(&self, new_run: NewRun) -> Result<RunRecord, StoreError> {
        validate_new_run(&new_run)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = run_by_idempotency_key(&tx, new_run.idempotency_key.as_str())? {
            if existing.request_digest == new_run.request_digest {
                tx.commit()?;
                return Ok(existing);
            }
            return Err(StoreError::IdempotencyConflict);
        }

        let id = RunId::new();
        let timestamp = now();
        let inserted = tx.execute(
            "INSERT INTO runs(
                id, conversation_id, idempotency_key, request_digest, status,
                next_event_sequence, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'queued', 2, ?5, ?5)",
            params![
                id.as_str(),
                new_run.conversation_id.as_ref().map(ConversationId::as_str),
                new_run.idempotency_key.as_str(),
                new_run.request_digest,
                timestamp
            ],
        );
        match inserted {
            Ok(_) => {}
            Err(error) if is_foreign_key(&error) => {
                return Err(StoreError::ConversationNotFound(
                    new_run
                        .conversation_id
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                ));
            }
            Err(error) => return Err(error.into()),
        }
        tx.execute(
            "INSERT INTO run_events(run_id, sequence, kind, payload_json, created_at)
             VALUES (?1, 1, 'run_queued', '{}', ?2)",
            params![id.as_str(), timestamp],
        )?;
        let record = run_by_id(&tx, &id)?
            .ok_or(StoreError::CorruptData("newly inserted run disappeared"))?;
        tx.commit()?;
        Ok(record)
    }

    pub fn get_run(&self, id: &RunId) -> Result<Option<RunRecord>, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        run_by_id(&conn, id)
    }

    pub fn run_snapshot(&self, id: &RunId) -> Result<Option<RunSnapshot>, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let Some(run) = run_by_id(&conn, id)? else {
            return Ok(None);
        };
        project_run_snapshot(&conn, run).map(Some)
    }

    pub fn transition_run(
        &self,
        id: &RunId,
        expected: RunState,
        next: RunState,
        event: NewRunEvent,
    ) -> Result<RunEvent, StoreError> {
        validate_event(&event)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = run_by_id(&tx, id)?.ok_or_else(|| StoreError::RunNotFound(id.to_string()))?;
        if run.status != expected {
            return Err(StoreError::UnexpectedStatus {
                expected,
                found: run.status,
            });
        }
        if run.status.is_terminal() {
            return Err(StoreError::TerminalRun {
                run_id: id.to_string(),
            });
        }
        if run.status.validate_transition(next).is_err() {
            return Err(StoreError::InvalidTransition {
                from: run.status,
                to: next,
            });
        }
        let stored = update_and_append(&tx, &run, next, event, &now())?;
        tx.commit()?;
        Ok(stored)
    }

    pub fn append_run_event(&self, id: &RunId, event: NewRunEvent) -> Result<RunEvent, StoreError> {
        validate_event(&event)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = run_by_id(&tx, id)?.ok_or_else(|| StoreError::RunNotFound(id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StoreError::TerminalRun {
                run_id: id.to_string(),
            });
        }
        let timestamp = now();
        let stored = append_event(&tx, &run, event, &timestamp)?;
        let next_sequence = sql_sequence(run.next_event_sequence + 1)?;
        tx.execute(
            "UPDATE runs SET next_event_sequence=?2, updated_at=?3 WHERE id=?1",
            params![id.as_str(), next_sequence, timestamp],
        )?;
        tx.commit()?;
        Ok(stored)
    }

    pub fn run_events(&self, id: &RunId) -> Result<Vec<RunEvent>, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let mut statement = conn.prepare(
            "SELECT run_id, sequence, kind, payload_json, created_at
             FROM run_events WHERE run_id=?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([id.as_str()], row_to_event)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Reads a bounded page using an exclusive sequence cursor.
    ///
    /// The first request supplies neither `after_sequence` nor `watermark` and
    /// receives a fixed watermark. Every continuation must supply both the
    /// returned cursor and that same watermark. Events appended later are
    /// excluded from the snapshot.
    pub fn run_events_page(
        &self,
        id: &RunId,
        after_sequence: u64,
        through_sequence: Option<u64>,
        limit: u16,
    ) -> Result<RunEventPage, StoreError> {
        if !(1..=MAX_RUN_EVENT_PAGE).contains(&limit) {
            return Err(StoreError::InvalidInput(
                "run event page limit must be within 1..=16",
            ));
        }
        if after_sequence > 0 && through_sequence.is_none() {
            return Err(StoreError::InvalidInput(
                "run event continuation requires a watermark",
            ));
        }

        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let run = run_by_id(&tx, id)?.ok_or_else(|| StoreError::RunNotFound(id.to_string()))?;
        let current_watermark = run
            .next_event_sequence
            .checked_sub(1)
            .filter(|sequence| *sequence > 0)
            .ok_or(StoreError::CorruptData("run event watermark is missing"))?;
        validate_event_snapshot(&tx, id, current_watermark)?;

        let fixed_watermark = through_sequence.unwrap_or(current_watermark);
        if fixed_watermark == 0 || fixed_watermark > current_watermark {
            return Err(StoreError::InvalidInput(
                "run event watermark is missing or in the future",
            ));
        }
        if after_sequence > fixed_watermark {
            return Err(StoreError::InvalidInput(
                "run event cursor is missing or beyond the watermark",
            ));
        }

        let after_sql = sql_sequence(after_sequence)?;
        let watermark_sql = sql_sequence(fixed_watermark)?;
        let limit_sql = i64::from(limit);
        let raw_events = {
            let mut statement = tx.prepare(
                "SELECT run_id, sequence, kind, payload_json, created_at
                 FROM run_events
                 WHERE run_id=?1 AND sequence>?2 AND sequence<=?3
                 ORDER BY sequence
                 LIMIT ?4",
            )?;
            let rows = statement.query_map(
                params![id.as_str(), after_sql, watermark_sql, limit_sql],
                row_to_event,
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let events = raw_events
            .into_iter()
            .map(|event| {
                let current_state = (event.sequence == current_watermark).then_some(run.status);
                project_run_event(event, current_state)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let next_after_sequence = events
            .last()
            .map(|event| event.sequence)
            .unwrap_or(after_sequence);
        let has_more = next_after_sequence < fixed_watermark;
        tx.commit()?;
        let page = RunEventPage {
            run_id: id.clone(),
            events,
            after_sequence,
            next_after_sequence,
            through_sequence: fixed_watermark,
            has_more,
        };
        page.validate()
            .map_err(|_| StoreError::CorruptData("run event page projection is invalid"))?;
        Ok(page)
    }

    pub fn record_audit(&self, event: NewAuditEvent) -> Result<AuditEvent, StoreError> {
        validate_audit_label(&event.action, "audit action is empty or exceeds 64 bytes")?;
        validate_audit_label(&event.outcome, "audit outcome is empty or exceeds 64 bytes")?;
        let metadata_json = serde_json::to_string(event.metadata.as_value())?;
        let timestamp = now();
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let inserted = conn.execute(
            "INSERT INTO audit_events(run_id, action, outcome, metadata_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.run_id.as_ref().map(RunId::as_str),
                event.action,
                event.outcome,
                metadata_json,
                timestamp
            ],
        );
        match inserted {
            Ok(_) => Ok(AuditEvent {
                id: conn.last_insert_rowid(),
                run_id: event.run_id,
                action: event.action,
                outcome: event.outcome,
                metadata: event.metadata.0,
                created_at: timestamp,
            }),
            Err(error) if is_foreign_key(&error) => Err(StoreError::RunNotFound(
                event.run_id.map_or_else(String::new, |id| id.to_string()),
            )),
            Err(error) => Err(error.into()),
        }
    }

    pub fn audit_events_for_run(
        &self,
        run_id: Option<&RunId>,
    ) -> Result<Vec<AuditEvent>, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let mut statement = conn.prepare(
            "SELECT id, run_id, action, outcome, metadata_json, created_at
             FROM audit_events
             WHERE run_id=?1 OR (?1 IS NULL AND run_id IS NULL)
             ORDER BY id",
        )?;
        let rows = statement.query_map([run_id.map(RunId::as_str)], row_to_audit)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn recover_interrupted(&self) -> Result<usize, StoreError> {
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        recover_interrupted_on(&mut conn)
    }
}

fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;",
    )?;
    Ok(())
}

fn recover_interrupted_on(conn: &mut Connection) -> Result<usize, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids = {
        let mut statement = tx.prepare(
            "SELECT id FROM runs
             WHERE status IN ('starting', 'running', 'cancel_requested')
             ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let timestamp = now();
    for raw_id in &ids {
        let id = parse_run_id(raw_id)?;
        let run = run_by_id(&tx, &id)?
            .ok_or(StoreError::CorruptData("recovery selected a missing run"))?;
        update_and_append(
            &tx,
            &run,
            RunState::Interrupted,
            NewRunEvent {
                kind: "run_recovered_interrupted".into(),
                payload: serde_json::json!({"reason": "daemon_restart"}),
            },
            &timestamp,
        )?;
    }
    tx.commit()?;
    Ok(ids.len())
}

fn update_and_append(
    tx: &Transaction<'_>,
    run: &RunRecord,
    next: RunState,
    event: NewRunEvent,
    timestamp: &str,
) -> Result<RunEvent, StoreError> {
    let sequence = sql_sequence(run.next_event_sequence)?;
    let next_sequence = sql_sequence(run.next_event_sequence + 1)?;
    let changed = tx.execute(
        "UPDATE runs
         SET status=?3, next_event_sequence=?4, updated_at=?5
         WHERE id=?1 AND status=?2 AND next_event_sequence=?6",
        params![
            run.id.as_str(),
            state_as_str(run.status),
            state_as_str(next),
            next_sequence,
            timestamp,
            sequence
        ],
    )?;
    if changed != 1 {
        return Err(StoreError::UnexpectedStatus {
            expected: run.status,
            found: run_by_id(tx, &run.id)?
                .ok_or_else(|| StoreError::RunNotFound(run.id.to_string()))?
                .status,
        });
    }
    append_event(tx, run, event, timestamp)
}

fn append_event(
    tx: &Transaction<'_>,
    run: &RunRecord,
    event: NewRunEvent,
    timestamp: &str,
) -> Result<RunEvent, StoreError> {
    let payload_json = serde_json::to_string(&event.payload)?;
    let sequence = sql_sequence(run.next_event_sequence)?;
    tx.execute(
        "INSERT INTO run_events(run_id, sequence, kind, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            run.id.as_str(),
            sequence,
            event.kind,
            payload_json,
            timestamp
        ],
    )?;
    Ok(RunEvent {
        run_id: run.id.clone(),
        sequence: run.next_event_sequence,
        kind: event.kind,
        payload: event.payload,
        created_at: timestamp.to_owned(),
    })
}

fn run_by_idempotency_key(conn: &Connection, key: &str) -> Result<Option<RunRecord>, StoreError> {
    conn.query_row(
        "SELECT id, conversation_id, idempotency_key, request_digest, status,
                next_event_sequence, created_at, updated_at
         FROM runs WHERE idempotency_key=?1",
        [key],
        row_to_run,
    )
    .optional()
    .map_err(Into::into)
}

fn run_by_id(conn: &Connection, id: &RunId) -> Result<Option<RunRecord>, StoreError> {
    conn.query_row(
        "SELECT id, conversation_id, idempotency_key, request_digest, status,
                next_event_sequence, created_at, updated_at
         FROM runs WHERE id=?1",
        [id.as_str()],
        row_to_run,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    let raw_id: String = row.get(0)?;
    let raw_conversation: Option<String> = row.get(1)?;
    let raw_status: String = row.get(4)?;
    let raw_sequence: i64 = row.get(5)?;
    Ok(RunRecord {
        id: parse_run_id(&raw_id).map_err(to_sql_error)?,
        conversation_id: raw_conversation
            .map(|value| parse_conversation_id(&value).map_err(to_sql_error))
            .transpose()?,
        idempotency_key: row
            .get::<_, String>(2)?
            .parse()
            .map_err(|_| to_sql_error(StoreError::CorruptData("invalid idempotency key")))?,
        request_digest: row.get(3)?,
        status: parse_run_state(&raw_status).map_err(to_sql_error)?,
        next_event_sequence: u64::try_from(raw_sequence)
            .map_err(|_| to_sql_error(StoreError::CorruptData("negative event sequence")))?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunEvent> {
    let raw_id: String = row.get(0)?;
    let sequence: i64 = row.get(1)?;
    let payload_json: String = row.get(3)?;
    Ok(RunEvent {
        run_id: parse_run_id(&raw_id).map_err(to_sql_error)?,
        sequence: u64::try_from(sequence)
            .map_err(|_| to_sql_error(StoreError::CorruptData("negative event sequence")))?,
        kind: row.get(2)?,
        payload: serde_json::from_str(&payload_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        created_at: row.get(4)?,
    })
}

fn validate_new_run(run: &NewRun) -> Result<(), StoreError> {
    if run.request_digest.len() != 64
        || !run
            .request_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StoreError::InvalidInput(
            "request digest must be 64 lower-case hexadecimal bytes",
        ));
    }
    Ok(())
}

fn state_as_str(state: RunState) -> &'static str {
    match state {
        RunState::Queued => "queued",
        RunState::Starting => "starting",
        RunState::Running => "running",
        RunState::CancelRequested => "cancel_requested",
        RunState::Succeeded => "succeeded",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
        RunState::Interrupted => "interrupted",
    }
}

fn backend_as_str(backend: BackendSelection) -> &'static str {
    match backend {
        BackendSelection::Cursor => "cursor",
        BackendSelection::Abi => "abi",
        BackendSelection::FoundationModels => "foundation_models",
        BackendSelection::Grok => "grok",
    }
}

fn parse_run_state(value: &str) -> Result<RunState, StoreError> {
    match value {
        "queued" => Ok(RunState::Queued),
        "starting" => Ok(RunState::Starting),
        "running" => Ok(RunState::Running),
        "cancel_requested" => Ok(RunState::CancelRequested),
        "succeeded" => Ok(RunState::Succeeded),
        "failed" => Ok(RunState::Failed),
        "cancelled" => Ok(RunState::Cancelled),
        "interrupted" => Ok(RunState::Interrupted),
        _ => Err(StoreError::CorruptData("unknown run status")),
    }
}

fn validate_event(event: &NewRunEvent) -> Result<(), StoreError> {
    validate_label(&event.kind, "event kind is empty or exceeds 64 bytes")?;
    if serde_json::to_vec(&event.payload)?.len() > MAX_EVENT_PAYLOAD_BYTES {
        return Err(StoreError::InvalidInput(
            "event payload exceeds 16384 bytes",
        ));
    }
    Ok(())
}

fn validate_label(value: &str, error: &'static str) -> Result<(), StoreError> {
    if value.is_empty() || value.len() > MAX_EVENT_KIND_BYTES || value.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidInput(error));
    }
    Ok(())
}

fn is_foreign_key(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
    )
}

fn parse_run_id(value: &str) -> Result<RunId, StoreError> {
    value
        .parse()
        .map_err(|_| StoreError::CorruptData("invalid run UUID"))
}

fn parse_conversation_id(value: &str) -> Result<ConversationId, StoreError> {
    value
        .parse()
        .map_err(|_| StoreError::CorruptData("invalid conversation UUID"))
}

fn sql_sequence(sequence: u64) -> Result<i64, StoreError> {
    i64::try_from(sequence)
        .map_err(|_| StoreError::CorruptData("event sequence exceeds SQLite integer range"))
}

fn to_sql_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn epoch_ms() -> Result<u64, StoreError> {
    u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| StoreError::CorruptData("system time precedes the Unix epoch"))
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
