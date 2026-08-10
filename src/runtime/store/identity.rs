//! Transactional conversation-identity metadata operations.

use super::{RuntimeStore, StoreError, now, parse_conversation_id};
use crate::app_core::ConversationId;
use crate::runtime::IdentityScopeSelection;
use crate::runtime::identity::{
    ConversationIdentityScope, IdentityCommit, IdentityOperation, IdentityScopeState,
    clear_all_sha256, edition_sha256, external_identity, is_lower_hex_sha256, mutation_sha256,
    scope_set_sha256,
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

#[path = "identity_receipts.rs"]
mod identity_receipts;
#[path = "identity_validation.rs"]
mod identity_validation;

use identity_receipts::*;
use identity_validation::{
    effect_on, mutation_receipt_on, validate_effect_receipt, validate_mutation_receipt,
    validate_selection_receipt,
};

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

    pub(crate) fn identity_scope_selection(
        &self,
        edition_slug: &str,
        scope: &ConversationIdentityScope,
    ) -> Result<IdentityScopeSelection, StoreError> {
        let edition_sha256 = edition_sha256(edition_slug).map_err(identity_input)?;
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
        let scope_clear = effect_on(
            &conn,
            "SELECT revision, cleared_at FROM conversation_identity_tombstones
             WHERE edition_sha256=?1 AND scope_sha256=?2",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        let all_clear = conn
            .query_row(
                "SELECT revision, cleared_at FROM conversation_identity_clear_all
                 WHERE edition_sha256=?1",
                [edition_sha256.as_str()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let latest_scope_receipt = mutation_receipt_on(
            &conn,
            "SELECT m.mutation_sha256, m.revision, m.operation, m.edition_sha256,
                    m.scope_sha256, m.scope_set_sha256, m.alias_sha256,
                    m.conversation_id, m.committed_at
             FROM conversation_identity_mutations m
             LEFT JOIN conversation_identity_mutation_scopes s
               ON s.mutation_sha256=m.mutation_sha256
             WHERE m.operation='clear_scope' AND m.edition_sha256=?1
               AND (m.scope_sha256=?2 OR s.scope_sha256=?2)
             ORDER BY m.revision DESC LIMIT 1",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        let latest_all_receipt = mutation_receipt_on(
            &conn,
            "SELECT mutation_sha256, revision, operation, edition_sha256, scope_sha256,
                    scope_set_sha256, alias_sha256, conversation_id, committed_at
             FROM conversation_identity_mutations
             WHERE operation='clear_all' AND edition_sha256=?1
             ORDER BY revision DESC LIMIT 1",
            [edition_sha256.as_str()],
        )?;
        let latest_save_receipt = mutation_receipt_on(
            &conn,
            "SELECT m.mutation_sha256, m.revision, m.operation, m.edition_sha256,
                    m.scope_sha256, m.scope_set_sha256, m.alias_sha256,
                    m.conversation_id, m.committed_at
             FROM conversation_identity_mutations m
             LEFT JOIN conversation_identity_mutation_scopes s
               ON s.mutation_sha256=m.mutation_sha256
             WHERE m.operation='save' AND m.edition_sha256=?1
               AND (
                    s.scope_sha256=?2
                    OR EXISTS(
                        SELECT 1 FROM conversation_identity_migrated_scopes v3
                        WHERE v3.edition_sha256=m.edition_sha256
                          AND v3.scope_sha256=?2
                          AND v3.alias_sha256=m.alias_sha256
                          AND v3.conversation_id=m.conversation_id
                          AND v3.revision=m.revision
                          AND v3.updated_at=m.committed_at
                    )
               )
             ORDER BY m.revision DESC LIMIT 1",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        for receipt in [
            latest_scope_receipt.as_ref(),
            latest_all_receipt.as_ref(),
            latest_save_receipt.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_mutation_receipt(&conn, receipt, &edition_sha256)?;
        }
        let migrated_save = revision_on(
            &conn,
            "SELECT revision FROM conversation_identity_migrated_scopes
             WHERE edition_sha256=?1 AND scope_sha256=?2",
            params![edition_sha256.as_str(), scope.as_sha256()],
        )?;
        let scope_clear_revision = scope_clear.as_ref().map(|effect| effect.0);
        let all_clear_revision = all_clear.as_ref().map(|effect| effect.0);
        let latest_scope_receipt_revision = latest_scope_receipt
            .as_ref()
            .map(|receipt| receipt.revision);
        let latest_all_receipt_revision =
            latest_all_receipt.as_ref().map(|receipt| receipt.revision);
        let latest_save_receipt_revision =
            latest_save_receipt.as_ref().map(|receipt| receipt.revision);
        for revision in scope_clear_revision
            .into_iter()
            .chain(all_clear_revision)
            .chain(latest_scope_receipt_revision)
            .chain(latest_all_receipt_revision)
            .chain(latest_save_receipt_revision)
            .chain(migrated_save)
        {
            validate_revision(revision)?;
        }
        let cleared_after = scope_clear_revision
            .into_iter()
            .chain(all_clear_revision)
            .max();
        let receipt_clear = latest_scope_receipt_revision
            .into_iter()
            .chain(latest_all_receipt_revision)
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
        let latest_save_authority = latest_save_receipt_revision
            .into_iter()
            .chain(migrated_save)
            .max();
        if latest_save_authority.is_some_and(|saved| {
            latest_clear_receipt.is_none_or(|cleared| saved > cleared)
                && selection_revision.is_none_or(|selected| selected < saved)
        }) {
            return Err(StoreError::CorruptData(
                "identity save receipt is missing its canonical selection",
            ));
        }
        validate_effect_receipt(
            &conn,
            &edition_sha256,
            scope,
            scope_clear.as_ref(),
            all_clear.as_ref(),
        )?;
        let Some((alias, conversation, revision, updated_at)) = selection else {
            return Ok(if cleared_after.is_some() {
                IdentityScopeSelection::Tombstoned
            } else {
                IdentityScopeSelection::Untracked
            });
        };
        validate_revision(revision)?;
        if !is_lower_hex_sha256(&alias) {
            return Err(StoreError::CorruptData(
                "identity scope selection contains invalid opaque material",
            ));
        }
        let conversation_id = parse_conversation_id(&conversation)?;
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
            return Ok(IdentityScopeSelection::Tombstoned);
        }
        Ok(IdentityScopeSelection::Selected {
            alias_sha256: alias,
            conversation_id,
        })
    }

    pub(crate) fn identity_scope_state(
        &self,
        edition_slug: &str,
        scope: &ConversationIdentityScope,
        mirror_candidate: Option<&str>,
    ) -> Result<IdentityScopeState, StoreError> {
        let selection = self.identity_scope_selection(edition_slug, scope)?;
        let candidate_matches = mirror_candidate
            .map(|candidate| selection.matches_external_id(candidate))
            .transpose()
            .map_err(identity_input)?;
        Ok(match selection {
            IdentityScopeSelection::Untracked => IdentityScopeState::Untracked,
            IdentityScopeSelection::Tombstoned => IdentityScopeState::Tombstoned,
            IdentityScopeSelection::Selected { .. } if candidate_matches == Some(true) => {
                IdentityScopeState::Current
            }
            IdentityScopeSelection::Selected { .. } => IdentityScopeState::Diverged,
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

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "identity_migration_tests.rs"]
mod migration_tests;
