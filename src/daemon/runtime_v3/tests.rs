use std::sync::Arc;
use std::time::Duration;

use abi_agent_runtime::{EchoProvider, RunBudget};

use super::*;
use crate::app_core::{
    APP_PROTOCOL_V1, ClaimStatus, V3GrantRequest, V3PageQuery, V3ResourceQuery, V3ToolCall,
};
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
fn manifest_models_negotiate_reads_and_append_after_route_entries() {
    let manifest_models = vec![V3EntityRecord {
        id: "abi-model:tiny".to_owned(),
        label: "local model tiny example/tiny@0123456789ab (not downloaded)".to_owned(),
        state: V3OperationState::NotDownloaded,
    }];
    let route = abi_route();
    let authority = V3RuntimeAuthority::from_provider_routes(
        [&route],
        manifest_models.clone(),
        store(),
        memory_route(),
        None,
    )
    .unwrap();
    let V3Event::Models(models) = authority
        .handle(V3Command::ListModels(V3PageQuery::default()))
        .unwrap()
    else {
        panic!("expected model inventory");
    };
    assert_eq!(models.through, 2);
    assert_eq!(models.records[0].id, "abi-local:local");
    assert_eq!(models.records[1].id, "abi-model:tiny");
    assert_eq!(models.records[1].state, V3OperationState::NotDownloaded);

    // Manifest-derived inventory alone grants read_models with no ABI route.
    let authority = V3RuntimeAuthority::from_provider_routes(
        [],
        manifest_models,
        store(),
        memory_route(),
        None,
    )
    .unwrap();
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap();
    let V3Event::Negotiated(negotiated) = authority
        .handle(V3Command::Negotiate(V3GrantRequest {
            supported_versions: vec![3],
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
    assert_eq!(models.records.len(), 1);
    assert_eq!(models.records[0].id, "abi-model:tiny");
}

#[test]
fn negotiates_only_startup_bound_abi_model_reads() {
    let route = abi_route();
    let authority = V3RuntimeAuthority::from_provider_routes(
        [&route],
        Vec::new(),
        store(),
        memory_route(),
        None,
    )
    .unwrap();
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
    let authority = V3RuntimeAuthority::from_provider_routes(
        [],
        Vec::new(),
        Arc::clone(&store),
        memory_route(),
        None,
    )
    .unwrap();
    let V3Event::Negotiated(negotiated) = authority
        .handle(V3Command::Negotiate(V3GrantRequest {
            supported_versions: vec![3],
            requested: V3CapabilitySet::from_sorted(vec![
                V3Capability::ListTools,
                V3Capability::InvokeTools,
                V3Capability::DecideToolApprovals,
                V3Capability::CancelTools,
                V3Capability::ReadMemory,
                V3Capability::ReadModels,
                V3Capability::ReadWorkers,
                V3Capability::ReadClaimsById,
                V3Capability::InferModels,
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
            V3Capability::ReadMemory,
            V3Capability::ReadWorkers,
            V3Capability::ReadClaimsById,
        ]
    } else {
        vec![
            V3Capability::ListTools,
            V3Capability::InvokeTools,
            V3Capability::ReadMemory,
            V3Capability::ReadWorkers,
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

include!("tests/approval_tests.rs");

#[test]
fn inventory_only_authority_refuses_exact_model_details() {
    let route = abi_route();
    let authority =
        V3RuntimeAuthority::from_provider_routes([&route], vec![], store(), memory_route(), None)
            .unwrap();
    let command = V3Command::ModelDetails(V3ResourceQuery {
        resource_id: "abi-local:local".into(),
    });
    assert!(!authority.authorizes(&V3CapabilitySet::deny_all(), &command));
    let forged =
        V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels, V3Capability::ManageModels])
            .unwrap();
    assert!(!authority.authorizes(&forged, &command));
    assert!(
        authority
            .handle(V3Command::ModelDetails(V3ResourceQuery {
                resource_id: "abi-local:local".into()
            }))
            .is_err()
    );
}

#[test]
fn read_workers_grant_yields_basic_empty_list_and_not_found_queries() {
    // Low-risk worker dispatch test: grant is always offered; lists are empty stubs;
    // specific health/job queries are not-found. Ties into future abi-worker without
    // promoting any capability claim.
    let authority =
        V3RuntimeAuthority::from_provider_routes([], Vec::new(), store(), memory_route(), None)
            .unwrap();
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadWorkers]).unwrap();
    let V3Event::Negotiated(negotiated) = authority
        .handle(V3Command::Negotiate(V3GrantRequest {
            supported_versions: vec![3],
            requested,
        }))
        .unwrap()
    else {
        panic!("expected negotiation");
    };
    assert_eq!(negotiated.granted.as_slice(), &[V3Capability::ReadWorkers]);

    let V3Event::Workers(workers) = authority
        .handle(V3Command::ListWorkers(V3PageQuery::default()))
        .unwrap()
    else {
        panic!("expected workers page");
    };
    assert_eq!(workers.after, 0);
    assert_eq!(workers.through, 0);
    assert!(workers.records.is_empty());

    let V3Event::Clusters(clusters) = authority
        .handle(V3Command::ListClusters(V3PageQuery::default()))
        .unwrap()
    else {
        panic!("expected clusters page");
    };
    assert!(clusters.records.is_empty());

    let bad = V3ResourceQuery {
        resource_id: "no-such-worker".into(),
    };
    assert_eq!(
        authority
            .handle(V3Command::WorkerHealth(bad.clone()))
            .unwrap_err()
            .code(),
        "not_found"
    );
    assert_eq!(
        authority
            .handle(V3Command::JobStatus(bad))
            .unwrap_err()
            .code(),
        "not_found"
    );
}
