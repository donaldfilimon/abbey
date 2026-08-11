//! Minimal daemon-owned authority for the first served protocol-v3 slice.
//!
//! Only bounded safe tool inventory/invocation, one default-edition pending
//! mutation plus exact approve/deny decisions, startup-bound ABI-local model
//! inventory, and exact stable-ID claim reads are representable here.
//! Execution, memory, training, worker, cancellation, and polling grants remain
//! absent.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use abi_agent_host::{ToolExecutionContext, ToolExecutor};
use abi_agent_runtime::{
    CancellationToken, EffectScopedPolicy, ExecutionPolicy, PolicyDecision, ToolCall, ToolStatus,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::app_core::{
    APP_PROTOCOL_V3, APP_SCHEMA_V3, BackendSelection, ClaimStatus, V3Capability, V3CapabilitySet,
    V3Command, V3EntityPage, V3EntityRecord, V3Event, V3GrantNegotiation, V3OperationState,
    V3StableClaim, V3ToolApprovalState, V3ToolApprovalStatus, V3ToolDecision, V3ToolPage,
    V3ToolResult,
};
use crate::runtime::{
    AuditMetadata, MAX_TOOL_APPROVAL_TTL_MS, NewAuditEvent, NewToolApproval, ProviderRoute,
    RuntimeStore, StoreError, ToolApprovalDecision, ToolApprovalRecord, ToolApprovalState,
};

use super::server::HandlerFailure;

mod tool_catalog;

use tool_catalog::{BoundTool, ToolRoute};

/// Frozen protocol-v3 grants plus presentation-safe tool, model, and claim authority.
pub(super) struct V3RuntimeAuthority {
    grants: V3CapabilitySet,
    tools: Vec<BoundTool>,
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
        let tools = tool_catalog::build().map_err(|()| internal_failure())?;
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
        let mut available = Vec::with_capacity(5);
        if !tools.is_empty() {
            available.push(V3Capability::ListTools);
            available.push(V3Capability::InvokeTools);
        }
        if tools
            .iter()
            .any(|tool| tool.route == ToolRoute::ApprovalRequired)
        {
            available.push(V3Capability::DecideToolApprovals);
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
            V3Command::ApproveTool(decision) => {
                self.decide_tool_approval(decision, ToolApprovalDecision::Approve)
            }
            V3Command::DenyTool(decision) => {
                self.decide_tool_approval(decision, ToolApprovalDecision::Deny)
            }
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
        call.validate().map_err(|_| invalid_command_failure())?;
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
        if tool.route == ToolRoute::ApprovalRequired {
            if !matches!(decision, PolicyDecision::RequireConfirmation { .. }) {
                return Err(capability_denied_failure());
            }
            let now = now_ms()?;
            let expires_at_ms = now
                .checked_add(MAX_TOOL_APPROVAL_TTL_MS)
                .ok_or_else(internal_failure)?;
            let call_digest = call
                .approval_digest()
                .map_err(|_| invalid_command_failure())?;
            let request = NewToolApproval {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                call_digest: call_digest.clone(),
                created_at_ms: now,
                expires_at_ms,
            };
            if let Err(error) = self.store.create_tool_approval(request) {
                if !matches!(error, StoreError::ToolApprovalConflict) {
                    self.forget_tool_call(&call.call_id);
                    return Err(internal_failure());
                }
                return Err(conflict_failure());
            }
            return Ok(V3Event::ToolApprovalStatus(V3ToolApprovalStatus {
                tool_id: call.tool_id,
                call_id: call.call_id,
                call_digest,
                state: V3ToolApprovalState::Pending,
                expires_at_ms,
            }));
        }
        if !matches!(decision, PolicyDecision::Allow) {
            return Err(capability_denied_failure());
        }
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

    fn decide_tool_approval(
        &self,
        decision: V3ToolDecision,
        outcome: ToolApprovalDecision,
    ) -> Result<V3Event, HandlerFailure> {
        decision.validate().map_err(|_| invalid_command_failure())?;
        let record = self
            .store
            .decide_tool_approval(
                &decision.call_id,
                &decision.call_digest,
                &decision.decision_id,
                outcome,
                now_ms()?,
            )
            .map_err(map_approval_store_error)?;
        Ok(V3Event::ToolApprovalStatus(approval_status(record)))
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

fn now_ms() -> Result<u64, HandlerFailure> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| internal_failure())?;
    u64::try_from(elapsed.as_millis()).map_err(|_| internal_failure())
}

fn approval_status(record: ToolApprovalRecord) -> V3ToolApprovalStatus {
    V3ToolApprovalStatus {
        tool_id: record.tool_id,
        call_id: record.call_id,
        call_digest: record.call_digest,
        state: match record.state {
            ToolApprovalState::Pending => V3ToolApprovalState::Pending,
            ToolApprovalState::Approved => V3ToolApprovalState::Approved,
            ToolApprovalState::Denied => V3ToolApprovalState::Denied,
            ToolApprovalState::Cancelled => V3ToolApprovalState::Cancelled,
            ToolApprovalState::Expired => V3ToolApprovalState::Expired,
            ToolApprovalState::Consumed => V3ToolApprovalState::Consumed,
        },
        expires_at_ms: record.expires_at_ms,
    }
}

fn map_approval_store_error(error: StoreError) -> HandlerFailure {
    match error {
        StoreError::ToolApprovalNotFound(_) => approval_not_found_failure(),
        StoreError::ToolApprovalConflict | StoreError::ToolApprovalDigestMismatch => {
            approval_conflict_failure()
        }
        StoreError::InvalidInput(_) => invalid_command_failure(),
        _ => internal_failure(),
    }
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

const fn approval_conflict_failure() -> HandlerFailure {
    HandlerFailure::new(
        "conflict",
        "tool approval decision conflicts with durable state",
    )
}

const fn approval_not_found_failure() -> HandlerFailure {
    HandlerFailure::new("not_found", "tool approval was not found")
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
mod tests;
