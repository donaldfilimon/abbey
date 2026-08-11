use std::sync::Arc;
use std::time::Duration;

use abi_agent_runtime::{EchoProvider, RunBudget};

use super::*;
use crate::app_core::{APP_PROTOCOL_V1, V3GrantRequest, V3PageQuery, V3ResourceQuery, V3ToolCall};
#[cfg(not(feature = "personal-edition"))]
use crate::app_core::{V3Action, V3ToolDecision};

fn store() -> Arc<RuntimeStore> {
    Arc::new(RuntimeStore::open(std::path::Path::new(":memory:")).unwrap())
}

fn memory_route() -> MemoryEffectRoute {
    MemoryEffectRoute::new(
        std::env::temp_dir().join(format!(
            "abbey-v3-memory-route-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        )),
        "sqlite".to_owned(),
    )
}

fn abi_route() -> ProviderRoute {
    ProviderRoute::new(
        BackendSelection::Abi,
        "local",
        Arc::new(EchoProvider::new()),
        RunBudget::unlimited()
            .with_max_events(8)
            .with_max_output_tokens(8)
            .with_max_duration(Duration::from_secs(1)),
    )
    .unwrap()
}

#[test]
fn negotiates_only_startup_bound_abi_model_reads() {
    let route = abi_route();
    let authority =
        V3RuntimeAuthority::from_provider_routes([&route], store(), memory_route()).unwrap();
    let requested =
        V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels, V3Capability::PollEvents])
            .unwrap();
    let V3Event::Negotiated(negotiated) = authority
        .handle(V3Command::Negotiate(V3GrantRequest {
            supported_versions: vec![1, 2, 3],
            requested,
        }))
        .unwrap()
    else {
        panic!("expected v3 negotiation");
    };
    assert_eq!(negotiated.granted.as_slice(), &[V3Capability::ReadModels]);

    let V3Event::Models(models) = authority
        .handle(V3Command::ListModels(V3PageQuery::default()))
        .unwrap()
    else {
        panic!("expected model inventory");
    };
    assert_eq!(models.after, 0);
    assert_eq!(models.through, 1);
    assert_eq!(models.records.len(), 1);
    assert_eq!(models.records[0].id, "abi-local:local");
    assert_eq!(
        authority
            .handle(V3Command::ListModels(V3PageQuery {
                after: 0,
                through: Some(2),
                limit: 1,
            }))
            .unwrap_err()
            .code(),
        "invalid_command"
    );
    assert_eq!(
        authority
            .handle(V3Command::PollEvents(V3PageQuery::default()))
            .unwrap_err()
            .code(),
        "capability_denied"
    );
}

#[test]
fn no_abi_route_still_negotiates_safe_tools_and_canonical_claim_reads() {
    let store = store();
    let authority =
        V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store), memory_route()).unwrap();
    let V3Event::Negotiated(negotiated) = authority
        .handle(V3Command::Negotiate(V3GrantRequest {
            supported_versions: vec![3],
            requested: V3CapabilitySet::from_sorted(vec![
                V3Capability::ListTools,
                V3Capability::InvokeTools,
                V3Capability::DecideToolApprovals,
                V3Capability::CancelTools,
                V3Capability::ReadModels,
                V3Capability::ReadClaimsById,
            ])
            .unwrap(),
        }))
        .unwrap()
    else {
        panic!("expected v3 negotiation");
    };
    let expected_grants = if crate::edition::ACTIVE == crate::edition::Edition::Safe {
        vec![
            V3Capability::ListTools,
            V3Capability::InvokeTools,
            V3Capability::DecideToolApprovals,
            V3Capability::CancelTools,
            V3Capability::ReadClaimsById,
        ]
    } else {
        vec![
            V3Capability::ListTools,
            V3Capability::InvokeTools,
            V3Capability::ReadClaimsById,
        ]
    };
    assert_eq!(negotiated.granted.as_slice(), expected_grants);
    let V3Event::Tools(tools) = authority
        .handle(V3Command::ListTools(V3PageQuery::default()))
        .unwrap()
    else {
        panic!("expected safe tool inventory");
    };
    assert_eq!(tools.after, 0);
    let expected_through = if crate::edition::ACTIVE == crate::edition::Edition::Safe {
        4
    } else {
        3
    };
    assert_eq!(tools.through, expected_through);
    assert_eq!(
        tools
            .tools
            .iter()
            .take(crate::mcp_host::SAFE_TOOLS.len())
            .map(|tool| tool.tool_id.as_str())
            .collect::<Vec<_>>(),
        crate::mcp_host::tool_names()
    );
    for (descriptor, safe) in tools.tools.iter().zip(crate::mcp_host::SAFE_TOOLS) {
        assert_eq!(descriptor.description, safe.description);
        assert_eq!(descriptor.input_schema, (safe.schema)());
        assert_eq!(descriptor.effect, crate::app_core::V3ToolEffect::ReadOnly);
    }
    if crate::edition::ACTIVE == crate::edition::Edition::Safe {
        let approval_tool = tools.tools.last().unwrap();
        assert_eq!(
            approval_tool.tool_id,
            tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID
        );
        assert_eq!(
            approval_tool.effect,
            crate::app_core::V3ToolEffect::Mutating
        );
    }
    let V3Event::Claim(claim) = authority
        .handle(V3Command::ClaimById(V3ResourceQuery {
            resource_id: "backend-cursor-agent".into(),
        }))
        .unwrap()
    else {
        panic!("expected stable claim");
    };
    assert_eq!(claim.id, "backend-cursor-agent");
    assert_eq!(claim.status, ClaimStatus::Current);
    assert_eq!(
        authority
            .handle(V3Command::ListModels(V3PageQuery::default()))
            .unwrap_err()
            .code(),
        "capability_denied"
    );
    assert_eq!(
        authority
            .handle(V3Command::ClaimById(V3ResourceQuery {
                resource_id: "backend-cursor".into(),
            }))
            .unwrap_err()
            .code(),
        "not_found"
    );
    assert_eq!(
        authority
            .handle(V3Command::InvokeTool(V3ToolCall {
                tool_id: "abbey_status".into(),
                call_id: "schema-retry".into(),
                input: serde_json::json!({"unexpected": true}),
            }))
            .unwrap_err()
            .code(),
        "invalid_command"
    );
    assert_eq!(
        authority
            .handle(V3Command::InvokeTool(V3ToolCall {
                tool_id: "missing_tool".into(),
                call_id: "unknown-call".into(),
                input: serde_json::json!({}),
            }))
            .unwrap_err()
            .code(),
        "not_found"
    );
    assert!(store.audit_events_for_run(None).unwrap().is_empty());
    let V3Event::ToolResult(result) = authority
        .handle(V3Command::InvokeTool(V3ToolCall {
            tool_id: "abbey_status".into(),
            call_id: "schema-retry".into(),
            input: serde_json::json!({}),
        }))
        .unwrap()
    else {
        panic!("expected bounded tool result");
    };
    assert_eq!(result.tool_id, "abbey_status");
    assert_eq!(result.call_id, "schema-retry");
    assert_eq!(result.state, V3OperationState::Succeeded);
    assert_eq!(result.output["protocol_version"], APP_PROTOCOL_V1);
    let audit = store.audit_events_for_run(None).unwrap();
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].action, "v3_tool_authorization");
    assert_eq!(audit[0].outcome, "allow");
    assert_eq!(audit[0].metadata["policy"], "effect-scoped");
    assert_eq!(audit[1].action, "v3_tool_execution");
    assert_eq!(audit[1].outcome, "succeeded");
    assert!(
        audit
            .iter()
            .all(|event| event.metadata.get("input").is_none())
    );
    assert_eq!(
        authority
            .handle(V3Command::InvokeTool(V3ToolCall {
                tool_id: "abbey_status".into(),
                call_id: "schema-retry".into(),
                input: serde_json::json!({}),
            }))
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert_eq!(
        authority
            .handle(V3Command::ListTools(V3PageQuery {
                after: 0,
                through: Some(expected_through + 1),
                limit: 1,
            }))
            .unwrap_err()
            .code(),
        "invalid_command"
    );
}

#[cfg(not(feature = "personal-edition"))]
#[test]
fn mutating_safe_tool_only_persists_an_exact_pending_approval() {
    let store = store();
    let authority =
        V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store), memory_route()).unwrap();
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
    let authority =
        V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store), memory_route()).unwrap();
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
    let authority =
        V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store), memory_route()).unwrap();
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
        let authority =
            V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store), memory_route())
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
        let authority =
            V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store), memory_route())
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
