//! Incompatible v1/v3 schemas must fail before dispatch can acquire authority.
use super::*;
use crate::daemon::ResponsePayload;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counter(AtomicUsize);
impl DaemonHandler for Counter {
    fn supports_version(&self, version: u16) -> bool {
        matches!(version, 1 | 2)
    }
    fn handle_versioned(&self, _: u16, _: AppCommand) -> Result<AppEvent, HandlerFailure> {
        self.0.fetch_add(1, Ordering::Relaxed);
        crate::app_core::AppService::default()
            .handle(AppCommand::Status)
            .map_err(|_| HandlerFailure::new("internal", "fixture status failed"))
    }
    fn supports_v3(&self) -> bool {
        true
    }
    fn authorizes_v3(&self, _: &V3CapabilitySet, _: &V3Command) -> bool {
        true
    }
    fn handle_v3(&self, _: V3Command) -> Result<V3Event, HandlerFailure> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(V3Event::Negotiated(crate::app_core::V3GrantNegotiation {
            protocol_version: crate::app_core::APP_PROTOCOL_V3,
            schema_version: crate::app_core::APP_SCHEMA_V3,
            granted: V3CapabilitySet::deny_all(),
        }))
    }
}

#[test]
fn completion_legacy_v3_schema_never_reaches_authority_dispatch() {
    let handler = Counter(AtomicUsize::new(0));
    for schema_version in [0, 1, 2, 3, u16::MAX] {
        let response = dispatch_authenticated_v3(
            V3RequestEnvelope {
                version: 3,
                schema_version,
                request_id: "schema-check".into(),
                bearer: "owned".into(),
                grants: V3CapabilitySet::deny_all(),
                command: V3Command::Negotiate(crate::app_core::V3GrantRequest {
                    supported_versions: vec![3],
                    requested: V3CapabilitySet::deny_all(),
                }),
            },
            &handler,
        );
        assert!(matches!(
            response.payload,
            super::super::V3ResponsePayload::Error {
                error: crate::app_core::V3Error {
                    code: V3ErrorCode::UnsupportedVersion,
                    ..
                }
            }
        ));
    }
    assert_eq!(handler.0.load(Ordering::Relaxed), 0);
}

#[test]
fn completion_legacy_v1_envelope_never_reaches_claim_dispatch() {
    let handler = Counter(AtomicUsize::new(0));
    for command in [
        AppCommand::Status,
        AppCommand::Claims(crate::app_core::ClaimsQuery::default()),
    ] {
        for schema in [None, Some(1), Some(3), Some(u16::MAX)] {
            let mut request: RequestEnvelope = serde_json::from_value(serde_json::json!({
                "version": 1, "request_id": "old-reader", "bearer": "owned", "command": command,
            }))
            .unwrap();
            request.schema_version = schema;
            let response = dispatch_authenticated(request, &handler);
            assert!(
                matches!(response.payload, ResponsePayload::Error { ref code, .. } if code == "unsupported_version")
            );
        }
    }
    assert_eq!(handler.0.load(Ordering::Relaxed), 0);
}

#[test]
fn completion_current_v1_schema_and_unchanged_v2_dispatch() {
    let handler = Counter(AtomicUsize::new(0));
    for (version, schema_version) in [(1, Some(2)), (2, None)] {
        let request = RequestEnvelope {
            version,
            schema_version,
            request_id: "current-reader".into(),
            bearer: "owned".into(),
            command: AppCommand::Status,
        };
        if version == 2 {
            assert_eq!(
                serde_json::to_value(&request).unwrap(),
                serde_json::json!({
                    "version": 2, "request_id": "current-reader", "bearer": "owned", "command": {"type": "status"}
                })
            );
        }
        assert!(matches!(
            dispatch_authenticated(request, &handler).payload,
            ResponsePayload::Ok { .. }
        ));
    }
    assert_eq!(handler.0.load(Ordering::Relaxed), 2);
}

#[test]
fn completion_v2_rejects_an_explicit_null_schema_header() {
    assert!(
        serde_json::from_value::<RequestEnvelope>(serde_json::json!({
            "version": 2, "schema_version": null, "request_id": "v2-reader",
            "bearer": "owned", "command": {"type": "status"}
        }))
        .is_err()
    );
}
