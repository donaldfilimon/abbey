//! Minimal daemon-owned authority for the first served protocol-v3 slice.
//!
//! Only the canonical safe read-only tool inventory and invocation,
//! startup-bound ABI-local model inventory, and exact stable-ID reads from
//! Abbey's canonical claim registry are representable here. Approval, memory,
//! training, worker, cancellation, and polling grants remain absent.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use abi_agent_host::{ToolExecutionContext, ToolExecutor};
use abi_agent_runtime::{
    CancellationToken, EffectScopedPolicy, ExecutionPolicy, ToolCall, ToolSpec, ToolStatus,
};
use jsonschema::Validator;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::app_core::{
    APP_PROTOCOL_V3, APP_SCHEMA_V3, BackendSelection, ClaimStatus, V3Capability, V3CapabilitySet,
    V3Command, V3EntityPage, V3EntityRecord, V3Event, V3GrantNegotiation, V3OperationState,
    V3StableClaim, V3ToolDescriptor, V3ToolPage, V3ToolResult,
};
use crate::runtime::{AuditMetadata, NewAuditEvent, ProviderRoute, RuntimeStore};

use super::server::HandlerFailure;

struct BoundSafeTool {
    descriptor: V3ToolDescriptor,
    spec: ToolSpec,
    validator: Validator,
}

/// Frozen protocol-v3 grants plus presentation-safe tool, model, and claim authority.
pub(super) struct V3RuntimeAuthority {
    grants: V3CapabilitySet,
    tools: Vec<BoundSafeTool>,
    models: Vec<V3EntityRecord>,
    store: Arc<RuntimeStore>,
    used_tool_call_ids: Mutex<HashSet<String>>,
}

impl V3RuntimeAuthority {
    /// Derive v3 authority from the same startup-owned provider objects used
    /// by execution. Foundation Models is deliberately not granted yet.
    pub(super) fn from_provider_routes<'a>(
        routes: impl IntoIterator<Item = &'a ProviderRoute>,
        store: Arc<RuntimeStore>,
    ) -> Result<Self, HandlerFailure> {
        let models = routes
            .into_iter()
            .filter(|provider| provider.backend() == BackendSelection::Abi)
            .map(|provider| V3EntityRecord {
                id: format!("abi-local:{}", provider.model()),
                label: format!("ABI local model {}", provider.model()),
                state: V3OperationState::Available,
            })
            .collect::<Vec<_>>();
        let descriptors = crate::mcp_host::v3_descriptors().map_err(|_| internal_failure())?;
        let specs = crate::mcp_host::v3_specs().map_err(|_| internal_failure())?;
        if descriptors.len() != specs.len() {
            return Err(internal_failure());
        }
        let tools = descriptors
            .into_iter()
            .zip(specs)
            .map(|(descriptor, spec)| {
                if descriptor.tool_id != spec.name {
                    return Err(internal_failure());
                }
                let validator = jsonschema::validator_for(&descriptor.input_schema)
                    .map_err(|_| internal_failure())?;
                Ok(BoundSafeTool {
                    descriptor,
                    spec,
                    validator,
                })
            })
            .collect::<Result<Vec<_>, HandlerFailure>>()?;
        let used_tool_call_ids = store
            .audit_events_for_run(None)
            .map_err(|_| internal_failure())?
            .into_iter()
            .filter(|event| event.action == "v3_tool_authorization")
            .filter_map(|event| {
                event
                    .metadata
                    .get("call_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        let mut available = Vec::with_capacity(4);
        if !tools.is_empty() {
            available.push(V3Capability::ListTools);
            available.push(V3Capability::InvokeTools);
        }
        if !models.is_empty() {
            available.push(V3Capability::ReadModels);
        }
        available.push(V3Capability::ReadClaimsById);
        let grants = V3CapabilitySet::from_sorted(available).map_err(|_| internal_failure())?;
        Ok(Self {
            grants,
            tools,
            models,
            store,
            used_tool_call_ids: Mutex::new(used_tool_call_ids),
        })
    }

    /// Dispatch negotiation or one explicitly granted read-only command.
    pub(super) fn handle(&self, command: V3Command) -> Result<V3Event, HandlerFailure> {
        if let V3Command::Negotiate(request) = command {
            let granted = self
                .grants
                .as_slice()
                .iter()
                .copied()
                .filter(|grant| request.requested.contains(*grant))
                .collect();
            let granted = V3CapabilitySet::from_sorted(granted).map_err(|_| internal_failure())?;
            return Ok(V3Event::Negotiated(V3GrantNegotiation {
                protocol_version: APP_PROTOCOL_V3,
                schema_version: APP_SCHEMA_V3,
                granted,
            }));
        }
        if !self.grants.permits(&command) {
            return Err(capability_denied_failure());
        }
        match command {
            V3Command::ListTools(page) => {
                let snapshot = u64::try_from(self.tools.len()).map_err(|_| internal_failure())?;
                let through = page.through.unwrap_or(snapshot);
                if through > snapshot || page.after > through {
                    return Err(invalid_command_failure());
                }
                let available = through.saturating_sub(page.after);
                let take = available.min(u64::from(page.limit));
                let skip = usize::try_from(page.after).map_err(|_| invalid_command_failure())?;
                let take = usize::try_from(take).map_err(|_| invalid_command_failure())?;
                Ok(V3Event::Tools(V3ToolPage {
                    after: page.after,
                    through,
                    tools: self
                        .tools
                        .iter()
                        .skip(skip)
                        .take(take)
                        .map(|tool| tool.descriptor.clone())
                        .collect(),
                }))
            }
            V3Command::InvokeTool(call) => self.invoke_tool(call),
            V3Command::ListModels(page) => {
                let snapshot = u64::try_from(self.models.len()).map_err(|_| internal_failure())?;
                let through = page.through.unwrap_or(snapshot);
                if through > snapshot || page.after > through {
                    return Err(invalid_command_failure());
                }
                let available = through.saturating_sub(page.after);
                let take = available.min(u64::from(page.limit));
                let skip = usize::try_from(page.after).map_err(|_| invalid_command_failure())?;
                let take = usize::try_from(take).map_err(|_| invalid_command_failure())?;
                Ok(V3Event::Models(V3EntityPage {
                    after: page.after,
                    through,
                    records: self.models.iter().skip(skip).take(take).cloned().collect(),
                }))
            }
            V3Command::ClaimById(query) => {
                let claim = crate::claims::CLAIMS
                    .iter()
                    .find(|claim| claim.id == query.resource_id)
                    .ok_or_else(not_found_failure)?;
                let claim = V3StableClaim {
                    id: claim.id.to_owned(),
                    name: claim.name.to_owned(),
                    status: claim_status(claim.status),
                    note: claim.note.to_owned(),
                };
                claim.validate().map_err(|_| internal_failure())?;
                Ok(V3Event::Claim(claim))
            }
            _ => Err(capability_denied_failure()),
        }
    }

    /// Validate every echoed grant against startup authority and the command.
    pub(super) fn authorizes(&self, grants: &V3CapabilitySet, command: &V3Command) -> bool {
        grants
            .as_slice()
            .iter()
            .all(|grant| self.grants.contains(*grant))
            && grants.permits(command)
    }

    fn invoke_tool(&self, call: crate::app_core::V3ToolCall) -> Result<V3Event, HandlerFailure> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.descriptor.tool_id == call.tool_id)
            .ok_or_else(tool_not_found_failure)?;
        tool.validator
            .validate(&call.input)
            .map_err(|_| invalid_command_failure())?;

        let input = serde_json::to_string(&call.input).map_err(|_| invalid_command_failure())?;
        let abi_call = ToolCall::new(&call.call_id, &call.tool_id, input);
        {
            let mut used = self
                .used_tool_call_ids
                .lock()
                .map_err(|_| internal_failure())?;
            if !used.insert(call.call_id.clone()) {
                return Err(conflict_failure());
            }
        }

        let input_digest = digest(abi_call.input.as_bytes());
        let policy = EffectScopedPolicy;
        let decision = policy.authorize(&abi_call, Some(&tool.spec));
        if self
            .record_tool_audit(
                "v3_tool_authorization",
                decision.as_str(),
                &call.call_id,
                &call.tool_id,
                &input_digest,
                None,
            )
            .is_err()
        {
            self.forget_tool_call(&call.call_id);
            return Err(internal_failure());
        }
        if !decision.is_allowed() {
            return Err(capability_denied_failure());
        }

        let cancellation = CancellationToken::new();
        let deadline = Instant::now() + Duration::from_secs(1);
        let output = match crate::mcp_host::V3SafeToolExecutor.execute(
            &abi_call,
            &tool.spec,
            ToolExecutionContext {
                cancellation: &cancellation,
                deadline,
            },
        ) {
            Ok(output) => output,
            Err(_) => {
                self.record_tool_audit(
                    "v3_tool_execution",
                    "failed",
                    &call.call_id,
                    &call.tool_id,
                    &input_digest,
                    None,
                )
                .map_err(|_| internal_failure())?;
                return Err(internal_failure());
            }
        };
        if Instant::now() >= deadline {
            self.record_tool_audit(
                "v3_tool_execution",
                "deadline_exceeded",
                &call.call_id,
                &call.tool_id,
                &input_digest,
                None,
            )
            .map_err(|_| internal_failure())?;
            return Err(deadline_failure());
        }
        let output_json = match serde_json::from_str(output.payload()) {
            Ok(output) => output,
            Err(_) => {
                self.record_tool_audit(
                    "v3_tool_execution",
                    "failed",
                    &call.call_id,
                    &call.tool_id,
                    &input_digest,
                    None,
                )
                .map_err(|_| internal_failure())?;
                return Err(internal_failure());
            }
        };
        let state = match output.status() {
            ToolStatus::Ok => V3OperationState::Succeeded,
            ToolStatus::Error | ToolStatus::Denied => V3OperationState::Failed,
        };
        let result = V3ToolResult {
            tool_id: call.tool_id,
            call_id: call.call_id,
            state,
            output: output_json,
        };
        if result.validate().is_err() {
            self.record_tool_audit(
                "v3_tool_execution",
                "response_too_large",
                &result.call_id,
                &result.tool_id,
                &input_digest,
                None,
            )
            .map_err(|_| internal_failure())?;
            return Err(response_too_large_failure());
        }
        let output_bytes = serde_json::to_vec(&result.output).map_err(|_| internal_failure())?;
        self.record_tool_audit(
            "v3_tool_execution",
            if state == V3OperationState::Succeeded {
                "succeeded"
            } else {
                "failed"
            },
            &result.call_id,
            &result.tool_id,
            &input_digest,
            Some((&digest(&output_bytes), output_bytes.len())),
        )
        .map_err(|_| internal_failure())?;
        Ok(V3Event::ToolResult(result))
    }

    fn record_tool_audit(
        &self,
        action: &str,
        outcome: &str,
        call_id: &str,
        tool_id: &str,
        input_digest: &str,
        output: Option<(&str, usize)>,
    ) -> Result<(), crate::runtime::StoreError> {
        let mut metadata = json!({
            "call_id": call_id,
            "tool_id": tool_id,
            "input_digest": input_digest,
            "effect": "read_only",
            "policy": "effect-scoped"
        });
        if let Some((output_digest, output_bytes)) = output {
            metadata["output_digest"] = json!(output_digest);
            metadata["output_bytes"] = json!(output_bytes);
        }
        self.store.record_audit(NewAuditEvent {
            run_id: None,
            action: action.to_owned(),
            outcome: outcome.to_owned(),
            metadata: AuditMetadata::new(metadata)?,
        })?;
        Ok(())
    }

    fn forget_tool_call(&self, call_id: &str) {
        if let Ok(mut used) = self.used_tool_call_ids.lock() {
            used.remove(call_id);
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

const fn invalid_command_failure() -> HandlerFailure {
    HandlerFailure::new("invalid_command", "command payload is invalid")
}

const fn capability_denied_failure() -> HandlerFailure {
    HandlerFailure::new(
        "capability_denied",
        "protocol-v3 capability was not granted",
    )
}

const fn conflict_failure() -> HandlerFailure {
    HandlerFailure::new("conflict", "tool call id was already used")
}

const fn tool_not_found_failure() -> HandlerFailure {
    HandlerFailure::new("not_found", "tool id was not found")
}

const fn deadline_failure() -> HandlerFailure {
    HandlerFailure::new("deadline_exceeded", "safe tool deadline was exceeded")
}

const fn response_too_large_failure() -> HandlerFailure {
    HandlerFailure::new("response_too_large", "safe tool result exceeded its bound")
}

const fn not_found_failure() -> HandlerFailure {
    HandlerFailure::new("not_found", "claim id was not found")
}

const fn internal_failure() -> HandlerFailure {
    HandlerFailure::new("runtime_unavailable", "runtime operation is unavailable")
}

const fn claim_status(status: crate::claims::Status) -> ClaimStatus {
    match status {
        crate::claims::Status::Current => ClaimStatus::Current,
        crate::claims::Status::Partial => ClaimStatus::Partial,
        crate::claims::Status::Proposed => ClaimStatus::Proposed,
        crate::claims::Status::Blocked => ClaimStatus::Blocked,
        crate::claims::Status::OutOfScope => ClaimStatus::OutOfScope,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use abi_agent_runtime::{EchoProvider, RunBudget};

    use super::*;
    use crate::app_core::{
        APP_PROTOCOL_V1, V3GrantRequest, V3PageQuery, V3ResourceQuery, V3ToolCall,
    };

    fn store() -> Arc<RuntimeStore> {
        Arc::new(RuntimeStore::open(std::path::Path::new(":memory:")).unwrap())
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
        let authority = V3RuntimeAuthority::from_provider_routes([&route], store()).unwrap();
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
        let authority = V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store)).unwrap();
        let V3Event::Negotiated(negotiated) = authority
            .handle(V3Command::Negotiate(V3GrantRequest {
                supported_versions: vec![3],
                requested: V3CapabilitySet::from_sorted(vec![
                    V3Capability::ListTools,
                    V3Capability::InvokeTools,
                    V3Capability::ReadModels,
                    V3Capability::ReadClaimsById,
                ])
                .unwrap(),
            }))
            .unwrap()
        else {
            panic!("expected v3 negotiation");
        };
        assert_eq!(
            negotiated.granted.as_slice(),
            &[
                V3Capability::ListTools,
                V3Capability::InvokeTools,
                V3Capability::ReadClaimsById,
            ]
        );
        let V3Event::Tools(tools) = authority
            .handle(V3Command::ListTools(V3PageQuery::default()))
            .unwrap()
        else {
            panic!("expected safe tool inventory");
        };
        assert_eq!(tools.after, 0);
        assert_eq!(tools.through, 3);
        assert_eq!(
            tools
                .tools
                .iter()
                .map(|tool| tool.tool_id.as_str())
                .collect::<Vec<_>>(),
            crate::mcp_host::tool_names()
        );
        for (descriptor, safe) in tools.tools.iter().zip(crate::mcp_host::SAFE_TOOLS) {
            assert_eq!(descriptor.description, safe.description);
            assert_eq!(descriptor.input_schema, (safe.schema)());
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
                    through: Some(4),
                    limit: 1,
                }))
                .unwrap_err()
                .code(),
            "invalid_command"
        );
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
                V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store)).unwrap();
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
        }
        {
            let store = Arc::new(RuntimeStore::open(&database).unwrap());
            let authority =
                V3RuntimeAuthority::from_provider_routes([], Arc::clone(&store)).unwrap();
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
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
