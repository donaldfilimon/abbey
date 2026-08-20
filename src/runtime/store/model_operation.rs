//! Durable model download/load/unload operation state.

use rusqlite::{Connection, OptionalExtension, params};

use super::{RuntimeStore, StoreError};

const MAX_ID_BYTES: usize = 128;
const MAX_MODEL_OPERATIONS: i64 = 4_096;

/// One lifecycle operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOperationKind {
    Download,
    Load,
    Unload,
}

impl ModelOperationKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Load => "load",
            Self::Unload => "unload",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "download" => Ok(Self::Download),
            "load" => Ok(Self::Load),
            "unload" => Ok(Self::Unload),
            _ => Err(StoreError::CorruptData("invalid model operation kind")),
        }
    }
}

/// Durable lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOperationState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ModelOperationState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(StoreError::CorruptData("invalid model operation state")),
        }
    }

    const fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Inputs for a new globally single-use operation ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewModelOperation {
    pub operation_id: String,
    pub model_id: String,
    pub revision: String,
    pub kind: ModelOperationKind,
    pub created_at_ms: u64,
}

/// One sanitized durable model operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOperationRecord {
    pub operation_id: String,
    pub model_id: String,
    pub revision: String,
    pub kind: ModelOperationKind,
    pub state: ModelOperationState,
    pub progress_basis_points: u16,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl RuntimeStore {
    /// Create one queued model operation; operation IDs are never reusable.
    pub fn create_model_operation(
        &self,
        operation: NewModelOperation,
    ) -> Result<ModelOperationRecord, StoreError> {
        validate_id(&operation.operation_id)?;
        validate_id(&operation.model_id)?;
        validate_id(&operation.revision)?;
        let created = i64::try_from(operation.created_at_ms)
            .map_err(|_| StoreError::InvalidInput("model operation time exceeds i64"))?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let count = conn.query_row("SELECT COUNT(*) FROM model_operations", [], |row| {
            row.get::<_, i64>(0)
        })?;
        if count >= MAX_MODEL_OPERATIONS {
            return Err(StoreError::ModelOperationCapacity);
        }
        let changed = conn.execute(
            "INSERT INTO model_operations(
                operation_id, model_id, revision, kind, state,
                progress_basis_points, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, 'queued', 0, ?5, ?5)",
            params![
                operation.operation_id,
                operation.model_id,
                operation.revision,
                operation.kind.as_str(),
                created
            ],
        );
        match changed {
            Ok(1) => row_by_id(&conn, &operation.operation_id)?
                .ok_or(StoreError::CorruptData("new model operation disappeared")),
            Err(error) if is_unique(&error) => Err(StoreError::ModelOperationConflict),
            Err(error) => Err(error.into()),
            Ok(_) => Err(StoreError::CorruptData("model operation insert count")),
        }
    }

    /// Read one operation by its exact opaque ID.
    pub fn model_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<ModelOperationRecord>, StoreError> {
        validate_id(operation_id)?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        row_by_id(&conn, operation_id)
    }

    /// Advance an operation through its strict state machine.
    pub fn transition_model_operation(
        &self,
        operation_id: &str,
        state: ModelOperationState,
        progress_basis_points: u16,
        updated_at_ms: u64,
    ) -> Result<ModelOperationRecord, StoreError> {
        validate_id(operation_id)?;
        if progress_basis_points > 10_000 {
            return Err(StoreError::InvalidInput("model progress exceeds 10000"));
        }
        let updated = i64::try_from(updated_at_ms)
            .map_err(|_| StoreError::InvalidInput("model operation time exceeds i64"))?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction()?;
        let current = row_by_id(&tx, operation_id)?
            .ok_or_else(|| StoreError::ModelOperationNotFound(operation_id.to_owned()))?;
        if current.state.terminal() || !permitted(current.state, state) {
            return Err(StoreError::ModelOperationConflict);
        }
        if progress_basis_points < current.progress_basis_points
            || updated_at_ms < current.updated_at_ms
            || (state == ModelOperationState::Succeeded && progress_basis_points != 10_000)
        {
            return Err(StoreError::ModelOperationConflict);
        }
        tx.execute(
            "UPDATE model_operations
             SET state=?2, progress_basis_points=?3, updated_at_ms=?4
             WHERE operation_id=?1",
            params![
                operation_id,
                state.as_str(),
                i64::from(progress_basis_points),
                updated
            ],
        )?;
        let record = row_by_id(&tx, operation_id)?.ok_or(StoreError::CorruptData(
            "transitioned model operation disappeared",
        ))?;
        tx.commit()?;
        Ok(record)
    }
}

pub(super) fn recover_incomplete_on(
    conn: &mut Connection,
    updated_at_ms: u64,
) -> Result<usize, StoreError> {
    let updated = i64::try_from(updated_at_ms)
        .map_err(|_| StoreError::InvalidInput("model operation time exceeds i64"))?;
    let changed = conn.execute(
        "UPDATE model_operations
         SET state='failed', updated_at_ms=MAX(updated_at_ms, ?1)
         WHERE state IN ('queued', 'running')",
        [updated],
    )?;
    Ok(changed)
}

fn permitted(from: ModelOperationState, to: ModelOperationState) -> bool {
    matches!(
        (from, to),
        (ModelOperationState::Queued, ModelOperationState::Running)
            | (ModelOperationState::Queued, ModelOperationState::Cancelled)
            | (ModelOperationState::Queued, ModelOperationState::Failed)
            | (ModelOperationState::Running, ModelOperationState::Running)
            | (ModelOperationState::Running, ModelOperationState::Succeeded)
            | (ModelOperationState::Running, ModelOperationState::Failed)
            | (ModelOperationState::Running, ModelOperationState::Cancelled)
    )
}

fn validate_id(value: &str) -> Result<(), StoreError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(StoreError::InvalidInput(
            "invalid model operation identifier",
        ));
    }
    Ok(())
}

fn row_by_id(
    conn: &Connection,
    operation_id: &str,
) -> Result<Option<ModelOperationRecord>, StoreError> {
    conn.query_row(
        "SELECT operation_id, model_id, revision, kind, state,
                progress_basis_points, created_at_ms, updated_at_ms
         FROM model_operations WHERE operation_id=?1",
        [operation_id],
        |row| {
            let progress = row.get::<_, i64>(5)?;
            let created = row.get::<_, i64>(6)?;
            let updated = row.get::<_, i64>(7)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                progress,
                created,
                updated,
            ))
        },
    )
    .optional()?
    .map(|row| {
        Ok(ModelOperationRecord {
            operation_id: row.0,
            model_id: row.1,
            revision: row.2,
            kind: ModelOperationKind::parse(&row.3)?,
            state: ModelOperationState::parse(&row.4)?,
            progress_basis_points: u16::try_from(row.5)
                .map_err(|_| StoreError::CorruptData("invalid model progress"))?,
            created_at_ms: u64::try_from(row.6)
                .map_err(|_| StoreError::CorruptData("invalid model creation time"))?,
            updated_at_ms: u64::try_from(row.7)
                .map_err(|_| StoreError::CorruptData("invalid model update time"))?,
        })
    })
    .transpose()
}

fn is_unique(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                || code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn operations_are_single_use_monotonic_and_recovered() {
        let store = RuntimeStore::open(Path::new(":memory:")).unwrap();
        let new = NewModelOperation {
            operation_id: "download-1".to_owned(),
            model_id: "fixture-1".to_owned(),
            revision: "a".repeat(40),
            kind: ModelOperationKind::Download,
            created_at_ms: 1,
        };
        assert_eq!(
            store.create_model_operation(new.clone()).unwrap().state,
            ModelOperationState::Queued
        );
        assert!(matches!(
            store.create_model_operation(new),
            Err(StoreError::ModelOperationConflict)
        ));
        store
            .transition_model_operation("download-1", ModelOperationState::Running, 100, 2)
            .unwrap();
        assert!(matches!(
            store.transition_model_operation("download-1", ModelOperationState::Running, 99, 3),
            Err(StoreError::ModelOperationConflict)
        ));

        let mut conn = store.conn.into_inner().unwrap();
        assert_eq!(recover_incomplete_on(&mut conn, 4).unwrap(), 1);
        let recovered = row_by_id(&conn, "download-1").unwrap().unwrap();
        assert_eq!(recovered.state, ModelOperationState::Failed);
        assert_eq!(recovered.progress_basis_points, 100);
    }

    #[test]
    fn operation_ledger_refuses_growth_beyond_its_hard_capacity() {
        let store = RuntimeStore::open(Path::new(":memory:")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute_batch(
                "WITH RECURSIVE counter(value) AS (
                    SELECT 1 UNION ALL SELECT value + 1 FROM counter WHERE value < 4096
                 )
                 INSERT INTO model_operations(
                    operation_id, model_id, revision, kind, state,
                    progress_basis_points, created_at_ms, updated_at_ms
                 )
                 SELECT printf('operation-%d', value), 'fixture',
                        '0123456789abcdef0123456789abcdef01234567',
                        'download', 'succeeded', 10000, value, value
                 FROM counter;",
            )
            .unwrap();
        }
        assert!(matches!(
            store.create_model_operation(NewModelOperation {
                operation_id: "one-too-many".to_owned(),
                model_id: "fixture".to_owned(),
                revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                kind: ModelOperationKind::Download,
                created_at_ms: 4097,
            }),
            Err(StoreError::ModelOperationCapacity)
        ));
    }
}
