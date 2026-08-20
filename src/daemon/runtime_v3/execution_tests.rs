#[cfg(not(feature = "personal-edition"))]
mod safe {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::super::*;
    use crate::app_core::{V3ToolCall, V3ToolDecision};
    use crate::memory::{MemoryRecord, MemoryStore, SqliteMemory};
    use crate::runtime::{ToolApprovalState, ToolExecutionState};

    fn fixture(label: &str) -> (PathBuf, Arc<RuntimeStore>, V3RuntimeAuthority) {
        let root = std::env::temp_dir().join(format!(
            "abbey-v3-execution-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let runtime = Arc::new(RuntimeStore::open(&root.join("runtime.sqlite")).unwrap());
        let authority = V3RuntimeAuthority::from_provider_routes(
            [],
            Vec::new(),
            Arc::clone(&runtime),
            MemoryEffectRoute::new(root.clone(), "sqlite".to_owned()),
            None,
        )
        .unwrap();
        (root, runtime, authority)
    }

    fn approve(authority: &V3RuntimeAuthority, call: &V3ToolCall, decision_id: &str) {
        let V3Event::ToolApprovalStatus(pending) = authority
            .handle(V3Command::InvokeTool(call.clone()))
            .unwrap()
        else {
            panic!("first exact call must remain pending");
        };
        assert_eq!(pending.state, V3ToolApprovalState::Pending);
        let V3Event::ToolApprovalStatus(approved) = authority
            .handle(V3Command::ApproveTool(V3ToolDecision {
                call_id: call.call_id.clone(),
                call_digest: pending.call_digest,
                decision_id: decision_id.to_owned(),
            }))
            .unwrap()
        else {
            panic!("exact decision must return approval status");
        };
        assert_eq!(approved.state, V3ToolApprovalState::Approved);
    }

    #[test]
    fn exact_approved_resubmission_marks_memory_obsolete_and_persists_terminal_digest() {
        let (root, runtime, authority) = fixture("success");
        let memory = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&root)).unwrap();
        let mut record = MemoryRecord::new_stm("stale", "private-payload-canary");
        record.id = "memory:served-success".to_owned();
        memory.store(record).unwrap();
        let call = V3ToolCall {
            tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.to_owned(),
            call_id: "served-success-call".to_owned(),
            input: serde_json::json!({"record_id": "memory:served-success"}),
        };
        approve(&authority, &call, "served-success-decision");

        let mut changed = call.clone();
        changed.input = serde_json::json!({"record_id": "memory:changed"});
        assert_eq!(
            authority
                .handle(V3Command::InvokeTool(changed))
                .unwrap_err()
                .code(),
            "conflict"
        );
        assert!(
            !memory
                .get("memory:served-success")
                .unwrap()
                .unwrap()
                .obsolete
        );

        let V3Event::ToolResult(result) = authority
            .handle(V3Command::InvokeTool(call.clone()))
            .unwrap()
        else {
            panic!("approved exact resubmission must return a terminal result");
        };
        assert_eq!(result.state, V3OperationState::Succeeded);
        assert_eq!(result.output["obsolete"], true);
        assert!(
            memory
                .get("memory:served-success")
                .unwrap()
                .unwrap()
                .obsolete
        );
        assert_eq!(
            runtime
                .tool_approval(&call.call_id, now_ms().unwrap())
                .unwrap()
                .unwrap()
                .state,
            ToolApprovalState::Consumed
        );
        let execution = runtime.tool_execution(&call.call_id).unwrap().unwrap();
        assert_eq!(execution.state, ToolExecutionState::Succeeded);
        assert_eq!(execution.result_digest.as_deref().unwrap().len(), 64);
        assert_eq!(
            runtime.tool_execution_events(&call.call_id).unwrap().len(),
            2
        );
        assert_eq!(
            authority
                .handle(V3Command::InvokeTool(call))
                .unwrap_err()
                .code(),
            "conflict"
        );
        let audit = runtime.audit_events_for_run(None).unwrap();
        assert_eq!(audit.len(), 2);
        assert!(audit.iter().all(|event| {
            event.metadata["effect"] == "mutating"
                && event.metadata["policy"] == "exact-call-approval"
                && event.metadata.get("record_id").is_none()
                && event.metadata.get("input").is_none()
        }));
        drop(authority);
        drop(runtime);
        drop(memory);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_memory_is_a_bounded_terminal_failure_not_an_unconsumed_retry() {
        let (root, runtime, authority) = fixture("failure");
        let call = V3ToolCall {
            tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.to_owned(),
            call_id: "served-failure-call".to_owned(),
            input: serde_json::json!({"record_id": "memory:missing"}),
        };
        approve(&authority, &call, "served-failure-decision");
        let V3Event::ToolResult(result) = authority
            .handle(V3Command::InvokeTool(call.clone()))
            .unwrap()
        else {
            panic!("effect failure must still be a correlated terminal result");
        };
        assert_eq!(result.state, V3OperationState::Failed);
        assert_eq!(
            result.output,
            serde_json::json!({"code": "memory_unavailable"})
        );
        assert_eq!(
            runtime
                .tool_execution(&call.call_id)
                .unwrap()
                .unwrap()
                .state,
            ToolExecutionState::Failed
        );
        drop(authority);
        drop(runtime);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_configured_backend_never_substitutes_sqlite_for_an_approved_effect() {
        let (root, runtime, _) = fixture("invalid-backend");
        let memory = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&root)).unwrap();
        let mut record = MemoryRecord::new_stm("current", "wrong-store-canary");
        record.id = "memory:wrong-store".to_owned();
        memory.store(record).unwrap();
        let authority = V3RuntimeAuthority::from_provider_routes(
            [],
            Vec::new(),
            Arc::clone(&runtime),
            MemoryEffectRoute::new(root.clone(), "invalid".to_owned()),
            None,
        )
        .unwrap();
        let call = V3ToolCall {
            tool_id: tool_catalog::MEMORY_MARK_OBSOLETE_TOOL_ID.to_owned(),
            call_id: "wrong-store-call".to_owned(),
            input: serde_json::json!({"record_id": "memory:wrong-store"}),
        };
        approve(&authority, &call, "wrong-store-decision");

        let V3Event::ToolResult(result) = authority
            .handle(V3Command::InvokeTool(call.clone()))
            .unwrap()
        else {
            panic!("invalid backend must return a bounded terminal result");
        };
        assert_eq!(result.state, V3OperationState::Failed);
        assert_eq!(
            result.output,
            serde_json::json!({"code": "memory_unavailable"})
        );
        assert!(!memory.get("memory:wrong-store").unwrap().unwrap().obsolete);
        assert_eq!(
            runtime
                .tool_execution(&call.call_id)
                .unwrap()
                .unwrap()
                .state,
            ToolExecutionState::Failed
        );
        drop(authority);
        drop(runtime);
        drop(memory);
        std::fs::remove_dir_all(root).unwrap();
    }
}
