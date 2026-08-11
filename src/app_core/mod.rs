//! Presentation-neutral, read-only Abbey application contracts.
//!
//! The CLI, TUI, daemon, and future desktop client can share this boundary
//! without inheriting one another's presentation or transport concerns.

mod context;
mod contracts;
mod ids;
mod policy;
mod routes;
mod run;
mod service;
mod v3;
#[cfg(test)]
mod v3_tests;
mod v3_tool;

pub use context::AppContext;
pub use contracts::{
    APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, APP_SCHEMA_V1, APP_SCHEMA_VERSION, AppCapability,
    AppCommand, AppEvent, ApprovalKind, ApprovalRequest, CapabilitySet, ClaimRecord, ClaimStatus,
    ClaimsQuery, ClaimsSnapshot, Edition, RuntimeState, RuntimeStatus, ValidationError,
};
pub use ids::{ConversationId, IdError, RunId};
pub use policy::StandardPolicy;
pub use routes::{MAX_ROUTE_AUDIT_PAGE, RouteAuditEntry, RouteAuditPage, RouteAuditQuery};
pub use run::{
    BackendSelection, ConversationMetadata, IdempotencyKey, MAX_RUN_EVENT_PAGE,
    RunCancellationReason, RunEventPage, RunEventRecord, RunEventsQuery, RunFailure,
    RunInterruptionReason, RunLifecycleEvent, RunMode, RunQuery, RunRequest, RunRouteCapability,
    RunSnapshot, RunState, RunSubmission, RunSubmissionDisposition,
};
pub use service::{AppService, AppServiceError};
pub use v3::{
    APP_PROTOCOL_V3, APP_SCHEMA_V3, MAX_V3_PAGE, V3Action, V3Capability, V3CapabilitySet,
    V3Command, V3EntityPage, V3EntityRecord, V3Error, V3ErrorCode, V3Event, V3EventPage,
    V3EventRecord, V3GrantNegotiation, V3GrantRequest, V3Metric, V3MetricPage, V3MetricQuery,
    V3ModelAction, V3OperationState, V3OperationStatus, V3PageQuery, V3ResourceQuery,
    V3SearchRequest, V3StableClaim, V3TrainingStart,
};
pub use v3_tool::{V3ToolCall, V3ToolDecision, V3ToolDescriptor, V3ToolPage, V3ToolResult};
