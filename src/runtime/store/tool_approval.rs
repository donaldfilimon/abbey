//! Durable, digest-bound tool approval state and append-only transitions.

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{RuntimeStore, StoreError};

/// Maximum server-issued approval lifetime: fifteen minutes.
pub const MAX_TOOL_APPROVAL_TTL_MS: u64 = 15 * 60 * 1_000;

/// Durable state of one exact tool call approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalState {
    Pending,
    Approved,
    Denied,
    Cancelled,
    Expired,
    Consumed,
}

impl ToolApprovalState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Consumed => "consumed",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "cancelled" => Ok(Self::Cancelled),
            "expired" => Ok(Self::Expired),
            "consumed" => Ok(Self::Consumed),
            _ => Err(StoreError::CorruptData("tool approval state is invalid")),
        }
    }
}

/// One durable approval record. It contains no raw tool input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalRecord {
    pub call_id: String,
    pub tool_id: String,
    pub call_digest: String,
    pub state: ToolApprovalState,
    pub decision_id: Option<String>,
    pub cancellation_id: Option<String>,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Creation request for one exact call awaiting policy approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewToolApproval {
    pub call_id: String,
    pub tool_id: String,
    pub call_digest: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

/// Explicit user decision; absence is never interpreted as approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalDecision {
    Approve,
    Deny,
}

/// One append-only approval transition for audit and deterministic reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalEvent {
    pub sequence: u64,
    pub call_id: String,
    pub state: ToolApprovalState,
    pub decision_id: Option<String>,
    pub cancellation_id: Option<String>,
    pub occurred_at_ms: u64,
}

impl RuntimeStore {
    /// Create one pending approval with a server-bounded expiry.
    pub fn create_tool_approval(
        &self,
        request: NewToolApproval,
    ) -> Result<ToolApprovalRecord, StoreError> {
        validate_id(&request.call_id, "tool approval call id is invalid")?;
        validate_id(&request.tool_id, "tool approval tool id is invalid")?;
        validate_digest(&request.call_digest)?;
        let ttl = request
            .expires_at_ms
            .checked_sub(request.created_at_ms)
            .ok_or(StoreError::InvalidInput("tool approval expiry is invalid"))?;
        if ttl == 0 || ttl > MAX_TOOL_APPROVAL_TTL_MS {
            return Err(StoreError::InvalidInput(
                "tool approval expiry exceeds the server bound",
            ));
        }
        let created_at = sql_u64(request.created_at_ms)?;
        let expires_at = sql_u64(request.expires_at_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO tool_approvals(
                call_id, tool_id, call_digest, state, decision_id, cancellation_id,
                created_at_ms, expires_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, 'pending', NULL, NULL, ?4, ?5, ?4)",
            params![
                request.call_id,
                request.tool_id,
                request.call_digest,
                created_at,
                expires_at
            ],
        )?;
        if inserted == 0 {
            return Err(StoreError::ToolApprovalConflict);
        }
        insert_event(
            &tx,
            &request.call_id,
            ToolApprovalState::Pending,
            None,
            None,
            created_at,
        )?;
        let record = read_record(&tx, &request.call_id)?
            .ok_or(StoreError::CorruptData("created tool approval is missing"))?;
        tx.commit()?;
        Ok(record)
    }

    /// Approve or deny one pending call, bound to its exact digest.
    pub fn decide_tool_approval(
        &self,
        call_id: &str,
        call_digest: &str,
        decision_id: &str,
        decision: ToolApprovalDecision,
        now_ms: u64,
    ) -> Result<ToolApprovalRecord, StoreError> {
        validate_id(call_id, "tool approval call id is invalid")?;
        validate_digest(call_digest)?;
        validate_id(decision_id, "tool approval decision id is invalid")?;
        let now = sql_u64(now_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = require_record(&tx, call_id)?;
        require_digest(&current, call_digest)?;
        if let Some(expired) = expire_if_needed(&tx, &current, now)? {
            tx.commit()?;
            return Ok(expired);
        }
        if current.state != ToolApprovalState::Pending || resolution_exists(&tx, decision_id)? {
            return Err(StoreError::ToolApprovalConflict);
        }
        let state = match decision {
            ToolApprovalDecision::Approve => ToolApprovalState::Approved,
            ToolApprovalDecision::Deny => ToolApprovalState::Denied,
        };
        tx.execute(
            "UPDATE tool_approvals
             SET state=?1, decision_id=?2, updated_at_ms=?3
             WHERE call_id=?4 AND state='pending'",
            params![state.as_str(), decision_id, now, call_id],
        )?;
        insert_event(&tx, call_id, state, Some(decision_id), None, now)?;
        let record = require_record(&tx, call_id)?;
        tx.commit()?;
        Ok(record)
    }

    /// Cancel one pending or approved call before it is consumed.
    pub fn cancel_tool_approval(
        &self,
        call_id: &str,
        cancellation_id: &str,
        now_ms: u64,
    ) -> Result<ToolApprovalRecord, StoreError> {
        validate_id(call_id, "tool approval call id is invalid")?;
        validate_id(cancellation_id, "tool cancellation id is invalid")?;
        let now = sql_u64(now_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = require_record(&tx, call_id)?;
        if let Some(expired) = expire_if_needed(&tx, &current, now)? {
            tx.commit()?;
            return Ok(expired);
        }
        if !matches!(
            current.state,
            ToolApprovalState::Pending | ToolApprovalState::Approved
        ) || resolution_exists(&tx, cancellation_id)?
        {
            return Err(StoreError::ToolApprovalConflict);
        }
        tx.execute(
            "UPDATE tool_approvals
             SET state='cancelled', cancellation_id=?1, updated_at_ms=?2
             WHERE call_id=?3 AND state IN ('pending', 'approved')",
            params![cancellation_id, now, call_id],
        )?;
        insert_event(
            &tx,
            call_id,
            ToolApprovalState::Cancelled,
            current.decision_id.as_deref(),
            Some(cancellation_id),
            now,
        )?;
        let record = require_record(&tx, call_id)?;
        tx.commit()?;
        Ok(record)
    }

    /// Atomically consume one non-expired approval exactly once.
    pub fn consume_tool_approval(
        &self,
        call_id: &str,
        call_digest: &str,
        now_ms: u64,
    ) -> Result<ToolApprovalRecord, StoreError> {
        validate_id(call_id, "tool approval call id is invalid")?;
        validate_digest(call_digest)?;
        let now = sql_u64(now_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = require_record(&tx, call_id)?;
        require_digest(&current, call_digest)?;
        if let Some(expired) = expire_if_needed(&tx, &current, now)? {
            tx.commit()?;
            return Ok(expired);
        }
        if current.state != ToolApprovalState::Approved {
            return Err(StoreError::ToolApprovalConflict);
        }
        tx.execute(
            "UPDATE tool_approvals SET state='consumed', updated_at_ms=?1
             WHERE call_id=?2 AND state='approved'",
            params![now, call_id],
        )?;
        insert_event(
            &tx,
            call_id,
            ToolApprovalState::Consumed,
            current.decision_id.as_deref(),
            None,
            now,
        )?;
        let record = require_record(&tx, call_id)?;
        tx.commit()?;
        Ok(record)
    }

    /// Read one approval, applying durable expiry first when necessary.
    pub fn tool_approval(
        &self,
        call_id: &str,
        now_ms: u64,
    ) -> Result<Option<ToolApprovalRecord>, StoreError> {
        validate_id(call_id, "tool approval call id is invalid")?;
        let now = sql_u64(now_ms)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(current) = read_record(&tx, call_id)? else {
            tx.commit()?;
            return Ok(None);
        };
        let record = expire_if_needed(&tx, &current, now)?.unwrap_or(current);
        tx.commit()?;
        Ok(Some(record))
    }

    /// Return the append-only transition history for one call.
    pub fn tool_approval_events(
        &self,
        call_id: &str,
    ) -> Result<Vec<ToolApprovalEvent>, StoreError> {
        validate_id(call_id, "tool approval call id is invalid")?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let mut statement = conn.prepare(
            "SELECT sequence, call_id, state, decision_id, cancellation_id, occurred_at_ms
             FROM tool_approval_events WHERE call_id=?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([call_id], row_to_event)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}

fn expire_if_needed(
    tx: &Transaction<'_>,
    current: &ToolApprovalRecord,
    now: i64,
) -> Result<Option<ToolApprovalRecord>, StoreError> {
    if !matches!(
        current.state,
        ToolApprovalState::Pending | ToolApprovalState::Approved
    ) || now < sql_u64(current.expires_at_ms)?
    {
        return Ok(None);
    }
    tx.execute(
        "UPDATE tool_approvals SET state='expired', updated_at_ms=?1
         WHERE call_id=?2 AND state IN ('pending', 'approved')",
        params![now, current.call_id],
    )?;
    insert_event(
        tx,
        &current.call_id,
        ToolApprovalState::Expired,
        current.decision_id.as_deref(),
        None,
        now,
    )?;
    Ok(Some(require_record(tx, &current.call_id)?))
}

fn require_record(tx: &Transaction<'_>, call_id: &str) -> Result<ToolApprovalRecord, StoreError> {
    read_record(tx, call_id)?.ok_or_else(|| StoreError::ToolApprovalNotFound(call_id.to_owned()))
}

fn read_record(
    tx: &Transaction<'_>,
    call_id: &str,
) -> Result<Option<ToolApprovalRecord>, StoreError> {
    tx.query_row(
        "SELECT call_id, tool_id, call_digest, state, decision_id, cancellation_id,
                created_at_ms, expires_at_ms, updated_at_ms
         FROM tool_approvals WHERE call_id=?1",
        [call_id],
        row_to_record,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolApprovalRecord> {
    let state = row.get::<_, String>(3)?;
    Ok(ToolApprovalRecord {
        call_id: row.get(0)?,
        tool_id: row.get(1)?,
        call_digest: row.get(2)?,
        state: ToolApprovalState::parse(&state).map_err(to_sql_error)?,
        decision_id: row.get(4)?,
        cancellation_id: row.get(5)?,
        created_at_ms: from_sql_u64(row.get(6)?)?,
        expires_at_ms: from_sql_u64(row.get(7)?)?,
        updated_at_ms: from_sql_u64(row.get(8)?)?,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolApprovalEvent> {
    let state = row.get::<_, String>(2)?;
    Ok(ToolApprovalEvent {
        sequence: from_sql_u64(row.get(0)?)?,
        call_id: row.get(1)?,
        state: ToolApprovalState::parse(&state).map_err(to_sql_error)?,
        decision_id: row.get(3)?,
        cancellation_id: row.get(4)?,
        occurred_at_ms: from_sql_u64(row.get(5)?)?,
    })
}

fn insert_event(
    tx: &Transaction<'_>,
    call_id: &str,
    state: ToolApprovalState,
    decision_id: Option<&str>,
    cancellation_id: Option<&str>,
    occurred_at_ms: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO tool_approval_events(
            call_id, state, decision_id, cancellation_id, occurred_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            call_id,
            state.as_str(),
            decision_id,
            cancellation_id,
            occurred_at_ms
        ],
    )?;
    Ok(())
}

fn resolution_exists(tx: &Transaction<'_>, id: &str) -> Result<bool, StoreError> {
    tx.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM tool_approvals WHERE decision_id=?1 OR cancellation_id=?1
         )",
        [id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn require_digest(record: &ToolApprovalRecord, digest: &str) -> Result<(), StoreError> {
    if record.call_digest == digest {
        Ok(())
    } else {
        Err(StoreError::ToolApprovalDigestMismatch)
    }
}

fn validate_id(value: &str, message: &'static str) -> Result<(), StoreError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(StoreError::InvalidInput(message));
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), StoreError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(StoreError::InvalidInput("tool approval digest is invalid"));
    }
    Ok(())
}

fn sql_u64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value)
        .map_err(|_| StoreError::InvalidInput("tool approval timestamp exceeds SQLite"))
}

fn from_sql_u64(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn to_sql_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn store() -> RuntimeStore {
        RuntimeStore::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn pending(call_id: &str, created_at_ms: u64, expires_at_ms: u64) -> NewToolApproval {
        NewToolApproval {
            call_id: call_id.into(),
            tool_id: "filesystem.write".into(),
            call_digest: DIGEST.into(),
            created_at_ms,
            expires_at_ms,
        }
    }

    #[test]
    fn approval_is_digest_bound_single_use_and_append_only() {
        let store = store();
        let created = store
            .create_tool_approval(pending("call-1", 1_000, 2_000))
            .unwrap();
        assert_eq!(created.state, ToolApprovalState::Pending);
        assert!(matches!(
            store.decide_tool_approval(
                "call-1",
                &"b".repeat(64),
                "decision-wrong",
                ToolApprovalDecision::Approve,
                1_100,
            ),
            Err(StoreError::ToolApprovalDigestMismatch)
        ));
        let approved = store
            .decide_tool_approval(
                "call-1",
                DIGEST,
                "decision-1",
                ToolApprovalDecision::Approve,
                1_100,
            )
            .unwrap();
        assert_eq!(approved.state, ToolApprovalState::Approved);
        let consumed = store
            .consume_tool_approval("call-1", DIGEST, 1_200)
            .unwrap();
        assert_eq!(consumed.state, ToolApprovalState::Consumed);
        assert!(matches!(
            store.consume_tool_approval("call-1", DIGEST, 1_300),
            Err(StoreError::ToolApprovalConflict)
        ));
        let events = store.tool_approval_events("call-1").unwrap();
        assert_eq!(
            events.iter().map(|event| event.state).collect::<Vec<_>>(),
            vec![
                ToolApprovalState::Pending,
                ToolApprovalState::Approved,
                ToolApprovalState::Consumed,
            ]
        );
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
    }

    #[test]
    fn denial_cancellation_and_expiry_are_durable_terminal_states() {
        let store = store();
        store
            .create_tool_approval(pending("call-deny", 1_000, 2_000))
            .unwrap();
        let denied = store
            .decide_tool_approval(
                "call-deny",
                DIGEST,
                "decision-deny",
                ToolApprovalDecision::Deny,
                1_100,
            )
            .unwrap();
        assert_eq!(denied.state, ToolApprovalState::Denied);
        assert!(matches!(
            store.cancel_tool_approval("call-deny", "cancel-denied", 1_200),
            Err(StoreError::ToolApprovalConflict)
        ));

        store
            .create_tool_approval(pending("call-cancel", 2_000, 3_000))
            .unwrap();
        let cancelled = store
            .cancel_tool_approval("call-cancel", "cancel-1", 2_100)
            .unwrap();
        assert_eq!(cancelled.state, ToolApprovalState::Cancelled);
        assert!(matches!(
            store.decide_tool_approval(
                "call-cancel",
                DIGEST,
                "decision-late",
                ToolApprovalDecision::Approve,
                2_200,
            ),
            Err(StoreError::ToolApprovalConflict)
        ));

        store
            .create_tool_approval(pending("call-expire", 3_000, 3_100))
            .unwrap();
        let expired = store.tool_approval("call-expire", 3_100).unwrap().unwrap();
        assert_eq!(expired.state, ToolApprovalState::Expired);
        assert!(matches!(
            store.decide_tool_approval(
                "call-expire",
                DIGEST,
                "decision-expired",
                ToolApprovalDecision::Approve,
                3_101,
            ),
            Err(StoreError::ToolApprovalConflict)
        ));
        assert_eq!(
            store
                .tool_approval_events("call-expire")
                .unwrap()
                .iter()
                .map(|event| event.state)
                .collect::<Vec<_>>(),
            vec![ToolApprovalState::Pending, ToolApprovalState::Expired]
        );
    }

    #[test]
    fn expiry_and_resolution_identifiers_are_server_bounded_and_unique() {
        let store = store();
        assert!(matches!(
            store.create_tool_approval(pending("too-long", 1, 1 + MAX_TOOL_APPROVAL_TTL_MS + 1)),
            Err(StoreError::InvalidInput(_))
        ));
        assert!(matches!(
            store.create_tool_approval(pending("zero", 1, 1)),
            Err(StoreError::InvalidInput(_))
        ));
        store
            .create_tool_approval(pending("call-a", 1_000, 2_000))
            .unwrap();
        store
            .create_tool_approval(pending("call-b", 1_000, 2_000))
            .unwrap();
        store
            .decide_tool_approval(
                "call-a",
                DIGEST,
                "resolution-1",
                ToolApprovalDecision::Approve,
                1_100,
            )
            .unwrap();
        assert!(matches!(
            store.decide_tool_approval(
                "call-b",
                DIGEST,
                "resolution-1",
                ToolApprovalDecision::Deny,
                1_100,
            ),
            Err(StoreError::ToolApprovalConflict)
        ));
        assert!(matches!(
            store.cancel_tool_approval("call-b", "resolution-1", 1_100),
            Err(StoreError::ToolApprovalConflict)
        ));
    }

    #[test]
    fn approved_state_reopens_and_can_still_be_consumed_once() {
        let root = std::env::temp_dir().join(format!(
            "abbey-tool-approval-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let database = root.join("runtime.sqlite");
        {
            let store = RuntimeStore::open(&database).unwrap();
            store
                .create_tool_approval(pending("call-reopen", 1_000, 2_000))
                .unwrap();
            store
                .decide_tool_approval(
                    "call-reopen",
                    DIGEST,
                    "decision-reopen",
                    ToolApprovalDecision::Approve,
                    1_100,
                )
                .unwrap();
        }
        {
            let store = RuntimeStore::open(&database).unwrap();
            let consumed = store
                .consume_tool_approval("call-reopen", DIGEST, 1_200)
                .unwrap();
            assert_eq!(consumed.state, ToolApprovalState::Consumed);
            assert_eq!(store.tool_approval_events("call-reopen").unwrap().len(), 3);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
