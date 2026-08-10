use super::*;
pub(super) fn validate_alias_provenance(
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

pub(super) fn validate_scope_set(
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

pub(super) fn next_revision(tx: &Transaction<'_>) -> Result<u64, StoreError> {
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

pub(super) fn commit_on(conn: &rusqlite::Connection) -> Result<Option<IdentityCommit>, StoreError> {
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

pub(super) fn operation_name(operation: IdentityOperation) -> &'static str {
    match operation {
        IdentityOperation::Save => "save",
        IdentityOperation::ClearScope => "clear_scope",
        IdentityOperation::ClearAll => "clear_all",
    }
}

pub(super) fn revision_on<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(sql, params, |row| row.get(0)).optional()?)
}

pub(super) fn validate_clear_effect(
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

pub(super) fn validate_revision(revision: i64) -> Result<(), StoreError> {
    if revision <= 0 {
        return Err(StoreError::CorruptData("identity revision is not positive"));
    }
    Ok(())
}

pub(super) fn reject_replayed_mutation(
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

pub(super) fn insert_mutation_receipt(
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

pub(super) fn validate_current_receipt(
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

pub(super) fn identity_input(_error: crate::runtime::identity::IdentityError) -> StoreError {
    StoreError::InvalidInput("conversation identity material is invalid")
}

pub(super) fn identity_conflict() -> StoreError {
    StoreError::InvalidInput("conversation identity collides with incompatible provenance")
}
