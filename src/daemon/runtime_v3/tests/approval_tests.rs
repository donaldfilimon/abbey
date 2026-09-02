#[cfg(not(feature = "personal-edition"))]
#[test]
fn mutating_safe_tool_only_persists_an_exact_pending_approval() {
    let store = store();
    let authority = V3RuntimeAuthority::from_provider_routes(
        [],
        Vec::new(),
        Arc::clone(&store),
        memory_route(),
        None,
    )
    .unwrap();
    let invalid = V3ToolCall {
        tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
        call_id: "pending-invalid".into(),
        input: serde_json::json!({"record_id": "contains spaces"}),
    };
    assert_eq!(
        authority
            .handle(V3Command::InvokeTool(invalid))
            .unwrap_err()
            .code(),
        "invalid_command"
    );
    assert!(store.tool_approval("pending-invalid", 1).unwrap().is_none());

    let call = V3ToolCall {
        tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
        call_id: "pending-call-1".into(),
        input: serde_json::json!({"record_id": "memory:one"}),
    };
    let expected_digest = call.approval_digest().unwrap();
    let V3Event::ToolApprovalStatus(status) = authority
        .handle(V3Command::InvokeTool(call.clone()))
        .unwrap()
    else {
        panic!("expected pending approval status");
    };
    assert_eq!(status.tool_id, call.tool_id);
    assert_eq!(status.call_id, call.call_id);
    assert_eq!(status.call_digest, expected_digest);
    assert_eq!(status.state, V3ToolApprovalState::Pending);

    let record = store
        .tool_approval(&call.call_id, status.expires_at_ms - 1)
        .unwrap()
        .unwrap();
    assert_eq!(record.tool_id, call.tool_id);
    assert_eq!(record.call_digest, expected_digest);
    assert_eq!(record.state, crate::runtime::ToolApprovalState::Pending);
    assert_eq!(store.tool_approval_events(&call.call_id).unwrap().len(), 1);
    assert!(store.audit_events_for_run(None).unwrap().is_empty());

    let mut changed = call;
    changed.input = serde_json::json!({"record_id": "memory:two"});
    assert_eq!(
        authority
            .handle(V3Command::InvokeTool(changed))
            .unwrap_err()
            .code(),
        "conflict"
    );
}

#[cfg(not(feature = "personal-edition"))]
#[test]
fn exact_pending_decisions_are_durable_without_implicit_execution() {
    let store = store();
    let authority = V3RuntimeAuthority::from_provider_routes(
        [],
        Vec::new(),
        Arc::clone(&store),
        memory_route(),
        None,
    )
    .unwrap();
    let pending = |call_id: &str, record_id: &str| V3ToolCall {
        tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
        call_id: call_id.into(),
        input: serde_json::json!({"record_id": record_id}),
    };

    let approve_call = pending("decision-call-approve", "memory:approve");
    let approve_digest = approve_call.approval_digest().unwrap();
    authority
        .handle(V3Command::InvokeTool(approve_call))
        .unwrap();
    assert_eq!(
        authority
            .handle(V3Command::ApproveTool(V3ToolDecision {
                call_id: "decision-call-approve".into(),
                call_digest: "0".repeat(64),
                decision_id: "decision-wrong-digest".into(),
            }))
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert_eq!(
        store
            .tool_approval("decision-call-approve", 1)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Pending
    );
    let V3Event::ToolApprovalStatus(approved) = authority
        .handle(V3Command::ApproveTool(V3ToolDecision {
            call_id: "decision-call-approve".into(),
            call_digest: approve_digest.clone(),
            decision_id: "decision-approve-1".into(),
        }))
        .unwrap()
    else {
        panic!("expected approved status");
    };
    assert_eq!(approved.state, V3ToolApprovalState::Approved);
    assert_eq!(approved.call_digest, approve_digest);
    assert_eq!(
        store
            .tool_approval("decision-call-approve", approved.expires_at_ms - 1)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Approved
    );
    assert_eq!(
        store
            .tool_approval_events("decision-call-approve")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![ToolApprovalState::Pending, ToolApprovalState::Approved]
    );

    let deny_call = pending("decision-call-deny", "memory:deny");
    let deny_digest = deny_call.approval_digest().unwrap();
    authority.handle(V3Command::InvokeTool(deny_call)).unwrap();
    let V3Event::ToolApprovalStatus(denied) = authority
        .handle(V3Command::DenyTool(V3ToolDecision {
            call_id: "decision-call-deny".into(),
            call_digest: deny_digest.clone(),
            decision_id: "decision-deny-1".into(),
        }))
        .unwrap()
    else {
        panic!("expected denied status");
    };
    assert_eq!(denied.state, V3ToolApprovalState::Denied);
    assert_eq!(denied.call_digest, deny_digest);
    assert_eq!(
        store
            .tool_approval_events("decision-call-deny")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![ToolApprovalState::Pending, ToolApprovalState::Denied]
    );

    let reused_id_call = pending("decision-call-reused-id", "memory:reused-id");
    let reused_id_digest = reused_id_call.approval_digest().unwrap();
    authority
        .handle(V3Command::InvokeTool(reused_id_call))
        .unwrap();
    assert_eq!(
        authority
            .handle(V3Command::DenyTool(V3ToolDecision {
                call_id: "decision-call-reused-id".into(),
                call_digest: reused_id_digest,
                decision_id: "decision-approve-1".into(),
            }))
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert_eq!(
        store
            .tool_approval("decision-call-reused-id", 1)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Pending
    );
    assert!(store.audit_events_for_run(None).unwrap().is_empty());

    assert_eq!(
        authority
            .handle(V3Command::ApproveTool(V3ToolDecision {
                call_id: "decision-call-deny".into(),
                call_digest: deny_digest,
                decision_id: "decision-after-deny".into(),
            }))
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert_eq!(
        authority
            .handle(V3Command::ApproveTool(V3ToolDecision {
                call_id: "missing-decision-call".into(),
                call_digest: "0".repeat(64),
                decision_id: "missing-decision".into(),
            }))
            .unwrap_err()
            .code(),
        "not_found"
    );

    store
        .create_tool_approval(NewToolApproval {
            call_id: "decision-call-expired".into(),
            tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
            call_digest: "b".repeat(64),
            created_at_ms: 1,
            expires_at_ms: 2,
        })
        .unwrap();
    let V3Event::ToolApprovalStatus(expired) = authority
        .handle(V3Command::ApproveTool(V3ToolDecision {
            call_id: "decision-call-expired".into(),
            call_digest: "b".repeat(64),
            decision_id: "decision-expired".into(),
        }))
        .unwrap()
    else {
        panic!("expected durable expired status");
    };
    assert_eq!(expired.state, V3ToolApprovalState::Expired);
    assert_eq!(
        store
            .tool_approval_events("decision-call-expired")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![ToolApprovalState::Pending, ToolApprovalState::Expired]
    );
}

#[cfg(not(feature = "personal-edition"))]
#[test]
fn exact_tool_cancellation_is_durable_without_consumption_or_execution() {
    let store = store();
    let authority = V3RuntimeAuthority::from_provider_routes(
        [],
        Vec::new(),
        Arc::clone(&store),
        memory_route(),
        None,
    )
    .unwrap();
    let pending = |call_id: &str| V3ToolCall {
        tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
        call_id: call_id.into(),
        input: serde_json::json!({"record_id": format!("memory:{call_id}")}),
    };

    let pending_call = pending("cancel-pending-call");
    authority
        .handle(V3Command::InvokeTool(pending_call))
        .unwrap();
    let V3Event::ToolApprovalStatus(cancelled) = authority
        .handle(V3Command::CancelTool(V3Action {
            resource_id: "cancel-pending-call".into(),
            operation_id: "cancellation-pending-1".into(),
        }))
        .unwrap()
    else {
        panic!("expected cancelled pending status");
    };
    assert_eq!(cancelled.state, V3ToolApprovalState::Cancelled);
    assert_eq!(
        store
            .tool_approval_events("cancel-pending-call")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![ToolApprovalState::Pending, ToolApprovalState::Cancelled]
    );
    assert_eq!(
        authority
            .handle(V3Command::CancelTool(V3Action {
                resource_id: "cancel-pending-call".into(),
                operation_id: "cancellation-after-terminal".into(),
            }))
            .unwrap_err()
            .code(),
        "conflict"
    );

    let approved_call = pending("cancel-approved-call");
    let approved_digest = approved_call.approval_digest().unwrap();
    authority
        .handle(V3Command::InvokeTool(approved_call))
        .unwrap();
    authority
        .handle(V3Command::ApproveTool(V3ToolDecision {
            call_id: "cancel-approved-call".into(),
            call_digest: approved_digest,
            decision_id: "decision-before-cancel".into(),
        }))
        .unwrap();
    let V3Event::ToolApprovalStatus(cancelled) = authority
        .handle(V3Command::CancelTool(V3Action {
            resource_id: "cancel-approved-call".into(),
            operation_id: "cancellation-approved-1".into(),
        }))
        .unwrap()
    else {
        panic!("expected cancelled approved status");
    };
    assert_eq!(cancelled.state, V3ToolApprovalState::Cancelled);
    assert_eq!(
        store
            .tool_approval_events("cancel-approved-call")
            .unwrap()
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![
            ToolApprovalState::Pending,
            ToolApprovalState::Approved,
            ToolApprovalState::Cancelled,
        ]
    );

    let reused_id_call = pending("cancel-reused-id-call");
    authority
        .handle(V3Command::InvokeTool(reused_id_call))
        .unwrap();
    assert_eq!(
        authority
            .handle(V3Command::CancelTool(V3Action {
                resource_id: "cancel-reused-id-call".into(),
                operation_id: "decision-before-cancel".into(),
            }))
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert_eq!(
        store
            .tool_approval("cancel-reused-id-call", 1)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Pending
    );
    assert_eq!(
        authority
            .handle(V3Command::CancelTool(V3Action {
                resource_id: "missing-cancel-call".into(),
                operation_id: "cancellation-missing".into(),
            }))
            .unwrap_err()
            .code(),
        "not_found"
    );

    store
        .create_tool_approval(NewToolApproval {
            call_id: "cancel-expired-call".into(),
            tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
            call_digest: "c".repeat(64),
            created_at_ms: 1,
            expires_at_ms: 2,
        })
        .unwrap();
    let V3Event::ToolApprovalStatus(expired) = authority
        .handle(V3Command::CancelTool(V3Action {
            resource_id: "cancel-expired-call".into(),
            operation_id: "cancellation-expired".into(),
        }))
        .unwrap()
    else {
        panic!("expected expired cancellation status");
    };
    assert_eq!(expired.state, V3ToolApprovalState::Expired);
    assert!(store.audit_events_for_run(None).unwrap().is_empty());
}

#[test]
fn persisted_authorization_rejects_duplicate_call_ids_after_reopen() {
    let root = std::env::temp_dir().join(format!(
        "abbey-v3-tool-reopen-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).unwrap();
    let database = root.join("runtime.sqlite");
    {
        let store = Arc::new(RuntimeStore::open(&database).unwrap());
        let authority = V3RuntimeAuthority::from_provider_routes(
            [],
            Vec::new(),
            Arc::clone(&store),
            memory_route(),
            None,
        )
        .unwrap();
        assert!(matches!(
            authority
                .handle(V3Command::InvokeTool(V3ToolCall {
                    tool_id: "abbey_status".into(),
                    call_id: "durable-call-1".into(),
                    input: serde_json::json!({}),
                }))
                .unwrap(),
            V3Event::ToolResult(_)
        ));
        #[cfg(not(feature = "personal-edition"))]
        assert!(matches!(
            authority
                .handle(V3Command::InvokeTool(V3ToolCall {
                    tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
                    call_id: "durable-pending-1".into(),
                    input: serde_json::json!({"record_id": "memory:durable"}),
                }))
                .unwrap(),
            V3Event::ToolApprovalStatus(_)
        ));
    }
    {
        let store = Arc::new(RuntimeStore::open(&database).unwrap());
        let authority = V3RuntimeAuthority::from_provider_routes(
            [],
            Vec::new(),
            Arc::clone(&store),
            memory_route(),
            None,
        )
        .unwrap();
        assert_eq!(
            authority
                .handle(V3Command::InvokeTool(V3ToolCall {
                    tool_id: "abbey_status".into(),
                    call_id: "durable-call-1".into(),
                    input: serde_json::json!({}),
                }))
                .unwrap_err()
                .code(),
            "conflict"
        );
        assert_eq!(store.audit_events_for_run(None).unwrap().len(), 2);
        #[cfg(not(feature = "personal-edition"))]
        assert_eq!(
            authority
                .handle(V3Command::InvokeTool(V3ToolCall {
                    tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.into(),
                    call_id: "durable-pending-1".into(),
                    input: serde_json::json!({"record_id": "memory:durable"}),
                }))
                .unwrap_err()
                .code(),
            "conflict"
        );
        #[cfg(not(feature = "personal-edition"))]
        assert_eq!(
            store
                .tool_approval_events("durable-pending-1")
                .unwrap()
                .len(),
            1
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
