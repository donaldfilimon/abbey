use serde::{Deserialize, Serialize};
use std::fmt;

use crate::app_core::{
    APP_PROTOCOL_V1, APP_PROTOCOL_V3, APP_PROTOCOL_VERSION, APP_SCHEMA_V3, AppCommand, AppEvent,
    V3Command, V3Error, V3ErrorCode, V3Event,
};

/// Original read-only daemon protocol retained by [`DaemonClient`](super::DaemonClient).
pub const PROTOCOL_VERSION: u16 = APP_PROTOCOL_V1;
/// Latest legacy-envelope protocol selected by [`super::DaemonClient`].
///
/// Protocol v3 is intentionally separate and therefore does not silently
/// change the version used by the v1/v2 client.
pub const CURRENT_PROTOCOL_VERSION: u16 = APP_PROTOCOL_VERSION;
pub const SUPPORTED_PROTOCOL_VERSIONS: &[u16] =
    &[PROTOCOL_VERSION, CURRENT_PROTOCOL_VERSION, APP_PROTOCOL_V3];

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope {
    pub version: u16,
    pub request_id: String,
    pub bearer: String,
    pub command: AppCommand,
}

impl fmt::Debug for RequestEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestEnvelope")
            .field("version", &self.version)
            .field("request_id", &self.request_id)
            .field("bearer", &"[REDACTED]")
            .field("command", &self.command)
            .finish()
    }
}

/// Authenticated envelope for the separate protocol-v3 command family.
///
/// Keeping this type separate prevents a v3 command from being decoded as a
/// legacy [`AppCommand`] and preserves the exact v1/v2 wire fixture.
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct V3RequestEnvelope {
    pub version: u16,
    pub schema_version: u16,
    pub request_id: String,
    pub bearer: String,
    pub grants: crate::app_core::V3CapabilitySet,
    pub command: V3Command,
}

impl fmt::Debug for V3RequestEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V3RequestEnvelope")
            .field("version", &self.version)
            .field("schema_version", &self.schema_version)
            .field("request_id", &self.request_id)
            .field("bearer", &"[REDACTED]")
            .field("grants", &self.grants)
            .field("command", &self.command)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResponseEnvelope {
    pub version: u16,
    pub request_id: String,
    pub payload: ResponsePayload,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResponsePayload {
    Ok { event: AppEvent },
    Error { code: String, message: String },
}

impl ResponseEnvelope {
    pub(crate) fn ok_for(version: u16, request_id: String, event: AppEvent) -> Self {
        Self {
            version,
            request_id,
            payload: ResponsePayload::Ok { event },
        }
    }

    pub(crate) fn error(
        request_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::error_for(PROTOCOL_VERSION, request_id, code, message)
    }

    pub(crate) fn error_for(
        version: u16,
        request_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            version,
            request_id: request_id.into(),
            payload: ResponsePayload::Error {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

/// Authenticated response envelope for protocol v3.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct V3ResponseEnvelope {
    pub version: u16,
    pub schema_version: u16,
    pub request_id: String,
    pub payload: V3ResponsePayload,
}

/// Successful or stable bounded protocol-v3 response.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum V3ResponsePayload {
    Ok { event: V3Event },
    Error { error: V3Error },
}

impl V3ResponseEnvelope {
    pub(crate) fn ok(request_id: String, event: V3Event) -> Self {
        Self {
            version: APP_PROTOCOL_V3,
            schema_version: APP_SCHEMA_V3,
            request_id,
            payload: V3ResponsePayload::Ok { event },
        }
    }

    pub(crate) fn error(
        request_id: impl Into<String>,
        code: V3ErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            version: APP_PROTOCOL_V3,
            schema_version: APP_SCHEMA_V3,
            request_id: request_id.into(),
            payload: V3ResponsePayload::Error {
                error: V3Error {
                    code,
                    message: message.into(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_schema_rejects_unknown_envelope_and_outcome_fields() {
        let unknown_envelope = serde_json::json!({
            "version": PROTOCOL_VERSION,
            "request_id": "r1",
            "payload": {"outcome": "error", "code": "denied", "message": "no"},
            "extra": true
        });
        assert!(serde_json::from_value::<ResponseEnvelope>(unknown_envelope).is_err());

        let unknown_outcome = serde_json::json!({
            "version": PROTOCOL_VERSION,
            "request_id": "r1",
            "payload": {
                "outcome": "error",
                "code": "denied",
                "message": "no",
                "extra": true
            }
        });
        assert!(serde_json::from_value::<ResponseEnvelope>(unknown_outcome).is_err());
    }

    #[test]
    fn protocol_v1_envelope_fixture_and_secret_redaction_remain_exact() {
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: "r1".into(),
            bearer: "super-secret-bearer".into(),
            command: AppCommand::Status,
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "version": 1,
                "request_id": "r1",
                "bearer": "super-secret-bearer",
                "command": {"type": "status"}
            })
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("super-secret-bearer"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn protocol_v3_envelope_is_separate_strict_and_redacted() {
        let request = V3RequestEnvelope {
            version: APP_PROTOCOL_V3,
            schema_version: APP_SCHEMA_V3,
            request_id: "v3-models".into(),
            bearer: "super-secret-bearer".into(),
            grants: crate::app_core::V3CapabilitySet::from_sorted(vec![
                crate::app_core::V3Capability::ReadModels,
            ])
            .unwrap(),
            command: V3Command::ListModels(Default::default()),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "version": 3,
                "schema_version": 3,
                "request_id": "v3-models",
                "bearer": "super-secret-bearer",
                "grants": {"capabilities": ["read_models"]},
                "command": {
                    "type": "list_models",
                    "payload": {"after": 0, "through": null, "limit": 32}
                }
            })
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("super-secret-bearer"));
        assert!(debug.contains("[REDACTED]"));
        let mut value = serde_json::to_value(request).unwrap();
        value["extra"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<V3RequestEnvelope>(value).is_err());
    }
}
