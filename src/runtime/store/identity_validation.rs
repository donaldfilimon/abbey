//! Canonical validation for conversation-identity selections and receipts.

use super::{StoreError, parse_conversation_id, validate_revision};
use crate::runtime::identity::{
    ConversationIdentityScope, IdentityOperation, clear_all_sha256, is_lower_hex_sha256,
    scope_set_sha256_from_hashes,
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};

const MAX_MIGRATED_V3_SCOPE_PERMUTATIONS: usize = 40_320;

pub(super) struct MutationReceipt {
    pub(super) mutation_sha256: String,
    pub(super) revision: i64,
    operation: IdentityOperation,
    edition_sha256: String,
    scope_sha256: String,
    scope_set_sha256: String,
    alias_sha256: Option<String>,
    conversation_id: Option<String>,
    committed_at: String,
}

pub(super) fn effect_on<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> Result<Option<(i64, String)>, StoreError> {
    Ok(conn
        .query_row(sql, params, |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?)
}

pub(super) fn mutation_receipt_on<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> Result<Option<MutationReceipt>, StoreError> {
    let row = conn
        .query_row(sql, params, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .optional()?;
    row.map(|row| {
        let operation = IdentityOperation::parse(&row.2)
            .ok_or(StoreError::CorruptData("unknown identity operation"))?;
        Ok(MutationReceipt {
            mutation_sha256: row.0,
            revision: row.1,
            operation,
            edition_sha256: row.3,
            scope_sha256: row.4,
            scope_set_sha256: row.5,
            alias_sha256: row.6,
            conversation_id: row.7,
            committed_at: row.8,
        })
    })
    .transpose()
}

pub(super) fn validate_mutation_receipt(
    conn: &rusqlite::Connection,
    receipt: &MutationReceipt,
    expected_edition_sha256: &str,
) -> Result<(), StoreError> {
    validate_revision(receipt.revision)?;
    for digest in [
        receipt.mutation_sha256.as_str(),
        receipt.edition_sha256.as_str(),
        receipt.scope_sha256.as_str(),
        receipt.scope_set_sha256.as_str(),
    ] {
        if !is_lower_hex_sha256(digest) {
            return Err(StoreError::CorruptData(
                "identity mutation receipt digest is not lower-case SHA-256",
            ));
        }
    }
    if receipt.edition_sha256 != expected_edition_sha256 {
        return Err(StoreError::CorruptData(
            "identity mutation receipt crosses edition authority",
        ));
    }
    validate_canonical_timestamp(
        &receipt.committed_at,
        "identity mutation receipt timestamp is not canonical UTC",
    )?;

    let mut statement = conn.prepare(
        "SELECT scope_sha256 FROM conversation_identity_mutation_scopes
         WHERE mutation_sha256=?1 ORDER BY rowid",
    )?;
    let stored_scopes = statement
        .query_map([receipt.mutation_sha256.as_str()], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let scope_refs = stored_scopes.iter().map(String::as_str).collect::<Vec<_>>();

    match receipt.operation {
        IdentityOperation::Save => {
            let (Some(alias_sha256), Some(conversation_id)) =
                (&receipt.alias_sha256, &receipt.conversation_id)
            else {
                return Err(StoreError::CorruptData(
                    "identity mutation receipt payload is inconsistent",
                ));
            };
            if !is_lower_hex_sha256(alias_sha256) || parse_conversation_id(conversation_id).is_err()
            {
                return Err(StoreError::CorruptData(
                    "identity save receipt payload is inconsistent",
                ));
            }
            validate_alias_mapping(conn, alias_sha256, conversation_id)?;
            if validate_migrated_v3_save_receipt(
                conn,
                receipt,
                alias_sha256,
                conversation_id,
                &scope_refs,
            )? {
                return Ok(());
            }
            if scope_refs.first().copied() != Some(receipt.scope_sha256.as_str())
                || scope_set_sha256_from_hashes(&scope_refs).map_err(|_| {
                    StoreError::CorruptData("identity save receipt scope set is invalid")
                })? != receipt.scope_set_sha256
            {
                return Err(StoreError::CorruptData(
                    "identity native-v4 save receipt scope digest is inconsistent",
                ));
            }
        }
        IdentityOperation::ClearScope => {
            if receipt.alias_sha256.is_some()
                || receipt.conversation_id.is_some()
                || scope_refs.as_slice() != [receipt.scope_sha256.as_str()]
                || scope_set_sha256_from_hashes(&scope_refs).map_err(|_| {
                    StoreError::CorruptData("identity clear-scope receipt scope set is invalid")
                })? != receipt.scope_set_sha256
            {
                return Err(StoreError::CorruptData(
                    "identity clear-scope receipt is inconsistent",
                ));
            }
        }
        IdentityOperation::ClearAll => {
            let clear_all = clear_all_sha256();
            if receipt.alias_sha256.is_some()
                || receipt.conversation_id.is_some()
                || !scope_refs.is_empty()
                || receipt.scope_sha256 != clear_all
                || receipt.scope_set_sha256 != clear_all
            {
                return Err(StoreError::CorruptData(
                    "identity clear-all receipt is inconsistent",
                ));
            }
        }
    }
    Ok(())
}

fn validate_migrated_v3_save_receipt(
    conn: &rusqlite::Connection,
    receipt: &MutationReceipt,
    alias_sha256: &str,
    conversation_id: &str,
    scope_refs: &[&str],
) -> Result<bool, StoreError> {
    let migrated_count = conn.query_row(
        "SELECT COUNT(*) FROM conversation_identity_migrated_scopes
         WHERE edition_sha256=?1 AND revision=?2 AND alias_sha256=?3
           AND conversation_id=?4 AND updated_at=?5",
        params![
            receipt.edition_sha256,
            receipt.revision,
            alias_sha256,
            conversation_id,
            receipt.committed_at
        ],
        |row| row.get::<_, i64>(0),
    )?;
    if migrated_count == 0 {
        return Ok(false);
    }
    // Migration 4 retained the exact v3 authority rows but not their original
    // order. First authenticate the complete set relationally, then authenticate
    // the stored digest against a bounded primary-first permutation search.
    let _ = scope_set_sha256_from_hashes(scope_refs).map_err(|_| {
        StoreError::CorruptData("identity migrated-v3 receipt scope membership is invalid")
    })?;
    if usize::try_from(migrated_count).ok() != Some(scope_refs.len())
        || !scope_refs.contains(&receipt.scope_sha256.as_str())
    {
        return Err(StoreError::CorruptData(
            "identity migrated-v3 receipt scope membership is inconsistent",
        ));
    }
    for scope_sha256 in scope_refs {
        let authenticated = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM conversation_identity_migrated_scopes
                WHERE edition_sha256=?1 AND scope_sha256=?2 AND revision=?3
                  AND alias_sha256=?4 AND conversation_id=?5 AND updated_at=?6
             )",
            params![
                receipt.edition_sha256,
                scope_sha256,
                receipt.revision,
                alias_sha256,
                conversation_id,
                receipt.committed_at
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if !authenticated {
            return Err(StoreError::CorruptData(
                "identity migrated-v3 receipt scope authority is inconsistent",
            ));
        }
    }
    validate_migrated_v3_scope_set_digest(receipt, scope_refs)?;
    Ok(true)
}

fn validate_migrated_v3_scope_set_digest(
    receipt: &MutationReceipt,
    scope_refs: &[&str],
) -> Result<(), StoreError> {
    let primary = receipt.scope_sha256.as_str();
    let mut remaining = scope_refs
        .iter()
        .copied()
        .filter(|scope| *scope != primary)
        .collect::<Vec<_>>();
    remaining.sort_unstable();

    let mut permutation_count = 1_usize;
    for factor in 2..=remaining.len() {
        permutation_count =
            permutation_count
                .checked_mul(factor)
                .ok_or(StoreError::CorruptData(
                    "identity migrated-v3 scope-set digest cannot be bounded",
                ))?;
        if permutation_count > MAX_MIGRATED_V3_SCOPE_PERMUTATIONS {
            return Err(StoreError::CorruptData(
                "identity migrated-v3 scope-set digest cannot be authenticated",
            ));
        }
    }

    loop {
        let ordered = std::iter::once(primary)
            .chain(remaining.iter().copied())
            .collect::<Vec<_>>();
        let digest = scope_set_sha256_from_hashes(&ordered).map_err(|_| {
            StoreError::CorruptData("identity migrated-v3 scope-set digest is invalid")
        })?;
        if digest == receipt.scope_set_sha256 {
            return Ok(());
        }
        if !advance_permutation(&mut remaining) {
            return Err(StoreError::CorruptData(
                "identity migrated-v3 scope-set digest is unauthenticated",
            ));
        }
    }
}

fn advance_permutation(values: &mut [&str]) -> bool {
    let Some(pivot) = (0..values.len().saturating_sub(1))
        .rev()
        .find(|&index| values[index] < values[index + 1])
    else {
        return false;
    };
    let Some(successor) = (pivot + 1..values.len())
        .rev()
        .find(|&index| values[pivot] < values[index])
    else {
        return false;
    };
    values.swap(pivot, successor);
    values[pivot + 1..].reverse();
    true
}

pub(super) fn validate_alias_mapping(
    conn: &rusqlite::Connection,
    alias_sha256: &str,
    conversation_id: &str,
) -> Result<(), StoreError> {
    let authenticated = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM conversation_identity_aliases a
            JOIN conversations c ON c.id=a.conversation_id
            WHERE a.alias_sha256=?1 AND a.conversation_id=?2
         )",
        params![alias_sha256, conversation_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !authenticated {
        return Err(StoreError::CorruptData(
            "identity selection has no exact alias provenance",
        ));
    }
    Ok(())
}

pub(super) fn validate_canonical_timestamp(
    timestamp: &str,
    message: &'static str,
) -> Result<(), StoreError> {
    let canonical = DateTime::parse_from_rfc3339(timestamp)
        .map_err(|_| StoreError::CorruptData(message))?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    if canonical != timestamp {
        return Err(StoreError::CorruptData(message));
    }
    Ok(())
}

pub(super) fn validate_effect_receipt(
    conn: &rusqlite::Connection,
    edition_sha256: &str,
    scope: &ConversationIdentityScope,
    scope_clear: Option<&(i64, String)>,
    all_clear: Option<&(i64, String)>,
) -> Result<(), StoreError> {
    for (operation, effect, scope_sha256) in [
        ("clear_scope", scope_clear, Some(scope.as_sha256())),
        ("clear_all", all_clear, None),
    ] {
        let Some((revision, cleared_at)) = effect else {
            continue;
        };
        validate_revision(*revision)?;
        validate_canonical_timestamp(
            cleared_at,
            "identity clear effect timestamp is not canonical UTC",
        )?;
        let exists = match scope_sha256 {
            Some(scope_sha256) => conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_identity_mutations
                 WHERE operation=?1 AND edition_sha256=?2 AND scope_sha256=?3
                   AND revision=?4 AND committed_at=?5)",
                params![
                    operation,
                    edition_sha256,
                    scope_sha256,
                    revision,
                    cleared_at
                ],
                |row| row.get::<_, bool>(0),
            )?,
            None => conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_identity_mutations
                 WHERE operation=?1 AND edition_sha256=?2 AND revision=?3
                   AND committed_at=?4)",
                params![operation, edition_sha256, revision, cleared_at],
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

pub(super) fn validate_selection_receipt(
    conn: &rusqlite::Connection,
    edition_sha256: &str,
    scope: &ConversationIdentityScope,
    alias_sha256: &str,
    conversation_id: &str,
    revision: i64,
    updated_at: &str,
) -> Result<(), StoreError> {
    validate_canonical_timestamp(
        updated_at,
        "identity scope selection timestamp is not canonical UTC",
    )?;
    validate_alias_mapping(conn, alias_sha256, conversation_id)?;
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
