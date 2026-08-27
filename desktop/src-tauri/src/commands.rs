//! The complete set of Tauri commands the Abbey desktop client exposes.
//!
//! Seven, all read-only, all enumerated in `main.rs`'s `generate_handler!`. The
//! webview can invoke nothing else: there is no `exec`, no submit, no cancel, no
//! command-name parameter that resolves to a subprocess, and no plugin in
//! `Cargo.toml` that would provide one.
//!
//! Five of them (`app_status`, `app_claims`, `app_routes`, `app_run_status`,
//! `app_run_events`) are application-core reads. `ReadStatus`/`ReadClaims`/
//! `ReadRoutes` are what `CapabilitySet::standard()` grants; `ReadRun` and
//! `ReadRunEvents` appear only on a protocol-v2 daemon. The other two describe
//! the client itself and touch no Abbey state.

use abbey::app_core::{
    ClaimsQuery, ClaimsSnapshot, RouteAuditPage, RouteAuditQuery, RunEventPage, RunEventsQuery,
    RunQuery, RunSnapshot, RuntimeStatus,
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
