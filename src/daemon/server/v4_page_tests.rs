use super::*;
use crate::app_core::*;
use serde_json::json;
fn response(event: V4Event) -> V4ResponseEnvelope {
    V4ResponseEnvelope {
        version: 4,
        schema_version: 3,
        request_id: "fixture".into(),
        payload: V4ResponsePayload::Ok { event },
    }
}
#[test]
fn typed_conversation_and_message_cursors_follow_only_retained_items() {
    let first = ConversationId::new();
    let second = ConversationId::new();
    let conversation = |id: &ConversationId| json!({"conversation_id":id,"id":id.to_string(),"title":"fixture","created_at_ms":1,"updated_at_ms":1,"archived":false,"pinned":false,"sequence":1,"message_count":0});
    let event:V4Event=serde_json::from_value(json!({"event":"conversations","data":{"conversations":[conversation(&first),conversation(&second)],"next_after":null}})).unwrap();
    let mut r = response(event);
    assert!(trim_tail(&mut r));
    let V4ResponsePayload::Ok {
        event: V4Event::Conversations(p),
    } = r.payload
    else {
        panic!()
    };
    assert_eq!(p.conversations.len(), 1);
    assert_eq!(p.next_after, Some(first.clone()));
    p.validate().unwrap();
    let run = RunId::new();
    let message = |seq| json!({"conversation_id":first,"run_id":run,"sequence":seq,"role":"assistant","state":"running","text":"🙂","text_bytes":4,"text_complete":true,"context_truncated":false,"created_at_ms":1});
    let event:V4Event=serde_json::from_value(json!({"event":"messages","data":{"conversation_id":first,"messages":[message(1),message(3)],"next_after":null}})).unwrap();
    let mut r = response(event);
    assert!(trim_tail(&mut r));
    let V4ResponsePayload::Ok {
        event: V4Event::Messages(p),
    } = &r.payload
    else {
        panic!()
    };
    assert_eq!(p.messages.len(), 1);
    assert_eq!(p.messages[0].text, "🙂");
    assert_eq!(p.next_after, Some(1));
    p.validate().unwrap();
    assert!(!trim_tail(&mut r));
}
#[test]
fn fixed_watermark_admin_pages_and_nonpages_are_not_generically_trimmed() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/application-schema3/positive.json"
    ))
    .unwrap();
    for index in [1, 2] {
        let event: V4Event = serde_json::from_value(fixture["events"][index].clone()).unwrap();
        let mut r = response(event);
        let before = serde_json::to_value(&r).unwrap();
        assert!(!trim_tail(&mut r));
        assert_eq!(serde_json::to_value(r).unwrap(), before);
    }
}
