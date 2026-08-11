//! Numbered, transactional migrations for `runtime.sqlite`.

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use thiserror::Error;

pub(super) const CURRENT_SCHEMA_VERSION: i64 = 5;

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

const MIGRATION_2: &str = r#"
CREATE TABLE legacy_conversation_imports (
    snapshot_sha256 TEXT PRIMARY KEY CHECK (
        length(snapshot_sha256) = 64
        AND snapshot_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    backup_sha256 TEXT NOT NULL CHECK (backup_sha256 = snapshot_sha256),
    source_count INTEGER NOT NULL CHECK (source_count > 0),
    entry_count INTEGER NOT NULL CHECK (entry_count >= 0),
    skipped_count INTEGER NOT NULL CHECK (skipped_count >= 0),
    captured_at TEXT NOT NULL,
    imported_at TEXT NOT NULL
);

CREATE TABLE legacy_conversation_aliases (
    alias_sha256 TEXT PRIMARY KEY CHECK (
        length(alias_sha256) = 64
        AND alias_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    conversation_id TEXT NOT NULL UNIQUE,
    first_import_sha256 TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT,
    FOREIGN KEY (first_import_sha256)
        REFERENCES legacy_conversation_imports(snapshot_sha256) ON DELETE RESTRICT
);

CREATE TABLE legacy_conversation_entries (
    snapshot_sha256 TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (
        source_kind IN ('history', 'chat_id', 'by_cwd')
    ),
    observed_at TEXT,
    PRIMARY KEY (snapshot_sha256, ordinal),
    FOREIGN KEY (snapshot_sha256)
        REFERENCES legacy_conversation_imports(snapshot_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (alias_sha256)
        REFERENCES legacy_conversation_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE INDEX idx_legacy_entries_conversation
    ON legacy_conversation_entries(conversation_id, snapshot_sha256, ordinal);
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE conversation_identity_aliases (
    alias_sha256 TEXT PRIMARY KEY CHECK (
        length(alias_sha256) = 64
        AND alias_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    conversation_id TEXT NOT NULL UNIQUE,
    origin TEXT NOT NULL CHECK (origin IN ('legacy_v2', 'runtime_v3')),
    created_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE TABLE conversation_identity_scopes (
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (edition_sha256, scope_sha256),
    FOREIGN KEY (alias_sha256)
        REFERENCES conversation_identity_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE TABLE conversation_identity_commit (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL CHECK (revision > 0),
    operation TEXT NOT NULL CHECK (operation = 'save'),
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_set_sha256 TEXT NOT NULL CHECK (
        length(scope_set_sha256) = 64
        AND scope_set_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    mutation_sha256 TEXT NOT NULL CHECK (
        length(mutation_sha256) = 64
        AND mutation_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    committed_at TEXT NOT NULL,
    FOREIGN KEY (alias_sha256)
        REFERENCES conversation_identity_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

INSERT INTO conversation_identity_aliases(
    alias_sha256, conversation_id, origin, created_at
)
SELECT alias_sha256, conversation_id, 'legacy_v2', imported_at
FROM legacy_conversation_aliases;

CREATE INDEX idx_conversation_identity_scope_alias
    ON conversation_identity_scopes(alias_sha256, conversation_id);
"#;

const MIGRATION_4: &str = r#"
ALTER TABLE conversation_identity_commit RENAME TO conversation_identity_commit_v3;

CREATE TABLE conversation_identity_commit (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL CHECK (revision > 0),
    operation TEXT NOT NULL CHECK (
        operation IN ('save', 'clear_scope', 'clear_all')
    ),
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_set_sha256 TEXT NOT NULL CHECK (
        length(scope_set_sha256) = 64
        AND scope_set_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    alias_sha256 TEXT,
    conversation_id TEXT,
    mutation_sha256 TEXT NOT NULL CHECK (
        length(mutation_sha256) = 64
        AND mutation_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    committed_at TEXT NOT NULL,
    CHECK (
        (operation = 'save' AND alias_sha256 IS NOT NULL AND conversation_id IS NOT NULL)
        OR
        (operation IN ('clear_scope', 'clear_all')
            AND alias_sha256 IS NULL AND conversation_id IS NULL)
    ),
    FOREIGN KEY (alias_sha256)
        REFERENCES conversation_identity_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

INSERT INTO conversation_identity_commit(
    singleton, revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
    alias_sha256, conversation_id, mutation_sha256, committed_at
)
SELECT singleton, revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
       alias_sha256, conversation_id, mutation_sha256, committed_at
FROM conversation_identity_commit_v3;

DROP TABLE conversation_identity_commit_v3;

CREATE TABLE conversation_identity_tombstones (
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    revision INTEGER NOT NULL CHECK (revision > 0),
    cleared_at TEXT NOT NULL,
    PRIMARY KEY (edition_sha256, scope_sha256)
);

CREATE TABLE conversation_identity_clear_all (
    edition_sha256 TEXT PRIMARY KEY CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    revision INTEGER NOT NULL CHECK (revision > 0),
    cleared_at TEXT NOT NULL
);

CREATE TABLE conversation_identity_mutations (
    mutation_sha256 TEXT PRIMARY KEY CHECK (
        length(mutation_sha256) = 64
        AND mutation_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    revision INTEGER NOT NULL UNIQUE CHECK (revision > 0),
    operation TEXT NOT NULL CHECK (
        operation IN ('save', 'clear_scope', 'clear_all')
    ),
    edition_sha256 TEXT NOT NULL,
    scope_sha256 TEXT NOT NULL,
    scope_set_sha256 TEXT NOT NULL,
    alias_sha256 TEXT,
    conversation_id TEXT,
    committed_at TEXT NOT NULL,
    CHECK (
        (operation = 'save' AND alias_sha256 IS NOT NULL AND conversation_id IS NOT NULL)
        OR
        (operation IN ('clear_scope', 'clear_all')
            AND alias_sha256 IS NULL AND conversation_id IS NULL)
    )
);

INSERT INTO conversation_identity_mutations(
    mutation_sha256, revision, operation, edition_sha256, scope_sha256,
    scope_set_sha256, alias_sha256, conversation_id, committed_at
)
SELECT mutation_sha256, revision, operation, edition_sha256, scope_sha256,
       scope_set_sha256, alias_sha256, conversation_id, committed_at
FROM conversation_identity_commit;

CREATE TABLE conversation_identity_mutation_scopes (
    mutation_sha256 TEXT NOT NULL,
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    PRIMARY KEY (mutation_sha256, scope_sha256),
    FOREIGN KEY (mutation_sha256)
        REFERENCES conversation_identity_mutations(mutation_sha256) ON DELETE RESTRICT
);

INSERT INTO conversation_identity_mutation_scopes(mutation_sha256, scope_sha256)
SELECT c.mutation_sha256, s.scope_sha256
FROM conversation_identity_commit c
JOIN conversation_identity_scopes s
  ON s.edition_sha256 = c.edition_sha256 AND s.revision = c.revision
WHERE c.operation = 'save';

CREATE TABLE conversation_identity_migrated_scopes (
    edition_sha256 TEXT NOT NULL,
    scope_sha256 TEXT NOT NULL,
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (edition_sha256, scope_sha256)
);

INSERT INTO conversation_identity_migrated_scopes(
    edition_sha256, scope_sha256, alias_sha256, conversation_id, revision, updated_at
)
SELECT edition_sha256, scope_sha256, alias_sha256, conversation_id, revision, updated_at
FROM conversation_identity_scopes;
"#;

const MIGRATION_5: &str = include_str!("migration_5_tool_approvals.sql");

const MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_1),
    (2, MIGRATION_2),
    (3, MIGRATION_3),
    (4, MIGRATION_4),
    (5, MIGRATION_5),
];

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
    #[error("legacy conversation metadata import violates its stable schema")]
    LegacyInvariant,
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

pub(super) fn apply(conn: &mut Connection, now: &str) -> Result<(), MigrationError> {
    apply_set(conn, now, MIGRATIONS, CURRENT_SCHEMA_VERSION)
}

#[cfg(test)]
pub(crate) fn apply_through_v3(conn: &mut Connection, now: &str) -> Result<(), MigrationError> {
    apply_set(conn, now, &MIGRATIONS[..3], 3)
}

pub(super) fn import_legacy(
    conn: &mut Connection,
    import: &super::legacy::PreparedLegacyImport,
    now: &str,
) -> Result<bool, MigrationError> {
    let source_count =
        i64::try_from(import.source_count).map_err(|_| MigrationError::LegacyInvariant)?;
    let entry_count =
        i64::try_from(import.entries.len()).map_err(|_| MigrationError::LegacyInvariant)?;
    let skipped_count =
        i64::try_from(import.skipped_count).map_err(|_| MigrationError::LegacyInvariant)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO legacy_conversation_imports(
            snapshot_sha256, backup_sha256, source_count, entry_count, skipped_count,
            captured_at, imported_at
         ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            import.snapshot_sha256.as_str(),
            source_count,
            entry_count,
            skipped_count,
            import.captured_at.as_str(),
            now
        ],
    )?;
    if inserted == 0 {
        let existing = tx.query_row(
            "SELECT backup_sha256, source_count, entry_count, skipped_count, captured_at
             FROM legacy_conversation_imports WHERE snapshot_sha256=?1",
            [&import.snapshot_sha256],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )?;
        if existing
            != (
                import.snapshot_sha256.clone(),
                source_count,
                entry_count,
                skipped_count,
                import.captured_at.clone(),
            )
        {
            return Err(MigrationError::LegacyInvariant);
        }
        tx.commit()?;
        return Ok(false);
    }

    for (ordinal, entry) in import.entries.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).map_err(|_| MigrationError::LegacyInvariant)?;
        let initial_timestamp = entry.observed_at.as_deref().unwrap_or(&import.captured_at);
        let alias_mapping = tx
            .query_row(
                "SELECT conversation_id FROM legacy_conversation_aliases
                 WHERE alias_sha256=?1",
                [&entry.alias_sha256],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if alias_mapping
            .as_deref()
            .is_some_and(|mapped| mapped != entry.conversation_id.as_str())
        {
            return Err(MigrationError::LegacyInvariant);
        }
        let conversation_alias = tx
            .query_row(
                "SELECT alias_sha256 FROM legacy_conversation_aliases
                 WHERE conversation_id=?1",
                [entry.conversation_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if conversation_alias
            .as_deref()
            .is_some_and(|mapped| mapped != entry.alias_sha256)
        {
            return Err(MigrationError::LegacyInvariant);
        }
        let conversation_exists = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1)",
            [entry.conversation_id.as_str()],
            |row| row.get::<_, bool>(0),
        )?;
        if conversation_exists && alias_mapping.is_none() {
            return Err(MigrationError::LegacyInvariant);
        }
        tx.execute(
            "INSERT OR IGNORE INTO conversations(id, created_at, updated_at)
             VALUES (?1, ?2, ?2)",
            params![entry.conversation_id.as_str(), initial_timestamp],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO legacy_conversation_aliases(
                alias_sha256, conversation_id, first_import_sha256, imported_at
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                entry.alias_sha256.as_str(),
                entry.conversation_id.as_str(),
                import.snapshot_sha256.as_str(),
                now
            ],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO conversation_identity_aliases(
                alias_sha256, conversation_id, origin, created_at
             ) VALUES (?1, ?2, 'legacy_v2', ?3)",
            params![
                entry.alias_sha256.as_str(),
                entry.conversation_id.as_str(),
                now
            ],
        )?;
        let mapped = tx.query_row(
            "SELECT conversation_id FROM legacy_conversation_aliases WHERE alias_sha256=?1",
            [&entry.alias_sha256],
            |row| row.get::<_, String>(0),
        )?;
        if mapped != entry.conversation_id.as_str() {
            return Err(MigrationError::LegacyInvariant);
        }
        let generic_mapping = tx.query_row(
            "SELECT conversation_id FROM conversation_identity_aliases
             WHERE alias_sha256=?1",
            [&entry.alias_sha256],
            |row| row.get::<_, String>(0),
        )?;
        if generic_mapping != entry.conversation_id.as_str() {
            return Err(MigrationError::LegacyInvariant);
        }
        tx.execute(
            "INSERT INTO legacy_conversation_entries(
                snapshot_sha256, ordinal, alias_sha256, conversation_id, source_kind,
                observed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                import.snapshot_sha256.as_str(),
                ordinal,
                entry.alias_sha256.as_str(),
                entry.conversation_id.as_str(),
                entry.source_kind.as_str(),
                entry.observed_at.as_deref()
            ],
        )?;
    }

    let mut aliases = import
        .entries
        .iter()
        .map(|entry| (entry.alias_sha256.as_str(), entry.conversation_id.as_str()))
        .collect::<Vec<_>>();
    aliases.sort_unstable();
    aliases.dedup();
    for (alias_sha256, conversation_id) in aliases {
        let envelope = tx.query_row(
            "SELECT MIN(e.observed_at), MAX(e.observed_at),
                    MIN(i.captured_at), MAX(i.captured_at)
             FROM legacy_conversation_entries e
             JOIN legacy_conversation_imports i
               ON i.snapshot_sha256=e.snapshot_sha256
             WHERE e.alias_sha256=?1",
            [alias_sha256],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )?;
        let created_at = envelope
            .0
            .or(envelope.2)
            .ok_or(MigrationError::LegacyInvariant)?;
        let updated_at = envelope
            .1
            .or(envelope.3)
            .ok_or(MigrationError::LegacyInvariant)?;
        tx.execute(
            "UPDATE conversations SET created_at=?2, updated_at=?3 WHERE id=?1",
            params![conversation_id, created_at, updated_at],
        )?;
    }
    tx.commit()?;
    Ok(true)
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
            5
        );
    }

    #[test]
    fn schema_v3_copies_v2_aliases_without_plaintext_material() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_set(&mut conn, "2026-08-08T00:00:00Z", &MIGRATIONS[..2], 2).unwrap();
        let identity = crate::runtime::identity::external_identity("private-v2-id").unwrap();
        conn.execute(
            "INSERT INTO conversations(id, created_at, updated_at) VALUES (?1, 't', 't')",
            [identity.conversation_id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO legacy_conversation_imports(
                snapshot_sha256, backup_sha256, source_count, entry_count,
                skipped_count, captured_at, imported_at
             ) VALUES (?1, ?1, 1, 1, 0, 't', 't')",
            ["a".repeat(64)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO legacy_conversation_aliases(
                alias_sha256, conversation_id, first_import_sha256, imported_at
             ) VALUES (?1, ?2, ?3, 't')",
            params![
                identity.alias_sha256,
                identity.conversation_id.as_str(),
                "a".repeat(64)
            ],
        )
        .unwrap();

        apply(&mut conn, "2026-08-08T00:00:01Z").unwrap();
        let copied = conn
            .query_row(
                "SELECT conversation_id, origin FROM conversation_identity_aliases",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(copied.0, identity.conversation_id.as_str());
        assert_eq!(copied.1, "legacy_v2");
    }

    #[test]
    fn schema_v4_preserves_v3_save_marker_and_adds_clear_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_set(&mut conn, "2026-08-08T00:00:00Z", &MIGRATIONS[..3], 3).unwrap();
        let identity = crate::runtime::identity::external_identity("private-v3-id").unwrap();
        let digest = "a".repeat(64);
        conn.execute(
            "INSERT INTO conversations(id, created_at, updated_at) VALUES (?1, 't', 't')",
            [identity.conversation_id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_identity_aliases(
                alias_sha256, conversation_id, origin, created_at
             ) VALUES (?1, ?2, 'runtime_v3', 't')",
            params![identity.alias_sha256, identity.conversation_id.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_identity_commit(
                singleton, revision, operation, edition_sha256, scope_sha256,
                scope_set_sha256, alias_sha256, conversation_id, mutation_sha256, committed_at
             ) VALUES (1, 7, 'save', ?1, ?1, ?1, ?2, ?3, ?1, 't')",
            params![
                digest,
                identity.alias_sha256,
                identity.conversation_id.as_str()
            ],
        )
        .unwrap();
        apply(&mut conn, "2026-08-08T00:00:01Z").unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT revision, operation, alias_sha256 FROM conversation_identity_commit",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?
                ))
            )
            .unwrap(),
            (7, "save".to_owned(), identity.alias_sha256)
        );
        for table in [
            "conversation_identity_tombstones",
            "conversation_identity_clear_all",
        ] {
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
    }

    #[test]
    fn schema_v5_adds_digest_bound_tool_approval_and_event_ledgers() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_set(&mut conn, "2026-08-08T00:00:00Z", &MIGRATIONS[..4], 4).unwrap();
        apply(&mut conn, "2026-08-08T00:00:01Z").unwrap();
        for table in ["tool_approvals", "tool_approval_events"] {
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
        assert!(
            conn.execute(
                "INSERT INTO tool_approvals(
                    call_id, tool_id, call_digest, state, created_at_ms,
                    expires_at_ms, updated_at_ms
                 ) VALUES ('call', 'tool', ?1, 'approved', 1, 2, 1)",
                ["a".repeat(64)]
            )
            .is_err(),
            "approved state requires a durable decision id"
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
