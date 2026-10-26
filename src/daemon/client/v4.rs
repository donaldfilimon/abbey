//! Send-once protocol-v4 client. It does not retry, downgrade, or replay an effect.
use super::DaemonClient;
use crate::app_core::{
    APP_PROTOCOL_V4, APP_SCHEMA_V4, ConversationId, IdempotencyKey, RunId, RunState,
    V4CapabilitySet, V4Command, V4ContentPage, V4ContentQuery, V4Conversation, V4ConversationPage,
    V4ConversationPatch, V4ConversationQuery, V4ConversationRename, V4ErrorCode, V4Event,
    V4GrantRequest, V4MessagePage, V4MessageQuery, V4ResourceQuery, V4Route, V4SubmitTurn,
    V4TurnSubmission,
};
use crate::daemon::{V4RequestEnvelope, V4ResponseEnvelope, V4ResponsePayload};
use std::fmt;

/// Privacy-safe failure: no transport, server text, or content is retained.
///
/// Windows named pipes: the v4 exchange stub below returns UnsupportedPlatform
/// on non-Unix. Conversation, task, automation, and action surfaces over named
/// pipes are deferred (see desktop-assistant plan Task 5). All non-Unix paths
/// are intentionally compile-time stubs that fail closed; no silent fallback.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum V4ClientError {
    #[error("protocol-v4 transport unavailable")]
    Transport,
    #[error("protocol-v4 unsupported platform")]
    UnsupportedPlatform,
    #[error("invalid protocol-v4 request")]
    InvalidRequest,
    #[error("invalid protocol-v4 response")]
    InvalidResponse,
    #[error("protocol-v4 capability not granted")]
    CapabilityDenied,
    #[error("protocol-v4 request rejected")]
    Rejected(V4ErrorCode),
}

/// Validated surface grants; execution authority remains owned by the host.
#[derive(Clone)]
pub struct V4DaemonSession {
    client: DaemonClient,
    granted: V4CapabilitySet,
    schema_version: u16,
}

impl fmt::Debug for V4DaemonSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("V4DaemonSession")
    }
}

impl DaemonClient {
    /// Negotiate v4 once, without retry or downgrade.
    pub fn negotiate_v4(
        &self,
        requested: V4CapabilitySet,
    ) -> Result<V4DaemonSession, V4ClientError> {
        self.negotiate_v4_schema(requested, APP_SCHEMA_V4)
    }
    /// Explicit conversation-only compatibility. Never selected as an automatic retry.
    pub fn negotiate_v4_schema2(
        &self,
        requested: V4CapabilitySet,
    ) -> Result<V4DaemonSession, V4ClientError> {
        self.negotiate_v4_schema(requested, crate::app_core::APP_SCHEMA_V4_CONVERSATIONS)
    }
    fn negotiate_v4_schema(
        &self,
        requested: V4CapabilitySet,
        schema_version: u16,
    ) -> Result<V4DaemonSession, V4ClientError> {
        if schema_version == crate::app_core::APP_SCHEMA_V4_CONVERSATIONS
            && requested.as_slice().iter().any(|c| !c.is_conversation())
        {
            return Err(V4ClientError::InvalidRequest);
        }
        let command = V4Command::Negotiate(V4GrantRequest {
            supported_versions: vec![APP_PROTOCOL_V4],
            requested,
        });
        let event = exchange(
            self,
            schema_version,
            V4CapabilitySet::deny_all(),
            command.clone(),
        )?;
        if !command.matches_event(&event) {
            return Err(V4ClientError::InvalidResponse);
        }
        let V4Event::Negotiated(n) = event else {
            return Err(V4ClientError::InvalidResponse);
        };
        if n.protocol_version != APP_PROTOCOL_V4
            || n.schema_version != schema_version
            || n.granted.validate().is_err()
        {
            return Err(V4ClientError::InvalidResponse);
        }
        Ok(V4DaemonSession {
            client: self.clone(),
            granted: n.granted,
            schema_version,
        })
    }
}

impl V4DaemonSession {
    /// Actual selected schema, distinct from the latest supported schema.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Exact negotiated grant set.
    #[must_use]
    pub fn grants(&self) -> &V4CapabilitySet {
        &self.granted
    }

    /// Create one conversation. The caller retries explicitly.
    pub fn create_conversation(
        &self,
        title: impl Into<String>,
    ) -> Result<V4Conversation, V4ClientError> {
        match self.request(V4Command::CreateConversation(title.into()))? {
            V4Event::Conversation(c) => Ok(c),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Read an exact accepted turn without replaying submission after a lost response.
    pub fn lookup_turn(
        &self,
        conversation: ConversationId,
        key: IdempotencyKey,
    ) -> Result<Option<V4TurnSubmission>, V4ClientError> {
        match self.request(V4Command::LookupTurn(conversation.clone(), key.clone()))? {
            V4Event::TurnLookup(found_conversation, found_key, submission)
                if found_conversation == conversation && found_key == key =>
            {
                Ok(submission)
            }
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Read one conversation summary. Message text stays on [`Self::read_messages`].
    pub fn read_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<V4Conversation, V4ClientError> {
        match self.request(V4Command::ReadConversation(conversation_id))? {
            V4Event::Conversation(c) => Ok(c),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// List conversation summaries.
    pub fn list_conversations(
        &self,
        query: V4ConversationQuery,
    ) -> Result<V4ConversationPage, V4ClientError> {
        match self.request(V4Command::ListConversations(query))? {
            V4Event::Conversations(page) => Ok(page),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Search conversation titles through the same bounded list contract.
    pub fn search_conversations(
        &self,
        query: V4ConversationQuery,
    ) -> Result<V4ConversationPage, V4ClientError> {
        self.list_conversations(query)
    }

    /// Rename one conversation using the canonical patch command.
    pub fn rename_conversation(
        &self,
        request: V4ConversationRename,
    ) -> Result<V4Conversation, V4ClientError> {
        let id = request
            .conversation_id
            .parse::<ConversationId>()
            .map_err(|_| V4ClientError::InvalidRequest)?;
        self.update_conversation(
            id,
            V4ConversationPatch {
                title: Some(request.title),
                archived: None,
                pinned: None,
            },
        )
    }

    /// Archive one exact conversation.
    pub fn archive_conversation(
        &self,
        query: V4ResourceQuery,
    ) -> Result<V4Conversation, V4ClientError> {
        match self.request(V4Command::ArchiveConversation(query))? {
            V4Event::Conversation(conversation) => Ok(conversation),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Delete one exact conversation. The caller must explicitly reconcile a lost response.
    pub fn delete_conversation(&self, query: V4ResourceQuery) -> Result<(), V4ClientError> {
        match self.request(V4Command::DeleteConversation(query))? {
            V4Event::Deleted(_) => Ok(()),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Apply one validated title, archive, or pin change.
    pub fn update_conversation(
        &self,
        conversation_id: ConversationId,
        patch: V4ConversationPatch,
    ) -> Result<V4Conversation, V4ClientError> {
        match self.request(V4Command::UpdateConversation(conversation_id, patch))? {
            V4Event::Conversation(conversation) => Ok(conversation),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Read one bounded message page. The caller supplies the next cursor.
    pub fn read_messages(&self, query: V4MessageQuery) -> Result<V4MessagePage, V4ClientError> {
        match self.request(V4Command::ReadMessages(query))? {
            V4Event::Messages(page) => Ok(page),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Alias for consumers that present a message list.
    pub fn list_messages(&self, query: V4MessageQuery) -> Result<V4MessagePage, V4ClientError> {
        match self.request(V4Command::ListMessages(query))? {
            V4Event::Messages(page) => Ok(page),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Submit one turn. A duplicate key returns the stored submission and does not replay it.
    pub fn submit_turn(&self, request: V4SubmitTurn) -> Result<V4TurnSubmission, V4ClientError> {
        match self.request(V4Command::SubmitTurn(request))? {
            V4Event::TurnSubmitted(submission) => Ok(submission),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Cancel one exact durable conversation/run pair, once. A completed race
    /// returns its actual terminal state; ambiguity must be reconciled by reads.
    pub fn cancel_turn(
        &self,
        conversation_id: ConversationId,
        run_id: RunId,
    ) -> Result<RunState, V4ClientError> {
        match self.request(V4Command::CancelTurn(
            conversation_id.clone(),
            run_id.clone(),
        ))? {
            V4Event::TurnCancelled(c, r, state) if c == conversation_id && r == run_id => Ok(state),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// Read content events from the last acknowledged sequence.
    pub fn read_content(&self, query: V4ContentQuery) -> Result<V4ContentPage, V4ClientError> {
        match self.request(V4Command::ReadContent(query))? {
            V4Event::Content(page) => Ok(page),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    /// List routes the daemon is willing to advertise. An empty list is honest.
    pub fn list_routes(&self) -> Result<Vec<V4Route>, V4ClientError> {
        match self.request(V4Command::ListRoutes)? {
            V4Event::Routes(routes) => Ok(routes),
            _ => Err(V4ClientError::InvalidResponse),
        }
    }

    fn request(&self, command: V4Command) -> Result<V4Event, V4ClientError> {
        command
            .validate_for_schema(self.schema_version)
            .map_err(|_| V4ClientError::InvalidRequest)?;
        if !self.granted.permits(&command) {
            return Err(V4ClientError::CapabilityDenied);
        }
        let event = exchange(
            &self.client,
            self.schema_version,
            self.granted.clone(),
            command.clone(),
        )?;
        if !command.matches_event(&event) {
            return Err(V4ClientError::InvalidResponse);
        }
        event
            .validate()
            .map_err(|_| V4ClientError::InvalidResponse)?;
        Ok(event)
    }
}

#[cfg(unix)]
fn exchange(
    client: &DaemonClient,
    schema_version: u16,
    grants: V4CapabilitySet,
    command: V4Command,
) -> Result<V4Event, V4ClientError> {
    let request = V4RequestEnvelope {
        version: APP_PROTOCOL_V4,
        schema_version,
        request_id: uuid::Uuid::new_v4().to_string(),
        bearer: client.config.bearer.as_str().to_owned(),
        grants,
        command,
    };
    let mut config = client.config.clone();
    let bytes = if schema_version == APP_SCHEMA_V4 {
        config.max_frame_len = config
            .max_frame_len
            .min(crate::app_core::MAX_V4_CONTROL_BYTES);
        super::unix::round_trip_schema3(&config, &request)
    } else {
        super::unix::round_trip(&config, &request)
    }
    .map_err(|_| V4ClientError::Transport)?;
    decode(&bytes, &request.request_id, schema_version)
}

#[cfg(not(unix))]
fn exchange(
    _: &DaemonClient,
    _: u16,
    _: V4CapabilitySet,
    _: V4Command,
) -> Result<V4Event, V4ClientError> {
    // Stub for Windows named-pipe transport. Retain the exact error so that
    // desktop IPC and all callers can distinguish UnsupportedPlatform from
    // transient transport errors. Implementation of named pipes + JobObject
    // supervision + private store checks is future work.
    Err(V4ClientError::UnsupportedPlatform)
}

fn decode(bytes: &[u8], request_id: &str, schema_version: u16) -> Result<V4Event, V4ClientError> {
    let response: V4ResponseEnvelope =
        serde_json::from_slice(bytes).map_err(|_| V4ClientError::InvalidResponse)?;
    if response.version != APP_PROTOCOL_V4
        || response.schema_version != schema_version
        || response.request_id != request_id
    {
        return Err(V4ClientError::InvalidResponse);
    }
    match response.payload {
        V4ResponsePayload::Ok { event } => match event {
            V4Event::Error(error) => Err(V4ClientError::Rejected(error.code)),
            other => Ok(other),
        },
        V4ResponsePayload::Error { error } => {
            error
                .validate()
                .map_err(|_| V4ClientError::InvalidResponse)?;
            Err(V4ClientError::Rejected(error.code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::V4ClientError;

    #[test]
    fn v4_client_errors_do_not_retain_transport_text() {
        let error = V4ClientError::Transport;
        assert!(!error.to_string().contains("bearer"));
        assert!(error.to_string().contains("transport"));
    }
}

#[cfg(all(test, unix))]
#[path = "v4/cancellation_tests.rs"]
mod cancellation_tests;

#[cfg(all(test, unix))]
#[path = "v4/lookup_tests.rs"]
mod lookup_tests;

#[cfg(all(test, unix))]
#[path = "v4/schema_tests.rs"]
mod schema_tests;

#[cfg(all(test, unix))]
#[path = "v4/page_tests.rs"]
mod page_tests;
