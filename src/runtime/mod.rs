//! Durable state for Abbey's application runtime.
//!
//! This module owns lifecycle persistence only. Model execution, tool dispatch,
//! presentation, and daemon transport stay outside the database layer.

mod delegated;
mod executor;
mod identity;
mod legacy;
mod manager;
mod migrations;
mod provider;
mod store;
pub(crate) mod supervisor;

pub use delegated::{
    DelegatedConfigError, DelegatedExecutor, DelegatedExecutorConfig, DelegatedLimits,
};
pub use executor::{CancellationToken, ExecutionError, ExecutionErrorKind, Executor};
pub use manager::{
    Clock, ManagerError, RunManager, RunManagerConfig, SubmitDisposition, SubmitResult, SystemClock,
};
pub use provider::{ModelProviderExecutor, ProviderConfigError, ProviderRoute};

pub(crate) use identity::{
    ConversationIdentityScope, IdentityCommit, IdentityScopeSelection, IdentityScopeState,
};

pub(crate) use legacy::prepare as prepare_legacy_import;

pub use store::{
    AuditEvent, AuditMetadata, ConversationBackend, NewAuditEvent, NewRun, NewRunEvent, RunEvent,
    RunRecord, RuntimeStore, StoreError,
};
