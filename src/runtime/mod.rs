//! Durable state for Abbey's application runtime.
//!
//! This module owns lifecycle persistence only. Model execution, tool dispatch,
//! presentation, and daemon transport stay outside the database layer.

mod executor;
mod manager;
mod migrations;
mod store;

pub use executor::{CancellationToken, ExecutionError, Executor};
pub use manager::{
    Clock, ManagerError, RunManager, RunManagerConfig, SubmitDisposition, SubmitResult, SystemClock,
};

pub use store::{
    AuditEvent, AuditMetadata, ConversationBackend, NewAuditEvent, NewRun, NewRunEvent, RunEvent,
    RunRecord, RuntimeStore, StoreError,
};
