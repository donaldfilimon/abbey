//! End-to-end provider capture to durable visible-content fixtures.
use super::*;
use crate::app_core::{IdempotencyKey, RunState, V4ContentQuery, V4MessageQuery};
use crate::runtime::NewRunEvent;
use abi_agent_runtime::ScriptedProvider;

#[test]
fn v4_large_provider_delta_reopens_as_lossless_bounded_pages() {
    let root = std::env::temp_dir().join(format!("abbey-large-delta-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = RuntimeStore::path_for_state_dir(&root);
    let store = RuntimeStore::open(&path).unwrap();
    let now = 1_700_000_000_000;
    let cid = store
        .v4_create_conversation("Large output", now)
        .unwrap()
        .conversation_id;
    let text = format!("{}{}tail", "a".repeat(16_383), "🙂".repeat(1000));
    let route = ProviderRoute::new(
        BackendSelection::Abi,
        "fixture",
        Arc::new(ScriptedProvider::new("large-fixture").with_text(text.clone())),
        RunBudget::unlimited()
            .with_max_events(16)
            .with_max_output_tokens(32)
            .with_max_duration(Duration::from_secs(1)),
    )
    .unwrap();
    let request = V4SubmitTurn {
        conversation_id: cid.clone(),
        idempotency_key: IdempotencyKey::new(),
        route_id: v4_route_id(BackendSelection::Abi, "fixture"),
        input: "hello".into(),
    };
    let sub = store.v4_admit_turn(&request, now).unwrap();
    for (from, to) in [
        (RunState::Queued, RunState::Starting),
        (RunState::Starting, RunState::Running),
    ] {
        store
            .transition_run(
                &sub.run_id,
                from,
                to,
                NewRunEvent {
                    kind: "run_started".into(),
                    payload: serde_json::json!({}),
                },
            )
            .unwrap();
    }
    ModelProviderExecutor::new([route])
        .unwrap()
        .execute_v4(&store, &sub.run_id, request, &CancellationToken::new())
        .unwrap();
    store
        .transition_run(
            &sub.run_id,
            RunState::Running,
            RunState::Succeeded,
            NewRunEvent {
                kind: "run_completed".into(),
                payload: serde_json::json!({}),
            },
        )
        .unwrap();
    drop(store);
    let store = RuntimeStore::open(&path).unwrap();
    let messages = store
        .v4_messages(&V4MessageQuery {
            conversation_id: cid.clone(),
            after: None,
            limit: 50,
        })
        .unwrap();
    let assistant = messages
        .messages
        .iter()
        .find(|m| m.role == crate::app_core::V4MessageRole::Assistant)
        .unwrap();
    assert_eq!(assistant.text_bytes, text.len() as u64);
    assert!(!assistant.text_complete);
    assert_eq!(assistant.state, RunState::Succeeded);
    let mut after = 0;
    let mut reconstructed = String::new();
    loop {
        let page = store
            .v4_content(&V4ContentQuery {
                conversation_id: cid.clone(),
                run_id: sub.run_id.clone(),
                after,
                limit: 1,
            })
            .unwrap();
        for event in &page.events {
            if let V4ContentPayload::TextDelta { text } = &event.payload {
                reconstructed.push_str(text);
            }
        }
        match page.next_after {
            Some(next) => after = next,
            None => break,
        }
    }
    assert_eq!(reconstructed, text);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
