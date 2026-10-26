//! Separately negotiated conversation contracts. These declarations grant no execution authority.
use super::{ConversationId, IdempotencyKey, RunId, RunState, V4Command, ValidationError};
use serde::{Deserialize, Serialize};

mod admin;
mod conversation;
pub use admin::*;
pub use conversation::*;

pub const APP_PROTOCOL_V4: u16 = 4;
/// Schema 2 publishes each route's exact model id as its own field.
///
/// A daemon and a client that disagree on this version fail closed at
/// negotiation instead of one side silently ignoring a field the other
/// requires.
pub const APP_SCHEMA_V4: u16 = 2;
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

pub use super::v4_permission::V4PermissionProfile;

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
