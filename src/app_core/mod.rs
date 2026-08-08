//! Presentation-neutral, read-only Abbey application contracts.
//!
//! The CLI, TUI, daemon, and future desktop client can share this boundary
//! without inheriting one another's presentation or transport concerns.

mod context;
mod contracts;
mod policy;
mod service;

pub use context::AppContext;
pub use contracts::{
    APP_PROTOCOL_VERSION, APP_SCHEMA_VERSION, AppCapability, AppCommand, AppEvent, ApprovalKind,
    ApprovalRequest, CapabilitySet, ClaimRecord, ClaimStatus, ClaimsQuery, ClaimsSnapshot,
    ConversationId, Edition, IdError, RunId, RuntimeState, RuntimeStatus, ValidationError,
};
pub use policy::StandardPolicy;
pub use service::{AppService, AppServiceError};
