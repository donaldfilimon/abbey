//! Bounded, exact durable history for one admitted conversational turn.
use super::super::{RuntimeStore, StoreError};
use crate::app_core::{MAX_V4_CONTENT_BYTES, RunId, RunState, V4ContentPayload, V4SubmitTurn};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

pub(crate) struct V4ContextPair {
    pub user: String,
    pub assistant: String,
}
pub(crate) struct V4TurnContext {
    pub pairs: Vec<V4ContextPair>,
    pub truncated: bool,
}

impl RuntimeStore {
    pub(crate) fn v4_turn_context(
        &self,
        run: &RunId,
        request: &V4SubmitTurn,
        max_bytes: usize,
    ) -> Result<V4TurnContext, StoreError> {
        request
            .validate()
            .map_err(|_| StoreError::InvalidInput("context request invalid"))?;
        if max_bytes > 24 * 1024 {
            return Err(StoreError::InvalidInput("context bound invalid"));
        }
        let mut conn = self.conn.lock().expect("runtime sqlite lock poisoned");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let mut digest = Sha256::new();
        digest.update(b"abbey:v4-turn-submit:v1\0");
        digest.update(serde_json::to_vec(request)?);
        let digest = format!("{:x}", digest.finalize());
        let current: Option<i64> = tx.query_row(
            "SELECT t.user_sequence FROM assistant_turns t JOIN runs r ON r.id=t.run_id JOIN assistant_messages m ON m.id=t.user_message_id WHERE t.run_id=?1 AND t.conversation_id=?2 AND r.conversation_id=?2 AND t.route_id=?3 AND r.request_digest=?4 AND r.idempotency_key=?5 AND m.conversation_id=?2 AND m.run_id=?1 AND m.role='user' AND m.sequence=t.user_sequence AND m.body=?6 AND m.text_bytes=length(CAST(m.body AS BLOB))",
            params![run.as_str(), request.conversation_id.as_str(), request.route_id, digest, request.idempotency_key.as_str(), request.input], |r| r.get(0)).optional()?;
        let before = current.ok_or(StoreError::AssistantConflict)?;
        let mut stmt = tx.prepare("SELECT t.run_id, t.user_message_id, a.id FROM assistant_turns t JOIN runs r ON r.id=t.run_id LEFT JOIN assistant_messages a ON a.run_id=t.run_id AND a.role='assistant' AND (t.assistant_message_id IS NULL OR t.assistant_message_id=a.id) WHERE t.conversation_id=?1 AND r.conversation_id=t.conversation_id AND t.route_id=?2 AND t.user_sequence<?3 AND r.status='succeeded' ORDER BY t.user_sequence DESC LIMIT 33")?;
        let identities = stmt
            .query_map(
                params![request.conversation_id.as_str(), request.route_id, before],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut result = V4TurnContext {
            pairs: Vec::new(),
            truncated: identities.len() > 32,
        };
        let mut remaining = max_bytes;
        for (rid, uid, aid) in identities.into_iter().take(32) {
            let Some(aid) = aid else {
                validate_empty_success(&tx, request.conversation_id.as_str(), &rid)?;
                continue;
            };
            let lengths: Option<(i64,i64)> = tx.query_row("SELECT length(CAST(u.body AS BLOB)), a.text_bytes FROM assistant_messages u JOIN assistant_messages a ON a.id=?2 WHERE u.id=?1 AND u.conversation_id=?3 AND a.conversation_id=?3 AND u.run_id=?4 AND a.run_id=?4 AND u.role='user' AND a.role='assistant' AND a.state='succeeded'",params![uid,aid,request.conversation_id.as_str(),rid], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let (user_bytes, assistant_bytes) =
                lengths.ok_or(StoreError::CorruptData("context pair invalid"))?;
            if user_bytes < 0
                || assistant_bytes < 0
                || assistant_bytes as usize > MAX_V4_CONTENT_BYTES
            {
                return Err(StoreError::CorruptData("context lengths invalid"));
            }
            // Do not materialize a large pair merely to discard it.
            if user_bytes as u64 + assistant_bytes as u64 > remaining as u64 {
                result.truncated = true;
                continue;
            }
            let user: String = tx.query_row(
                "SELECT body FROM assistant_messages WHERE id=?1",
                [uid],
                |r| r.get(0),
            )?;
            let preview: String = tx.query_row("SELECT body FROM assistant_messages WHERE id=?1 AND length(CAST(body AS BLOB))<=16384", [&aid], |r| r.get(0)).optional()?.ok_or(StoreError::CorruptData("context preview invalid"))?;
            let mut first_text = true;
            let mut legacy_marker = false;
            let mut events = tx.prepare("SELECT e.kind,e.byte_count,length(CAST(p.payload_json AS BLOB)),p.payload_json FROM assistant_content_events e LEFT JOIN assistant_content_payloads p ON p.conversation_id=e.conversation_id AND p.sequence=e.sequence WHERE e.conversation_id=?1 AND e.run_id=?2 ORDER BY e.sequence LIMIT 65537")?;
            let mut rows = events.query(params![request.conversation_id.as_str(), rid])?;
            let output_budget = remaining - user.len();
            let mut assistant =
                String::with_capacity((assistant_bytes as usize).min(output_budget));
            let mut actual_bytes = 0usize;
            let mut legacy_encoding = false;
            let mut oversized = false;
            let mut count = 0usize;
            let mut terminal = false;
            let mut parsed_bytes = 0usize;
            while let Some(row) = rows.next()? {
                count += 1;
                let size: Option<i64> = row.get(2)?;
                if count > 65536
                    || terminal
                    || size.is_none_or(|n| {
                        n < 0 || n as usize > crate::app_core::MAX_V4_DELTA_BYTES * 6 + 1024
                    })
                {
                    return Err(StoreError::CorruptData("context event bound invalid"));
                }
                parsed_bytes = parsed_bytes.saturating_add(size.unwrap_or_default() as usize);
                if parsed_bytes > MAX_V4_CONTENT_BYTES.saturating_mul(6).saturating_add(1024) {
                    return Err(StoreError::CorruptData("context aggregate bound invalid"));
                }
                let json: String = row.get(3)?;
                let payload: V4ContentPayload = serde_json::from_str(&json)
                    .map_err(|_| StoreError::CorruptData("context payload invalid"))?;
                payload
                    .validate()
                    .map_err(|_| StoreError::CorruptData("context payload invalid"))?;
                match payload {
                    V4ContentPayload::TextDelta { text } => {
                        let bytes: i64 = row.get(1)?;
                        let kind: String = row.get(0)?;
                        legacy_encoding |= bytes == json.len() as i64;
                        if (bytes != text.len() as i64 && bytes != json.len() as i64)
                            || kind != "output_bounded"
                        {
                            return Err(StoreError::CorruptData("context text metadata invalid"));
                        }
                        if first_text
                            && count == 1
                            && text == "[admitted]"
                            && bytes == json.len() as i64
                        {
                            first_text = false;
                            legacy_marker = true;
                            continue;
                        }
                        first_text = false;
                        actual_bytes = actual_bytes.saturating_add(text.len());
                        if actual_bytes > MAX_V4_CONTENT_BYTES {
                            return Err(StoreError::CorruptData("context text invalid"));
                        }
                        if actual_bytes > output_budget {
                            oversized = true;
                        }
                        let mut take = text.len().min(output_budget - assistant.len());
                        while !text.is_char_boundary(take) {
                            take -= 1;
                        }
                        assistant.push_str(&text[..take]);
                    }
                    V4ContentPayload::Terminal {
                        state: RunState::Succeeded,
                    } => terminal = true,
                    V4ContentPayload::Terminal { .. } => {
                        return Err(StoreError::CorruptData("context terminal invalid"));
                    }
                    _ => {}
                }
            }
            // Old admission used a serialized-count first event outside the
            // assistant preview. Prefer its removal only when the remainder
            // independently agrees with the stored reply evidence.
            let strip_marker = legacy_marker
                && !preview.is_empty()
                && assistant.starts_with(&preview)
                && actual_bytes >= preview.len()
                && (actual_bytes == assistant_bytes as usize
                    || assistant_bytes as usize == preview.len());
            if legacy_marker && !strip_marker {
                actual_bytes = actual_bytes.saturating_add("[admitted]".len());
                let mut restored = String::with_capacity(output_budget.min(actual_bytes));
                restored.push_str("[admitted]");
                let mut take = assistant
                    .len()
                    .min(output_budget.saturating_sub(restored.len()));
                while !assistant.is_char_boundary(take) {
                    take -= 1;
                }
                restored.push_str(&assistant[..take]);
                assistant = restored;
                oversized |= actual_bytes > output_budget;
            }
            if !terminal
                || (actual_bytes != assistant_bytes as usize
                    && !(legacy_encoding
                        && assistant_bytes as usize == preview.len()
                        && actual_bytes >= preview.len()))
                || !assistant.starts_with(&preview)
            {
                return Err(StoreError::CorruptData("context output incomplete"));
            }
            if oversized {
                result.truncated = true;
                continue;
            }
            remaining -= user.len() + assistant.len();
            result.pairs.push(V4ContextPair { user, assistant });
        }
        result.pairs.reverse();
        drop(stmt);
        tx.commit()?;
        Ok(result)
    }
}

fn validate_empty_success(
    tx: &rusqlite::Transaction<'_>,
    conversation: &str,
    run: &str,
) -> Result<(), StoreError> {
    let mut statement = tx.prepare("SELECT e.byte_count, e.kind, CASE WHEN length(CAST(p.payload_json AS BLOB))<=1024 THEN p.payload_json ELSE NULL END FROM assistant_content_events e LEFT JOIN assistant_content_payloads p ON p.conversation_id=e.conversation_id AND p.sequence=e.sequence WHERE e.conversation_id=?1 AND e.run_id=?2 ORDER BY e.sequence LIMIT 4")?;
    let mut rows = statement.query(params![conversation, run])?;
    let mut terminal = false;
    let mut index = 0;
    let mut notice = false;
    while let Some(row) = rows.next()? {
        if terminal || index >= 3 {
            return Err(StoreError::CorruptData("empty context events invalid"));
        }
        let json: Option<String> = row.get(2)?;
        let json = json.ok_or(StoreError::CorruptData("empty context payload missing"))?;
        let payload: V4ContentPayload = serde_json::from_str(&json)
            .map_err(|_| StoreError::CorruptData("empty context payload invalid"))?;
        payload
            .validate()
            .map_err(|_| StoreError::CorruptData("empty context payload invalid"))?;
        let bytes: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        if bytes != json.len() as i64 {
            return Err(StoreError::CorruptData("empty context byte count invalid"));
        }
        match payload {
            V4ContentPayload::Terminal {
                state: RunState::Succeeded,
            } if kind == "completed" => terminal = true,
            V4ContentPayload::Notice {
                code: crate::app_core::V4NoticeCode::Truncated,
            } if !notice && kind == "output_bounded" => notice = true,
            V4ContentPayload::TextDelta { text }
                if index == 0 && text == "[admitted]" && kind == "output_bounded" => {}
            _ => return Err(StoreError::CorruptData("empty context content invalid")),
        }
        index += 1;
    }
    if !terminal {
        return Err(StoreError::CorruptData("empty context terminal missing"));
    }
    Ok(())
}
