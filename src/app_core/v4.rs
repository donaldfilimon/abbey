//! Separately negotiated conversation contracts. These declarations grant no execution authority.
use super::{ConversationId, IdempotencyKey, RunId, RunState, V4Command, ValidationError};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const APP_PROTOCOL_V4: u16 = 4;
pub const APP_SCHEMA_V4: u16 = 1;
pub const MAX_V4_INPUT_BYTES: usize = 32 * 1024;
pub const MAX_V4_TITLE_BYTES: usize = 256;
pub const MAX_V4_PAGE: u16 = 50;
pub const MAX_V4_PAGE_BYTES: usize = 512 * 1024;
pub const MAX_V4_CONTENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_V4_CONTENT_EVENTS: u64 = 65_536;
pub const MAX_V4_DELTA_BYTES: usize = 16 * 1024;
pub const MAX_V4_QUERY_BYTES: usize = 256;
// JavaScript safe integer ceiling; timestamps use Unix milliseconds.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub(super) fn ensure(ok: bool) -> Result<(), ValidationError> {
    if ok {
        Ok(())
    } else {
        Err(ValidationError::new("invalid v4 contract"))
    }
}
pub(super) fn text(value: &str, max: usize) -> Result<(), ValidationError> {
    ensure(!value.trim().is_empty() && value.len() <= max && !value.contains('\0'))
}
pub(super) fn label(value: &str) -> Result<(), ValidationError> {
    text(value, 256)?;
    ensure(!value.chars().any(char::is_control))
}
pub(super) fn route(value: &str) -> Result<(), ValidationError> {
    ensure(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)),
    )
}
pub(super) fn timestamp(value: u64) -> Result<(), ValidationError> {
    ensure(value <= MAX_SAFE_INTEGER)
}
pub(super) fn limit(value: u16) -> Result<(), ValidationError> {
    ensure((1..=MAX_V4_PAGE).contains(&value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum V4Capability {
    ReadConversations,
    WriteConversations,
    SubmitTurns,
    ReadTurns,
    CancelTurns,
    ReadRoutes,
    ReadTasks,
    WriteTasks,
    ReadAutomations,
    WriteAutomations,
    ReadPermissions,
    WritePermissions,
    DecideActions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct V4CapabilitySet {
    capabilities: Vec<V4Capability>,
}

impl V4CapabilitySet {
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure(!self.capabilities.windows(2).any(|p| p[0] >= p[1]))
    }
}
impl V4CapabilitySet {
    pub fn from_sorted(capabilities: Vec<V4Capability>) -> Result<Self, ValidationError> {
        let set = Self { capabilities };
        set.validate()?;
        Ok(set)
    }
    pub const fn deny_all() -> Self {
        Self {
            capabilities: Vec::new(),
        }
    }
    pub fn as_slice(&self) -> &[V4Capability] {
        &self.capabilities
    }
    pub fn contains(&self, capability: V4Capability) -> bool {
        self.capabilities.binary_search(&capability).is_ok()
    }
    pub fn permits(&self, command: &V4Command) -> bool {
        command
            .required_capability()
            .is_none_or(|c| self.contains(c))
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4Conversation {
    pub conversation_id: ConversationId,
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub archived: bool,
    pub pinned: bool,
    pub sequence: u64,
    pub message_count: u64,
}

impl V4Conversation {
    pub fn validate(&self) -> Result<(), ValidationError> {
        label(&self.title)?;
        timestamp(self.updated_at_ms)?;
        timestamp(self.sequence)?;
        timestamp(self.message_count)?;
        ensure(self.sequence > 0 && self.created_at_ms <= self.updated_at_ms)
    }
}
impl fmt::Debug for V4Conversation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4Conversation([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ConversationPatch {
    pub title: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
}

impl V4ConversationPatch {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(t) = &self.title {
            label(t)?;
        }
        ensure(self.title.is_some() || self.archived.is_some() || self.pinned.is_some())
    }
}
impl fmt::Debug for V4ConversationPatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4ConversationPatch([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ConversationQuery {
    pub search: Option<String>,
    pub after: Option<ConversationId>,
    pub limit: u16,
    pub archived: bool,
}

impl V4ConversationQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        limit(self.limit)?;
        if let Some(s) = &self.search {
            text(s, MAX_V4_QUERY_BYTES)?;
        }
        Ok(())
    }
}
impl fmt::Debug for V4ConversationQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4ConversationQuery([redacted])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct V4MessageQuery {
    pub conversation_id: ConversationId,
    pub after: Option<u64>,
    pub limit: u16,
}

impl V4MessageQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        limit(self.limit)?;
        if let Some(s) = self.after {
            timestamp(s)?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4SubmitTurn {
    pub conversation_id: ConversationId,
    pub idempotency_key: IdempotencyKey,
    pub route_id: String,
    pub input: String,
}

impl V4SubmitTurn {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.route_id)?;
        text(&self.input, MAX_V4_INPUT_BYTES)
    }
}
impl fmt::Debug for V4SubmitTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4SubmitTurn([redacted])")
    }
}

/// Name used by the runtime store for one turn submission.
pub type V4TurnSubmit = V4SubmitTurn;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct V4TurnSubmission {
    pub conversation_id: ConversationId,
    pub run_id: RunId,
    pub idempotency_key: IdempotencyKey,
    pub user_message_sequence: u64,
    pub state: RunState,
    pub duplicate: bool,
}

impl V4TurnSubmission {
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.user_message_sequence > 0 && self.user_message_sequence <= MAX_SAFE_INTEGER)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum V4MessageRole {
    User,
    Assistant,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4Message {
    pub conversation_id: ConversationId,
    pub run_id: RunId,
    pub sequence: u64,
    pub role: V4MessageRole,
    pub state: RunState,
    pub text: String,
    pub text_bytes: u64,
    pub text_complete: bool,
    pub created_at_ms: u64,
}

impl V4Message {
    pub fn validate(&self) -> Result<(), ValidationError> {
        timestamp(self.created_at_ms)?;
        ensure(
            self.sequence > 0
                && self.sequence <= MAX_SAFE_INTEGER
                && self.text_bytes <= MAX_V4_CONTENT_BYTES as u64
                && self.text.len() as u64 <= self.text_bytes
                && self.text_complete == (self.text.len() as u64 == self.text_bytes)
                && self.text.len()
                    <= if self.role == V4MessageRole::User {
                        MAX_V4_INPUT_BYTES
                    } else {
                        MAX_V4_DELTA_BYTES
                    }
                && (self.role != V4MessageRole::User || self.text_complete)
                && !self.text.contains('\0'),
        )
    }
}
impl fmt::Debug for V4Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4Message([redacted])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct V4Route {
    pub route_id: String,
    pub backend: String,
    pub label: String,
    pub available: bool,
}

impl V4Route {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.route_id)?;
        route(&self.backend)?;
        label(&self.label)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ConversationPage {
    pub conversations: Vec<V4Conversation>,
    pub next_after: Option<ConversationId>,
}

impl V4ConversationPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        page_bytes(self)?;
        ensure(self.conversations.len() <= usize::from(MAX_V4_PAGE))?;
        for c in &self.conversations {
            c.validate()?;
        }
        ensure(self.next_after.as_ref().is_none_or(|id| {
            self.conversations
                .last()
                .is_some_and(|c| &c.conversation_id == id)
        }))
    }
}
impl fmt::Debug for V4ConversationPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4ConversationPage([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4MessagePage {
    pub conversation_id: ConversationId,
    pub messages: Vec<V4Message>,
    pub next_after: Option<u64>,
}

impl V4MessagePage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        page_bytes(self)?;
        ensure(self.messages.len() <= usize::from(MAX_V4_PAGE))?;
        let mut last = 0;
        for m in &self.messages {
            m.validate()?;
            ensure(m.conversation_id == self.conversation_id && m.sequence > last)?;
            last = m.sequence;
        }
        ensure(
            self.next_after
                .is_none_or(|s| !self.messages.is_empty() && s == last),
        )
    }
}
impl fmt::Debug for V4MessagePage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4MessagePage([redacted])")
    }
}

pub(super) fn page_bytes<T: Serialize>(value: &T) -> Result<(), ValidationError> {
    ensure(
        serde_json::to_vec(value)
            .map_err(|_| ValidationError::new("invalid v4 page"))?
            .len()
            <= MAX_V4_PAGE_BYTES,
    )
}

impl<'de> Deserialize<'de> for V4CapabilitySet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            capabilities: Vec<V4Capability>,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            capabilities: w.capabilities,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4Conversation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            id: String,
            title: String,
            created_at_ms: u64,
            updated_at_ms: u64,
            archived: bool,
            pinned: bool,
            sequence: u64,
            message_count: u64,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        ensure(w.id == w.conversation_id.to_string()).map_err(serde::de::Error::custom)?;
        let value = Self {
            conversation_id: w.conversation_id,
            id: w.id,
            title: w.title,
            created_at_ms: w.created_at_ms,
            updated_at_ms: w.updated_at_ms,
            archived: w.archived,
            pinned: w.pinned,
            sequence: w.sequence,
            message_count: w.message_count,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ConversationPatch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            title: Option<String>,
            archived: Option<bool>,
            pinned: Option<bool>,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            title: w.title,
            archived: w.archived,
            pinned: w.pinned,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ConversationQuery {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            search: Option<String>,
            after: Option<ConversationId>,
            limit: u16,
            archived: bool,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            search: w.search,
            after: w.after,
            limit: w.limit,
            archived: w.archived,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4MessageQuery {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            after: Option<u64>,
            limit: u16,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            after: w.after,
            limit: w.limit,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4SubmitTurn {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            idempotency_key: IdempotencyKey,
            route_id: String,
            input: String,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            idempotency_key: w.idempotency_key,
            route_id: w.route_id,
            input: w.input,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4TurnSubmission {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            run_id: RunId,
            idempotency_key: IdempotencyKey,
            user_message_sequence: u64,
            state: RunState,
            duplicate: bool,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            run_id: w.run_id,
            idempotency_key: w.idempotency_key,
            user_message_sequence: w.user_message_sequence,
            state: w.state,
            duplicate: w.duplicate,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4Message {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            run_id: RunId,
            sequence: u64,
            role: V4MessageRole,
            state: RunState,
            text: String,
            text_bytes: u64,
            text_complete: bool,
            created_at_ms: u64,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            run_id: w.run_id,
            sequence: w.sequence,
            role: w.role,
            state: w.state,
            text: w.text,
            text_bytes: w.text_bytes,
            text_complete: w.text_complete,
            created_at_ms: w.created_at_ms,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4Route {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            route_id: String,
            backend: String,
            label: String,
            available: bool,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            route_id: w.route_id,
            backend: w.backend,
            label: w.label,
            available: w.available,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ConversationPage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversations: Vec<V4Conversation>,
            next_after: Option<ConversationId>,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversations: w.conversations,
            next_after: w.next_after,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4MessagePage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            messages: Vec<V4Message>,
            next_after: Option<u64>,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            messages: w.messages,
            next_after: w.next_after,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl V4TurnSubmission {
    pub fn validate_for(&self, request: &V4SubmitTurn) -> Result<(), ValidationError> {
        self.validate()?;
        request.validate()?;
        ensure(
            self.conversation_id == request.conversation_id
                && self.idempotency_key == request.idempotency_key,
        )
    }
}

impl<'de> Deserialize<'de> for V4Capability {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Wire {
            ReadConversations,
            WriteConversations,
            SubmitTurns,
            ReadTurns,
            CancelTurns,
            ReadRoutes,
        }
        Ok(
            match Wire::deserialize(deserializer)
                .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
            {
                Wire::ReadConversations => Self::ReadConversations,
                Wire::WriteConversations => Self::WriteConversations,
                Wire::SubmitTurns => Self::SubmitTurns,
                Wire::ReadTurns => Self::ReadTurns,
                Wire::CancelTurns => Self::CancelTurns,
                Wire::ReadRoutes => Self::ReadRoutes,
            },
        )
    }
}

impl<'de> Deserialize<'de> for V4MessageRole {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Wire {
            User,
            Assistant,
        }
        Ok(
            match Wire::deserialize(deserializer)
                .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
            {
                Wire::User => Self::User,
                Wire::Assistant => Self::Assistant,
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct V4PageQuery {
    pub after: u64,
    pub through: Option<u64>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4SearchQuery {
    pub query: String,
    pub page: V4PageQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4ResourceQuery {
    pub resource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4Task {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4TaskPage {
    pub tasks: Vec<V4Task>,
    pub next_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4TaskCreate {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4TaskState {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4Automation {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4AutomationPage {
    pub automations: Vec<V4Automation>,
    pub next_after: Option<u64>,
}

// Task2 support (minimal to enable store compile against contracts)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4PermissionProfile {
    pub profile_id: String,
    pub tool_ids: Vec<String>,
    pub application_ids: Vec<String>,
    pub filesystem_roots: Vec<String>,
    pub browser_domains: Vec<String>,
    pub max_runtime_ms: u64,
    pub max_output_bytes: u64,
}

// Missing v4 protocol types (stubs sufficient for compilation and current tests; full validation later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4Error {
    pub code: crate::app_core::V4ErrorCode,
    pub message: String,
}

impl V4Error {
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure(
            !self.message.is_empty()
                && self.message.len() <= 256
                && !self.message.chars().any(char::is_control),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum V4ActionDecisionKind {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4Action {
    pub resource_id: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4GrantNegotiation {
    pub protocol_version: u16,
    pub schema_version: u16,
    pub granted: V4CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4GrantRequest {
    pub supported_versions: Vec<u16>,
    pub requested: V4CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum V4TurnState {
    Queued,
    Starting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    CancelRequested,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4AutomationUpsert {
    pub automation_id: String,
    pub instruction: String,
    pub schedule: V4ScheduleKind,
    pub cron: Option<String>,
    pub timezone: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub budget_tokens: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum V4ScheduleKind {
    Cron,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4ConversationCreate {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4ConversationRename {
    pub conversation_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4ActionDecision {
    pub action_id: String,
    pub decision_id: String,
    pub kind: V4ActionDecisionKind,
}
