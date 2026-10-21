use super::*;
use crate::app_core::{V4ContentPayload, V4Message, V4MessageQuery, V4NoticeCode, V4SubmitTurn};
#[test]
fn v4_context_truncation_projection_short_reply_exact_run_and_reopen() {
    let root = std::env::temp_dir().join(format!("abbey-notice-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = RuntimeStore::path_for_state_dir(&root);
    let store = RuntimeStore::open(&path).unwrap();
    let cid = store
        .v4_create_conversation("notice", 1700000000000)
        .unwrap()
        .conversation_id;
    for (key, truncated) in [("a", true), ("b", false)] {
        let request = V4SubmitTurn {
            conversation_id: cid.clone(),
            idempotency_key: key.parse().unwrap(),
            route_id: "abi.local".into(),
            input: "hello".into(),
        };
        let run = store.v4_admit_turn(&request, 1700000000000).unwrap().run_id;
        for (from, to) in [
            (RunState::Queued, RunState::Starting),
            (RunState::Starting, RunState::Running),
        ] {
            store
                .transition_run(
                    &run,
                    from,
                    to,
                    NewRunEvent {
                        kind: "fixture".into(),
                        payload: serde_json::json!({}),
                    },
                )
                .unwrap();
        }
        {
            store
                .v4_append_content(
                    &run,
                    V4ContentPayload::Notice {
                        code: if truncated {
                            V4NoticeCode::Truncated
                        } else {
                            V4NoticeCode::Reconnecting
                        },
                    },
                    1700000000000,
                )
                .unwrap();
        }
        store
            .v4_append_content(
                &run,
                V4ContentPayload::TextDelta {
                    text: "short reply".into(),
                },
                1700000000000,
            )
            .unwrap();
        store
            .transition_run(
                &run,
                RunState::Running,
                RunState::Succeeded,
                NewRunEvent {
                    kind: "fixture".into(),
                    payload: serde_json::json!({}),
                },
            )
            .unwrap();
    }
    for store in [store, RuntimeStore::open_metadata(&path).unwrap()] {
        let page = store
            .v4_messages(&V4MessageQuery {
                conversation_id: cid.clone(),
                after: None,
                limit: 10,
            })
            .unwrap();
        assert_eq!(page.messages.len(), 4);
        for (index, message) in page.messages.iter().enumerate() {
            let value = serde_json::to_value(message).unwrap();
            assert_eq!(
                value.get("context_truncated"),
                Some(&serde_json::json!(index == 1))
            );
            assert!(message.text_complete);
            assert!(!message.text.contains("truncated"));
            let mut legacy = value.clone();
            legacy.as_object_mut().unwrap().remove("context_truncated");
            let decoded: V4Message = serde_json::from_value(legacy).unwrap();
            assert_eq!(
                serde_json::to_value(decoded).unwrap()["context_truncated"],
                false
            );
            if index % 2 == 0 {
                let mut forged = value;
                forged["context_truncated"] = true.into();
                assert!(serde_json::from_value::<V4Message>(forged).is_err());
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
