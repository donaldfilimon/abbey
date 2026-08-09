//! The complete set of Tauri commands the Abbey desktop client exposes.
//!
//! Five, all read-only, all enumerated in `main.rs`'s `generate_handler!`. The
//! webview can invoke nothing else: there is no `exec`, no `run`, no
//! command-name parameter that resolves to a subprocess, and no plugin in
//! `Cargo.toml` that would provide one.
//!
//! Three of them (`app_status`, `app_claims`, `app_routes`) are exactly the
//! three capabilities `abbey::app_core::CapabilitySet::standard()` grants —
//! `ReadStatus`, `ReadClaims`, and `ReadRoutes`. The other two describe the
//! client itself and touch no Abbey state.

use abbey::app_core::{
    ClaimsQuery, ClaimsSnapshot, RouteAuditPage, RouteAuditQuery, RuntimeStatus,
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
