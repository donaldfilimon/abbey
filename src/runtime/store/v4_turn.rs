#![allow(
    dead_code,
    clippy::collapsible_if,
    reason = "v4 turn/content storage Task 2"
)]
//! Atomic admit, append content deltas (text), finalize one assistant msg, recovery as interrupted.
//! Bounded sequences, per-conversation isolation, digest binding. No legacy cascade.

use super::super::{
    NewRunEvent, RunRecord, RuntimeStore, StoreError, run_by_id, run_by_idempotency_key,
    sql_sequence, state_as_str,
};
use super::{active_runs, conversation_row, ms, scalar, stamp};
use crate::app_core::{
    ConversationId, IdempotencyKey, RunId, RunState, V4ContentEvent, V4ContentPage,
    V4ContentPayload, V4ContentQuery, V4TurnSubmission, V4TurnSubmit,
};

use chrono::Utc;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_SEQUENCE: u64 = 65_536;

impl RuntimeStore {
    /// Read an exact admitted turn without resubmitting or adopting work.
    pub(crate) fn v4_lookup_turn(
        &self,
        conversation: &ConversationId,
        key: &IdempotencyKey,
    ) -> Result<Option<V4TurnSubmission>, StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        if conversation_row(&conn, conversation)?.is_none() {
            return Err(StoreError::AssistantNotFound);
        }
        let Some(run) = run_by_idempotency_key(&conn, key.as_str())? else {
            return Ok(None);
        };
        if run.conversation_id.as_ref() != Some(conversation) {
            return Err(StoreError::AssistantNotFound);
        }
        let bound: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM assistant_turns WHERE run_id=?1 AND conversation_id=?2)",
            params![run.id.as_str(), conversation.as_str()],
            |row| row.get(0),
        )?;
        if !bound {
            return Err(StoreError::AssistantNotFound);
        }
        submission_for(&conn, &run.id).map(Some)
    }

    /// Check the immutable v4 turn binding before permitting a manager effect.
    /// Legacy runs have no assistant_turns row and cannot acquire v4 authority.
    pub(crate) fn v4_require_turn(
        &self,
        conversation_id: &ConversationId,
        run_id: &RunId,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM assistant_turns WHERE conversation_id=?1 AND run_id=?2)",
            params![conversation_id.as_str(), run_id.as_str()],
            |row| row.get(0),
        )?;
        if exists {
            Ok(())
        } else {
            Err(StoreError::AssistantNotFound)
        }
    }

    /// Atomic admit: creates run row + queued event + user message + turn binding in one tx.
    /// Rejects conflicting idempotency (stable duplicate) and overlapping active in conv.
    pub(crate) fn v4_admit_turn(
        &self,
        submit: &V4TurnSubmit,
        now_ms: u64,
    ) -> Result<V4TurnSubmission, StoreError> {
        submit
            .validate()
            .map_err(|_| StoreError::InvalidInput("turn submit invalid"))?;
        let conv_id: ConversationId = submit.conversation_id.clone();
        let key: IdempotencyKey = submit.idempotency_key.clone();
        let digest = submit_digest(submit)?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = run_by_idempotency_key(&tx, key.as_str())? {
            let dup = duplicate_submission(&tx, &existing, submit)?;
            tx.commit()?;
            return Ok(dup);
        }
        if conversation_row(&tx, &conv_id)?.is_none() {
            return Err(StoreError::AssistantNotFound);
        }
        if active_runs(&tx, &conv_id)? > 0 {
            return Err(StoreError::AssistantConflict);
        }
        // Admission must retain room for visible output and a terminal marker.
        // The old synthetic admission event implicitly checked capacity here.
        let last_content = scalar(
            &tx,
            "SELECT COALESCE(MAX(sequence), 0) FROM assistant_content_events WHERE conversation_id=?1",
            [conv_id.as_str()],
        )?;
        if last_content > MAX_SEQUENCE - 2 {
            return Err(StoreError::AssistantConflict);
        }
        let run_id = RunId::new();
        let stamp = stamp(now_ms)?;
        tx.execute(
            "INSERT INTO runs(id, conversation_id, idempotency_key, request_digest, status, next_event_sequence, created_at, updated_at) VALUES (?1,?2,?3,?4,'queued',2,?5,?5)",
            params![run_id.as_str(), conv_id.as_str(), key.as_str(), digest, stamp],
        )?;
        tx.execute("INSERT INTO run_events(run_id, sequence, kind, payload_json, created_at) VALUES (?1,1,'run_queued','{}',?2)", params![run_id.as_str(), stamp])?;
        let seq = next_message_sequence(&tx, &conv_id)?;
        let msg_id = Uuid::new_v4().to_string();
        let bytes = submit.input.len() as i64;
        tx.execute(
            "INSERT INTO assistant_messages(id, conversation_id, run_id, role, sequence, body, text_bytes, text_complete, state, created_at_ms) VALUES (?1,?2,?3,'user',?4,?5,?6,1,'queued',?7)",
            params![msg_id, conv_id.as_str(), run_id.as_str(), sql_sequence(seq)?, &submit.input, bytes, ms(now_ms)?],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO assistant_permission_profiles(profile_id, document_json, updated_at_ms) VALUES ('desk.default', '{}', ?1)",
            [ms(now_ms)?],
        )?;
        tx.execute(
            "INSERT INTO assistant_turns(run_id, conversation_id, user_message_id, assistant_message_id, user_sequence, route_id, permission_profile_id, created_at_ms) VALUES (?1,?2,?3,NULL,?4,?5,'desk.default',?6)",
            params![run_id.as_str(), conv_id.as_str(), msg_id, sql_sequence(seq)?, &submit.route_id, ms(now_ms)?],
        )?;
        tx.execute(
            "UPDATE assistant_conversations SET updated_at_ms=?2 WHERE id=?1",
            params![conv_id.as_str(), ms(now_ms)?],
        )?;
        let sub = V4TurnSubmission {
            conversation_id: conv_id,
            run_id,
            idempotency_key: key,
            user_message_sequence: seq,
            state: RunState::Queued,
            duplicate: false,
        };
        sub.validate()
            .map_err(|_| StoreError::CorruptData("turn sub invalid"))?;
        tx.commit()?;
        Ok(sub)
    }

    /// Append ordered bounded content delta for the run. Atomic with message update.
    /// For assistant, appends to preview text (bounded delta).
    pub(crate) fn v4_append_content(
        &self,
        run_id: &RunId,
        payload: V4ContentPayload,
        now_ms: u64,
    ) -> Result<V4ContentEvent, StoreError> {
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = run_by_id(&tx, run_id)?.ok_or(StoreError::AssistantNotFound)?;
        if run.status.is_terminal() {
            return Err(StoreError::TerminalRun {
                run_id: run.id.to_string(),
            });
        }
        let conv = turn_conversation(&tx, run_id.as_str())?;
        let conv_id: ConversationId = conv
            .parse()
            .map_err(|_| StoreError::CorruptData("bad cid"))?;
        let event = insert_content_event(&tx, &conv_id, run_id, payload, now_ms)?;
        // update message preview for assistant deltas
        if let V4ContentPayload::TextDelta { text } = &event.payload {
            if !text.is_empty() {
                update_assistant_preview(&tx, run_id, text, now_ms)?;
            }
        }
        tx.commit()?;
        Ok(event)
    }

    pub(crate) fn v4_content(&self, query: &V4ContentQuery) -> Result<V4ContentPage, StoreError> {
        query
            .validate()
            .map_err(|_| StoreError::InvalidInput("content query invalid"))?;
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        if conversation_row(&tx, &query.conversation_id)?.is_none() {
            return Err(StoreError::AssistantNotFound);
        }
        let mut events = content_page(
            &tx,
            &query.conversation_id,
            &query.run_id,
            query.after,
            query.limit,
        )?;
        tx.commit()?;
        // Account for JSON escaping and envelope fields, not just text bytes.
        while !events.is_empty() {
            let candidate = V4ContentPage {
                conversation_id: query.conversation_id.clone(),
                run_id: query.run_id.clone(),
                after: query.after,
                next_after: events.last().and_then(|event| {
                    (!matches!(event.payload, V4ContentPayload::Terminal { .. }))
                        .then_some(event.sequence)
                }),
                events: events.clone(),
            };
            if serde_json::to_vec(&candidate)?.len() <= crate::app_core::MAX_V4_PAGE_BYTES {
                break;
            }
            events.pop();
        }
        let terminal = events
            .last()
            .is_some_and(|event| matches!(event.payload, V4ContentPayload::Terminal { .. }));
        let next_after = if terminal {
            None
        } else {
            events.last().map(|event| event.sequence)
        };
        let page = V4ContentPage {
            conversation_id: query.conversation_id.clone(),
            run_id: query.run_id.clone(),
            after: query.after,
            events,
            next_after,
        };
        page.validate()
            .map_err(|_| StoreError::CorruptData("content page invalid"))?;
        Ok(page)
    }

    pub(crate) fn v4_content_events(
        &self,
        q: &V4ContentQuery,
    ) -> Result<V4ContentPage, StoreError> {
        self.v4_content(q)
    }

    /// Called on terminal lifecycle to finalize exactly one assistant msg + terminal event.
    pub(crate) fn v4_finalize_turn(
        &self,
        run_id: &RunId,
        final_state: RunState,
        _now_ms: u64,
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = run_by_id(&tx, run_id)?.ok_or(StoreError::AssistantNotFound)?;
        if !final_state.is_terminal() || run.status != final_state {
            return Err(StoreError::AssistantConflict);
        }
        // Completeness describes the preview, not whether execution is terminal.
        tx.execute(
            "UPDATE assistant_messages SET state=?2, text_complete=(length(CAST(body AS BLOB))=text_bytes) WHERE run_id=?1 AND role='assistant'",
            params![run_id.as_str(), state_as_str(final_state)],
        )?;
        // The run transition already writes the one terminal content event
        // through sync_message_state in the same transaction.
        tx.commit()?;
        Ok(())
    }

    /// Fail a turn that is still queued because no worker adopted it.
    /// A turn that has already moved on is returned unchanged.
    pub(crate) fn v4_fail_unadopted_turn(
        &self,
        run_id: &RunId,
    ) -> Result<V4TurnSubmission, StoreError> {
        let outcome = self.transition_run(
            run_id,
            RunState::Queued,
            RunState::Failed,
            NewRunEvent {
                kind: "run_failed".into(),
                payload: serde_json::json!({
                    "code": "executor_unsupported",
                    "message": "executor does not support this request"
                }),
            },
        );
        match outcome {
            Ok(_)
            | Err(StoreError::UnexpectedStatus { .. })
            | Err(StoreError::TerminalRun { .. }) => {}
            Err(error) => return Err(error),
        }
        let conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        submission_for(&conn, run_id)
    }
}

fn submit_digest(s: &V4TurnSubmit) -> Result<String, StoreError> {
    let enc = serde_json::to_vec(s)?;
    let mut h = Sha256::new();
    h.update(b"abbey:v4-turn-submit:v1\0");
    h.update(enc);
    Ok(format!("{:x}", h.finalize()))
}

fn duplicate_submission(
    tx: &Transaction<'_>,
    existing: &RunRecord,
    submit: &V4TurnSubmit,
) -> Result<V4TurnSubmission, StoreError> {
    let digest = submit_digest(submit)?;
    if existing.request_digest != digest
        || existing.conversation_id.as_ref() != Some(&submit.conversation_id)
    {
        return Err(StoreError::IdempotencyConflict);
    }
    let seq: i64 = tx.query_row(
        "SELECT user_sequence FROM assistant_turns WHERE run_id=?1",
        [existing.id.as_str()],
        |row| row.get(0),
    )?;
    let submission = V4TurnSubmission {
        conversation_id: submit.conversation_id.clone(),
        run_id: existing.id.clone(),
        idempotency_key: submit.idempotency_key.clone(),
        user_message_sequence: u64::try_from(seq).unwrap_or(0),
        state: existing.status,
        duplicate: true,
    };
    submission
        .validate()
        .map_err(|_| StoreError::CorruptData("turn sub invalid"))?;
    Ok(submission)
}

fn submission_for(
    conn: &rusqlite::Connection,
    run_id: &RunId,
) -> Result<V4TurnSubmission, StoreError> {
    let run = run_by_id(conn, run_id)?.ok_or(StoreError::AssistantNotFound)?;
    let seq: i64 = conn.query_row(
        "SELECT user_sequence FROM assistant_turns WHERE run_id=?1",
        [run.id.as_str()],
        |row| row.get(0),
    )?;
    let submission = V4TurnSubmission {
        conversation_id: run
            .conversation_id
            .clone()
            .ok_or(StoreError::CorruptData("v4 run missing conversation"))?,
        run_id: run.id.clone(),
        idempotency_key: run.idempotency_key,
        user_message_sequence: u64::try_from(seq).map_err(|_| StoreError::CorruptData("seq"))?,
        state: run.status,
        duplicate: false,
    };
    submission
        .validate()
        .map_err(|_| StoreError::CorruptData("turn sub invalid"))?;
    Ok(submission)
}

fn turn_conversation(tx: &Transaction<'_>, run: &str) -> Result<String, StoreError> {
    tx.query_row(
        "SELECT conversation_id FROM assistant_turns WHERE run_id=?1",
        [run],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

fn next_message_sequence(tx: &Transaction<'_>, cid: &ConversationId) -> Result<u64, StoreError> {
    let n = scalar(
        tx,
        "SELECT COALESCE(MAX(sequence), 0) FROM assistant_messages WHERE conversation_id=?1",
        [cid.as_str()],
    )?;
    Ok(n + 1)
}

fn insert_content_event(
    tx: &Transaction<'_>,
    cid: &ConversationId,
    rid: &RunId,
    payload: V4ContentPayload,
    now_ms: u64,
) -> Result<V4ContentEvent, StoreError> {
    payload
        .validate()
        .map_err(|_| StoreError::InvalidInput("content payload invalid"))?;
    let last = scalar(
        tx,
        "SELECT COALESCE(MAX(sequence), 0) FROM assistant_content_events WHERE conversation_id=?1",
        [cid.as_str()],
    )?;
    let seq = last
        .checked_add(1)
        .filter(|s| *s <= MAX_SEQUENCE)
        .ok_or(StoreError::AssistantConflict)?;
    // Keep capacity for the lifecycle's terminal marker.
    if !matches!(payload, V4ContentPayload::Terminal { .. }) && seq == MAX_SEQUENCE {
        return Err(StoreError::AssistantConflict);
    }
    let pjson = serde_json::to_string(&payload)?;
    // The existing schema bounds content bytes, not escaped JSON bytes.
    let byte_count = match &payload {
        V4ContentPayload::TextDelta { text } => text.len(),
        _ => pjson.len(),
    };
    tx.execute(
        "INSERT INTO assistant_content_events(conversation_id, sequence, request_id, run_id, kind, byte_count, timestamp_ms) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![cid.as_str(), sql_sequence(seq)?, rid.as_str(), rid.as_str(), kind_of(&payload), byte_count as i64, ms(now_ms)? ],
    )?;
    // store full payload in assistant_content_payloads for recovery
    tx.execute(
        "INSERT OR IGNORE INTO assistant_content_payloads(conversation_id, sequence, payload_json) VALUES (?1,?2,?3)",
        params![cid.as_str(), sql_sequence(seq)?, pjson],
    )?;
    let ev = V4ContentEvent {
        conversation_id: cid.clone(),
        run_id: rid.clone(),
        sequence: seq,
        timestamp_ms: now_ms,
        payload,
    };
    ev.validate()
        .map_err(|_| StoreError::CorruptData("content event invalid"))?;
    Ok(ev)
}

fn kind_of(payload: &V4ContentPayload) -> &'static str {
    match payload {
        V4ContentPayload::TextDelta { .. } => "output_bounded",
        V4ContentPayload::Terminal { state } => match state {
            RunState::Cancelled | RunState::CancelRequested => "cancelled",
            RunState::Failed => "failed",
            RunState::Interrupted => "interrupted",
            _ => "completed",
        },
        V4ContentPayload::Tool {
            state: crate::app_core::V4ToolState::Started,
            ..
        } => "started",
        _ => "output_bounded",
    }
}

fn update_assistant_preview(
    tx: &Transaction<'_>,
    rid: &RunId,
    delta: &str,
    now: u64,
) -> Result<(), StoreError> {
    let existing: Option<(String, i64)> = tx
        .query_row(
            "SELECT body, text_bytes FROM assistant_messages WHERE run_id=?1 AND role='assistant'",
            [rid.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let total = existing
        .as_ref()
        .map_or(0, |(_, bytes)| *bytes)
        .checked_add(delta.len() as i64)
        .filter(|bytes| (0..=crate::app_core::MAX_V4_CONTENT_BYTES as i64).contains(bytes))
        .ok_or(StoreError::AssistantConflict)?;
    let mut combined = existing
        .as_ref()
        .map_or_else(String::new, |(body, _)| body.clone());
    // Once a preview is truncated it must remain a prefix, even when a later
    // ASCII delta could fit after a multi-byte character that did not fit.
    if existing
        .as_ref()
        .is_none_or(|(body, bytes)| i64::try_from(body.len()).ok() == Some(*bytes))
    {
        let mut end = delta.len().min(16 * 1024 - combined.len());
        while !delta.is_char_boundary(end) {
            end -= 1;
        }
        combined.push_str(&delta[..end]);
    }
    let b = total;
    if existing.is_some() {
        tx.execute("UPDATE assistant_messages SET body=?2, text_bytes=?3, text_complete=0, state='running' WHERE run_id=?1 AND role='assistant'", params![rid.as_str(), combined, b])?;
    } else {
        let seq = next_message_sequence(
            tx,
            &turn_conversation(tx, rid.as_str())?
                .parse()
                .map_err(|_| StoreError::CorruptData("bad cid"))?,
        )?;
        let mid = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO assistant_messages(id,conversation_id,run_id,role,sequence,body,text_bytes,text_complete,state,created_at_ms) VALUES (?1,?2,?3,'assistant',?4,?5,?6,0,'running',?7)",
            params![mid, turn_conversation(tx, rid.as_str())?, rid.as_str(), sql_sequence(seq)?, combined, b, ms(now)?],
        )?;
    }
    Ok(())
}

fn content_page(
    tx: &Transaction<'_>,
    cid: &ConversationId,
    rid: &RunId,
    after: u64,
    limit: u16,
) -> Result<Vec<V4ContentEvent>, StoreError> {
    if turn_conversation(tx, rid.as_str())? != cid.as_str() {
        return Err(StoreError::AssistantNotFound);
    }
    if after != 0 {
        let belongs: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM assistant_content_events WHERE conversation_id=?1 AND run_id=?2 AND sequence=?3)",
            params![cid.as_str(), rid.as_str(), sql_sequence(after)?], |row| row.get(0),
        )?;
        if !belongs {
            return Err(StoreError::InvalidInput("content cursor invalid"));
        }
    }
    let mut stmt = tx.prepare("SELECT e.sequence, e.timestamp_ms, p.payload_json FROM assistant_content_events e LEFT JOIN assistant_content_payloads p ON p.conversation_id=e.conversation_id AND p.sequence=e.sequence WHERE e.conversation_id=?1 AND e.run_id=?2 AND e.sequence > ?3 ORDER BY e.sequence LIMIT ?4")?;
    let rows = stmt
        .query_map(
            params![
                cid.as_str(),
                rid.as_str(),
                sql_sequence(after)?,
                i64::from(limit)
            ],
            |r| {
                let seq: i64 = r.get(0)?;
                let ts: i64 = r.get(1)?;
                let pjson: String = r.get(2)?;
                let payload: V4ContentPayload = serde_json::from_str(&pjson).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok((
                    u64::try_from(seq).unwrap_or(0),
                    u64::try_from(ts).unwrap_or(0),
                    payload,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(seq, ts, payload)| {
            let e = V4ContentEvent {
                conversation_id: cid.clone(),
                run_id: rid.clone(),
                sequence: seq,
                timestamp_ms: ts,
                payload,
            };
            e.validate()
                .map_err(|_| StoreError::CorruptData("ev invalid"))?;
            Ok(e)
        })
        .collect::<Result<Vec<_>, _>>()
}

fn turn_conversation_id(tx: &Transaction<'_>, rid: &RunId) -> Result<ConversationId, StoreError> {
    let s: String = turn_conversation(tx, rid.as_str())?;
    s.parse().map_err(|_| StoreError::CorruptData("cid"))
}

// for reopen sync to mark interrupted
pub(crate) fn mark_interrupted(
    tx: &Transaction<'_>,
    rid: &RunId,
    now_ms: u64,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE assistant_messages SET state='interrupted', text_complete=(length(CAST(body AS BLOB))=text_bytes) WHERE run_id=?1",
        [rid.as_str()],
    )?;
    let cid = turn_conversation_id(tx, rid)?;
    let _ = insert_content_event(
        tx,
        &cid,
        rid,
        V4ContentPayload::Terminal {
            state: RunState::Interrupted,
        },
        now_ms,
    );
    Ok(())
}

/// Project a lifecycle transition onto v4 messages without copying the prompt.
pub(crate) fn sync_message_state(
    tx: &rusqlite::Transaction<'_>,
    rid: &RunId,
    next: RunState,
) -> Result<(), StoreError> {
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM assistant_turns WHERE run_id=?1",
            [rid.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(());
    }
    tx.execute(
        "UPDATE assistant_messages SET state=?2, text_complete=(length(CAST(body AS BLOB))=text_bytes) WHERE run_id=?1",
        params![rid.as_str(), state_as_str(next)],
    )?;
    if !next.is_terminal() {
        return Ok(());
    }
    let marked: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM assistant_content_events WHERE run_id=?1 AND kind IN ('completed','cancelled','failed','interrupted')",
            [rid.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if marked.is_some() {
        return Ok(());
    }
    let now_ms = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0);
    match insert_content_event(
        tx,
        &turn_conversation_id(tx, rid)?,
        rid,
        V4ContentPayload::Terminal { state: next },
        now_ms,
    ) {
        Ok(_) | Err(StoreError::AssistantConflict) => Ok(()),
        Err(error) => Err(error),
    }
}
