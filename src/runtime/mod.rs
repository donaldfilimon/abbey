//! Durable state for Abbey's application runtime.
//!
//! This module owns lifecycle persistence only. Model execution, tool dispatch,
//! presentation, and daemon transport stay outside the database layer.

mod migrations;
mod store;

pub use store::{
    AuditEvent, AuditMetadata, ConversationBackend, NewAuditEvent, NewRun, NewRunEvent, RunEvent,
    RunRecord, RunStatus, RuntimeStore, StoreError,
};
