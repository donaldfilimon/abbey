//! Schema-aware authenticated envelopes. Bearer values are always redacted.
use super::*;
use crate::app_core::V4Event;
use std::fmt;

/// Authenticated envelope for the separate protocol-v4 command family.
///
/// This is not a v1, v2, or v3 command. Decoding it as [`crate::daemon::RequestEnvelope`]
/// would either fail or, worse, accept a conversation body as a legacy event.
#[derive(Clone, Serialize, PartialEq)]
pub struct V4RequestEnvelope {
    pub version: u16,
    pub schema_version: u16,
    pub request_id: String,
    pub bearer: String,
    pub grants: crate::app_core::V4CapabilitySet,
    pub command: V4Command,
}

impl fmt::Debug for V4RequestEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V4RequestEnvelope")
            .field("version", &self.version)
            .field("schema_version", &self.schema_version)
            .field("request_id", &self.request_id)
            .field("bearer", &"[REDACTED]")
            .field("grants", &self.grants)
            .field("command", &self.command)
            .finish()
    }
}

/// Authenticated response envelope for protocol v4.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct V4ResponseEnvelope {
    pub version: u16,
    pub schema_version: u16,
    pub request_id: String,
    pub payload: V4ResponsePayload,
}

/// Successful or stable bounded protocol-v4 response.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum V4ResponsePayload {
    Ok { event: V4Event },
    Error { error: V4Error },
}
