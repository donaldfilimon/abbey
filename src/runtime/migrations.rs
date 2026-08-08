//! Numbered, transactional migrations for `runtime.sqlite`.

use rusqlite::{Connection, OptionalExtension, Transaction};
use thiserror::Error;

pub(super) const CURRENT_SCHEMA_VERSION: i64 = 1;

const CREATE_LEDGER: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    applied_at TEXT NOT NULL
);
"#;

const MIGRATION_1: &str = r#"
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE conversation_backends (
    conversation_id TEXT NOT NULL,
    backend TEXT NOT NULL,
    backend_conversation_id TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (conversation_id, backend),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    request_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'queued', 'starting', 'running', 'cancel_requested',
            'succeeded', 'failed', 'cancelled', 'interrupted'
        )
    ),
    next_event_sequence INTEGER NOT NULL DEFAULT 1 CHECK (next_event_sequence >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE TABLE run_events (
    run_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (run_id, sequence),
    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
);

CREATE TABLE audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE SET NULL
);

CREATE INDEX idx_runs_conversation ON runs(conversation_id, created_at);
CREATE INDEX idx_runs_status ON runs(status, created_at);
CREATE INDEX idx_run_events_created ON run_events(created_at);
CREATE INDEX idx_audit_events_run ON audit_events(run_id, created_at);
"#;

const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_1)];

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("runtime database schema {found} is newer than supported schema {supported}")]
    FutureSchema { found: i64, supported: i64 },
    #[error("runtime database migration {version} failed: {source}")]
    Apply {
        version: i64,
        #[source]
        source: rusqlite::Error,
    },
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

pub(super) fn apply(conn: &mut Connection, now: &str) -> Result<(), MigrationError> {
    apply_set(conn, now, MIGRATIONS, CURRENT_SCHEMA_VERSION)
}

fn apply_set(
    conn: &mut Connection,
    now: &str,
    migrations: &[(i64, &str)],
    supported_version: i64,
) -> Result<(), MigrationError> {
    let tx = conn.transaction()?;
    tx.execute_batch(CREATE_LEDGER)?;
    tx.commit()?;

    let found = current_version(conn)?;
    if found > supported_version {
        return Err(MigrationError::FutureSchema {
            found,
            supported: supported_version,
        });
    }

    for &(version, sql) in migrations {
        if version <= found {
            continue;
        }
        let tx = conn.transaction()?;
        apply_one(&tx, version, sql, now)
            .map_err(|source| MigrationError::Apply { version, source })?;
        tx.commit()
            .map_err(|source| MigrationError::Apply { version, source })?;
    }
    Ok(())
}

fn apply_one(tx: &Transaction<'_>, version: i64, sql: &str, now: &str) -> rusqlite::Result<()> {
    tx.execute_batch(sql)?;
    tx.execute(
        "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
        (version, now),
    )?;
    Ok(())
}

fn current_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
        row.get::<_, Option<i64>>(0)
    })
    .optional()
    .map(|value| value.flatten().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, "2026-08-08T00:00:00Z").unwrap();
        apply(&mut conn, "2026-08-08T00:00:01Z").unwrap();
        assert_eq!(current_version(&conn).unwrap(), CURRENT_SCHEMA_VERSION);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_ledger() {
        let mut conn = Connection::open_in_memory().unwrap();
        let bad = "CREATE TABLE should_rollback (id INTEGER); this is not sql;";
        assert!(apply_set(&mut conn, "now", &[(1, bad)], 1).is_err());
        assert_eq!(current_version(&conn).unwrap(), 0);
        let exists = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='should_rollback')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap();
        assert!(!exists);
    }

    #[test]
    fn future_schema_is_rejected_without_mutation() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_LEDGER).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 'future')",
            [CURRENT_SCHEMA_VERSION + 1],
        )
        .unwrap();
        assert!(matches!(
            apply(&mut conn, "now"),
            Err(MigrationError::FutureSchema { .. })
        ));
        assert_eq!(current_version(&conn).unwrap(), CURRENT_SCHEMA_VERSION + 1);
    }
}
