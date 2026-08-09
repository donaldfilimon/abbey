use abbey::app_core::{
    APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, APP_SCHEMA_V1, APP_SCHEMA_VERSION, AppCapability,
    AppCommand, AppContext, AppEvent, AppService, BackendSelection, CapabilitySet, ClaimStatus,
    ClaimsQuery, ConversationId, Edition, IdempotencyKey, RunEventPage, RunEventRecord,
    RunEventsQuery, RunId, RunLifecycleEvent, RunMode, RunQuery, RunRequest, RunRouteCapability,
};

/// The compiled edition this test binary was built for. The safe build must
/// still report `standard`; the personal build must report itself rather than
/// impersonating the public edition.
#[cfg(not(feature = "personal-edition"))]
const EXPECTED_EDITION: Edition = Edition::Standard;
#[cfg(feature = "personal-edition")]
const EXPECTED_EDITION: Edition = Edition::Personal;

#[test]
fn public_identifiers_and_commands_have_stable_wire_shapes() {
    let run = RunId::new();
    let conversation = ConversationId::new();
    assert_eq!(
        serde_json::from_str::<RunId>(&serde_json::to_string(&run).unwrap()).unwrap(),
        run
    );
    assert_eq!(
        serde_json::from_str::<ConversationId>(&serde_json::to_string(&conversation).unwrap())
            .unwrap(),
        conversation
    );

    let command = AppCommand::Claims(ClaimsQuery {
        status: Some(ClaimStatus::Proposed),
        contains: Some("desktop".into()),
    });
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({
            "type": "claims",
            "payload": {"status": "proposed", "contains": "desktop"}
        })
    );
    assert!(serde_json::from_str::<AppCommand>(r#"{"type":"unknown"}"#).is_err());
}

#[test]
fn standard_service_advertises_only_implemented_read_capabilities() {
    let event = AppService::default().handle(AppCommand::Status).unwrap();
    let AppEvent::Status(status) = event else {
        panic!("status command must return a status event");
    };

    assert_eq!(status.protocol_version, APP_PROTOCOL_V1);
    assert_eq!(status.schema_version, APP_SCHEMA_V1);
    assert_eq!(status.edition, EXPECTED_EDITION);
    assert!(status.run_routes.is_empty());
    let wire = serde_json::to_value(&status).unwrap();
    assert!(wire.get("run_routes").is_none());
    assert_eq!(
        status.capabilities.as_slice(),
        &[
            AppCapability::ReadStatus,
            AppCapability::ReadClaims,
            AppCapability::ReadRoutes,
        ]
    );
    assert!(status.capabilities.contains(AppCapability::ReadStatus));
    assert!(status.capabilities.contains(AppCapability::ReadClaims));
    assert!(status.capabilities.contains(AppCapability::ReadRoutes));
}

#[test]
fn runtime_v2_context_advertises_only_explicit_routes_and_capabilities() {
    let context = AppContext::runtime_v2(vec![
        RunRouteCapability {
            backend: BackendSelection::Abi,
            modes: vec![RunMode::OneShot, RunMode::Background],
        },
        RunRouteCapability {
            backend: BackendSelection::FoundationModels,
            modes: vec![RunMode::OneShot, RunMode::Background],
        },
    ])
    .unwrap();
    let status = context.status();
    assert_eq!(status.protocol_version, APP_PROTOCOL_VERSION);
    assert_eq!(status.schema_version, APP_SCHEMA_VERSION);
    assert_eq!(status.edition, EXPECTED_EDITION);
    assert_eq!(status.capabilities, CapabilitySet::runtime_v2());
    assert_eq!(status.run_routes.len(), 2);
    status.validate().unwrap();
}

#[test]
fn runtime_v2_context_without_routes_starts_without_submission_authority() {
    let context = AppContext::runtime_v2(Vec::new()).unwrap();
    let status = context.status();
    assert_eq!(status.protocol_version, APP_PROTOCOL_VERSION);
    assert!(status.run_routes.is_empty());
    assert_eq!(
        status.capabilities,
        CapabilitySet::runtime_v2_for_routes(false)
    );
    assert!(status.capabilities.contains(AppCapability::ReadRun));
    assert!(status.capabilities.contains(AppCapability::ReadRunEvents));
    assert!(status.capabilities.contains(AppCapability::CancelRun));
    assert!(!status.capabilities.contains(AppCapability::SubmitRun));
    status.validate().unwrap();
}

#[test]
fn protocol_v2_commands_and_sanitized_events_have_stable_shapes() {
    let run_id = RunId::new();
    let request = RunRequest {
        idempotency_key: "integration-request".parse::<IdempotencyKey>().unwrap(),
        conversation_id: None,
        mode: RunMode::Background,
        backend: BackendSelection::Abi,
        input: "private input".into(),
        labels: vec!["test".into()],
    };
    let command = AppCommand::SubmitRun(request);
    assert_eq!(command.minimum_protocol_version(), APP_PROTOCOL_VERSION);
    command.validate().unwrap();

    assert_eq!(
        serde_json::to_value(AppCommand::GetRun(RunQuery {
            run_id: run_id.clone()
        }))
        .unwrap(),
        serde_json::json!({
            "type": "get_run",
            "payload": {"run_id": run_id}
        })
    );

    let query = RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 0,
        through_sequence: Some(1),
        limit: 16,
    };
    query.validate().unwrap();
    let encoded = serde_json::to_value(AppCommand::RunEvents(query)).unwrap();
    assert_eq!(encoded["type"], "run_events");
    assert_eq!(encoded["payload"]["after_sequence"], 0);
    assert_eq!(encoded["payload"]["through_sequence"], 1);
    assert_eq!(encoded["payload"]["limit"], 16);

    let page = RunEventPage {
        run_id: run_id.clone(),
        events: vec![RunEventRecord {
            run_id: run_id.clone(),
            sequence: 1,
            recorded_at: "2026-08-08T12:00:00Z".into(),
            event: RunLifecycleEvent::Queued,
        }],
        after_sequence: 0,
        next_after_sequence: 1,
        through_sequence: 1,
        has_more: false,
    };
    page.validate().unwrap();
    let encoded = serde_json::to_string(&AppEvent::RunEvents(page)).unwrap();
    assert!(!encoded.contains("private input"));
    assert!(!encoded.contains("payload_json"));
}

#[test]
fn capability_deserialization_fails_closed_on_duplicates_or_reordering() {
    let duplicate = serde_json::from_value::<CapabilitySet>(serde_json::json!({
        "capabilities": ["read_status", "read_status"]
    }))
    .unwrap();
    assert!(duplicate.validate().is_err());

    let reordered = serde_json::from_value::<CapabilitySet>(serde_json::json!({
        "capabilities": ["read_claims", "read_status"]
    }))
    .unwrap();
    assert!(reordered.validate().is_err());
}
