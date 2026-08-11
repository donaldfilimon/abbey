//! Typed runtime-store records shared by lifecycle persistence operations.

use crate::app_core::{BackendSelection, ConversationId, IdempotencyKey, RunId, RunState};
use serde_json::Value;

/// New durable run request after app-core validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub conversation_id: Option<ConversationId>,
    pub idempotency_key: IdempotencyKey,
    /// Lower-case hexadecimal SHA-256 of the canonical request envelope.
    pub request_digest: String,
}

/// Durable run identity and lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub id: RunId,
    pub conversation_id: Option<ConversationId>,
    pub idempotency_key: IdempotencyKey,
    pub request_digest: String,
    pub status: RunState,
    pub next_event_sequence: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// New bounded event to append to a run.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRunEvent {
    pub kind: String,
    pub payload: Value,
}

/// One durable event from the append-only run lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub struct RunEvent {
    pub run_id: RunId,
    pub sequence: u64,
    pub kind: String,
    pub payload: Value,
    pub created_at: String,
}

/// Persistent provider binding for a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBackend {
    pub conversation_id: ConversationId,
    pub backend: BackendSelection,
    pub backend_conversation_id: Option<String>,
    pub updated_at: String,
}
