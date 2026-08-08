//! Presentation-neutral, read-only Abbey application contracts.
//!
//! The CLI, TUI, daemon, and future desktop client can share this boundary
//! without inheriting one another's presentation or transport concerns.

mod context;
mod contracts;
mod ids;
mod policy;
mod run;
mod service;

pub use context::AppContext;
pub use contracts::{
    APP_PROTOCOL_VERSION, APP_SCHEMA_VERSION, AppCapability, AppCommand, AppEvent, ApprovalKind,
    ApprovalRequest, CapabilitySet, ClaimRecord, ClaimStatus, ClaimsQuery, ClaimsSnapshot, Edition,
    RuntimeState, RuntimeStatus, ValidationError,
};
pub use ids::{ConversationId, IdError, RunId};
pub use policy::StandardPolicy;
pub use run::{
    BackendSelection, ConversationMetadata, IdempotencyKey, RunEventRecord, RunFailure,
    RunLifecycleEvent, RunMode, RunRequest, RunSnapshot, RunState,
};
pub use service::{AppService, AppServiceError};
