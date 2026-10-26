//! Public wire characterizations for behavior-preserving contract extraction.
use abbey::app_core::{
    APP_PROTOCOL_V4, APP_SCHEMA_V4, V4Capability, V4CapabilitySet, V4Command, V4Event, V4Message,
    V4Route,
};
use serde_json::{Value, json};

const CONVERSATION: &str = "550e8400-e29b-41d4-a716-446655440000";
const RUN: &str = "550e8400-e29b-41d4-a716-446655440001";

fn assert_command(wire: Value) {
    let command: V4Command = serde_json::from_value(wire.clone()).unwrap();
    command.validate().unwrap();
    assert_eq!(serde_json::to_value(&command).unwrap(), wire);
}

#[test]
fn served_v4_commands_preserve_exact_tagging_and_public_types() {
    for wire in [
        json!({"command":"negotiate", "data":{"supported_versions":[APP_PROTOCOL_V4], "requested":{"capabilities":["read_conversations","submit_turns"]}}}),
        json!({"command":"list_conversations", "data":{"search":"私", "after":null,"limit":50,"archived":false}}),
        json!({"command":"create_conversation", "data":"private title"}),
        json!({"command":"read_conversation", "data":CONVERSATION}),
        json!({"command":"update_conversation", "data":[CONVERSATION,{"title":null,"archived":false,"pinned":true}]}),
        json!({"command":"read_messages", "data":{"conversation_id":CONVERSATION,"after":1,"limit":50}}),
        json!({"command":"submit_turn", "data":{"conversation_id":CONVERSATION,"idempotency_key":"request:1","route_id":"abi.local","input":"私\nhello\t🙂"}}),
        json!({"command":"read_content", "data":{"conversation_id":CONVERSATION,"run_id":RUN,"after":0,"limit":50}}),
        json!({"command":"cancel_turn", "data":[CONVERSATION,RUN]}),
        json!({"command":"lookup_turn", "data":[CONVERSATION,"request:1"]}),
        json!({"command":"list_routes"}),
    ] {
        assert_command(wire);
    }
    let grants = V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap();
    assert_eq!(
        serde_json::to_value(grants).unwrap(),
        json!({"capabilities":["read_conversations"]})
    );
}

#[test]
fn served_v4_events_preserve_exact_payloads_and_redacted_debug() {
    let conversation = json!({"conversation_id":CONVERSATION,"id":CONVERSATION,"title":"private title","created_at_ms":1,"updated_at_ms":2,"archived":false,"pinned":true,"sequence":1,"message_count":2});
    let message = json!({"conversation_id":CONVERSATION,"run_id":RUN,"sequence":1,"role":"user","state":"succeeded","text":"private\n🙂","text_bytes":12,"text_complete":true,"context_truncated":false,"created_at_ms":1});
    let submission = json!({"conversation_id":CONVERSATION,"run_id":RUN,"idempotency_key":"request:1","user_message_sequence":1,"state":"queued","duplicate":false});
    for wire in [
        json!({"event":"negotiated","data":{"protocol_version":APP_PROTOCOL_V4,"schema_version":APP_SCHEMA_V4,"granted":{"capabilities":["read_conversations"]}}}),
        json!({"event":"conversation","data":conversation}),
        json!({"event":"conversations","data":{"conversations":[conversation],"next_after":CONVERSATION}}),
        json!({"event":"messages","data":{"conversation_id":CONVERSATION,"messages":[message],"next_after":1}}),
        json!({"event":"turn_submitted","data":submission}),
        json!({"event":"turn_lookup","data":[CONVERSATION,"request:1",submission]}),
        json!({"event":"turn_cancelled","data":[CONVERSATION,RUN,"cancelled"]}),
        json!({"event":"routes","data":[{"route_id":"abi.local","backend":"abi","model":"fixture.model","label":"local model","available":true}]}),
    ] {
        let event: V4Event = serde_json::from_value(wire.clone()).unwrap();
        event.validate().unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
        assert!(!format!("{event:?}").contains("private"));
    }
}

#[test]
fn active_v4_contracts_reject_unknown_fields_and_invalid_correlations() {
    let base = json!({"command":"submit_turn", "data":{"conversation_id":CONVERSATION,"idempotency_key":"request:1","route_id":"abi.local","input":"private"}});
    for (field, value) in [
        ("executable", json!("sh")),
        ("route_id", json!("../route")),
        ("input", json!("\u{0000}")),
    ] {
        let mut wire = base.clone();
        wire["data"][field] = value;
        assert!(serde_json::from_value::<V4Command>(wire).is_err());
    }
    for wire in [
        json!({"event":"conversation","data":{"conversation_id":CONVERSATION,"id":RUN,"title":"title","created_at_ms":1,"updated_at_ms":2,"archived":false,"pinned":false,"sequence":1,"message_count":0}}),
        json!({"event":"turn_lookup","data":[CONVERSATION,"different-key",{"conversation_id":CONVERSATION,"run_id":RUN,"idempotency_key":"request:1","user_message_sequence":1,"state":"queued","duplicate":false}]}),
        json!({"event":"routes","data":[{"route_id":"abi.local","backend":"abi","label":"missing exact model","available":true}]}),
    ] {
        assert!(serde_json::from_value::<V4Event>(wire).is_err());
    }
}

#[test]
fn message_previews_and_routes_keep_current_validation_boundaries() {
    let mut message = json!({"conversation_id":CONVERSATION,"run_id":RUN,"sequence":1,"role":"assistant","state":"running","text":"🙂","text_bytes":8,"text_complete":false,"context_truncated":true,"created_at_ms":1});
    assert!(serde_json::from_value::<V4Message>(message.clone()).is_ok());
    message["text_complete"] = json!(true);
    assert!(serde_json::from_value::<V4Message>(message).is_err());
    let mut route = json!({"route_id":"abi.local","backend":"abi","model":"fixture.model","label":"local model","available":true});
    assert!(serde_json::from_value::<V4Route>(route.clone()).is_ok());
    route["model"] = json!("../other");
    assert!(serde_json::from_value::<V4Route>(route).is_err());
}
