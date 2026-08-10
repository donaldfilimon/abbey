use super::*;
use crate::app_core::{
    AppCapability, AppCommand, AppEvent, CapabilitySet, ClaimsQuery, ClaimsSnapshot,
};

fn request() -> RunRequest {
    RunRequest {
        idempotency_key: "client:request-1".parse().unwrap(),
        conversation_id: Some(ConversationId::new()),
        mode: RunMode::Background,
        backend: BackendSelection::Abi,
        input: "Summarize the checked workspace.".into(),
        labels: vec!["workspace".into(), "summary".into()],
    }
}

fn snapshot(state: RunState, failure: Option<RunFailure>) -> RunSnapshot {
    RunSnapshot {
        run_id: RunId::new(),
        conversation_id: None,
        idempotency_key: IdempotencyKey::new(),
        state,
        created_at: "2026-08-08T12:00:00Z".into(),
        updated_at: "2026-08-08T12:00:02Z".into(),
        failure,
        event_count: 4,
    }
}

#[test]
fn protocol_v1_read_only_fixtures_remain_exact() {
    assert_eq!(
        serde_json::to_value(AppCommand::Claims(ClaimsQuery::default())).unwrap(),
        serde_json::json!({
            "type": "claims",
            "payload": {"status": null, "contains": null}
        })
    );
    assert_eq!(
        serde_json::to_value(AppEvent::Claims(ClaimsSnapshot {
            claims: Vec::new(),
            matched: 0,
        }))
        .unwrap(),
        serde_json::json!({
            "type": "claims",
            "payload": {"claims": [], "matched": 0}
        })
    );
    assert_eq!(
        CapabilitySet::standard().as_slice(),
        &[
            AppCapability::ReadStatus,
            AppCapability::ReadClaims,
            AppCapability::ReadRoutes,
        ]
    );
}

#[test]
fn idempotency_keys_are_bounded_and_wire_validated() {
    let key: IdempotencyKey = "client:request-1".parse().unwrap();
    assert_eq!(serde_json::to_string(&key).unwrap(), "\"client:request-1\"");
    assert!("".parse::<IdempotencyKey>().is_err());
    assert!("contains space".parse::<IdempotencyKey>().is_err());
    assert!(serde_json::from_value::<IdempotencyKey>(serde_json::json!("x".repeat(129))).is_err());
}

#[test]
fn requests_bound_input_labels_and_unknown_fields() {
    request().validate().unwrap();
    let mut oversized = request();
    oversized.input = "x".repeat(MAX_INPUT_BYTES + 1);
    assert!(oversized.validate().is_err());

    let mut too_many_labels = request();
    too_many_labels.labels = vec!["label".into(); MAX_LABELS + 1];
    assert!(too_many_labels.validate().is_err());

    let mut json = serde_json::to_value(request()).unwrap();
    json.as_object_mut()
        .unwrap()
        .insert("argv".into(), serde_json::json!(["sh", "-c"]));
    assert!(serde_json::from_value::<RunRequest>(json).is_err());
    let debug = format!("{:?}", request());
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("Summarize the checked workspace"));
}

#[test]
fn protocol_v2_run_command_fixture_is_exact_and_rejects_unknown_fields() {
    let request = request();
    let command = AppCommand::SubmitRun(request.clone());
    assert_eq!(command.minimum_protocol_version(), 2);
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({
            "type": "submit_run",
            "payload": {
                "idempotency_key": "client:request-1",
                "conversation_id": request.conversation_id,
                "mode": "background",
                "backend": "abi",
                "input": "Summarize the checked workspace.",
                "labels": ["workspace", "summary"]
            }
        })
    );

    let invalid = serde_json::json!({
        "type": "get_run",
        "payload": {"run_id": RunId::new(), "extra": true}
    });
    assert!(serde_json::from_value::<AppCommand>(invalid).is_err());
}

#[test]
fn event_queries_are_bounded_and_watermark_consistent() {
    let run_id = RunId::new();
    let query: RunEventsQuery = serde_json::from_value(serde_json::json!({
        "run_id": run_id
    }))
    .unwrap();
    assert_eq!(query.after_sequence, 0);
    assert_eq!(query.through_sequence, None);
    assert_eq!(query.limit, MAX_RUN_EVENT_PAGE);
    query.validate().unwrap();

    let mut invalid = query.clone();
    invalid.limit = 0;
    assert!(invalid.validate().is_err());
    invalid.limit = MAX_RUN_EVENT_PAGE + 1;
    assert!(invalid.validate().is_err());
    invalid.limit = 1;
    invalid.after_sequence = 4;
    invalid.through_sequence = Some(3);
    assert!(invalid.validate().is_err());
}

#[test]
fn routes_are_ordered_and_duplicate_free() {
    let route = RunRouteCapability {
        backend: BackendSelection::Abi,
        modes: vec![RunMode::OneShot, RunMode::Background],
    };
    route.validate().unwrap();

    let mut invalid = route.clone();
    invalid.modes.reverse();
    assert!(invalid.validate().is_err());
    invalid.modes = vec![RunMode::OneShot, RunMode::OneShot];
    assert!(invalid.validate().is_err());
    invalid.modes.clear();
    assert!(invalid.validate().is_err());
}

#[test]
fn lifecycle_rejects_skips_repeats_and_terminal_mutation() {
    RunState::Queued
        .validate_transition(RunState::Starting)
        .unwrap();
    RunState::Starting
        .validate_transition(RunState::Running)
        .unwrap();
    RunState::Running
        .validate_transition(RunState::Succeeded)
        .unwrap();
    assert!(
        RunState::Queued
            .validate_transition(RunState::Running)
            .is_err()
    );
    assert!(
        RunState::Running
            .validate_transition(RunState::Running)
            .is_err()
    );
    assert!(
        RunState::Succeeded
            .validate_transition(RunState::Running)
            .is_err()
    );
}

#[test]
fn snapshots_require_state_consistent_monotonic_timestamps() {
    let succeeded = snapshot(RunState::Succeeded, None);
    succeeded.validate().unwrap();

    let mut invalid = succeeded.clone();
    invalid.failure = Some(RunFailure {
        code: "provider-error".into(),
        message: "provider rejected request".into(),
        retryable: false,
    });
    assert!(invalid.validate().is_err());

    invalid = succeeded;
    invalid.updated_at = "2026-08-08T11:59:59Z".into();
    assert!(invalid.validate().is_err());

    invalid.updated_at = invalid.created_at.clone();
    invalid.event_count = 0;
    assert!(invalid.validate().is_err());

    snapshot(
        RunState::Failed,
        Some(RunFailure {
            code: "provider-error".into(),
            message: "provider rejected request".into(),
            retryable: false,
        }),
    )
    .validate()
    .unwrap();
}

#[test]
fn event_records_are_sanitized_and_validate_sequence_timestamp() {
    let record = RunEventRecord {
        run_id: RunId::new(),
        sequence: 2,
        recorded_at: "2026-08-08T12:00:01Z".into(),
        event: RunLifecycleEvent::Running,
    };
    record.validate().unwrap();

    let serialized = serde_json::to_string(&record).unwrap();
    assert!(!serialized.contains("input"));
    assert!(!serialized.contains("payload_json"));

    let mut invalid = record;
    invalid.sequence = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn event_pages_are_contiguous_bounded_and_self_consistent() {
    let run_id = RunId::new();
    let records = [RunLifecycleEvent::Starting, RunLifecycleEvent::Running]
        .into_iter()
        .enumerate()
        .map(|(offset, event)| RunEventRecord {
            run_id: run_id.clone(),
            sequence: 2 + u64::try_from(offset).unwrap(),
            recorded_at: "2026-08-08T12:00:01Z".into(),
            event,
        })
        .collect::<Vec<_>>();
    let page = RunEventPage {
        run_id: run_id.clone(),
        events: records,
        after_sequence: 1,
        next_after_sequence: 3,
        through_sequence: 4,
        has_more: true,
    };
    page.validate().unwrap();

    let mut invalid = page.clone();
    invalid.events[1].sequence = 4;
    assert!(invalid.validate().is_err());
    invalid = page.clone();
    invalid.events[1].run_id = RunId::new();
    assert!(invalid.validate().is_err());
    invalid = page.clone();
    invalid.next_after_sequence = 2;
    assert!(invalid.validate().is_err());

    let empty = RunEventPage {
        run_id,
        events: Vec::new(),
        after_sequence: 4,
        next_after_sequence: 4,
        through_sequence: 4,
        has_more: false,
    };
    empty.validate().unwrap();
}

#[test]
fn conversation_metadata_is_bounded_and_monotonic() {
    let mut metadata = ConversationMetadata {
        conversation_id: ConversationId::new(),
        title: Some("Abbey completion".into()),
        created_at: "2026-08-08T12:00:00Z".into(),
        updated_at: "2026-08-08T12:00:01Z".into(),
        run_count: 1,
    };
    metadata.validate().unwrap();
    metadata.updated_at = "2026-08-08T11:59:59Z".into();
    assert!(metadata.validate().is_err());
}
