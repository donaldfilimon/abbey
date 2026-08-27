//! The complete set of Tauri commands the Abbey desktop client exposes.
//!
//! Ten, all read-only, all enumerated in `main.rs`'s `generate_handler!`. The
//! webview can invoke nothing else: there is no `exec`, no submit, no cancel, no
//! invoke, no obsolete, no command-name parameter that resolves to a subprocess,
//! and no plugin in `Cargo.toml` that would provide one.
//!
//! Protocol v1/v2 reads use `AppCommand`. Protocol-v3 memory search/metadata
//! use a separate session that requests `ReadMemory` only.

use abbey::app_core::{
    ClaimsQuery, ClaimsSnapshot, RouteAuditPage, RouteAuditQuery, RunEventPage, RunEventsQuery,
    RunQuery, RunSnapshot, RuntimeStatus, V3CapabilitySet, V3EntityPage, V3EntityRecord,
    V3ResourceQuery, V3SearchRequest,
};
use abbey::edition::ACTIVE;

use crate::backend;
use crate::ipc::{BundleIdentity, ConnectionInfo, IpcError};

/// `ReadStatus` — process identity, build stamp, and granted capabilities.
#[tauri::command]
pub fn app_status() -> Result<RuntimeStatus, IpcError> {
    backend::status()
}

/// `ReadClaims` — the canonical capability ledger, filtered by a bounded query
/// that the application core validates.
#[tauri::command]
pub fn app_claims(query: ClaimsQuery) -> Result<ClaimsSnapshot, IpcError> {
    backend::claims(query)
}

/// `ReadRoutes` — a bounded, sanitized tail of the persona/role routing audit
/// log. The page carries an opaque `ws-<digest>` workspace label, never a
/// filesystem path: this process has no filesystem plugin with which it could
/// read the log directly, so the sanitized contract is the only route to it.
#[tauri::command]
pub fn app_routes(query: RouteAuditQuery) -> Result<RouteAuditPage, IpcError> {
    backend::routes(query)
}

/// `ReadRun` — one durable run snapshot. User input and provider output are
/// not on the type; submit and cancel are different commands and are not
/// registered here.
#[tauri::command]
pub fn app_run_status(query: RunQuery) -> Result<RunSnapshot, IpcError> {
    backend::run_status(query)
}

/// `ReadRunEvents` — a bounded, snapshot-consistent page of sanitized
/// lifecycle events for one run. This is not a live subscription.
#[tauri::command]
pub fn app_run_events(query: RunEventsQuery) -> Result<RunEventPage, IpcError> {
    backend::run_events(query)
}

/// Negotiated protocol-v3 grants. The desktop requests `ReadMemory` only.
/// No bearer → empty set. Never falls back to opening the memory store.
#[tauri::command]
pub fn app_v3_grants() -> Result<V3CapabilitySet, IpcError> {
    backend::v3_grants()
}

/// `ReadMemory` search over sanitized summaries in `memory-v1-summary`.
#[tauri::command]
pub fn app_memory_search(request: V3SearchRequest) -> Result<V3EntityPage, IpcError> {
    backend::memory_search(request)
}

/// `ReadMemory` metadata for one opaque domain-separated record id.
#[tauri::command]
pub fn app_memory_metadata(query: V3ResourceQuery) -> Result<V3EntityRecord, IpcError> {
    backend::memory_metadata(query)
}

/// How this client reaches Abbey. No secret material; see `ipc::ConnectionInfo`.
#[tauri::command]
pub fn app_connection() -> ConnectionInfo {
    backend::connection()
}

/// Packaged identity of the running build, derived from `abbey::edition`.
#[tauri::command]
pub fn app_bundle_identity(app: tauri::AppHandle) -> BundleIdentity {
    bundle_identity(configured_identifier(&app))
}

pub fn configured_identifier(app: &tauri::AppHandle) -> String {
    tauri::Manager::config(app).identifier.clone()
}

pub fn bundle_identity(configured_bundle_id: String) -> BundleIdentity {
    let identity = ACTIVE.identity();
    BundleIdentity {
        edition: match ACTIVE {
            abbey::edition::Edition::Safe => abbey::app_core::Edition::Standard,
            abbey::edition::Edition::Personal => abbey::app_core::Edition::Personal,
        },
        product_name: identity.product_name.to_owned(),
        binary_name: identity.binary_name.to_owned(),
        daemon_binary_name: identity.daemon_binary_name.to_owned(),
        bundle_id: identity.bundle_id.to_owned(),
        configured_bundle_id,
    }
}
