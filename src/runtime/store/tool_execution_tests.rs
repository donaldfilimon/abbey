use super::*;
use crate::runtime::{NewToolApproval, ToolApprovalDecision};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RESULT_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn pending(call_id: &str, created_at_ms: u64, expires_at_ms: u64) -> NewToolApproval {
    NewToolApproval {
        call_id: call_id.into(),
        tool_id: "abbey_memory_mark_obsolete".into(),
        call_digest: DIGEST.into(),
        created_at_ms,
        expires_at_ms,
    }
}

fn approve(store: &RuntimeStore, call_id: &str, decision_id: &str) {
    store
        .create_tool_approval(pending(call_id, 1_000, 5_000))
        .unwrap();
    store
        .decide_tool_approval(
            call_id,
            DIGEST,
            decision_id,
            ToolApprovalDecision::Approve,
            1_100,
        )
        .unwrap();
}

#[test]
fn preparation_atomically_consumes_exact_approval_before_terminal_result() {
    let store = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    approve(&store, "call-prepare", "decision-prepare");
    assert!(matches!(
        store.prepare_tool_execution("call-prepare", &"c".repeat(64), "execution-wrong", 1_200,),
        Err(StoreError::ToolApprovalDigestMismatch)
    ));
    assert!(store.tool_execution("call-prepare").unwrap().is_none());
    assert_eq!(
        store
            .tool_approval("call-prepare", 1_200)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Approved
    );

    let ToolExecutionPreparation::Prepared(prepared) = store
        .prepare_tool_execution("call-prepare", DIGEST, "execution-prepare", 1_200)
        .unwrap()
    else {
        panic!("non-expired exact approval must prepare");
    };
    assert_eq!(prepared.state, ToolExecutionState::Prepared);
    assert_eq!(prepared.result_digest, None);
    assert_eq!(
        store
            .tool_approval("call-prepare", 1_200)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Consumed
    );
    assert_eq!(
        store
            .tool_approval_events("call-prepare")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![
            ToolApprovalState::Pending,
            ToolApprovalState::Approved,
            ToolApprovalState::Consumed,
        ]
    );
    assert!(matches!(
        store.prepare_tool_execution("call-prepare", DIGEST, "execution-repeat", 1_300),
        Err(StoreError::ToolExecutionConflict)
    ));

    let completed = store
        .complete_tool_execution(
            "call-prepare",
            "execution-prepare",
            ToolExecutionOutcome::Succeeded,
            RESULT_DIGEST,
            1_300,
        )
        .unwrap();
    assert_eq!(completed.state, ToolExecutionState::Succeeded);
    assert_eq!(completed.result_digest.as_deref(), Some(RESULT_DIGEST));
    assert_eq!(
        store
            .tool_execution_events("call-prepare")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![ToolExecutionState::Prepared, ToolExecutionState::Succeeded]
    );
    assert!(matches!(
        store.complete_tool_execution(
            "call-prepare",
            "execution-prepare",
            ToolExecutionOutcome::Failed,
            RESULT_DIGEST,
            1_400,
        ),
        Err(StoreError::ToolExecutionConflict)
    ));
}

#[test]
fn expiry_and_operation_id_reuse_fail_closed_without_prepared_intent() {
    let store = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    store
        .create_tool_approval(pending("call-expired", 1_000, 1_100))
        .unwrap();
    store
        .decide_tool_approval(
            "call-expired",
            DIGEST,
            "decision-expired",
            ToolApprovalDecision::Approve,
            1_050,
        )
        .unwrap();
    let ToolExecutionPreparation::Expired(expired) = store
        .prepare_tool_execution("call-expired", DIGEST, "execution-expired", 1_100)
        .unwrap()
    else {
        panic!("approval at its deadline must expire");
    };
    assert_eq!(expired.state, ToolApprovalState::Expired);
    assert!(store.tool_execution("call-expired").unwrap().is_none());

    approve(&store, "call-execution-id", "decision-execution-id");
    store
        .prepare_tool_execution(
            "call-execution-id",
            DIGEST,
            "globally-unique-operation",
            1_200,
        )
        .unwrap();
    store
        .create_tool_approval(pending("call-reuse", 1_000, 5_000))
        .unwrap();
    assert!(matches!(
        store.decide_tool_approval(
            "call-reuse",
            DIGEST,
            "globally-unique-operation",
            ToolApprovalDecision::Deny,
            1_300,
        ),
        Err(StoreError::ToolApprovalConflict)
    ));
    assert!(matches!(
        store.cancel_tool_approval("call-reuse", "globally-unique-operation", 1_300),
        Err(StoreError::ToolApprovalConflict)
    ));
}

#[test]
fn reopen_interrupts_prepared_effect_and_requires_a_fresh_approved_call() {
    let root = std::env::temp_dir().join(format!(
        "abbey-tool-execution-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).unwrap();
    let database = root.join("runtime.sqlite");
    {
        let store = RuntimeStore::open(&database).unwrap();
        approve(&store, "call-interrupted", "decision-interrupted");
        store
            .prepare_tool_execution("call-interrupted", DIGEST, "execution-interrupted", 1_200)
            .unwrap();
    }
    {
        let metadata = RuntimeStore::open_metadata(&database).unwrap();
        assert_eq!(metadata.recovered_tool_executions(), 0);
        assert_eq!(
            metadata
                .tool_execution("call-interrupted")
                .unwrap()
                .unwrap()
                .state,
            ToolExecutionState::Prepared
        );
    }
    {
        let reopened = RuntimeStore::open(&database).unwrap();
        assert_eq!(reopened.recovered_tool_executions(), 1);
        assert_eq!(
            reopened
                .tool_execution("call-interrupted")
                .unwrap()
                .unwrap()
                .state,
            ToolExecutionState::Interrupted
        );
        assert_eq!(
            reopened
                .tool_execution_events("call-interrupted")
                .unwrap()
                .iter()
                .map(|event| event.state)
                .collect::<Vec<_>>(),
            vec![
                ToolExecutionState::Prepared,
                ToolExecutionState::Interrupted
            ]
        );
        assert!(matches!(
            reopened.complete_tool_execution(
                "call-interrupted",
                "execution-interrupted",
                ToolExecutionOutcome::Succeeded,
                RESULT_DIGEST,
                2_000,
            ),
            Err(StoreError::ToolExecutionConflict)
        ));

        approve(&reopened, "call-explicit-retry", "decision-explicit-retry");
        let retry = reopened
            .prepare_tool_execution(
                "call-explicit-retry",
                DIGEST,
                "execution-explicit-retry",
                1_200,
            )
            .unwrap();
        assert!(matches!(retry, ToolExecutionPreparation::Prepared(_)));
    }
    std::fs::remove_dir_all(root).unwrap();
}
