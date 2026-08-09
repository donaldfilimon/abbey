//! Transactional conversation-identity metadata operations.

use super::{RuntimeStore, StoreError, now, parse_conversation_id};
use crate::app_core::ConversationId;
use crate::runtime::identity::{
    ConversationIdentityScope, IdentityCommit, IdentityOperation, IdentityScopeState,
    clear_all_sha256, edition_sha256, external_identity, is_lower_hex_sha256, mutation_sha256,
    scope_set_sha256,
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
                && existing.alias_sha256.as_deref() == Some(external.alias_sha256.as_str())
                && existing.conversation_id.as_ref() == Some(&external.conversation_id)
            {
                validate_alias_provenance(&tx, &external.alias_sha256, &external.conversation_id)?;
                validate_scope_set(&tx, &edition_sha256, scopes, &external)?;
                validate_current_receipt(&tx, &existing, scopes)?;
                tx.commit()?;
                return Ok(existing);
            }
            return Err(identity_conflict());
        }
        reject_replayed_mutation(&tx, &mutation_sha256)?;

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
                "DELETE FROM conversation_identity_tombstones
                 WHERE edition_sha256=?1 AND scope_sha256=?2",
                params![edition_sha256.as_str(), scope.as_sha256()],
            )?;
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
        insert_mutation_receipt(&tx, &committed, scopes)?;
        tx.commit()?;
        Ok(committed)
    }

    /// Canonically tombstone either the supplied edition scopes or every
    /// scope in the edition. Opaque aliases and conversation records remain.
    pub(crate) fn clear_conversation_identity(
        &self,
        edition_slug: &str,
        scopes: Option<&[ConversationIdentityScope]>,
        mutation_token: &str,
    ) -> Result<IdentityCommit, StoreError> {
        let edition_sha256 = edition_sha256(edition_slug).map_err(identity_input)?;
        let mutation_sha256 = mutation_sha256(mutation_token).map_err(identity_input)?;
        let (operation, primary_scope, scope_set) = match scopes {
            Some(scopes) => {
                if scopes.len() != 1 {
                    return Err(StoreError::InvalidInput(
                        "conversation identity clear requires exactly one scope",
                    ));
                }
                let scope_set = scope_set_sha256(scopes).map_err(identity_input)?;
                (
                    IdentityOperation::ClearScope,
                    scopes[0].as_sha256().to_owned(),
                    scope_set,
                )
            }
            None => {
                let clear_all = clear_all_sha256();
                (IdentityOperation::ClearAll, clear_all.clone(), clear_all)
            }
        };
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = commit_on(&tx)?
            && existing.mutation_sha256 == mutation_sha256
        {
            let matches = match scopes {
                Some(scopes) => existing.matches_clear_scopes(edition_slug, scopes, mutation_token),
                None => existing.matches_clear_all(edition_slug, mutation_token),
            };
            if matches {
                validate_clear_effect(&tx, &edition_sha256, scopes, &existing)?;
                validate_current_receipt(&tx, &existing, scopes.unwrap_or_default())?;
                tx.commit()?;
                return Ok(existing);
            }
            return Err(identity_conflict());
        }
        reject_replayed_mutation(&tx, &mutation_sha256)?;

        let revision = next_revision(&tx)?;
        let sql_revision = i64::try_from(revision)
            .map_err(|_| StoreError::CorruptData("identity revision exceeds SQLite integer"))?;
        let timestamp = now();
        match scopes {
            Some(scopes) => {
                for scope in scopes {
                    tx.execute(
                        "DELETE FROM conversation_identity_scopes
                         WHERE edition_sha256=?1 AND scope_sha256=?2",
                        params![edition_sha256.as_str(), scope.as_sha256()],
                    )?;
                    tx.execute(
                        "INSERT INTO conversation_identity_tombstones(
                            edition_sha256, scope_sha256, revision, cleared_at
                         ) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(edition_sha256, scope_sha256) DO UPDATE SET
                            revision=excluded.revision, cleared_at=excluded.cleared_at",
                        params![
                            edition_sha256.as_str(),
                            scope.as_sha256(),
                            sql_revision,
                            timestamp
                        ],
                    )?;
                }
            }
            None => {
                tx.execute(
                    "DELETE FROM conversation_identity_scopes WHERE edition_sha256=?1",
                    [edition_sha256.as_str()],
                )?;
                tx.execute(
                    "DELETE FROM conversation_identity_tombstones WHERE edition_sha256=?1",
                    [edition_sha256.as_str()],
                )?;
                tx.execute(
                    "INSERT INTO conversation_identity_clear_all(
                        edition_sha256, revision, cleared_at
                     ) VALUES (?1, ?2, ?3)
                     ON CONFLICT(edition_sha256) DO UPDATE SET
                        revision=excluded.revision, cleared_at=excluded.cleared_at",
                    params![edition_sha256.as_str(), sql_revision, timestamp],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO conversation_identity_commit(
                singleton, revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
                alias_sha256, conversation_id, mutation_sha256, committed_at
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7)
             ON CONFLICT(singleton) DO UPDATE SET
                revision=excluded.revision, operation=excluded.operation,
                edition_sha256=excluded.edition_sha256, scope_sha256=excluded.scope_sha256,
                scope_set_sha256=excluded.scope_set_sha256, alias_sha256=NULL,
                conversation_id=NULL, mutation_sha256=excluded.mutation_sha256,
                committed_at=excluded.committed_at",
            params![
                sql_revision,
                operation_name(operation),
                edition_sha256.as_str(),
                primary_scope.as_str(),
                scope_set.as_str(),
                mutation_sha256.as_str(),
                timestamp
            ],
        )?;
        let committed = commit_on(&tx)?.ok_or(StoreError::CorruptData(
            "identity clear commit disappeared inside transaction",
        ))?;
        insert_mutation_receipt(&tx, &committed, scopes.unwrap_or_default())?;
        tx.commit()?;
        Ok(committed)
    }

    pub(crate) fn verify_clear_conversation_identity(
        &self,
        edition_slug: &str,
        scopes: Option<&[ConversationIdentityScope]>,
        commit: &IdentityCommit,
    ) -> Result<(), StoreError> {
        let edition_sha256 = edition_sha256(edition_slug).map_err(identity_input)?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        validate_clear_effect(&conn, &edition_sha256, scopes, commit)
    }

    pub(crate) fn identity_scope_state(
        &self,
        edition_slug: &str,
        scope: &ConversationIdentityScope,
        mirror_candidate: Option<&str>,
    ) -> Result<IdentityScopeState, StoreError> {
        let edition_sha256 = edition_sha256(edition_slug).map_err(identity_input)?;
        let candidate = mirror_candidate
            .map(external_identity)
            .transpose()
            .map_err(identity_input)?;
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let selection = conn
            .query_row(
                "SELECT alias_sha256, conversation_id, revision, updated_at
                 FROM conversation_identity_scopes
                 WHERE edition_sha256=?1 AND scope_sha256=?2",
                params![edition_sha256.as_str(), scope.as_sha256()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let scope_clear = revision_on(
            &conn,
            "SELECT revision FROM conversation_identity_tombstones
             WHERE edition_sha256=?1 AND scope_sha256=?2",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        let all_clear = conn
            .query_row(
                "SELECT revision FROM conversation_identity_clear_all WHERE edition_sha256=?1",
                [edition_sha256.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let latest_scope_receipt = revision_on(
            &conn,
            "SELECT revision FROM conversation_identity_mutations
             WHERE operation='clear_scope' AND edition_sha256=?1 AND scope_sha256=?2
             ORDER BY revision DESC LIMIT 1",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        let latest_all_receipt = revision_on(
            &conn,
            "SELECT revision FROM conversation_identity_mutations
             WHERE operation='clear_all' AND edition_sha256=?1
             ORDER BY revision DESC LIMIT 1",
            [edition_sha256.as_str()],
        )?;
        let latest_save_receipt = revision_on(
            &conn,
            "SELECT m.revision FROM conversation_identity_mutations m
             JOIN conversation_identity_mutation_scopes s
               ON s.mutation_sha256=m.mutation_sha256
             WHERE m.operation='save' AND m.edition_sha256=?1 AND s.scope_sha256=?2
             ORDER BY m.revision DESC LIMIT 1",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        let migrated_save = revision_on(
            &conn,
            "SELECT revision FROM conversation_identity_migrated_scopes
             WHERE edition_sha256=?1 AND scope_sha256=?2",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        for revision in scope_clear
            .into_iter()
            .chain(all_clear)
            .chain(latest_scope_receipt)
            .chain(latest_all_receipt)
            .chain(latest_save_receipt)
            .chain(migrated_save)
        {
            validate_revision(revision)?;
        }
        let cleared_after = scope_clear.into_iter().chain(all_clear).max();
        let receipt_clear = latest_scope_receipt
            .into_iter()
            .chain(latest_all_receipt)
            .max();
        let selection_revision = selection.as_ref().map(|row| row.2);
        if receipt_clear.is_some_and(|receipt| {
            selection_revision.is_none_or(|selected| selected <= receipt)
                && cleared_after.is_none_or(|effect| effect < receipt)
        }) {
            return Err(StoreError::CorruptData(
                "identity clear receipt is missing its canonical effect",
            ));
        }
        let latest_clear_receipt = receipt_clear;
        let latest_save_authority = latest_save_receipt.into_iter().chain(migrated_save).max();
        if latest_save_authority.is_some_and(|saved| {
            latest_clear_receipt.is_none_or(|cleared| saved > cleared)
                && selection_revision.is_none_or(|selected| selected < saved)
        }) {
            return Err(StoreError::CorruptData(
                "identity save receipt is missing its canonical selection",
            ));
        }
        validate_effect_receipt(&conn, &edition_sha256, scope, scope_clear, all_clear)?;
        let Some((alias, conversation, revision, updated_at)) = selection else {
            return Ok(if cleared_after.is_some() {
                IdentityScopeState::Tombstoned
            } else {
                IdentityScopeState::Untracked
            });
        };
        validate_revision(revision)?;
        if !is_lower_hex_sha256(&alias) || parse_conversation_id(&conversation).is_err() {
            return Err(StoreError::CorruptData(
                "identity scope selection contains invalid opaque material",
            ));
        }
        validate_selection_receipt(
            &conn,
            &edition_sha256,
            scope,
            &alias,
            &conversation,
            revision,
            &updated_at,
        )?;
        if cleared_after.is_some_and(|cleared| cleared >= revision) {
            return Ok(IdentityScopeState::Tombstoned);
        }
        Ok(match candidate {
            Some(candidate)
                if candidate.alias_sha256 == alias
                    && candidate.conversation_id.as_str() == conversation =>
            {
                IdentityScopeState::Current
            }
            _ => IdentityScopeState::Diverged,
        })
    }

    /// Refuse a current-scope mirror deletion unless the candidate belongs
    /// uniquely to that scope (the global fallback is intentionally ignored).
    pub(crate) fn authorize_clear_scope_candidate(
        &self,
        edition_slug: &str,
        scope: &ConversationIdentityScope,
        mirror_candidate: Option<&str>,
    ) -> Result<(), StoreError> {
        match self.identity_scope_state(edition_slug, scope, mirror_candidate)? {
            IdentityScopeState::Current | IdentityScopeState::Untracked
                if mirror_candidate.is_some() => {}
            IdentityScopeState::Current => {
                return Err(StoreError::InvalidInput(
                    "canonical conversation scope is missing its mirror",
                ));
            }
            IdentityScopeState::Untracked => return Ok(()),
            IdentityScopeState::Tombstoned if mirror_candidate.is_none() => return Ok(()),
            IdentityScopeState::Tombstoned => {
                return Err(StoreError::InvalidInput(
                    "conversation mirror is superseded by a clear tombstone",
                ));
            }
            IdentityScopeState::Diverged => {
                return Err(StoreError::InvalidInput(
                    "conversation mirror diverges from the cleared scope",
                ));
            }
        }
        let Some(candidate) = mirror_candidate else {
            return Err(StoreError::InvalidInput(
                "canonical conversation scope is missing its mirror",
            ));
        };
        let edition_sha256 = edition_sha256(edition_slug).map_err(identity_input)?;
        let external = external_identity(candidate).map_err(identity_input)?;
        let global = ConversationIdentityScope::global();
        if scope == &global {
            return Ok(());
        }
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let owners = conn.query_row(
            "SELECT COUNT(*) FROM conversation_identity_scopes
             WHERE edition_sha256=?1 AND scope_sha256<>?2 AND scope_sha256<>?3
               AND alias_sha256=?4 AND conversation_id=?5",
            params![
                edition_sha256.as_str(),
                scope.as_sha256(),
                global.as_sha256(),
                external.alias_sha256.as_str(),
                external.conversation_id.as_str()
            ],
            |row| row.get::<_, i64>(0),
        )?;
        if owners != 0 {
            return Err(StoreError::InvalidInput(
                "conversation mirror is shared by another working-directory scope",
            ));
        }
        Ok(())
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
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
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
        for digest in [&row.2, &row.3, &row.4, &row.7] {
            if !is_lower_hex_sha256(digest) {
                return Err(StoreError::CorruptData(
                    "identity commit digest is not lower-case SHA-256",
                ));
            }
        }
        if row
            .5
            .as_deref()
            .is_some_and(|digest| !is_lower_hex_sha256(digest))
        {
            return Err(StoreError::CorruptData(
                "identity commit digest is not lower-case SHA-256",
            ));
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
        let has_identity = row.5.is_some() && row.6.is_some();
        if (operation == IdentityOperation::Save) != has_identity {
            return Err(StoreError::CorruptData(
                "identity commit operation payload is inconsistent",
            ));
        }
        if operation == IdentityOperation::ClearAll {
            let clear_all = clear_all_sha256();
            if row.3 != clear_all || row.4 != clear_all {
                return Err(StoreError::CorruptData(
                    "clear-all identity commit digest is inconsistent",
                ));
            }
        }
        Ok(IdentityCommit {
            revision,
            operation,
            edition_sha256: row.2,
            scope_sha256: row.3,
            scope_set_sha256: row.4,
            alias_sha256: row.5,
            conversation_id: row.6.as_deref().map(parse_conversation_id).transpose()?,
            mutation_sha256: row.7,
            committed_at,
        })
    })
    .transpose()
}

fn operation_name(operation: IdentityOperation) -> &'static str {
    match operation {
        IdentityOperation::Save => "save",
        IdentityOperation::ClearScope => "clear_scope",
        IdentityOperation::ClearAll => "clear_all",
    }
}

fn revision_on<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(sql, params, |row| row.get(0)).optional()?)
}

fn validate_clear_effect(
    conn: &rusqlite::Connection,
    edition_sha256: &str,
    scopes: Option<&[ConversationIdentityScope]>,
    commit: &IdentityCommit,
) -> Result<(), StoreError> {
    let sql_revision = i64::try_from(commit.revision)
        .map_err(|_| StoreError::CorruptData("identity revision exceeds SQLite integer"))?;
    match scopes {
        Some(scopes) if commit.operation == IdentityOperation::ClearScope => {
            for scope in scopes {
                let selection_count = conn.query_row(
                    "SELECT COUNT(*) FROM conversation_identity_scopes
                     WHERE edition_sha256=?1 AND scope_sha256=?2",
                    params![edition_sha256, scope.as_sha256()],
                    |row| row.get::<_, i64>(0),
                )?;
                let tombstone = revision_on(
                    conn,
                    "SELECT revision FROM conversation_identity_tombstones
                     WHERE edition_sha256=?1 AND scope_sha256=?2",
                    params![edition_sha256, scope.as_sha256()],
                )?;
                if selection_count != 0 || tombstone != Some(sql_revision) {
                    return Err(StoreError::CorruptData(
                        "identity clear-scope effect is inconsistent",
                    ));
                }
            }
        }
        None if commit.operation == IdentityOperation::ClearAll => {
            let selections = conn.query_row(
                "SELECT COUNT(*) FROM conversation_identity_scopes WHERE edition_sha256=?1",
                [edition_sha256],
                |row| row.get::<_, i64>(0),
            )?;
            let tombstones = conn.query_row(
                "SELECT COUNT(*) FROM conversation_identity_tombstones WHERE edition_sha256=?1",
                [edition_sha256],
                |row| row.get::<_, i64>(0),
            )?;
            let clear_all = revision_on(
                conn,
                "SELECT revision FROM conversation_identity_clear_all WHERE edition_sha256=?1",
                [edition_sha256],
            )?;
            if selections != 0 || tombstones != 0 || clear_all != Some(sql_revision) {
                return Err(StoreError::CorruptData(
                    "identity clear-all effect is inconsistent",
                ));
            }
        }
        _ => {
            return Err(StoreError::CorruptData(
                "identity clear operation does not match its scope set",
            ));
        }
    }
    Ok(())
}

fn validate_revision(revision: i64) -> Result<(), StoreError> {
    if revision <= 0 {
        return Err(StoreError::CorruptData("identity revision is not positive"));
    }
    Ok(())
}

fn validate_effect_receipt(
    conn: &rusqlite::Connection,
    edition_sha256: &str,
    scope: &ConversationIdentityScope,
    scope_clear: Option<i64>,
    all_clear: Option<i64>,
) -> Result<(), StoreError> {
    for (operation, revision, scope_sha256) in [
        ("clear_scope", scope_clear, Some(scope.as_sha256())),
        ("clear_all", all_clear, None),
    ] {
        let Some(revision) = revision else {
            continue;
        };
        let exists = match scope_sha256 {
            Some(scope_sha256) => conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_identity_mutations
                 WHERE operation=?1 AND edition_sha256=?2 AND scope_sha256=?3
                   AND revision=?4)",
                params![operation, edition_sha256, scope_sha256, revision],
                |row| row.get::<_, bool>(0),
            )?,
            None => conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_identity_mutations
                 WHERE operation=?1 AND edition_sha256=?2 AND revision=?3)",
                params![operation, edition_sha256, revision],
                |row| row.get::<_, bool>(0),
            )?,
        };
        if !exists {
            return Err(StoreError::CorruptData(
                "identity clear effect has no mutation receipt",
            ));
        }
    }
    Ok(())
}

fn validate_selection_receipt(
    conn: &rusqlite::Connection,
    edition_sha256: &str,
    scope: &ConversationIdentityScope,
    alias_sha256: &str,
    conversation_id: &str,
    revision: i64,
    updated_at: &str,
) -> Result<(), StoreError> {
    let authenticated = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM conversation_identity_mutations m
            JOIN conversation_identity_mutation_scopes s
              ON s.mutation_sha256=m.mutation_sha256
            WHERE m.operation='save' AND m.edition_sha256=?1 AND s.scope_sha256=?2
              AND m.alias_sha256=?3 AND m.conversation_id=?4 AND m.revision=?5
              AND m.committed_at=?6
            UNION ALL
            SELECT 1 FROM conversation_identity_migrated_scopes v3
            WHERE v3.edition_sha256=?1 AND v3.scope_sha256=?2
              AND v3.alias_sha256=?3 AND v3.conversation_id=?4
              AND v3.revision=?5 AND v3.updated_at=?6
         )",
        params![
            edition_sha256,
            scope.as_sha256(),
            alias_sha256,
            conversation_id,
            revision,
            updated_at
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if !authenticated {
        return Err(StoreError::CorruptData(
            "identity scope selection has no exact save receipt",
        ));
    }
    Ok(())
}

fn reject_replayed_mutation(
    conn: &rusqlite::Connection,
    mutation_sha256: &str,
) -> Result<(), StoreError> {
    let seen = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversation_identity_mutations
         WHERE mutation_sha256=?1)",
        [mutation_sha256],
        |row| row.get::<_, bool>(0),
    )?;
    if seen {
        return Err(identity_conflict());
    }
    Ok(())
}

fn insert_mutation_receipt(
    conn: &rusqlite::Connection,
    commit: &IdentityCommit,
    scopes: &[ConversationIdentityScope],
) -> Result<(), StoreError> {
    let revision = i64::try_from(commit.revision)
        .map_err(|_| StoreError::CorruptData("identity revision exceeds SQLite integer"))?;
    conn.execute(
        "INSERT INTO conversation_identity_mutations(
            mutation_sha256, revision, operation, edition_sha256, scope_sha256,
            scope_set_sha256, alias_sha256, conversation_id, committed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            commit.mutation_sha256.as_str(),
            revision,
            operation_name(commit.operation),
            commit.edition_sha256.as_str(),
            commit.scope_sha256.as_str(),
            commit.scope_set_sha256.as_str(),
            commit.alias_sha256.as_deref(),
            commit.conversation_id.as_ref().map(ConversationId::as_str),
            commit.committed_at.as_str()
        ],
    )?;
    for scope in scopes {
        conn.execute(
            "INSERT INTO conversation_identity_mutation_scopes(
                mutation_sha256, scope_sha256
             ) VALUES (?1, ?2)",
            params![commit.mutation_sha256.as_str(), scope.as_sha256()],
        )?;
    }
    Ok(())
}

fn validate_current_receipt(
    conn: &rusqlite::Connection,
    commit: &IdentityCommit,
    scopes: &[ConversationIdentityScope],
) -> Result<(), StoreError> {
    let revision = i64::try_from(commit.revision)
        .map_err(|_| StoreError::CorruptData("identity revision exceeds SQLite integer"))?;
    let count = conn.query_row(
        "SELECT COUNT(*) FROM conversation_identity_mutations
         WHERE mutation_sha256=?1 AND revision=?2 AND operation=?3
           AND edition_sha256=?4 AND scope_sha256=?5 AND scope_set_sha256=?6
           AND alias_sha256 IS ?7 AND conversation_id IS ?8 AND committed_at=?9",
        params![
            commit.mutation_sha256.as_str(),
            revision,
            operation_name(commit.operation),
            commit.edition_sha256.as_str(),
            commit.scope_sha256.as_str(),
            commit.scope_set_sha256.as_str(),
            commit.alias_sha256.as_deref(),
            commit.conversation_id.as_ref().map(ConversationId::as_str),
            commit.committed_at.as_str()
        ],
        |row| row.get::<_, i64>(0),
    )?;
    if count != 1 {
        return Err(StoreError::CorruptData(
            "identity mutation receipt is inconsistent",
        ));
    }
    let stored_scope_count = conn.query_row(
        "SELECT COUNT(*) FROM conversation_identity_mutation_scopes
         WHERE mutation_sha256=?1",
        [commit.mutation_sha256.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    if usize::try_from(stored_scope_count).ok() != Some(scopes.len()) {
        return Err(StoreError::CorruptData(
            "identity mutation receipt scope set is inconsistent",
        ));
    }
    for scope in scopes {
        let exists = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_identity_mutation_scopes
             WHERE mutation_sha256=?1 AND scope_sha256=?2)",
            params![commit.mutation_sha256.as_str(), scope.as_sha256()],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(StoreError::CorruptData(
                "identity mutation receipt scope set is inconsistent",
            ));
        }
    }
    Ok(())
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
