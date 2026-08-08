use serde::{Deserialize, Serialize};

use crate::app_core::{APP_PROTOCOL_VERSION, AppCommand, AppEvent};

pub const PROTOCOL_VERSION: u16 = APP_PROTOCOL_VERSION;

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope {
    pub version: u16,
    pub request_id: String,
    pub bearer: String,
    pub command: AppCommand,
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
    pub(crate) fn ok(request_id: String, event: AppEvent) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            payload: ResponsePayload::Ok { event },
        }
    }

    pub(crate) fn error(
        request_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
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
}
