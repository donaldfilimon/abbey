#![allow(
    dead_code,
    clippy::too_many_arguments,
    clippy::collapsible_if,
    reason = "protocol-v4 storage for Task 2; daemon dispatch follows"
)]
//! Focused v4 conversation/message storage for durable assistant per Desktop Plan Task 2.
//! Additive tables, atomic admit, append content, recovery interrupted, no legacy cascade.
//! Bounded, isolated. File kept small.

use super::{RuntimeStore, StoreError, sql_sequence};
use crate::app_core::{
    ConversationId, MAX_V4_DELTA_BYTES, MAX_V4_INPUT_BYTES, MAX_V4_PAGE_BYTES, RunState,
    V4Conversation, V4ConversationPage, V4ConversationPatch, V4ConversationQuery, V4Message,
    V4MessagePage, V4MessageQuery, V4MessageRole,
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

#[path = "v4_context.rs"]
mod context;

#[path = "v4_admin.rs"]
mod admin;
#[path = "v4_turn.rs"]
mod turn;

pub(super) use turn::sync_message_state;

impl RuntimeStore {
    pub(crate) fn v4_create_conversation(
        &self,
        title: &str,
        now_ms: u64,
    ) -> Result<V4Conversation, StoreError> {
        let id = ConversationId::new();
        insert_conversation(self, &id, title, true, now_ms, false)
    }

    pub(crate) fn v4_note_legacy_shell(
        &self,
        id: &ConversationId,
        title: &str,
        now_ms: u64,
    ) -> Result<V4Conversation, StoreError> {
        insert_conversation(self, id, title, false, now_ms, true)
    }

    pub(crate) fn v4_conversation(
        &self,
        id: &ConversationId,
    ) -> Result<V4Conversation, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        conversation_row(&conn, id)?.ok_or(StoreError::AssistantNotFound)
    }

    pub(crate) fn v4_list_conversations(
        &self,
        query: &V4ConversationQuery,
    ) -> Result<V4ConversationPage, StoreError> {
        query
            .validate()
            .map_err(|_| StoreError::InvalidInput("conversation query invalid"))?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let listed = list_conversations(&tx, query)?;
        tx.commit()?;
        Ok(listed)
    }

    pub(crate) fn v4_search_conversations(
        &self,
        _query: &V4ConversationQuery,
    ) -> Result<V4ConversationPage, StoreError> {
        // search uses list for now; full in later
        self.v4_list_conversations(_query)
    }

    pub(crate) fn v4_patch_conversation(
        &self,
        id: &ConversationId,
        patch: &V4ConversationPatch,
        now_ms: u64,
    ) -> Result<V4Conversation, StoreError> {
        id.to_string(); // validate
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        touch_conversation(&tx, id)?;
        if let Some(t) = &patch.title {
            tx.execute("UPDATE assistant_conversations SET title=?2, updated_at_ms=?3 WHERE id=?1 AND deleted=0", params![id.as_str(), t, ms(now_ms)?])?;
        }
        if let Some(a) = patch.archived {
            tx.execute("UPDATE assistant_conversations SET archived=?2, updated_at_ms=?3 WHERE id=?1 AND deleted=0", params![id.as_str(), i64::from(a), ms(now_ms)?])?;
        }
        if let Some(p) = patch.pinned {
            tx.execute("UPDATE assistant_conversations SET pinned=?2, updated_at_ms=?3 WHERE id=?1 AND deleted=0", params![id.as_str(), i64::from(p), ms(now_ms)?])?;
        }
        let c = required_conversation(&tx, id)?;
        tx.commit()?;
        Ok(c)
    }

    pub(crate) fn v4_rename_conversation(
        &self,
        id: &ConversationId,
        title: &str,
        now_ms: u64,
    ) -> Result<V4Conversation, StoreError> {
        let patch = V4ConversationPatch {
            title: Some(title.to_owned()),
            archived: None,
            pinned: None,
        };
        self.v4_patch_conversation(id, &patch, now_ms)
    }

    pub(crate) fn v4_archive_conversation(
        &self,
        id: &ConversationId,
        now_ms: u64,
    ) -> Result<V4Conversation, StoreError> {
        let patch = V4ConversationPatch {
            title: None,
            archived: Some(true),
            pinned: None,
        };
        self.v4_patch_conversation(id, &patch, now_ms)
    }

    pub(crate) fn v4_set_pinned(
        &self,
        id: &ConversationId,
        pinned: bool,
        now_ms: u64,
    ) -> Result<V4Conversation, StoreError> {
        let patch = V4ConversationPatch {
            title: None,
            archived: None,
            pinned: Some(pinned),
        };
        self.v4_patch_conversation(id, &patch, now_ms)
    }

    pub(crate) fn v4_delete_conversation(
        &self,
        id: &ConversationId,
        now_ms: u64,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        touch_conversation(&tx, id)?;
        if active_runs(&tx, id)? > 0 {
            return Err(StoreError::AssistantConflict);
        }
        tx.execute(
            "DELETE FROM assistant_content_events WHERE conversation_id=?1",
            [id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM assistant_messages WHERE conversation_id=?1",
            [id.as_str()],
        )?;
        tx.execute(
            "UPDATE assistant_conversations SET deleted=1, updated_at_ms=?2 WHERE id=?1",
            params![id.as_str(), ms(now_ms)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn v4_messages(&self, query: &V4MessageQuery) -> Result<V4MessagePage, StoreError> {
        query
            .validate()
            .map_err(|_| StoreError::InvalidInput("message query invalid"))?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        if conversation_row(&tx, &query.conversation_id)?.is_none() {
            return Err(StoreError::AssistantNotFound);
        }
        let max = scalar(
            &tx,
            "SELECT COALESCE(MAX(sequence), 0) FROM assistant_messages WHERE conversation_id=?1",
            [query.conversation_id.as_str()],
        )?;
        let mut messages = message_page(
            &tx,
            &query.conversation_id,
            query.after,
            max,
            query.limit.saturating_add(1),
        )?;
        let has_more = messages.len() > usize::from(query.limit);
        messages.truncate(usize::from(query.limit));
        let next_after = has_more
            .then(|| messages.last().map(|message| message.sequence))
            .flatten();
        tx.commit()?;
        let page = fit_message_page(V4MessagePage {
            conversation_id: query.conversation_id.clone(),
            messages,
            next_after,
        })?;
        page.validate()
            .map_err(|_| StoreError::CorruptData("v4 message page invalid"))?;
        Ok(page)
    }
}

fn insert_conversation(
    store: &RuntimeStore,
    id: &ConversationId,
    title: &str,
    imported: bool,
    now_ms: u64,
    require: bool,
) -> Result<V4Conversation, StoreError> {
    let mut conn = store.conn.lock().expect("runtime sqlite lock poisoned");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = stamp(now_ms)?;
    if require {
        let known: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM conversations WHERE id=?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if known.is_none() {
            return Err(StoreError::ConversationNotFound(id.to_string()));
        }
    } else {
        tx.execute(
            "INSERT OR IGNORE INTO conversations(id, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![id.as_str(), stamp],
        )?;
    }
    let occ: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM assistant_conversations WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    if occ.is_some() {
        return Err(StoreError::AssistantConflict);
    }
    let seq = scalar(
        &tx,
        "SELECT COALESCE(MAX(sequence), 0) FROM assistant_conversations",
        [],
    )? + 1;
    tx.execute(
        "INSERT INTO assistant_conversations(id, sequence, title, archived, pinned, imported_history, deleted, created_at_ms, updated_at_ms) VALUES (?1, ?2, ?3, 0, 0, ?4, 0, ?5, ?5)",
        params![id.as_str(), sql_sequence(seq)?, title, i64::from(imported), ms(now_ms)?],
    )?;
    let c = required_conversation(&tx, id)?;
    tx.commit()?;
    Ok(c)
}

fn list_conversations(
    tx: &Transaction<'_>,
    q: &V4ConversationQuery,
) -> Result<V4ConversationPage, StoreError> {
    let _max = scalar(
        tx,
        "SELECT COALESCE(MAX(sequence), 0) FROM assistant_conversations",
        [],
    )?;
    let pat = q.search.as_deref().map(like_contains);
    let after_seq: i64 = if let Some(ref after) = q.after {
        tx.query_row(
            "SELECT sequence FROM assistant_conversations WHERE id=?1 AND deleted=0 AND archived=?2 AND (?3 IS NULL OR title LIKE ?3 ESCAPE '\\')",
            params![after.as_str(), i64::from(q.archived), pat],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::InvalidInput("conversation cursor invalid"))?
    } else {
        0
    };
    let fetch = q.limit.saturating_add(1);
    let mut stmt = tx.prepare(
        "SELECT id FROM assistant_conversations WHERE deleted=0 AND sequence > ?1 AND (?2 IS NULL OR title LIKE ?2 ESCAPE '\\') AND archived=?3 ORDER BY sequence LIMIT ?4"
    )?;
    let ids: Vec<String> = stmt
        .query_map(
            params![after_seq, pat, i64::from(q.archived), i64::from(fetch)],
            |r| r.get(0),
        )?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    let mut convs = vec![];
    for s in ids {
        let cid: ConversationId = s
            .parse()
            .map_err(|_| StoreError::CorruptData("bad conv id"))?;
        convs.push(required_conversation(tx, &cid)?);
    }
    let has_more = convs.len() > usize::from(q.limit);
    convs.truncate(usize::from(q.limit));
    let next = has_more
        .then(|| convs.last().map(|c| c.conversation_id.clone()))
        .flatten();
    let page = V4ConversationPage {
        conversations: convs,
        next_after: next,
    };
    page.validate()
        .map_err(|_| StoreError::CorruptData("v4 conv page invalid"))?;
    Ok(page)
}

fn required_conversation(
    conn: &rusqlite::Connection,
    id: &ConversationId,
) -> Result<V4Conversation, StoreError> {
    conversation_row(conn, id)?.ok_or(StoreError::AssistantNotFound)
}

fn conversation_row(
    conn: &rusqlite::Connection,
    id: &ConversationId,
) -> Result<Option<V4Conversation>, StoreError> {
    let row = conn.query_row(
        "SELECT id, title, archived, pinned, created_at_ms, updated_at_ms, sequence, (SELECT COUNT(*) FROM assistant_messages WHERE conversation_id=?1) FROM assistant_conversations WHERE id=?1 AND deleted=0",
        [id.as_str()],
        |row| {
            Ok(V4Conversation {
                conversation_id: id.clone(),
                id: id.to_string(),
                title: row.get(1)?,
                created_at_ms: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                updated_at_ms: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                archived: row.get::<_, i64>(2)? == 1,
                pinned: row.get::<_, i64>(3)? == 1,
                sequence: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
                message_count: u64::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
            })
        },
    ).optional()?;
    if let Some(ref c) = row {
        c.validate()
            .map_err(|_| StoreError::CorruptData("stored conv invalid"))?;
    }
    Ok(row)
}

fn message_page(
    tx: &Transaction<'_>,
    cid: &ConversationId,
    after: Option<u64>,
    _through: u64,
    limit: u16,
) -> Result<Vec<V4Message>, StoreError> {
    let after = after.unwrap_or(0);
    let mut stmt = tx.prepare(
        "SELECT id, conversation_id, run_id, role, sequence, body, text_bytes, text_complete, state, created_at_ms, EXISTS(SELECT 1 FROM assistant_content_events e JOIN assistant_content_payloads p ON p.conversation_id=e.conversation_id AND p.sequence=e.sequence WHERE m.role='assistant' AND e.conversation_id=m.conversation_id AND e.run_id=m.run_id AND e.request_id=m.run_id AND e.kind='output_bounded' AND p.payload_json=?4 AND e.byte_count=length(CAST(p.payload_json AS BLOB))) FROM assistant_messages m WHERE conversation_id=?1 AND sequence > ?2 ORDER BY sequence LIMIT ?3"
    )?;
    let rows = stmt
        .query_map(
            params![
                cid.as_str(),
                sql_sequence(after)?,
                i64::from(limit),
                serde_json::to_string(&crate::app_core::V4ContentPayload::Notice {
                    code: crate::app_core::V4NoticeCode::Truncated
                })?
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, bool>(10)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(_id, _c, run, role_s, seq, body, tb, _tc, st, created, context_truncated)| {
                let role = parse_role(&role_s);
                project_message(
                    cid,
                    &run,
                    role,
                    u64::try_from(seq).unwrap_or(0),
                    body,
                    u64::try_from(tb).unwrap_or(0),
                    state_from_str(&st),
                    context_truncated,
                    u64::try_from(created).unwrap_or(0),
                )
            },
        )
        .collect()
}

fn project_message(
    cid: &ConversationId,
    run: &str,
    role: V4MessageRole,
    sequence: u64,
    mut text: String,
    stored_bytes: u64,
    state: RunState,
    context_truncated: bool,
    created_at_ms: u64,
) -> Result<V4Message, StoreError> {
    let cap = if role == V4MessageRole::User {
        MAX_V4_INPUT_BYTES
    } else {
        MAX_V4_DELTA_BYTES
    };
    if text.len() > cap {
        let mut end = cap;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let shown = u64::try_from(text.len()).unwrap_or(u64::MAX);
    let (text_bytes, text_complete) = if role == V4MessageRole::User {
        (shown, true)
    } else {
        let text_bytes = stored_bytes.max(shown);
        (text_bytes, shown == text_bytes)
    };
    let message = V4Message {
        conversation_id: cid.clone(),
        run_id: run
            .parse()
            .map_err(|_| StoreError::CorruptData("stored msg invalid"))?,
        sequence,
        role,
        state,
        text,
        text_bytes,
        text_complete,
        context_truncated,
        created_at_ms,
    };
    message
        .validate()
        .map_err(|_| StoreError::CorruptData("stored msg invalid"))?;
    Ok(message)
}

fn fit_message_page(mut page: V4MessagePage) -> Result<V4MessagePage, StoreError> {
    let mut has_more = page.next_after.is_some();
    while page.messages.len() > 1 && serde_json::to_vec(&page)?.len() > MAX_V4_PAGE_BYTES {
        page.messages.pop();
        has_more = true;
    }
    page.next_after = has_more
        .then(|| page.messages.last().map(|message| message.sequence))
        .flatten();
    Ok(page)
}

fn parse_role(s: &str) -> V4MessageRole {
    match s {
        "assistant" => V4MessageRole::Assistant,
        _ => V4MessageRole::User,
    }
}

fn active_runs(tx: &Transaction<'_>, cid: &ConversationId) -> Result<u64, StoreError> {
    scalar(
        tx,
        "SELECT COUNT(*) FROM runs WHERE conversation_id=?1 AND status IN ('queued','starting','running','cancel_requested')",
        [cid.as_str()],
    )
}

fn touch_conversation(tx: &Transaction<'_>, id: &ConversationId) -> Result<(), StoreError> {
    if conversation_row(tx, id)?.is_none() {
        return Err(StoreError::AssistantNotFound);
    }
    Ok(())
}

fn scalar(
    conn: &rusqlite::Connection,
    sql: &str,
    p: impl rusqlite::Params,
) -> Result<u64, StoreError> {
    let v: i64 = conn.query_row(sql, p, |r| r.get(0))?;
    u64::try_from(v).map_err(|_| StoreError::CorruptData("neg count"))
}

fn ms(v: u64) -> Result<i64, StoreError> {
    i64::try_from(v).map_err(|_| StoreError::InvalidInput("ts range"))
}

fn stamp(v: u64) -> Result<String, StoreError> {
    let t = DateTime::<Utc>::from_timestamp_millis(ms(v)?)
        .ok_or(StoreError::InvalidInput("ts range"))?;
    Ok(t.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn like_contains(v: &str) -> String {
    let mut e = String::new();
    for c in v.chars() {
        if matches!(c, '\\' | '%' | '_') {
            e.push('\\');
        }
        e.push(c);
    }
    format!("%{e}%")
}

fn state_from_str(s: &str) -> RunState {
    match s {
        "queued" => RunState::Queued,
        "starting" => RunState::Starting,
        "running" => RunState::Running,
        "cancel_requested" => RunState::CancelRequested,
        "succeeded" => RunState::Succeeded,
        "failed" => RunState::Failed,
        "cancelled" => RunState::Cancelled,
        "interrupted" => RunState::Interrupted,
        _ => RunState::Queued,
    }
}

fn state_as_str(s: RunState) -> &'static str {
    match s {
        RunState::Queued => "queued",
        RunState::Starting => "starting",
        RunState::Running => "running",
        RunState::Succeeded => "succeeded",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
        RunState::CancelRequested => "cancel_requested",
        RunState::Interrupted => "interrupted",
    }
}
