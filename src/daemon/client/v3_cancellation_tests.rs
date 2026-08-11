//! Protocol-v3 cancellation client fixtures, isolated to keep every test
//! module below Abbey's 800-line soft cap.

use super::*;
use crate::app_core::{V3Action, V3ToolApprovalState, V3ToolApprovalStatus};

#[test]
fn v3_tool_cancellation_is_single_shot_correlated_and_bounded() {
    let root = scratch_dir("v3-tool-cancellation");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        let granted = V3CapabilitySet::from_sorted(vec![V3Capability::CancelTools]).unwrap();
        negotiation_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                negotiation_request.request_id,
                V3Event::Negotiated(V3GrantNegotiation {
                    protocol_version: crate::app_core::APP_PROTOCOL_V3,
                    schema_version: crate::app_core::APP_SCHEMA_V3,
                    granted: granted.clone(),
                }),
            )))
            .unwrap();

        for (expected_call_id, response_call_id, state) in [
            (
                "cancel-client-1",
                "cancel-client-1",
                V3ToolApprovalState::Cancelled,
            ),
            (
                "cancel-client-expired",
                "cancel-client-expired",
                V3ToolApprovalState::Expired,
            ),
            (
                "cancel-client-mismatch",
                "different-call",
                V3ToolApprovalState::Cancelled,
            ),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_v3_request(&mut stream);
            assert_eq!(request.grants, granted);
            let V3Command::CancelTool(action) = request.command else {
                panic!("expected exact tool cancellation command");
            };
            assert_eq!(action.resource_id, expected_call_id);
            stream
                .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                    request.request_id,
                    V3Event::ToolApprovalStatus(V3ToolApprovalStatus {
                        tool_id: "abbey_memory_mark_obsolete".into(),
                        call_id: response_call_id.into(),
                        call_digest: "a".repeat(64),
                        state,
                        expires_at_ms: 2_000,
                    }),
                )))
                .unwrap();
        }
    });

    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::CancelTools]).unwrap();
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    let action = |call_id: &str, cancellation_id: &str| V3Action {
        resource_id: call_id.into(),
        operation_id: cancellation_id.into(),
    };
    assert_eq!(
        session
            .cancel_tool(action("cancel-client-1", "cancellation-client-1"))
            .unwrap()
            .state,
        V3ToolApprovalState::Cancelled
    );
    assert_eq!(
        session
            .cancel_tool(action(
                "cancel-client-expired",
                "cancellation-client-expired"
            ))
            .unwrap()
            .state,
        V3ToolApprovalState::Expired
    );
    assert!(matches!(
        session.cancel_tool(action(
            "cancel-client-mismatch",
            "cancellation-client-mismatch"
        )),
        Err(ClientError::InvalidV3Response)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_tool_cancellation_refuses_locally_when_negotiation_denies_the_grant() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::CancelTools]).unwrap();
    let (config, thread, root) = fake_v3_server(|request| {
        encoded_v3_response(V3ResponseEnvelope::ok(
            request.request_id,
            V3Event::Negotiated(V3GrantNegotiation {
                protocol_version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                granted: V3CapabilitySet::deny_all(),
            }),
        ))
    });
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    assert!(matches!(
        session.cancel_tool(V3Action {
            resource_id: "cancel-denied".into(),
            operation_id: "cancellation-denied".into(),
        }),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::CancelTools
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
