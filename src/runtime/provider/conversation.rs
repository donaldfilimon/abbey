//! Stateless context drawn only from an exact durable v4 turn binding.
use super::*;
use crate::app_core::{MAX_V4_INPUT_BYTES, V4NoticeCode};

pub(super) fn prompt_for_turn(
    store: &RuntimeStore,
    run_id: &RunId,
    request: &V4SubmitTurn,
) -> Result<String, ExecutionError> {
    // Reserve framing space for at most32 whole pairs. Never truncate the
    // current authorized input or let historical text enlarge process authority.
    let available = MAX_V4_INPUT_BYTES
        .saturating_sub(request.input.len())
        .saturating_sub(2048)
        .min(24 * 1024);
    let context = store
        .v4_turn_context(run_id, request, available)
        .map_err(|_| unsupported())?;
    if context.truncated {
        store
            .v4_append_content(
                run_id,
                V4ContentPayload::Notice {
                    code: V4NoticeCode::Truncated,
                },
                V4ContentSink::now_ms(),
            )
            .map_err(|_| provider_failed())?;
    }
    if context.pairs.is_empty() {
        return Ok(request.input.clone());
    }
    let mut history = String::new();
    for pair in context.pairs {
        history.push_str("User:\n");
        history.push_str(&pair.user);
        history.push_str("\nAssistant:\n");
        history.push_str(&pair.assistant);
        history.push('\n');
    }
    let prompt = format!(
        "{}\n{}",
        crate::agent::conversation_context_prefix(&history),
        request.input
    );
    if prompt.len() > MAX_V4_INPUT_BYTES {
        return Err(unsupported());
    }
    Ok(prompt)
}
