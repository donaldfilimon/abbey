use super::*;
use crate::app_core::{RunId, V4SubmitTurn};

#[test]
fn v4_context_rejects_unadmitted_turn_and_invalid_budget() {
    let root = std::env::temp_dir().join(format!("abbey-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    let conversation = store
        .v4_create_conversation("Context", 1_700_000_000_000)
        .unwrap();
    let submit = V4SubmitTurn {
        conversation_id: conversation.conversation_id,
        idempotency_key: "context-key".parse().unwrap(),
        route_id: "abi.local".into(),
        input: "hello".into(),
    };
    assert!(store.v4_turn_context(&RunId::new(), &submit, 8192).is_err());
    let admitted = store.v4_admit_turn(&submit, 1_700_000_000_000).unwrap();
    assert!(
        store
            .v4_turn_context(&admitted.run_id, &submit, 24 * 1024 + 1)
            .is_err()
    );
    let context = store
        .v4_turn_context(&admitted.run_id, &submit, 8192)
        .unwrap();
    assert!(context.pairs.is_empty());
    assert!(
        !store
            .v4_turn_context(&admitted.run_id, &submit, 0)
            .unwrap()
            .truncated
    );
    let mut forged = submit.clone();
    forged.input = "different".into();
    assert!(
        store
            .v4_turn_context(&admitted.run_id, &forged, 8192)
            .is_err()
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

fn completed(store: &RuntimeStore, request: &V4SubmitTurn, output: &str) -> RunId {
    let run = store
        .v4_admit_turn(request, 1_700_000_000_000)
        .unwrap()
        .run_id;
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
    for chunk in output.as_bytes().chunks(4000) {
        store
            .v4_append_content(
                &run,
                crate::app_core::V4ContentPayload::TextDelta {
                    text: String::from_utf8(chunk.to_vec()).unwrap(),
                },
                1_700_000_000_000,
            )
            .unwrap();
    }
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
    run
}

#[test]
fn v4_context_full_events_isolated_and_bounded() {
    let root = std::env::temp_dir().join(format!("abbey-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    let cid = store
        .v4_create_conversation("A", 1_700_000_000_000)
        .unwrap()
        .conversation_id;
    let other = store
        .v4_create_conversation("B", 1_700_000_000_000)
        .unwrap()
        .conversation_id;
    let mut request = V4SubmitTurn {
        conversation_id: cid.clone(),
        idempotency_key: "context-a".parse().unwrap(),
        route_id: "abi.local".into(),
        input: "first".into(),
    };
    let answer = "a".repeat(18000);
    let prior = completed(&store, &request, &answer);
    request.conversation_id = other;
    request.idempotency_key = "context-b".parse().unwrap();
    completed(&store, &request, "private");
    request.conversation_id = cid;
    request.route_id = "fm.system".into();
    request.idempotency_key = "context-c".parse().unwrap();
    completed(&store, &request, "other route");
    request.route_id = "abi.local".into();
    request.input = "next".into();
    request.idempotency_key = "context-d".parse().unwrap();
    let current = store.v4_admit_turn(&request, 1_700_000_000_000).unwrap();
    let context = store
        .v4_turn_context(&current.run_id, &request, 24000)
        .unwrap();
    assert_eq!(context.pairs.len(), 1);
    assert_eq!(context.pairs[0].user, "first");
    assert_eq!(context.pairs[0].assistant, answer);
    assert!(!context.truncated);
    assert!(
        store
            .v4_turn_context(&current.run_id, &request, 0)
            .unwrap()
            .truncated
    );
    let small = store
        .v4_turn_context(&current.run_id, &request, 100)
        .unwrap();
    assert!(small.pairs.is_empty());
    assert!(small.truncated);
    store.conn.lock().unwrap().execute("UPDATE assistant_content_events SET byte_count=9999 WHERE run_id=?1 AND kind='output_bounded'",[prior.as_str()]).unwrap();
    assert!(
        store
            .v4_turn_context(&current.run_id, &request, 24000)
            .is_err()
    );
    store.conn.lock().unwrap().execute("UPDATE assistant_content_events SET byte_count=(SELECT length(CAST(p.payload_json AS BLOB)) FROM assistant_content_payloads p WHERE p.conversation_id=assistant_content_events.conversation_id AND p.sequence=assistant_content_events.sequence) WHERE run_id=?1",[prior.as_str()]).unwrap();
    store.conn.lock().unwrap().execute("DELETE FROM assistant_content_payloads WHERE (conversation_id,sequence) IN (SELECT conversation_id,sequence FROM assistant_content_events WHERE run_id=?1 AND kind='output_bounded' LIMIT 1)",[prior.as_str()]).unwrap();
    assert!(
        store
            .v4_turn_context(&current.run_id, &request, 24000)
            .is_err()
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v4_context_recent_pairs_order_and_failed_exclusion_survive_reopen() {
    let root = std::env::temp_dir().join(format!("abbey-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = RuntimeStore::path_for_state_dir(&root);
    let store = RuntimeStore::open(&path).unwrap();
    let cid = store
        .v4_create_conversation("A", 1_700_000_000_000)
        .unwrap()
        .conversation_id;
    let mut request = V4SubmitTurn {
        conversation_id: cid,
        idempotency_key: "unused".parse().unwrap(),
        route_id: "abi.local".into(),
        input: "".into(),
    };
    for n in 0..35 {
        request.idempotency_key = format!("context-{n}").parse().unwrap();
        request.input = format!("question {n}");
        let run = completed(&store, &request, &format!("answer {n}"));
        if n == 34 {
            store
                .conn
                .lock()
                .unwrap()
                .execute(
                    "UPDATE runs SET status='failed' WHERE id=?1",
                    [run.as_str()],
                )
                .unwrap();
        }
    }
    request.idempotency_key = "current".parse().unwrap();
    request.input = "next".into();
    let run = store
        .v4_admit_turn(&request, 1_700_000_000_000)
        .unwrap()
        .run_id;
    let context = store.v4_turn_context(&run, &request, 24000).unwrap();
    assert_eq!(context.pairs.len(), 32);
    assert!(context.truncated);
    assert_eq!(context.pairs.first().unwrap().user, "question 2");
    assert_eq!(context.pairs.last().unwrap().assistant, "answer 33");
    drop(store);
    let reopened = RuntimeStore::open(&path).unwrap();
    let context = reopened.v4_turn_context(&run, &request, 24000).unwrap();
    assert_eq!(context.pairs.len(), 32);
    assert_eq!(context.pairs.last().unwrap().assistant, "answer 33");
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v4_context_legacy_marker() {
    legacy_context("marker", "real reply".into(), true);
}
#[test]
fn v4_context_legacy_preview() {
    legacy_context("preview", "a".repeat(18000), false);
}
#[test]
fn v4_context_legacy_genuine_marker() {
    legacy_context("genuine", "[admitted]".into(), false);
}
fn legacy_context(label: &str, output: String, synthetic: bool) {
    let root = std::env::temp_dir().join(format!("abbey-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    let cid = store
        .v4_create_conversation(label, 1_700_000_000_000)
        .unwrap()
        .conversation_id;
    let mut request = V4SubmitTurn {
        conversation_id: cid,
        idempotency_key: "prior".parse().unwrap(),
        route_id: "abi.local".into(),
        input: "question".into(),
    };
    let prior = completed(&store, &request, &output);
    {
        let conn = store.conn.lock().unwrap();
        if synthetic {
            conn.execute_batch("BEGIN; PRAGMA defer_foreign_keys=ON;")
                .unwrap();
            // Old admission emitted a separate text event but never entered the preview.
            conn.execute("UPDATE assistant_content_events SET sequence=3 WHERE run_id=?1 AND kind='completed'",[prior.as_str()]).unwrap();
            conn.execute(
                "UPDATE assistant_content_payloads SET sequence=3 WHERE sequence=2",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO assistant_content_events SELECT conversation_id,2,request_id,run_id,kind,byte_count,timestamp_ms FROM assistant_content_events WHERE sequence=1",[]).unwrap();
            conn.execute("INSERT INTO assistant_content_payloads SELECT conversation_id,2,payload_json FROM assistant_content_payloads WHERE sequence=1",[]).unwrap();
            let marker = serde_json::to_string(&crate::app_core::V4ContentPayload::TextDelta {
                text: "[admitted]".into(),
            })
            .unwrap();
            conn.execute(
                "UPDATE assistant_content_payloads SET payload_json=?1 WHERE sequence=1",
                [marker],
            )
            .unwrap();
            conn.execute_batch("COMMIT;").unwrap();
        }
        conn.execute("UPDATE assistant_content_events SET byte_count=(SELECT length(CAST(p.payload_json AS BLOB)) FROM assistant_content_payloads p WHERE p.conversation_id=assistant_content_events.conversation_id AND p.sequence=assistant_content_events.sequence) WHERE run_id=?1",[prior.as_str()]).unwrap();
        conn.execute("UPDATE assistant_messages SET text_bytes=length(CAST(body AS BLOB)) WHERE run_id=?1 AND role='assistant'",[prior.as_str()]).unwrap();
    }
    request.idempotency_key = "current".parse().unwrap();
    request.input = "next".into();
    let current = store.v4_admit_turn(&request, 1_700_000_000_000).unwrap();
    let context = store
        .v4_turn_context(&current.run_id, &request, 24000)
        .unwrap_or_else(|_| panic!("{label} history rejected"));
    assert_eq!(context.pairs.len(), 1, "{label}");
    assert_eq!(context.pairs[0].assistant, output, "{label}");
    if output.len() > 16000 {
        let context = store
            .v4_turn_context(&current.run_id, &request, 17000)
            .unwrap();
        assert!(context.pairs.is_empty());
        assert!(context.truncated);
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v4_context_legacy_marker_reply_is_itself_marker() {
    legacy_context("marker-reply", "[admitted]".into(), true);
}
#[test]
fn v4_context_legacy_marker_reply_starts_marker() {
    legacy_context("marker-prefix", "[admitted]actual".into(), true);
}
#[test]
fn v4_context_empty_success_does_not_poison_next_turn() {
    let root = std::env::temp_dir().join(format!("abbey-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    let cid = store
        .v4_create_conversation("empty", 1700000000000)
        .unwrap()
        .conversation_id;
    let mut request = V4SubmitTurn {
        conversation_id: cid,
        idempotency_key: "empty".parse().unwrap(),
        route_id: "abi.local".into(),
        input: "hello".into(),
    };
    completed(&store, &request, "[admitted]");
    request.idempotency_key = "empty-after-marker".parse().unwrap();
    let prior = completed(&store, &request, "");
    request.idempotency_key = "next".parse().unwrap();
    let run = store.v4_admit_turn(&request, 1700000000000).unwrap().run_id;
    let context = store.v4_turn_context(&run, &request, 24000).unwrap();
    assert_eq!(context.pairs.len(), 1);
    assert_eq!(context.pairs[0].assistant, "[admitted]");
    assert!(!context.truncated);
    store.conn.lock().unwrap().execute("DELETE FROM assistant_content_payloads WHERE (conversation_id,sequence) IN (SELECT conversation_id,sequence FROM assistant_content_events WHERE run_id=?1)",[prior.as_str()]).unwrap();
    assert!(store.v4_turn_context(&run, &request, 24000).is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
