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
