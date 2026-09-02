//! Protocol-v3 client fixtures kept separate from the legacy client suite so
//! both test modules remain below Abbey's 800-line soft cap.

use super::*;
use crate::app_core::{
    ClaimStatus, V3ModelAction, V3OperationState, V3OperationStatus, V3ResourceQuery,
    V3StableClaim, V3ToolApprovalState, V3ToolApprovalStatus, V3ToolCall, V3ToolDecision,
    V3ToolDescriptor, V3ToolInvocation, V3ToolPage, V3ToolResult,
};

#[test]
fn v3_session_echoes_exact_negotiated_grants_for_one_model_read() {
    let root = scratch_dir("v3-models");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        assert!(negotiation_request.grants.as_slice().is_empty());
        let V3Command::Negotiate(requested) = negotiation_request.command else {
            panic!("expected negotiation request");
        };
        assert_eq!(requested.requested.as_slice(), &[V3Capability::ReadModels]);
        let granted = V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap();
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

        let (mut model_stream, _) = listener.accept().unwrap();
        let model_request = read_v3_request(&mut model_stream);
        assert_eq!(model_request.grants, granted);
        let V3Command::ListModels(query) = model_request.command else {
            panic!("expected model inventory request");
        };
        assert_eq!(query.after, 0);
        assert_eq!(query.through, Some(1));
        assert_eq!(query.limit, 1);
        model_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                model_request.request_id,
                V3Event::Models(V3EntityPage {
                    after: 0,
                    through: 1,
                    records: vec![V3EntityRecord {
                        id: "abi-local:local".into(),
                        label: "ABI local model local".into(),
                        state: V3OperationState::Available,
                    }],
                }),
            )))
            .unwrap();
    });

    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap();
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    assert_eq!(
        session.negotiation().granted.as_slice(),
        &[V3Capability::ReadModels]
    );
    let page = session
        .list_models(V3PageQuery {
            after: 0,
            through: Some(1),
            limit: 1,
        })
        .unwrap();
    assert_eq!(page.records[0].id, "abi-local:local");
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_rejects_unrequested_grants_and_never_downgrades_or_retries() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap();
    let (config, thread, root) = fake_v3_server(|request| {
        let granted =
            V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels, V3Capability::PollEvents])
                .unwrap();
        encoded_v3_response(V3ResponseEnvelope::ok(
            request.request_id,
            V3Event::Negotiated(V3GrantNegotiation {
                protocol_version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                granted,
            }),
        ))
    });
    assert!(matches!(
        DaemonClient::new(config).negotiate_v3(requested.clone()),
        Err(ClientError::InvalidV3Response)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_v3_server(|request| {
        encoded_v3_response(V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::UnsupportedVersion,
            "protocol version is unsupported",
        ))
    });
    assert!(matches!(
        DaemonClient::new(config).negotiate_v3(requested),
        Err(ClientError::DaemonV3 {
            code: V3ErrorCode::UnsupportedVersion,
            ..
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_model_read_refuses_locally_when_negotiation_denies_the_grant() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap();
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
        session.list_models(V3PageQuery::default()),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::ReadModels
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_model_lifecycle_echoes_exact_grants_and_correlates_every_status() {
    let root = scratch_dir("v3-model-lifecycle");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        let V3Command::Negotiate(requested) = negotiation_request.command else {
            panic!("expected negotiation request");
        };
        let grants = V3CapabilitySet::from_sorted(vec![
            V3Capability::ReadModels,
            V3Capability::DownloadModels,
            V3Capability::ManageModels,
        ])
        .unwrap();
        assert_eq!(requested.requested, grants);
        negotiation_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                negotiation_request.request_id,
                V3Event::Negotiated(V3GrantNegotiation {
                    protocol_version: crate::app_core::APP_PROTOCOL_V3,
                    schema_version: crate::app_core::APP_SCHEMA_V3,
                    granted: grants.clone(),
                }),
            )))
            .unwrap();

        for expected in ["download", "download_status", "load", "inference", "unload"] {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_v3_request(&mut stream);
            assert_eq!(request.grants, grants);
            let (operation_id, state) = match request.command {
                V3Command::DownloadModel(action) => {
                    assert_eq!(expected, "download");
                    (action.operation_id, V3OperationState::Queued)
                }
                V3Command::ModelDownloadStatus(query) => {
                    assert_eq!(expected, "download_status");
                    assert_eq!(query.resource_id, "download-1");
                    (query.resource_id, V3OperationState::Running)
                }
                V3Command::LoadModel(action) => {
                    assert_eq!(expected, "load");
                    (action.operation_id, V3OperationState::Queued)
                }
                V3Command::InferenceStatus(query) => {
                    assert_eq!(expected, "inference");
                    assert_eq!(query.resource_id, "load-1");
                    (query.resource_id, V3OperationState::Succeeded)
                }
                V3Command::UnloadModel(action) => {
                    assert_eq!(expected, "unload");
                    (action.operation_id, V3OperationState::Succeeded)
                }
                other => panic!("unexpected model command: {other:?}"),
            };
            let progress = match state {
                V3OperationState::Running => 5_000,
                V3OperationState::Succeeded => 10_000,
                _ => 0,
            };
            stream
                .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                    request.request_id,
                    V3Event::ModelStatus(V3OperationStatus {
                        operation_id,
                        resource_id: "fixture-bigram".into(),
                        state,
                        progress_basis_points: progress,
                    }),
                )))
                .unwrap();
        }
    });

    let grants = V3CapabilitySet::from_sorted(vec![
        V3Capability::ReadModels,
        V3Capability::DownloadModels,
        V3Capability::ManageModels,
    ])
    .unwrap();
    let session = DaemonClient::new(config).negotiate_v3(grants).unwrap();
    let action = |operation_id: &str| V3ModelAction {
        model_id: "fixture-bigram".into(),
        revision: "0123456789abcdef0123456789abcdef01234567".into(),
        operation_id: operation_id.into(),
    };
    assert_eq!(
        session.download_model(action("download-1")).unwrap().state,
        V3OperationState::Queued
    );
    assert_eq!(
        session
            .model_download_status(V3ResourceQuery {
                resource_id: "download-1".into(),
            })
            .unwrap()
            .progress_basis_points,
        5_000
    );
    assert_eq!(
        session.load_model(action("load-1")).unwrap().state,
        V3OperationState::Queued
    );
    assert_eq!(
        session
            .inference_status(V3ResourceQuery {
                resource_id: "load-1".into(),
            })
            .unwrap()
            .state,
        V3OperationState::Succeeded
    );
    assert_eq!(
        session.unload_model(action("unload-1")).unwrap().state,
        V3OperationState::Succeeded
    );
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_claim_read_echoes_grants_and_requires_the_exact_response_id() {
    let root = scratch_dir("v3-claim");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        let V3Command::Negotiate(requested) = negotiation_request.command else {
            panic!("expected negotiation request");
        };
        assert_eq!(
            requested.requested.as_slice(),
            &[V3Capability::ReadClaimsById]
        );
        let granted = V3CapabilitySet::from_sorted(vec![V3Capability::ReadClaimsById]).unwrap();
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

        let (mut claim_stream, _) = listener.accept().unwrap();
        let claim_request = read_v3_request(&mut claim_stream);
        assert_eq!(claim_request.grants, granted);
        let V3Command::ClaimById(query) = claim_request.command else {
            panic!("expected stable claim request");
        };
        assert_eq!(query.resource_id, "backend-cursor-agent");
        claim_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                claim_request.request_id,
                V3Event::Claim(V3StableClaim {
                    id: query.resource_id,
                    name: "cursor-agent backend (CLI/TUI)".into(),
                    status: ClaimStatus::Current,
                    note: "bounded canonical fixture".into(),
                }),
            )))
            .unwrap();

        let (mut mismatch_stream, _) = listener.accept().unwrap();
        let mismatch_request = read_v3_request(&mut mismatch_stream);
        assert_eq!(mismatch_request.grants, granted);
        assert!(matches!(mismatch_request.command, V3Command::ClaimById(_)));
        mismatch_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                mismatch_request.request_id,
                V3Event::Claim(V3StableClaim {
                    id: "different-stable-id".into(),
                    name: "wrong claim".into(),
                    status: ClaimStatus::Current,
                    note: "must be rejected".into(),
                }),
            )))
            .unwrap();
    });

    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadClaimsById]).unwrap();
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    let claim = session
        .claim_by_id(V3ResourceQuery {
            resource_id: "backend-cursor-agent".into(),
        })
        .unwrap();
    assert_eq!(claim.id, "backend-cursor-agent");
    assert_eq!(claim.status, ClaimStatus::Current);
    assert!(matches!(
        session.claim_by_id(V3ResourceQuery {
            resource_id: "ci-self-hosted-linux-proof".into(),
        }),
        Err(ClientError::InvalidV3Response)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_claim_read_refuses_locally_when_negotiation_denies_the_grant() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadClaimsById]).unwrap();
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
        session.claim_by_id(V3ResourceQuery {
            resource_id: "backend-cursor-agent".into(),
        }),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::ReadClaimsById
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

include!("v3_tool_tests.rs");
