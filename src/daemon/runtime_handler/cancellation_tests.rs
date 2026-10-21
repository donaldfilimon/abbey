//! Exact v4 authority and real fixed-provider cancellation regressions.
use super::*;
use crate::app_core::{
    ConversationId, RunId, V4Capability, V4CapabilitySet, V4ContentPayload, V4ContentQuery,
    V4MessageQuery, V4TurnSubmit,
};

fn conversation(harness: &Harness) -> ConversationId {
    let V4Event::Conversation(conversation) = harness
        .handler
        .handle_v4(V4Command::CreateConversation("Cancel fixture".into()))
        .unwrap()
    else {
        panic!("expected conversation")
    };
    conversation.conversation_id
}

fn turn(harness: &Harness, conversation_id: ConversationId) -> RunId {
    let V4Event::TurnSubmitted(turn) = harness
        .handler
        .handle_v4(V4Command::SubmitTurn(V4TurnSubmit {
            conversation_id,
            idempotency_key: "cancel-v4-turn".parse().unwrap(),
            route_id: "abi.local".into(),
            input: "bounded cancellation fixture".into(),
        }))
        .unwrap()
    else {
        panic!("expected turn")
    };
    turn.run_id
}

#[test]
fn v4_cancel_requires_exact_assistant_turn_and_grant_before_manager_effect() {
    let harness = Harness::start("sleep 5");
    let conversation_id = conversation(&harness);
    let run_id = turn(&harness, conversation_id.clone());
    let wrong = conversation(&harness);
    let legacy = harness.submit("cancel-legacy-isolation", "legacy fixture");
    for (c, r) in [
        (wrong, run_id.clone()),
        (conversation_id.clone(), RunId::new()),
        (conversation_id.clone(), legacy.clone()),
    ] {
        let failure = harness
            .handler
            .handle_v4(V4Command::CancelTurn(c, r))
            .unwrap_err();
        assert_eq!(failure.code(), "not_found");
    }
    assert!(!harness.handler.authorizes_v4(
        &V4CapabilitySet::deny_all(),
        &V4Command::CancelTurn(conversation_id.clone(), run_id.clone())
    ));
    for id in [&run_id, &legacy] {
        let record = harness.handler.store.get_run(id).unwrap().unwrap();
        assert!(!matches!(
            record.status,
            RunState::CancelRequested | RunState::Cancelled
        ));
        assert!(
            !harness
                .handler
                .store
                .run_events(id)
                .unwrap()
                .iter()
                .any(|event| event.kind.contains("cancel"))
        );
    }
    let grants = V4CapabilitySet::from_sorted(vec![V4Capability::CancelTurns]).unwrap();
    let negotiated = harness
        .handler
        .handle_v4(V4Command::Negotiate(crate::app_core::V4GrantRequest {
            supported_versions: vec![4],
            requested: grants.clone(),
        }))
        .unwrap();
    assert!(matches!(negotiated, V4Event::Negotiated(n) if n.granted == grants));
    assert!(
        harness
            .handler
            .authorizes_v4(&grants, &V4Command::CancelTurn(conversation_id, run_id))
    );
}

#[test]
fn v4_cancel_running_provider_persists_cancelled_messages_and_terminal_content() {
    let harness = Harness::start("printf started > provider-started; sleep 5");
    let conversation_id = conversation(&harness);
    let run_id = turn(&harness, conversation_id.clone());
    let deadline = Instant::now() + Duration::from_secs(3);
    while !harness.root.join("workspace/provider-started").exists() {
        assert!(
            Instant::now() < deadline,
            "fixed provider did not actually start"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        harness
            .handler
            .store
            .get_run(&run_id)
            .unwrap()
            .unwrap()
            .status,
        RunState::Running
    );
    let event = harness
        .handler
        .handle_v4(V4Command::CancelTurn(
            conversation_id.clone(),
            run_id.clone(),
        ))
        .unwrap();
    assert!(
        matches!(event, V4Event::TurnCancelled(c, r, RunState::CancelRequested | RunState::Cancelled) if c == conversation_id && r == run_id)
    );
    assert_eq!(harness.terminal(&run_id).state, RunState::Cancelled);
    let path = RuntimeStore::path_for_state_dir(&harness.root.join("daemon"));
    harness.handler.manager.shutdown().unwrap();
    let reopened = RuntimeStore::open(&path).unwrap();
    assert_eq!(
        reopened.get_run(&run_id).unwrap().unwrap().status,
        RunState::Cancelled
    );
    let messages = reopened
        .v4_messages(&V4MessageQuery {
            conversation_id: conversation_id.clone(),
            after: None,
            limit: 16,
        })
        .unwrap();
    assert!(!messages.messages.is_empty());
    assert!(
        messages
            .messages
            .iter()
            .all(|message| message.state == RunState::Cancelled && message.text_complete)
    );
    assert!(messages.messages.iter().any(|message| message.role
        == crate::app_core::V4MessageRole::User
        && message.text == "bounded cancellation fixture"));
    let content = reopened
        .v4_content(&V4ContentQuery {
            conversation_id,
            run_id,
            after: 0,
            limit: 50,
        })
        .unwrap();
    assert_eq!(
        content
            .events
            .iter()
            .filter(|event| matches!(
                event.payload,
                V4ContentPayload::Terminal {
                    state: RunState::Cancelled
                }
            ))
            .count(),
        1
    );
}

#[test]
fn v4_cancel_completion_race_returns_the_actual_terminal_state() {
    let harness = Harness::start("printf 'finished reply'");
    let conversation_id = conversation(&harness);
    let run_id = turn(&harness, conversation_id.clone());
    assert_eq!(harness.terminal(&run_id).state, RunState::Succeeded);
    let event = harness
        .handler
        .handle_v4(V4Command::CancelTurn(
            conversation_id.clone(),
            run_id.clone(),
        ))
        .unwrap();
    assert!(
        matches!(event, V4Event::TurnCancelled(c,r,RunState::Succeeded) if c == conversation_id && r == run_id)
    );
    assert!(
        !harness
            .handler
            .store
            .run_events(&run_id)
            .unwrap()
            .iter()
            .any(|event| event.kind.contains("cancel"))
    );
}
