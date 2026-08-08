//! Stable serializable types at Abbey's application boundary.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};
use uuid::Uuid;

/// Version of the command/event exchange protocol.
pub const APP_PROTOCOL_VERSION: u16 = 1;
/// Version of the serialized application-state schema.
pub const APP_SCHEMA_VERSION: u16 = 1;

const MAX_CLAIMS_FILTER_BYTES: usize = 256;
const MAX_APPROVAL_SUMMARY_BYTES: usize = 1_024;

macro_rules! string_uuid_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Generate a new opaque identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            /// Return the canonical lower-case UUID representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let parsed = Uuid::parse_str(value).map_err(|_| IdError)?;
                if parsed.to_string() != value {
                    return Err(IdError);
                }
                Ok(Self(value.to_owned()))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value
                    .parse()
                    .map_err(|_| de::Error::custom("expected a canonical UUID"))
            }
        }
    };
}

string_uuid_id!(RunId, "Identifier for one application run.");
string_uuid_id!(
    ConversationId,
    "Identifier for a conversation spanning one or more runs."
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdError;

impl fmt::Display for IdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier must be a canonical lower-case UUID")
    }
}

impl std::error::Error for IdError {}

/// Abbey edition represented by this public-safe contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edition {
    Standard,
}

/// Commands accepted by the initial shared application service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AppCommand {
    Status,
    Claims(ClaimsQuery),
}

impl AppCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Status => Ok(()),
            Self::Claims(query) => query.validate(),
        }
    }
}

/// Events emitted by the presentation-neutral application service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AppEvent {
    Status(RuntimeStatus),
    Claims(ClaimsSnapshot),
    ApprovalRequested(ApprovalRequest),
}

/// Runtime state exposed without process or user-state details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Ready,
}

/// Safe, immutable process identity and supported application operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeStatus {
    pub protocol_version: u16,
    pub schema_version: u16,
    pub edition: Edition,
    pub state: RuntimeState,
    pub version: String,
    pub build_git: String,
    pub build_target: String,
    pub capabilities: CapabilitySet,
}

/// Application operations granted by this edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppCapability {
    ReadStatus,
    ReadClaims,
}

/// Ordered, duplicate-free capability declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySet {
    capabilities: Vec<AppCapability>,
}

impl CapabilitySet {
    #[must_use]
    pub fn standard() -> Self {
        Self {
            capabilities: vec![AppCapability::ReadStatus, AppCapability::ReadClaims],
        }
    }

    #[must_use]
    pub fn contains(&self, capability: AppCapability) -> bool {
        self.capabilities.binary_search(&capability).is_ok()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[AppCapability] {
        &self.capabilities
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.capabilities.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ValidationError::new(
                "capabilities must be strictly ordered and duplicate-free",
            ));
        }
        Ok(())
    }
}

/// Canonical claim status, independent of terminal rendering labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Current,
    Partial,
    Proposed,
    Blocked,
    OutOfScope,
}

/// Bounded, typed query over the canonical claims table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClaimsQuery {
    pub status: Option<ClaimStatus>,
    pub contains: Option<String>,
}

impl ClaimsQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let Some(filter) = self.contains.as_deref() else {
            return Ok(());
        };
        let filter = filter.trim();
        if filter.is_empty() {
            return Err(ValidationError::new("claims filter cannot be empty"));
        }
        if filter.len() > MAX_CLAIMS_FILTER_BYTES {
            return Err(ValidationError::new("claims filter exceeds 256 bytes"));
        }
        if filter.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "claims filter cannot contain control characters",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRecord {
    pub name: String,
    pub status: ClaimStatus,
    pub note: String,
    pub instead: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimsSnapshot {
    pub claims: Vec<ClaimRecord>,
    pub matched: usize,
}

/// Sensitive operation class shown to a user before any future execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    FileSystemMutation,
    NetworkAccess,
    ProcessExecution,
    PrivilegeElevation,
}

/// A presentation-neutral approval prompt. This type grants no authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequest {
    pub run_id: RunId,
    pub kind: ApprovalKind,
    pub summary: String,
}

impl ApprovalRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let summary = self.summary.trim();
        if summary.is_empty() {
            return Err(ValidationError::new("approval summary cannot be empty"));
        }
        if summary.len() > MAX_APPROVAL_SUMMARY_BYTES {
            return Err(ValidationError::new("approval summary exceeds 1024 bytes"));
        }
        if summary.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "approval summary cannot contain control characters",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    message: &'static str,
}

impl ValidationError {
    #[must_use]
    pub const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip_as_canonical_strings() {
        let id = RunId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<RunId>(&json).unwrap(), id);
        assert!(serde_json::from_str::<RunId>("\"NOT-A-UUID\"").is_err());
        assert!(
            "550E8400-E29B-41D4-A716-446655440000"
                .parse::<RunId>()
                .is_err()
        );
    }

    #[test]
    fn commands_have_a_stable_tagged_wire_shape() {
        let command = AppCommand::Claims(ClaimsQuery {
            status: Some(ClaimStatus::Current),
            contains: Some("memory".into()),
        });
        assert_eq!(
            serde_json::to_value(command).unwrap(),
            serde_json::json!({
                "type": "claims",
                "payload": {"status": "current", "contains": "memory"}
            })
        );
    }

    #[test]
    fn events_have_a_stable_tagged_wire_shape() {
        let event = AppEvent::Claims(ClaimsSnapshot {
            claims: Vec::new(),
            matched: 0,
        });
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "type": "claims",
                "payload": {"claims": [], "matched": 0}
            })
        );
    }

    #[test]
    fn free_text_fields_are_bounded_and_reject_controls() {
        let empty = ClaimsQuery {
            contains: Some("  ".into()),
            ..ClaimsQuery::default()
        };
        assert!(empty.validate().is_err());

        let control = ApprovalRequest {
            run_id: RunId::new(),
            kind: ApprovalKind::NetworkAccess,
            summary: "request\nsecret".into(),
        };
        assert!(control.validate().is_err());
    }

    #[test]
    fn standard_capabilities_are_read_only() {
        let capabilities = CapabilitySet::standard();
        capabilities.validate().unwrap();
        assert_eq!(
            capabilities.as_slice(),
            &[AppCapability::ReadStatus, AppCapability::ReadClaims]
        );
    }
}
