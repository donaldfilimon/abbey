//! SQLite-backed runtime lifecycle store.

use super::migrations;
use crate::app_core::{ConversationId, RunId};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;
use thiserror::Error;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_EVENT_KIND_BYTES: usize = 64;
const MAX_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;

mod audit;

pub use audit::{AuditEvent, AuditMetadata, NewAuditEvent};
use audit::{row_to_audit, validate_audit_label};

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
    #[error("run status changed concurrently: expected {expected}, found {found}")]
    UnexpectedStatus {
        expected: RunStatus,
        found: RunStatus,
    },
    #[error("invalid run transition from {from} to {to}")]
    InvalidTransition { from: RunStatus, to: RunStatus },
    #[error("terminal run {run_id} cannot be modified")]
    TerminalRun { run_id: String },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    CancelRequested,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl RunStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::CancelRequested => "cancel_requested",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    const fn permits(self, next: Self) -> bool {
        match self {
            Self::Queued => matches!(next, Self::Starting | Self::Cancelled | Self::Failed),
            Self::Starting => matches!(
                next,
                Self::Running
                    | Self::CancelRequested
                    | Self::Cancelled
                    | Self::Failed
                    | Self::Interrupted
            ),
            Self::Running => matches!(
                next,
                Self::CancelRequested
                    | Self::Succeeded
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Interrupted
            ),
            Self::CancelRequested => matches!(
                next,
                Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
            ),
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted => false,
        }
    }
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = StoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "cancel_requested" => Ok(Self::CancelRequested),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(StoreError::CorruptData("unknown run status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub conversation_id: Option<ConversationId>,
    pub idempotency_key: String,
    /// Lower-case hexadecimal SHA-256 of the canonical request envelope.
    pub request_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub id: RunId,
    pub conversation_id: Option<ConversationId>,
    pub idempotency_key: String,
    pub request_digest: String,
    pub status: RunStatus,
    pub next_event_sequence: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewRunEvent {
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunEvent {
    pub run_id: RunId,
    pub sequence: u64,
    pub kind: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBackend {
    pub conversation_id: ConversationId,
    pub backend: String,
    pub backend_conversation_id: Option<String>,
    pub updated_at: String,
}

pub struct RuntimeStore {
    conn: Mutex<Connection>,
    recovered_runs: usize,
}

impl RuntimeStore {
    #[must_use]
    pub fn path_for_state_dir(state_dir: &Path) -> PathBuf {
        state_dir.join("runtime.sqlite")
    }

    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        migrations::apply(&mut conn, &now())?;
        let recovered_runs = recover_interrupted_on(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            recovered_runs,
        })
    }

    #[must_use]
    pub const fn recovered_runs(&self) -> usize {
        self.recovered_runs
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
        backend: &str,
        backend_conversation_id: Option<&str>,
    ) -> Result<ConversationBackend, StoreError> {
        validate_label(backend, "backend is empty or exceeds 64 bytes")?;
        if backend_conversation_id.is_some_and(|value| value.len() > 512) {
            return Err(StoreError::InvalidInput(
                "backend conversation id exceeds 512 bytes",
            ));
        }
        let timestamp = now();
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
                backend,
                backend_conversation_id,
                timestamp
            ],
        );
        match changed {
            Ok(_) => Ok(ConversationBackend {
                conversation_id: conversation_id.clone(),
                backend: backend.to_owned(),
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
        if let Some(existing) = run_by_idempotency_key(&tx, &new_run.idempotency_key)? {
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
             ) VALUES (?1, ?2, ?3, ?4, 'queued', 1, ?5, ?5)",
            params![
                id.as_str(),
                new_run.conversation_id.as_ref().map(ConversationId::as_str),
                new_run.idempotency_key,
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
             VALUES (?1, 0, 'run_queued', '{}', ?2)",
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

    pub fn transition_run(
        &self,
        id: &RunId,
        expected: RunStatus,
        next: RunStatus,
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
        if !run.status.permits(next) {
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
        tx.execute(
            "UPDATE runs SET next_event_sequence=?2, updated_at=?3 WHERE id=?1",
            params![id.as_str(), run.next_event_sequence + 1, timestamp],
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
        let (sql, parameter) = if let Some(run_id) = run_id {
            (
                "SELECT id, run_id, action, outcome, metadata_json, created_at
                 FROM audit_events WHERE run_id=?1 ORDER BY id",
                Some(run_id.as_str()),
            )
        } else {
            (
                "SELECT id, run_id, action, outcome, metadata_json, created_at
                 FROM audit_events WHERE run_id IS NULL ORDER BY id",
                None,
            )
        };
        let mut statement = conn.prepare(sql)?;
        let rows = statement.query_map([parameter], row_to_audit)?;
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
            RunStatus::Interrupted,
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
    next: RunStatus,
    event: NewRunEvent,
    timestamp: &str,
) -> Result<RunEvent, StoreError> {
    let changed = tx.execute(
        "UPDATE runs
         SET status=?3, next_event_sequence=?4, updated_at=?5
         WHERE id=?1 AND status=?2 AND next_event_sequence=?6",
        params![
            run.id.as_str(),
            run.status.as_str(),
            next.as_str(),
            run.next_event_sequence + 1,
            timestamp,
            run.next_event_sequence
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
    tx.execute(
        "INSERT INTO run_events(run_id, sequence, kind, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            run.id.as_str(),
            run.next_event_sequence,
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
        idempotency_key: row.get(2)?,
        request_digest: row.get(3)?,
        status: raw_status.parse().map_err(to_sql_error)?,
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
    let key = run.idempotency_key.trim();
    if key.is_empty() || key.len() > MAX_IDEMPOTENCY_KEY_BYTES || key.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidInput(
            "idempotency key is empty, contains controls, or exceeds 256 bytes",
        ));
    }
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

fn to_sql_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
