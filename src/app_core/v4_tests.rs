use super::*;
use serde_json::json;

#[test]
fn v4_input_preserves_multiline_unicode_and_bounds_bytes() {
    let make = |input: String| json!({"conversation_id": ConversationId::new(), "idempotency_key":"request:1", "route_id":"local", "input":input});
    let text = "私\n/path/authorized\t🙂";
    let turn: V4SubmitTurn = serde_json::from_value(make(text.into())).unwrap();
    assert_eq!(turn.input, text);
    assert!(!format!("{turn:?}").contains(text));
    assert!(serde_json::from_value::<V4SubmitTurn>(make("🙂".repeat(8192))).is_ok());
    assert!(serde_json::from_value::<V4SubmitTurn>(make("🙂".repeat(8193))).is_err());
    assert!(serde_json::from_value::<V4SubmitTurn>(make(" \n".into())).is_err());
    let mut extra = make("hello".into());
    extra["executable"] = json!("sh");
    assert!(serde_json::from_value::<V4SubmitTurn>(extra).is_err());
}

#[test]
fn v4_capabilities_are_canonical_and_narrow() {
    assert!(
        serde_json::from_value::<V4CapabilitySet>(
            json!({"capabilities":["read_turns","submit_turns"]})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<V4CapabilitySet>(
            json!({"capabilities":["read_turns","read_turns"]})
        )
        .is_err()
    );
    let set = V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap();
    assert!(
        set.permits(&V4Command::ListConversations(V4ConversationQuery {
            search: None,
            after: None,
            limit: 50,
            archived: false
        }))
    );
    assert!(!set.permits(&V4Command::ListRoutes));
}

#[test]
fn v4_content_correlation_and_sequence_fail_closed() {
    let conversation_id = ConversationId::new();
    let run_id = RunId::new();
    let event = V4ContentEvent {
        conversation_id: conversation_id.clone(),
        run_id: run_id.clone(),
        sequence: 1,
        timestamp_ms: 0,
        payload: V4ContentPayload::TextDelta {
            text: "private\n🙂".into(),
        },
    };
    assert!(event.validate_after(&conversation_id, &run_id, 0).is_ok());
    assert!(
        event
            .validate_after(&ConversationId::new(), &run_id, 0)
            .is_err()
    );
    assert!(event.validate_after(&conversation_id, &run_id, 1).is_err());
    assert!(!format!("{event:?}").contains("private"));
    let mut value = serde_json::to_value(&event).unwrap();
    value["timestamp_ms"] = json!(u64::MAX);
    assert!(serde_json::from_value::<V4ContentEvent>(value).is_err());
}

#[test]
fn v4_pages_bound_serialized_bytes_and_preserve_content() {
    let conversation_id = ConversationId::new();
    let run_id = RunId::new();
    let events = (1..=50)
        .map(|sequence| V4ContentEvent {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            sequence,
            timestamp_ms: 0,
            payload: V4ContentPayload::TextDelta {
                text: "\n".repeat(MAX_V4_DELTA_BYTES),
            },
        })
        .collect();
    let mut page = V4ContentPage {
        conversation_id: conversation_id.clone(),
        run_id: run_id.clone(),
        after: 0,
        events,
        next_after: Some(50),
    };
    assert!(page.validate().is_err());
    page.events.truncate(10);
    page.next_after = Some(10);
    assert!(page.validate().is_ok());
    page.events[0].payload = V4ContentPayload::TextDelta {
        text: " \n\t".into(),
    };
    assert!(page.validate().is_ok());
    page.events[1].payload = V4ContentPayload::Terminal {
        state: RunState::Running,
    };
    assert!(page.validate().is_err());
    page.events[1].payload = V4ContentPayload::Terminal {
        state: RunState::Succeeded,
    };
    assert!(page.validate().is_err()); // no events after terminal
}

#[test]
fn v4_content_sequences_allow_filtered_per_run_gaps_but_reject_replay() {
    let conversation_id = ConversationId::new();
    let run_id = RunId::new();
    let event = |sequence| V4ContentEvent {
        conversation_id: conversation_id.clone(),
        run_id: run_id.clone(),
        sequence,
        timestamp_ms: 0,
        payload: V4ContentPayload::TextDelta {
            text: "delta".into(),
        },
    };
    let mut page = V4ContentPage {
        conversation_id: conversation_id.clone(),
        run_id: run_id.clone(),
        after: 0,
        events: vec![event(2), event(5)],
        next_after: Some(5),
    };
    assert!(page.validate().is_ok());
    page.events[1] = event(2);
    assert!(page.validate().is_err());
}

#[test]
fn v4_commands_require_their_specific_capability() {
    let read = V4Command::ListConversations(V4ConversationQuery {
        search: None,
        after: None,
        limit: 1,
        archived: false,
    });
    let submit = V4Command::SubmitTurn(V4SubmitTurn {
        conversation_id: ConversationId::new(),
        idempotency_key: "turn-1".parse().unwrap(),
        route_id: "abi.local".into(),
        input: "hello".into(),
    });
    let content = V4Command::ReadContent(V4ContentQuery {
        conversation_id: ConversationId::new(),
        run_id: RunId::new(),
        after: 0,
        limit: 1,
    });
    assert_eq!(
        read.required_capability(),
        Some(V4Capability::ReadConversations)
    );
    assert_eq!(
        submit.required_capability(),
        Some(V4Capability::SubmitTurns)
    );
    assert_eq!(content.required_capability(), Some(V4Capability::ReadTurns));
    assert_eq!(
        V4Command::ListRoutes.required_capability(),
        Some(V4Capability::ReadRoutes)
    );
}

#[test]
fn v4_queries_titles_and_ids_are_strict() {
    let query = |search: String, limit| json!({"search":search,"after":null,"limit":limit,"archived":false});
    assert!(serde_json::from_value::<V4ConversationQuery>(query("🙂".repeat(64), 50)).is_ok());
    assert!(serde_json::from_value::<V4ConversationQuery>(query("🙂".repeat(65), 50)).is_err());
    assert!(serde_json::from_value::<V4ConversationQuery>(query("hi".into(), 0)).is_err());
    assert!(
        serde_json::from_value::<V4ConversationPatch>(json!({"title":"🙂".repeat(64)})).is_ok()
    );
    assert!(
        serde_json::from_value::<V4ConversationPatch>(json!({"title":"🙂".repeat(65)})).is_err()
    );
    assert!(serde_json::from_value::<V4ConversationPatch>(json!({})).is_err());
    assert!(
        serde_json::from_value::<V4Command>(
            json!({"command":"read_conversation","conversation_id":"../path"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<V4Command>(json!({"command":"list_routes","environment":{}}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<V4CapabilitySet>(json!({"capabilities":[],"extra":true})).is_err()
    );
}

#[test]
fn v4_submission_and_debug_are_correlated() {
    let request = V4SubmitTurn {
        conversation_id: ConversationId::new(),
        idempotency_key: IdempotencyKey::new(),
        route_id: "local".into(),
        input: "secret user text".into(),
    };
    let mut submission = V4TurnSubmission {
        conversation_id: request.conversation_id.clone(),
        run_id: RunId::new(),
        idempotency_key: request.idempotency_key.clone(),
        user_message_sequence: 1,
        state: RunState::Queued,
        duplicate: false,
    };
    assert!(submission.validate_for(&request).is_ok());
    submission.conversation_id = ConversationId::new();
    assert!(submission.validate_for(&request).is_err());
    submission.conversation_id = request.conversation_id.clone();
    submission.idempotency_key = IdempotencyKey::new();
    assert!(submission.validate_for(&request).is_err());
    let command = V4Command::SubmitTurn(request);
    assert!(!format!("{command:?}").contains("secret user text"));
    let event = V4Event::Conversation(V4Conversation {
        conversation_id: ConversationId::new(),
        id: "c1".into(),
        title: "secret title".into(),
        created_at_ms: 0,
        updated_at_ms: 0,
        archived: false,
        pinned: false,
        sequence: 0,
        message_count: 0,
    });
    assert!(!format!("{event:?}").contains("secret title"));
}

#[test]
fn v4_envelopes_validate_payloads_during_decode() {
    assert!(serde_json::from_value::<V4Command>(json!({"command":"negotiate","data":{"supported_versions":[3],"requested":{"capabilities":[]}}})).is_err());
    let long_title = serde_json::to_value(V4Command::CreateConversation("x".repeat(257))).unwrap();
    assert!(serde_json::from_value::<V4Command>(long_title).is_err());
    let invalid_cancel = serde_json::to_value(V4Event::TurnCancelled(
        ConversationId::new(),
        RunId::new(),
        RunState::Running,
    ))
    .unwrap();
    assert!(serde_json::from_value::<V4Event>(invalid_cancel).is_err());
    for state in [
        RunState::Succeeded,
        RunState::Failed,
        RunState::Cancelled,
        RunState::Interrupted,
        RunState::CancelRequested,
    ] {
        let response = serde_json::to_value(V4Event::TurnCancelled(
            ConversationId::new(),
            RunId::new(),
            state,
        ))
        .unwrap();
        assert!(serde_json::from_value::<V4Event>(response).is_ok());
    }

    assert!(
        serde_json::from_value::<V4ContentPayload>(
            json!({"kind":"terminal","data":{"state":"running"}})
        )
        .is_err()
    );
}

#[test]
fn v4_command_and_event_serde_round_trip_the_wire_contract() {
    let requested = V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap();
    let command = V4Command::Negotiate(V4GrantRequest {
        supported_versions: vec![APP_PROTOCOL_V4],
        requested: requested.clone(),
    });
    let encoded = serde_json::to_value(&command).unwrap();
    assert_eq!(
        serde_json::from_value::<V4Command>(encoded).unwrap(),
        command
    );

    let create = V4Command::CreateConversation("Desk".into());
    let encoded = serde_json::to_value(&create).unwrap();
    assert_eq!(
        serde_json::from_value::<V4Command>(encoded).unwrap(),
        create
    );

    let negotiated = V4Event::Negotiated(V4GrantNegotiation {
        protocol_version: APP_PROTOCOL_V4,
        schema_version: APP_SCHEMA_V4,
        granted: requested,
    });
    let encoded = serde_json::to_value(&negotiated).unwrap();
    assert_eq!(
        serde_json::from_value::<V4Event>(encoded).unwrap(),
        negotiated
    );

    let conversation = V4Conversation {
        conversation_id: ConversationId::new(),
        id: String::new(),
        title: "Desk".into(),
        created_at_ms: 10,
        updated_at_ms: 11,
        archived: false,
        pinned: true,
        sequence: 7,
        message_count: 2,
    };
    let mut conversation = conversation;
    conversation.id = conversation.conversation_id.to_string();
    let event = V4Event::Conversation(conversation);
    let encoded = serde_json::to_value(&event).unwrap();
    assert_eq!(serde_json::from_value::<V4Event>(encoded).unwrap(), event);
}

#[test]
fn v4_routes_publish_their_exact_model_id() {
    let route = json!({
        "route_id": "abi.local",
        "backend": "abi",
        "model": "local",
        "label": "ABI local (local)",
        "available": true,
    });
    let decoded: V4Route = serde_json::from_value(route.clone()).expect("valid route");
    assert_eq!(decoded.backend, "abi");
    assert_eq!(decoded.model, "local");
    // The model is its own wire field, not text recovered from the label.
    assert_eq!(serde_json::to_value(&decoded).unwrap(), route);

    // A route without the model field is refused: the schema requires it.
    let mut missing = route.clone();
    missing.as_object_mut().expect("object").remove("model");
    assert!(serde_json::from_value::<V4Route>(missing).is_err());

    // A model that is not a bounded route token is refused too.
    let mut hostile = route.clone();
    hostile["model"] = json!("../escape");
    assert!(serde_json::from_value::<V4Route>(hostile).is_err());
}

#[test]
fn v4_decode_errors_never_echo_untrusted_values() {
    const SENTINEL: &str = "PRIVATE_ATTACKER_SENTINEL";
    fn check<T: serde::de::DeserializeOwned>(value: serde_json::Value) {
        let error = match serde_json::from_value::<T>(value) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("malformed input unexpectedly accepted"),
        };
        assert!(!error.contains(SENTINEL), "untrusted data leaked: {error}");
    }
    check::<V4Capability>(json!(SENTINEL));
    check::<V4MessageRole>(json!(SENTINEL));
    check::<V4ToolState>(json!(SENTINEL));
    check::<V4NoticeCode>(json!(SENTINEL));
    check::<V4ErrorCode>(json!(SENTINEL));
    check::<V4CapabilitySet>(json!({"capabilities":[SENTINEL]}));
    check::<V4CapabilitySet>(json!({"capabilities":[], SENTINEL:true}));
    check::<V4ConversationPatch>(json!({"pinned":SENTINEL}));
    check::<V4ConversationPatch>(json!({SENTINEL:true}));
    check::<V4ContentPayload>(json!({"kind":SENTINEL,"data":{}}));
    check::<V4Command>(json!({"command":SENTINEL,"data":{}}));
    check::<V4Event>(json!({"event":SENTINEL,"data":{}}));
    check::<V4Event>(json!({"event":"error","data":{"code":SENTINEL}}));
    check::<V4ContentPayload>(
        json!({"kind":"usage","data":{"input_tokens":SENTINEL,"output_tokens":0}}),
    );
}
