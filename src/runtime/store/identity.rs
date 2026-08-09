//! Transactional conversation-identity metadata operations.

use super::{RuntimeStore, StoreError, now, parse_conversation_id};
use crate::app_core::ConversationId;
use crate::runtime::identity::{
    ConversationIdentityScope, IdentityCommit, IdentityOperation, edition_sha256,
    external_identity, is_lower_hex_sha256, mutation_sha256, scope_set_sha256,
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

impl RuntimeStore {
    /// Canonically commit an opaque external identity for one edition scope.
    ///
    /// The raw external id and mutation token are never written to SQLite.
    /// Repeating the same token and exact material is idempotent; reusing a
    /// token for different material fails closed.
    pub(crate) fn save_conversation_identity(
        &self,
        edition_slug: &str,
        scopes: &[ConversationIdentityScope],
        external_id: &str,
        mutation_token: &str,
    ) -> Result<IdentityCommit, StoreError> {
        let edition_sha256 = edition_sha256(edition_slug).map_err(identity_input)?;
        let external = external_identity(external_id).map_err(identity_input)?;
        let mutation_sha256 = mutation_sha256(mutation_token).map_err(identity_input)?;
        let scope_set_sha256 = scope_set_sha256(scopes).map_err(identity_input)?;
        let primary_scope = &scopes[0];
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if let Some(existing) = commit_on(&tx)?
            && existing.mutation_sha256 == mutation_sha256
        {
            if existing.edition_sha256 == edition_sha256
                && existing.scope_sha256 == primary_scope.as_sha256()
                && existing.scope_set_sha256 == scope_set_sha256
                && existing.alias_sha256 == external.alias_sha256
                && existing.conversation_id == external.conversation_id
            {
                validate_alias_provenance(&tx, &external.alias_sha256, &external.conversation_id)?;
                validate_scope_set(&tx, &edition_sha256, scopes, &external)?;
                tx.commit()?;
                return Ok(existing);
            }
            return Err(identity_conflict());
        }

        validate_alias_provenance(&tx, &external.alias_sha256, &external.conversation_id)?;
        let timestamp = now();
        tx.execute(
            "INSERT OR IGNORE INTO conversations(id, created_at, updated_at)
             VALUES (?1, ?2, ?2)",
            params![external.conversation_id.as_str(), timestamp],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO conversation_identity_aliases(
                alias_sha256, conversation_id, origin, created_at
             ) VALUES (?1, ?2, 'runtime_v3', ?3)",
            params![
                external.alias_sha256.as_str(),
                external.conversation_id.as_str(),
                timestamp
            ],
        )?;
        validate_alias_provenance(&tx, &external.alias_sha256, &external.conversation_id)?;

        let revision = next_revision(&tx)?;
        let sql_revision = i64::try_from(revision)
            .map_err(|_| StoreError::CorruptData("identity revision exceeds SQLite integer"))?;
        for scope in scopes {
            tx.execute(
                "INSERT INTO conversation_identity_scopes(
                edition_sha256, scope_sha256, alias_sha256, conversation_id,
                revision, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(edition_sha256, scope_sha256) DO UPDATE SET
                alias_sha256=excluded.alias_sha256,
                conversation_id=excluded.conversation_id,
                revision=excluded.revision,
                updated_at=excluded.updated_at",
                params![
                    edition_sha256.as_str(),
                    scope.as_sha256(),
                    external.alias_sha256.as_str(),
                    external.conversation_id.as_str(),
                    sql_revision,
                    timestamp
                ],
            )?;
        }
        tx.execute(
            "INSERT INTO conversation_identity_commit(
                singleton, revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
                alias_sha256, conversation_id, mutation_sha256, committed_at
             ) VALUES (1, ?1, 'save', ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(singleton) DO UPDATE SET
                revision=excluded.revision,
                operation=excluded.operation,
                edition_sha256=excluded.edition_sha256,
                scope_sha256=excluded.scope_sha256,
                scope_set_sha256=excluded.scope_set_sha256,
                alias_sha256=excluded.alias_sha256,
                conversation_id=excluded.conversation_id,
                mutation_sha256=excluded.mutation_sha256,
                committed_at=excluded.committed_at",
            params![
                sql_revision,
                edition_sha256.as_str(),
                primary_scope.as_sha256(),
                scope_set_sha256.as_str(),
                external.alias_sha256.as_str(),
                external.conversation_id.as_str(),
                mutation_sha256.as_str(),
                timestamp
            ],
        )?;
        let committed = commit_on(&tx)?.ok_or(StoreError::CorruptData(
            "identity commit disappeared inside transaction",
        ))?;
        tx.commit()?;
        Ok(committed)
    }

    pub(crate) fn current_identity_commit(&self) -> Result<Option<IdentityCommit>, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        commit_on(&conn)
    }
}

fn validate_alias_provenance(
    tx: &Transaction<'_>,
    alias_sha256: &str,
    conversation_id: &ConversationId,
) -> Result<(), StoreError> {
    let by_alias = tx
        .query_row(
            "SELECT conversation_id FROM conversation_identity_aliases
             WHERE alias_sha256=?1",
            [alias_sha256],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if by_alias
        .as_deref()
        .is_some_and(|mapped| mapped != conversation_id.as_str())
    {
        return Err(identity_conflict());
    }
    let by_conversation = tx
        .query_row(
            "SELECT alias_sha256 FROM conversation_identity_aliases
             WHERE conversation_id=?1",
            [conversation_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if by_conversation
        .as_deref()
        .is_some_and(|mapped| mapped != alias_sha256)
    {
        return Err(identity_conflict());
    }
    let conversation_exists = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1)",
        [conversation_id.as_str()],
        |row| row.get::<_, bool>(0),
    )?;
    if conversation_exists && by_alias.is_none() {
        return Err(identity_conflict());
    }
    Ok(())
}

fn validate_scope_set(
    tx: &Transaction<'_>,
    edition_sha256: &str,
    scopes: &[ConversationIdentityScope],
    external: &crate::runtime::identity::ExternalIdentity,
) -> Result<(), StoreError> {
    for scope in scopes {
        let stored = tx
            .query_row(
                "SELECT alias_sha256, conversation_id FROM conversation_identity_scopes
                 WHERE edition_sha256=?1 AND scope_sha256=?2",
                params![edition_sha256, scope.as_sha256()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        if stored.as_ref()
            != Some(&(
                external.alias_sha256.clone(),
                external.conversation_id.to_string(),
            ))
        {
            return Err(identity_conflict());
        }
    }
    Ok(())
}

fn next_revision(tx: &Transaction<'_>) -> Result<u64, StoreError> {
    let current = tx
        .query_row(
            "SELECT revision FROM conversation_identity_commit WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    let current = u64::try_from(current)
        .map_err(|_| StoreError::CorruptData("identity revision is negative"))?;
    current
        .checked_add(1)
        .ok_or(StoreError::CorruptData("identity revision overflow"))
}

fn commit_on(conn: &rusqlite::Connection) -> Result<Option<IdentityCommit>, StoreError> {
    conn.query_row(
        "SELECT revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
                alias_sha256, conversation_id, mutation_sha256, committed_at
         FROM conversation_identity_commit WHERE singleton=1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        },
    )
    .optional()?
    .map(|row| {
        let revision = u64::try_from(row.0)
            .map_err(|_| StoreError::CorruptData("identity revision is not positive"))?;
        if revision == 0 {
            return Err(StoreError::CorruptData("identity revision is not positive"));
        }
        let operation = IdentityOperation::parse(&row.1)
            .ok_or(StoreError::CorruptData("unknown identity operation"))?;
        for digest in [&row.2, &row.3, &row.4, &row.5, &row.7] {
            if !is_lower_hex_sha256(digest) {
                return Err(StoreError::CorruptData(
                    "identity commit digest is not lower-case SHA-256",
                ));
            }
        }
        let committed_at = DateTime::parse_from_rfc3339(&row.8)
            .map_err(|_| StoreError::CorruptData("identity commit timestamp is invalid"))?
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        if committed_at != row.8 {
            return Err(StoreError::CorruptData(
                "identity commit timestamp is not canonical UTC",
            ));
        }
        Ok(IdentityCommit {
            revision,
            operation,
            edition_sha256: row.2,
            scope_sha256: row.3,
            scope_set_sha256: row.4,
            alias_sha256: row.5,
            conversation_id: parse_conversation_id(&row.6)?,
            mutation_sha256: row.7,
            committed_at,
        })
    })
    .transpose()
}

fn identity_input(_error: crate::runtime::identity::IdentityError) -> StoreError {
    StoreError::InvalidInput("conversation identity material is invalid")
}

fn identity_conflict() -> StoreError {
    StoreError::InvalidInput("conversation identity collides with incompatible provenance")
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
