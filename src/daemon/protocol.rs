use serde::{Deserialize, Serialize};
use std::fmt;

use crate::app_core::{APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, AppCommand, AppEvent};

/// Original read-only daemon protocol retained by [`DaemonClient`](super::DaemonClient).
pub const PROTOCOL_VERSION: u16 = APP_PROTOCOL_V1;
/// Latest daemon protocol understood by the server.
pub const CURRENT_PROTOCOL_VERSION: u16 = APP_PROTOCOL_VERSION;
pub const SUPPORTED_PROTOCOL_VERSIONS: &[u16] = &[PROTOCOL_VERSION, CURRENT_PROTOCOL_VERSION];

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
}
