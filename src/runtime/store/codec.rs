use super::{BackendSelection, ConversationId, RunEvent, RunId, RunRecord, RunState, StoreError};

pub(super) fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
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

pub(super) fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunEvent> {
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

pub(super) fn state_as_str(state: RunState) -> &'static str {
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

pub(super) fn backend_as_str(backend: BackendSelection) -> &'static str {
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

pub(super) fn parse_run_id(value: &str) -> Result<RunId, StoreError> {
    value
        .parse()
        .map_err(|_| StoreError::CorruptData("invalid run UUID"))
}

pub(super) fn parse_conversation_id(value: &str) -> Result<ConversationId, StoreError> {
    value
        .parse()
        .map_err(|_| StoreError::CorruptData("invalid conversation UUID"))
}

pub(super) fn sql_sequence(sequence: u64) -> Result<i64, StoreError> {
    i64::try_from(sequence)
        .map_err(|_| StoreError::CorruptData("event sequence exceeds SQLite integer range"))
}

pub(super) fn to_sql_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
