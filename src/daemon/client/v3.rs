//! Typed protocol-v3 session reads over the shared bounded daemon transport.
//!
//! All non-Unix (Windows) entry points are explicit stubs returning
//! ClientError::UnsupportedPlatform. Windows named-pipe transport is
//! unavailable until implemented (desktop plan Task 5). The many per-method
//! `#[cfg(not(unix))]` blocks below carry the same "remains fail-closed"
//! comments; they exist so that adding Windows support touches only the
//! transport layer, not the typed surface. Native adapters are separate
//! (unavailable).

use crate::app_core::{
    V3Action, V3Capability, V3Command, V3EntityPage, V3Event, V3ModelAction,
    V3ModelInferenceRequest, V3ModelInferenceResult, V3OperationStatus, V3PageQuery,
    V3ResourceQuery, V3ToolApprovalState, V3ToolApprovalStatus, V3ToolCall, V3ToolDecision,
    V3ToolInvocation, V3ToolPage, V3ToolResult,
};

use super::{ClientError, V3DaemonSession};

// The memory and claims domains of this session live in child modules. They
// resolve `super::unix` and `super::v3_event_name` against this module, so
// those parent-module paths are re-exported here unchanged.
#[cfg(unix)]
use super::unix;
use super::v3_event_name;

#[path = "v3/v3_memory.rs"]
mod v3_memory;

#[path = "v3/v3_claims.rs"]
mod v3_claims;

impl V3DaemonSession {
    /// Return the validated negotiation result for presentation or inspection.
    #[must_use]
    pub const fn negotiation(&self) -> &crate::app_core::V3GrantNegotiation {
        &self.negotiation
    }

    /// Read one bounded fixed-watermark page from the canonical safe registry.
    ///
    /// Descriptors carry no invocation authority. The request echoes the
    /// private negotiated grant set and is sent exactly once without retry or
    /// downgrade.
    #[cfg(unix)]
    pub fn list_tools(&self, query: V3PageQuery) -> Result<V3ToolPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ListTools) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ListTools,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListTools(query),
        )?;
        let V3Event::Tools(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "tool inventory",
                received: super::v3_event_name(&event),
            });
        };
        page.validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        let expected = usize::try_from(
            page.through
                .saturating_sub(page.after)
                .min(u64::from(query.limit)),
        )
        .map_err(|_| ClientError::InvalidV3Response)?;
        if page.after != query.after
            || query.through.is_some_and(|through| page.through != through)
            || page.tools.len() != expected
        {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_tools(&self, _query: V3PageQuery) -> Result<V3ToolPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Request one schema-validated tool exactly once.
    ///
    /// The daemon owns the registry, policy, execution, deadline, result bound,
    /// duplicate-call rejection, and persistent audit. This client only checks
    /// its private negotiated grant and terminal response correlation.
    #[cfg(unix)]
    pub fn request_tool(&self, call: V3ToolCall) -> Result<V3ToolInvocation, ClientError> {
        call.validate().map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::InvokeTools) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::InvokeTools,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::InvokeTool(call.clone()),
        )?;
        match event {
            V3Event::ToolResult(result) => {
                result
                    .validate()
                    .map_err(|_| ClientError::InvalidV3Response)?;
                if result.tool_id != call.tool_id || result.call_id != call.call_id {
                    return Err(ClientError::InvalidV3Response);
                }
                Ok(V3ToolInvocation::Completed(result))
            }
            V3Event::ToolApprovalStatus(status) => {
                status
                    .validate()
                    .map_err(|_| ClientError::InvalidV3Response)?;
                if status.tool_id != call.tool_id
                    || status.call_id != call.call_id
                    || status.state != V3ToolApprovalState::Pending
                    || status.call_digest
                        != call
                            .approval_digest()
                            .map_err(|_| ClientError::InvalidV3Request)?
                {
                    return Err(ClientError::InvalidV3Response);
                }
                Ok(V3ToolInvocation::ApprovalRequired(status))
            }
            event => Err(ClientError::UnexpectedV3Event {
                expected: "tool result or approval status",
                received: super::v3_event_name(&event),
            }),
        }
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn request_tool(&self, _call: V3ToolCall) -> Result<V3ToolInvocation, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Invoke a read-only tool, retaining the pre-approval source contract.
    ///
    /// Mutating calls are sent once but return an explicit typed error that
    /// contains only their durable approval correlation and digest.
    pub fn invoke_tool(&self, call: V3ToolCall) -> Result<V3ToolResult, ClientError> {
        match self.request_tool(call)? {
            V3ToolInvocation::Completed(result) => Ok(result),
            V3ToolInvocation::ApprovalRequired(status) => {
                Err(ClientError::V3ToolApprovalRequired {
                    call_id: status.call_id,
                    call_digest: status.call_digest,
                    expires_at_ms: status.expires_at_ms,
                })
            }
        }
    }

    /// Approve one exact pending tool call without consuming or executing it.
    pub fn approve_tool(
        &self,
        decision: V3ToolDecision,
    ) -> Result<V3ToolApprovalStatus, ClientError> {
        self.decide_tool(decision, true)
    }

    /// Deny one exact pending tool call without executing it.
    pub fn deny_tool(&self, decision: V3ToolDecision) -> Result<V3ToolApprovalStatus, ClientError> {
        self.decide_tool(decision, false)
    }

    #[cfg(unix)]
    fn decide_tool(
        &self,
        decision: V3ToolDecision,
        approve: bool,
    ) -> Result<V3ToolApprovalStatus, ClientError> {
        decision
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self
            .negotiation
            .granted
            .contains(V3Capability::DecideToolApprovals)
        {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::DecideToolApprovals,
            });
        }
        let command = if approve {
            V3Command::ApproveTool(decision.clone())
        } else {
            V3Command::DenyTool(decision.clone())
        };
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            command,
        )?;
        let V3Event::ToolApprovalStatus(status) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "tool approval status",
                received: super::v3_event_name(&event),
            });
        };
        status
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        let expected = if approve {
            V3ToolApprovalState::Approved
        } else {
            V3ToolApprovalState::Denied
        };
        if status.call_id != decision.call_id
            || status.call_digest != decision.call_digest
            || !matches!(status.state, state if state == expected || state == V3ToolApprovalState::Expired)
        {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(status)
    }

    #[cfg(not(unix))]
    fn decide_tool(
        &self,
        _decision: V3ToolDecision,
        _approve: bool,
    ) -> Result<V3ToolApprovalStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Cancel one exact pending or approved tool record without executing it.
    #[cfg(unix)]
    pub fn cancel_tool(&self, action: V3Action) -> Result<V3ToolApprovalStatus, ClientError> {
        action
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::CancelTools) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::CancelTools,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::CancelTool(action.clone()),
        )?;
        let V3Event::ToolApprovalStatus(status) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "tool approval status",
                received: super::v3_event_name(&event),
            });
        };
        status
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if status.call_id != action.resource_id
            || !matches!(
                status.state,
                V3ToolApprovalState::Cancelled | V3ToolApprovalState::Expired
            )
        {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(status)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn cancel_tool(&self, _action: V3Action) -> Result<V3ToolApprovalStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Read one bounded fixed-watermark model inventory page.
    ///
    /// The request echoes the exact negotiated grant set and is sent once. It
    /// is never retried or replayed, even though this first command is a read,
    /// so future v3 mutations cannot accidentally inherit retry behavior.
    #[cfg(unix)]
    pub fn list_models(&self, query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadModels) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadModels,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListModels(query),
        )?;
        let V3Event::Models(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model inventory",
                received: super::v3_event_name(&event),
            });
        };
        page.validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if page.after != query.after
            || query.through.is_some_and(|through| page.through != through)
            || page.records.len() > usize::from(query.limit)
        {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_models(&self, _query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Bounded fixed-watermark list of clusters (stub; always empty until worker integration).
    #[cfg(unix)]
    pub fn list_clusters(&self, query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadWorkers) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadWorkers,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListClusters(query),
        )?;
        let V3Event::Clusters(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "cluster inventory",
                received: super::v3_event_name(&event),
            });
        };
        page.validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_clusters(&self, _query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Bounded fixed-watermark list of workers (stub; always empty until worker integration).
    #[cfg(unix)]
    pub fn list_workers(&self, query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadWorkers) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadWorkers,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListWorkers(query),
        )?;
        let V3Event::Workers(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "worker inventory",
                received: super::v3_event_name(&event),
            });
        };
        page.validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_workers(&self, _query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Health record for one worker (stub: always not-found until integration).
    #[cfg(unix)]
    pub fn worker_health(
        &self,
        query: V3ResourceQuery,
    ) -> Result<crate::app_core::V3EntityRecord, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadWorkers) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadWorkers,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::WorkerHealth(query.clone()),
        )?;
        let V3Event::WorkerHealth(record) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "worker health",
                received: super::v3_event_name(&event),
            });
        };
        record
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        // For stub, resource_id need not match anything specific.
        Ok(record)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn worker_health(
        &self,
        _query: V3ResourceQuery,
    ) -> Result<crate::app_core::V3EntityRecord, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Status for one job (stub: always not-found until integration).
    #[cfg(unix)]
    pub fn job_status(&self, query: V3ResourceQuery) -> Result<V3OperationStatus, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadWorkers) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadWorkers,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::JobStatus(query.clone()),
        )?;
        let V3Event::JobStatus(status) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "job status",
                received: super::v3_event_name(&event),
            });
        };
        status
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        Ok(status)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn job_status(&self, _query: V3ResourceQuery) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Discover one exact immutable signed startup model without retry or replay.
    #[cfg(unix)]
    pub fn model_details(
        &self,
        query: V3ResourceQuery,
    ) -> Result<crate::app_core::V3ModelDetails, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadModels) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadModels,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ModelDetails(query.clone()),
        )?;
        let V3Event::ModelDetails(details) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model details",
                received: super::v3_event_name(&event),
            });
        };
        details
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if details.model_id != query.resource_id {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(details)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn model_details(
        &self,
        _query: V3ResourceQuery,
    ) -> Result<crate::app_core::V3ModelDetails, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Start one exact immutable-revision model download, sending it once.
    #[cfg(unix)]
    pub fn download_model(&self, action: V3ModelAction) -> Result<V3OperationStatus, ClientError> {
        self.model_action(
            V3Capability::DownloadModels,
            V3Command::DownloadModel(action.clone()),
            &action,
        )
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn download_model(&self, _action: V3ModelAction) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Read one exact durable download operation without retry or replay.
    #[cfg(unix)]
    pub fn model_download_status(
        &self,
        query: V3ResourceQuery,
    ) -> Result<V3OperationStatus, ClientError> {
        self.model_status_query(
            V3Capability::ReadModels,
            V3Command::ModelDownloadStatus(query.clone()),
            &query,
        )
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn model_download_status(
        &self,
        _query: V3ResourceQuery,
    ) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Start one exact model load under startup-owned device policy.
    #[cfg(unix)]
    pub fn load_model(&self, action: V3ModelAction) -> Result<V3OperationStatus, ClientError> {
        self.model_action(
            V3Capability::ManageModels,
            V3Command::LoadModel(action.clone()),
            &action,
        )
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn load_model(&self, _action: V3ModelAction) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Unload one exact model revision without selecting another model.
    #[cfg(unix)]
    pub fn unload_model(&self, action: V3ModelAction) -> Result<V3OperationStatus, ClientError> {
        self.model_action(
            V3Capability::ManageModels,
            V3Command::UnloadModel(action.clone()),
            &action,
        )
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn unload_model(&self, _action: V3ModelAction) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Cancel a queued or running model lifecycle operation (download/load/unload).
    /// Operation must not be terminal; returns updated status on success.
    #[cfg(unix)]
    pub fn cancel_model(&self, action: V3Action) -> Result<V3OperationStatus, ClientError> {
        action
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self
            .negotiation
            .granted
            .contains(V3Capability::ManageModels)
        {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ManageModels,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::CancelModel(action.clone()),
        )?;
        let V3Event::ModelStatus(status) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model status",
                received: super::v3_event_name(&event),
            });
        };
        status
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if status.operation_id != action.operation_id {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(status)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn cancel_model(&self, _action: V3Action) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Read a load/unload operation or exact loaded-model inference evidence.
    #[cfg(unix)]
    pub fn inference_status(
        &self,
        query: V3ResourceQuery,
    ) -> Result<V3OperationStatus, ClientError> {
        self.model_status_query(
            V3Capability::ReadModels,
            V3Command::InferenceStatus(query.clone()),
            &query,
        )
    }

    /// Run one already-loaded exact model revision synchronously and send once.
    #[cfg(unix)]
    pub fn infer_model(
        &self,
        request: V3ModelInferenceRequest,
    ) -> Result<V3ModelInferenceResult, ClientError> {
        request
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        self.require(V3Capability::InferModels)?;
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::InferModel(request.clone()),
        )?;
        let V3Event::ModelInference(result) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model inference",
                received: super::v3_event_name(&event),
            });
        };
        result
            .validate_for(&request)
            .map_err(|_| ClientError::InvalidV3Response)?;
        Ok(result)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn infer_model(
        &self,
        _request: V3ModelInferenceRequest,
    ) -> Result<V3ModelInferenceResult, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn inference_status(
        &self,
        _query: V3ResourceQuery,
    ) -> Result<V3OperationStatus, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    #[cfg(unix)]
    fn model_action(
        &self,
        capability: V3Capability,
        command: V3Command,
        action: &V3ModelAction,
    ) -> Result<V3OperationStatus, ClientError> {
        action
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        self.require(capability)?;
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            command,
        )?;
        let V3Event::ModelStatus(status) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model status",
                received: super::v3_event_name(&event),
            });
        };
        status
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if status.operation_id != action.operation_id || status.resource_id != action.model_id {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(status)
    }

    #[cfg(unix)]
    fn model_status_query(
        &self,
        capability: V3Capability,
        command: V3Command,
        query: &V3ResourceQuery,
    ) -> Result<V3OperationStatus, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        self.require(capability)?;
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            command,
        )?;
        let V3Event::ModelStatus(status) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model status",
                received: super::v3_event_name(&event),
            });
        };
        status
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if status.operation_id != query.resource_id && status.resource_id != query.resource_id {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(status)
    }

    fn require(&self, capability: V3Capability) -> Result<(), ClientError> {
        if !self.negotiation.granted.contains(capability) {
            return Err(ClientError::V3CapabilityNotGranted { capability });
        }
        Ok(())
    }
}
