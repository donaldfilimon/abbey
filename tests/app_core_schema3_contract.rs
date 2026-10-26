//! Schema 3 regression witnesses; no service or execution authority.
use abbey::app_core::{APP_SCHEMA_V4, V4Command, V4Event, V4RequestEnvelope};
use serde_json::json;

#[test]
fn schema3_replaces_reserved_placeholders_and_rejects_unbound_mutations() {
    assert_eq!(APP_SCHEMA_V4, 3);
    for wire in [
        json!({"command":"create_task","data":{"title":"unbound"}}),
        json!({"command":"retry_task","data":{"resource_id":"task.one"}}),
        json!({"command":"set_automation_enabled","data":{"resource_id":"automation.one"}}),
        json!({"command":"decide_action","data":{"action_id":"action.one","decision_id":"decision.one","kind":"Approve"}}),
    ] {
        assert!(serde_json::from_value::<V4Command>(wire).is_err());
    }
    for wire in [
        json!({"event":"task","data":{"id":"task.one"}}),
        json!({"event":"automation","data":{}}),
    ] {
        assert!(serde_json::from_value::<V4Event>(wire).is_err());
    }
}

fn create() -> serde_json::Value {
    json!({"command":"create_task","data":{
        "operation_id":"create:1", "conversation_id":"550e8400-e29b-41d4-a716-446655440000",
        "goal":"finish this finite task", "workspace_id":"workspace.one", "route_id":"abi.local",
        "permission_profile_id":"profile.one", "profile_revision":1, "budget_tokens":100,
        "execution_kind":"read_only"
    }})
}

#[test]
fn schema3_create_is_bounded_and_carries_no_caller_authority() {
    let wire = create();
    let parsed: V4Command = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    for (field, value) in [
        ("profile_revision", json!(0)),
        ("budget_tokens", json!(0)),
        ("budget_tokens", json!(100_001)),
        ("goal", json!("x".repeat(1025))),
        ("workspace_id", json!("../escape")),
        ("principal", json!("administrator")),
        ("task_id", json!("caller-task")),
        ("edition", json!("personal")),
        ("operation_id", json!("")),
        ("execution_kind", json!("arbitrary_shell")),
    ] {
        let mut bad = wire.clone();
        bad["data"][field] = value;
        assert!(
            serde_json::from_value::<V4Command>(bad).is_err(),
            "accepted {field}"
        );
    }
    let mut positive = wire.clone();
    positive["data"]["goal"] = json!("x".repeat(1024));
    assert!(serde_json::from_value::<V4Command>(positive).is_ok());
}

#[test]
fn frozen_positive_corpus_roundtrips_and_schema2_refuses_every_admin_shape() {
    use abbey::daemon::{V4RequestEnvelope, V4ResponseEnvelope};
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/positive.json")).unwrap();
    for wire in corpus["commands"].as_array().unwrap() {
        let command: V4Command = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&command).unwrap(), *wire);
        assert!(command.validate_for_schema(3).is_ok());
        assert!(command.validate_for_schema(2).is_err());
        for schema in [2, 3] {
            let envelope = json!({"version":4,"schema_version":schema,"request_id":"fixture","bearer":"fixture-not-live","grants":{"capabilities":[]},"command":wire});
            assert_eq!(
                serde_json::from_value::<V4RequestEnvelope>(envelope).is_ok(),
                schema == 3
            );
        }
        let mut extra = wire.clone();
        extra["data"]["forged_owner"] = json!("operator");
        assert!(serde_json::from_value::<V4Command>(extra).is_err());
    }
    for wire in corpus["events"].as_array().unwrap() {
        let event: V4Event = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), *wire);
        for schema in [2, 3] {
            let envelope = json!({"version":4,"schema_version":schema,"request_id":"fixture","payload":{"outcome":"ok","event":wire}});
            assert_eq!(
                serde_json::from_value::<V4ResponseEnvelope>(envelope).is_ok(),
                schema == 3
            );
        }
    }
}

#[test]
fn event_pages_preserve_fixed_watermarks_and_closed_data() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/positive.json")).unwrap();
    let base = corpus["events"][2].clone();
    for changed in [
        {
            let mut x = base.clone();
            x["data"]["events"][0]["task_id"] = json!("task.other");
            x
        },
        {
            let mut x = base.clone();
            x["data"]["events"][0]["sequence"] = json!(2);
            x
        },
        {
            let mut x = base.clone();
            x["data"]["events"][0]["kind"] =
                json!({"kind":"arbitrary_execution","data":{"command":"sh"}});
            x
        },
        {
            let mut x = base.clone();
            x["data"]["through"] = json!(9_007_199_254_740_992u64);
            x
        },
        {
            let mut x = base.clone();
            x["data"]["after"] = json!(2);
            x
        },
        {
            let mut x = base.clone();
            let e = x["data"]["events"][0].clone();
            x["data"]["events"].as_array_mut().unwrap().push(e);
            x
        },
    ] {
        assert!(serde_json::from_value::<V4Event>(changed).is_err());
    }
}

#[test]
fn schedule_grammar_reuses_existing_calendar_validation() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/positive.json")).unwrap();
    let base = corpus["commands"][7].clone();
    for schedule in [
        json!({"kind":"once","data":{"at_ms":0}}),
        json!({"kind":"hourly","data":{"minute":59}}),
        json!({"kind":"weekly","data":{"weekday":0,"hour":23,"minute":59}}),
        json!({"kind":"cron","data":{"expression":"0 * * * *"}}),
    ] {
        let mut wire = base.clone();
        wire["data"]["schedule"] = schedule;
        assert!(serde_json::from_value::<V4Command>(wire).is_ok());
    }
    for (key, val) in [
        ("timezone", json!("Mars/Olympus")),
        (
            "schedule",
            json!({"kind":"daily","data":{"hour":24,"minute":0}}),
        ),
        (
            "schedule",
            json!({"kind":"cron","data":{"expression":"* *"}}),
        ),
        (
            "schedule",
            json!({"kind":"daily","data":{"hour":9,"minute":30,"shell":"sh"}}),
        ),
    ] {
        let mut wire = base.clone();
        wire["data"][key] = val;
        assert!(serde_json::from_value::<V4Command>(wire).is_err());
    }
}

#[test]
fn shared_malformed_corpus_refuses_and_literal_schema2_envelopes_roundtrip() {
    let negative: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/negative.json")).unwrap();
    for wire in negative["commands"].as_array().unwrap() {
        assert!(serde_json::from_value::<V4Command>(wire.clone()).is_err());
    }
    let legacy: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/application-schema3/schema2-conversation.json"
    ))
    .unwrap();
    let req: abbey::app_core::V4RequestEnvelope =
        serde_json::from_value(legacy["request"].clone()).unwrap();
    let res: abbey::app_core::V4ResponseEnvelope =
        serde_json::from_value(legacy["response"].clone()).unwrap();
    assert_eq!(serde_json::to_value(req).unwrap(), legacy["request"]);
    assert_eq!(serde_json::to_value(res).unwrap(), legacy["response"]);
}

#[test]
fn terminal_attempt_cannot_finish_after_its_task_projection_timestamp() {
    let mut x: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/positive.json")).unwrap();
    let task = &mut x["events"][0];
    task["data"]["state"] = json!("succeeded");
    task["data"]["current_attempt"]["state"] = json!("succeeded");
    task["data"]["current_attempt"]["finished_at_ms"] = json!(2);
    assert!(serde_json::from_value::<V4Event>(task.clone()).is_err());
}
#[test]
fn schema3_envelope_ids_are_bounded_at_decode() {
    let x: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/application-schema3/schema2-conversation.json"
    ))
    .unwrap();
    let mut request = x["request"].clone();
    request["schema_version"] = json!(3);
    request["request_id"] = json!("x".repeat(129));
    assert!(serde_json::from_value::<V4RequestEnvelope>(request).is_err());
}
#[test]
fn task_cancellation_cannot_acknowledge_an_unchanged_queued_state() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/positive.json")).unwrap();
    let command: abbey::app_core::V4Command =
        serde_json::from_value(corpus["commands"][1].clone()).unwrap();
    let event: V4Event = serde_json::from_value(corpus["events"][0].clone()).unwrap();
    assert!(!command.matches_event(&event));
}

#[test]
fn self_review_boundary_controls_remain_accepted() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/application-schema3/positive.json")).unwrap();
    let mut task = corpus["events"][0].clone();
    task["data"]["state"] = json!("cancelled");
    task["data"]["current_attempt"]["state"] = json!("cancelled");
    task["data"]["current_attempt"]["finished_at_ms"] = json!(1);
    let event: V4Event = serde_json::from_value(task).unwrap();
    let command: V4Command = serde_json::from_value(corpus["commands"][1].clone()).unwrap();
    assert!(command.matches_event(&event));
    let mut request: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/application-schema3/schema2-conversation.json"
    ))
    .unwrap();
    request["request"]["schema_version"] = json!(3);
    request["request"]["request_id"] = json!("x".repeat(128));
    assert!(serde_json::from_value::<V4RequestEnvelope>(request["request"].clone()).is_ok());
}
