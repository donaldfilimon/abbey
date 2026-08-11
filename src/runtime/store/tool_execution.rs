//! Crash-recoverable admission ledger for approved tool effects.
//!
//! Preparation atomically consumes the exact digest-bound approval and records
//! intent before any effect may run. A daemon reopen marks unfinished intent
//! interrupted. Retrying an ambiguous effect therefore requires a fresh call
//! and approval; this module never stores raw tool input or output.

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::tool_approval::{
    expire_if_needed, from_sql_u64, insert_event as insert_approval_event, require_digest,
    require_record as require_approval, resolution_exists, sql_u64, validate_digest, validate_id,
};
use super::{RuntimeStore, StoreError, ToolApprovalRecord, ToolApprovalState};

/// Durable lifecycle state for one admitted tool effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionState {
    Prepared,
    Interrupted,
    Succeeded,
    Failed,
}

impl ToolExecutionState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Interrupted => "interrupted",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "interrupted" => Ok(Self::Interrupted),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(StoreError::CorruptData("tool execution state is invalid")),
        }
    }
}

/// Terminal result classification recorded without raw output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionOutcome {
    Succeeded,
    Failed,
}

impl ToolExecutionOutcome {
    const fn state(self) -> ToolExecutionState {
        match self {
            Self::Succeeded => ToolExecutionState::Succeeded,
            Self::Failed => ToolExecutionState::Failed,
        }
    }
}

/// Durable raw-free execution intent or terminal result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionRecord {
    pub call_id: String,
    pub execution_id: String,
    pub state: ToolExecutionState,
    pub result_digest: Option<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

/// One append-only execution transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionEvent {
    pub sequence: u64,
    pub call_id: String,
    pub execution_id: String,
    pub state: ToolExecutionState,
    pub result_digest: Option<String>,
    pub occurred_at_ms: u64,
}

/// Result of attempting to prepare one approved exact call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolExecutionPreparation {
    Prepared(ToolExecutionRecord),
    Expired(ToolApprovalRecord),
}

impl RuntimeStore {
    /// Atomically consume one approval and persist execution intent.
    ///
    /// No effect may run before this returns `Prepared`. A prepared record is
    /// single-use; an unfinished record becomes `Interrupted` on daemon reopen.
    pub fn prepare_tool_execution(
        &self,
        call_id: &str,
        call_digest: &str,
        execution_id: &str,
        now_ms: u64,
    ) -> Result<ToolExecutionPreparation, StoreError> {
        validate_id(call_id, "tool execution call id is invalid")?;
        validate_digest(call_digest)?;
        validate_id(execution_id, "tool execution id is invalid")?;
        let now = sql_u64(now_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = require_approval(&tx, call_id)?;
        require_digest(&current, call_digest)?;
        if let Some(expired) = expire_if_needed(&tx, &current, now)? {
            tx.commit()?;
            return Ok(ToolExecutionPreparation::Expired(expired));
        }
        if current.state != ToolApprovalState::Approved
            || resolution_exists(&tx, execution_id)?
            || read_record(&tx, call_id)?.is_some()
        {
            return Err(StoreError::ToolExecutionConflict);
        }
        let changed = tx.execute(
            "UPDATE tool_approvals SET state='consumed', updated_at_ms=?1
             WHERE call_id=?2 AND state='approved'",
            params![now, call_id],
        )?;
        if changed != 1 {
            return Err(StoreError::ToolExecutionConflict);
        }
        insert_approval_event(
            &tx,
            call_id,
            ToolApprovalState::Consumed,
            current.decision_id.as_deref(),
            None,
            now,
        )?;
        tx.execute(
            "INSERT INTO tool_executions(
                call_id, execution_id, state, result_digest, started_at_ms, finished_at_ms
             ) VALUES (?1, ?2, 'prepared', NULL, ?3, NULL)",
            params![call_id, execution_id, now],
        )?;
        insert_execution_event(
            &tx,
            call_id,
            execution_id,
            ToolExecutionState::Prepared,
            None,
            now,
        )?;
        let record = require_execution(&tx, call_id)?;
        tx.commit()?;
        Ok(ToolExecutionPreparation::Prepared(record))
    }

    /// Record one terminal raw-free outcome for a prepared effect.
    pub fn complete_tool_execution(
        &self,
        call_id: &str,
        execution_id: &str,
        outcome: ToolExecutionOutcome,
        result_digest: &str,
        now_ms: u64,
    ) -> Result<ToolExecutionRecord, StoreError> {
        validate_id(call_id, "tool execution call id is invalid")?;
        validate_id(execution_id, "tool execution id is invalid")?;
        validate_digest(result_digest)?;
        let now = sql_u64(now_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = require_execution(&tx, call_id)?;
        if current.execution_id != execution_id
            || current.state != ToolExecutionState::Prepared
            || now_ms < current.started_at_ms
        {
            return Err(StoreError::ToolExecutionConflict);
        }
        let state = outcome.state();
        let changed = tx.execute(
            "UPDATE tool_executions
             SET state=?1, result_digest=?2, finished_at_ms=?3
             WHERE call_id=?4 AND execution_id=?5 AND state='prepared'",
            params![state.as_str(), result_digest, now, call_id, execution_id],
        )?;
        if changed != 1 {
            return Err(StoreError::ToolExecutionConflict);
        }
        insert_execution_event(&tx, call_id, execution_id, state, Some(result_digest), now)?;
        let record = require_execution(&tx, call_id)?;
        tx.commit()?;
        Ok(record)
    }

    /// Read one raw-free execution record.
    pub fn tool_execution(&self, call_id: &str) -> Result<Option<ToolExecutionRecord>, StoreError> {
        validate_id(call_id, "tool execution call id is invalid")?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        read_record(&conn, call_id)
    }

    /// Return append-only execution transitions for one call.
    pub fn tool_execution_events(
        &self,
        call_id: &str,
    ) -> Result<Vec<ToolExecutionEvent>, StoreError> {
        validate_id(call_id, "tool execution call id is invalid")?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let mut statement = conn.prepare(
            "SELECT sequence, call_id, execution_id, state, result_digest, occurred_at_ms
             FROM tool_execution_events WHERE call_id=?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([call_id], row_to_event)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}

pub(super) fn recover_prepared_on(conn: &mut Connection, now_ms: u64) -> Result<usize, StoreError> {
    let now = sql_u64(now_ms)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let prepared = {
        let mut statement = tx.prepare(
            "SELECT call_id, execution_id FROM tool_executions
             WHERE state='prepared' ORDER BY started_at_ms, call_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (call_id, execution_id) in &prepared {
        let changed = tx.execute(
            "UPDATE tool_executions
             SET state='interrupted', finished_at_ms=?1
             WHERE call_id=?2 AND execution_id=?3 AND state='prepared'",
            params![now, call_id, execution_id],
        )?;
        if changed != 1 {
            return Err(StoreError::ToolExecutionConflict);
        }
        insert_execution_event(
            &tx,
            call_id,
            execution_id,
            ToolExecutionState::Interrupted,
            None,
            now,
        )?;
    }
    tx.commit()?;
    Ok(prepared.len())
}

fn require_execution(conn: &Connection, call_id: &str) -> Result<ToolExecutionRecord, StoreError> {
    read_record(conn, call_id)?.ok_or_else(|| StoreError::ToolExecutionNotFound(call_id.to_owned()))
}

fn read_record(
    conn: &Connection,
    call_id: &str,
) -> Result<Option<ToolExecutionRecord>, StoreError> {
    conn.query_row(
        "SELECT call_id, execution_id, state, result_digest, started_at_ms, finished_at_ms
         FROM tool_executions WHERE call_id=?1",
        [call_id],
        row_to_record,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolExecutionRecord> {
    let state = row.get::<_, String>(2)?;
    Ok(ToolExecutionRecord {
        call_id: row.get(0)?,
        execution_id: row.get(1)?,
        state: ToolExecutionState::parse(&state).map_err(to_sql_error)?,
        result_digest: row.get(3)?,
        started_at_ms: from_sql_u64(row.get(4)?)?,
        finished_at_ms: row
            .get::<_, Option<i64>>(5)?
            .map(from_sql_u64)
            .transpose()?,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolExecutionEvent> {
    let state = row.get::<_, String>(3)?;
    Ok(ToolExecutionEvent {
        sequence: from_sql_u64(row.get(0)?)?,
        call_id: row.get(1)?,
        execution_id: row.get(2)?,
        state: ToolExecutionState::parse(&state).map_err(to_sql_error)?,
        result_digest: row.get(4)?,
        occurred_at_ms: from_sql_u64(row.get(5)?)?,
    })
}

fn insert_execution_event(
    tx: &Transaction<'_>,
    call_id: &str,
    execution_id: &str,
    state: ToolExecutionState,
    result_digest: Option<&str>,
    occurred_at_ms: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO tool_execution_events(
            call_id, execution_id, state, result_digest, occurred_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            call_id,
            execution_id,
            state.as_str(),
            result_digest,
            occurred_at_ms
        ],
    )?;
    Ok(())
}

fn to_sql_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
#[path = "tool_execution_tests.rs"]
mod tests;
