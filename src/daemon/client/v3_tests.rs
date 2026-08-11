//! Protocol-v3 client fixtures kept separate from the legacy client suite so
//! both test modules remain below Abbey's 800-line soft cap.

use super::*;
use crate::app_core::{ClaimStatus, V3ResourceQuery, V3StableClaim, V3ToolDescriptor, V3ToolPage};

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

#[test]
fn v3_tool_inventory_echoes_grants_and_correlates_the_fixed_page() {
    let root = scratch_dir("v3-tools");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        let V3Command::Negotiate(requested) = negotiation_request.command else {
            panic!("expected negotiation request");
        };
        assert_eq!(requested.requested.as_slice(), &[V3Capability::ListTools]);
        let granted = V3CapabilitySet::from_sorted(vec![V3Capability::ListTools]).unwrap();
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

        let (mut tools_stream, _) = listener.accept().unwrap();
        let tools_request = read_v3_request(&mut tools_stream);
        assert_eq!(tools_request.grants, granted);
        let V3Command::ListTools(query) = tools_request.command else {
            panic!("expected tool inventory request");
        };
        assert_eq!(query.after, 1);
        assert_eq!(query.through, Some(2));
        assert_eq!(query.limit, 1);
        tools_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                tools_request.request_id,
                V3Event::Tools(V3ToolPage {
                    after: 1,
                    through: 2,
                    tools: vec![V3ToolDescriptor {
                        tool_id: "abbey_claims".into(),
                        description: "Read bounded canonical claims".into(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "additionalProperties": false
                        }),
                    }],
                }),
            )))
            .unwrap();

        let (mut mismatch_stream, _) = listener.accept().unwrap();
        let mismatch_request = read_v3_request(&mut mismatch_stream);
        assert_eq!(mismatch_request.grants, granted);
        assert!(matches!(mismatch_request.command, V3Command::ListTools(_)));
        mismatch_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                mismatch_request.request_id,
                V3Event::Tools(V3ToolPage {
                    after: 0,
                    through: 2,
                    tools: Vec::new(),
                }),
            )))
            .unwrap();
    });

    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ListTools]).unwrap();
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    let query = V3PageQuery {
        after: 1,
        through: Some(2),
        limit: 1,
    };
    let page = session.list_tools(query).unwrap();
    assert_eq!(page.tools[0].tool_id, "abbey_claims");
    assert!(matches!(
        session.list_tools(query),
        Err(ClientError::InvalidV3Response)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_tool_inventory_refuses_locally_when_negotiation_denies_the_grant() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ListTools]).unwrap();
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
        session.list_tools(V3PageQuery::default()),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::ListTools
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
