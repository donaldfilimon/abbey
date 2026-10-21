//! Enumerated reads, signed-model lifecycle and daemon-only conversation metadata/history.
//! No generic effect, inference, shell, credentials, or paths cross IPC.

use abbey::app_core::{
    ClaimsQuery, ClaimsSnapshot, ConversationId, IdempotencyKey, RouteAuditPage, RouteAuditQuery,
    RunEventPage, RunEventsQuery, RunId, RunQuery, RunSnapshot, RunState, RuntimeStatus,
    V3CapabilitySet, V3EntityPage, V3EntityRecord, V3ModelAction, V3ModelDetails,
    V3OperationStatus, V3PageQuery, V3ResourceQuery, V3SearchRequest, V3StableClaim, V3ToolPage,
    V4CapabilitySet, V4ContentPage, V4ContentQuery, V4Conversation, V4ConversationCreate,
    V4ConversationPage, V4ConversationPatch, V4ConversationQuery, V4ConversationRename,
    V4MessagePage, V4MessageQuery, V4ResourceQuery, V4Route, V4SubmitTurn, V4TurnSubmission,
};
use abbey::edition::ACTIVE;

use crate::backend;
use crate::ipc::{
    BundleIdentity, ConnectionInfo, IntegrationFact, IntegrationState, IpcError, SystemIntegration,
};

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

/// Negotiated protocol-v3 grants. The desktop requests exactly `ReadMemory`,
/// `ReadModels`, and `ReadClaimsById`. `ReadModels` is edition-scoped, so a
/// daemon without model authority omits it and the Models surface stays
/// unavailable; `ReadClaimsById` is granted unconditionally at daemon startup.
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

/// `ReadModels` — bounded inventory via the unchanged read-only session.
#[tauri::command]
pub async fn app_models_list(query: V3PageQuery) -> Result<V3EntityPage, IpcError> {
    model_task(move || backend::models_list(query)).await
}

/// `ReadClaimsById` — one canonical claim by exact stable ID. Complements the
/// protocol-v1 `app_claims` ledger read rather than replacing it.
#[tauri::command]
pub fn app_claim_by_id(query: V3ResourceQuery) -> Result<V3StableClaim, IpcError> {
    backend::claim_by_id(query)
}

/// `ListTools` — the tool inventory the daemon actually advertises.
///
/// Read-only *enumeration*. The session behind this command negotiates
/// `ListTools` and nothing else, so this surface can show a tool and can
/// never call one: there is no invoke, approve, deny, or cancel command in
/// this desktop client, and the mutating tool the safe edition advertises is
/// listed with its effect class only.
///
/// Registered in the `generate_handler!` block in `src/main.rs`; the
/// frontend's command list is checked against that block in both directions
/// by `scripts/verify-bundle.mjs`, so the two cannot drift apart.
#[tauri::command]
pub async fn app_tools_list(query: V3PageQuery) -> Result<V3ToolPage, IpcError> {
    tools_task(move || backend::tools_list(query)).await
}

/// This build's own host-OS integration, as read-only facts.
///
/// Derived from the compiled-in capability manifest and the edition table —
/// never from the host. The desktop process has no filesystem, shell,
/// process, or opener plugin with which it could inspect the OS, and it does
/// not need one to state what it does not do.
///
/// Registered in the `generate_handler!` block in `src/main.rs`.
#[tauri::command]
pub fn app_system_integration() -> SystemIntegration {
    system_integration()
}

/// The host-integration facts this build reports.
///
/// Each fact is a statement about the shipped build, and each is written so
/// that it stays true as long as the build does: the desktop registers
/// nothing at login, installs and starts no service, spawns no helper
/// process, and cancels nothing when its window closes.
fn system_integration() -> SystemIntegration {
    // The compiled-in capability manifest this build ships. Read at compile
    // time, not at runtime: the desktop has no filesystem plugin, and a
    // manifest the app cannot read is a manifest it cannot misreport either.
    const CAPABILITY_MANIFEST: &str = include_str!("../capabilities/default.json");
    // Permission prefixes that could reach the filesystem, a shell, a
    // process, or the OS opener. The shipped build declares none of them.
    const EXECUTION_CAPABLE_PREFIXES: &[&str] = &["fs:", "shell:", "process:", "opener:"];

    let manifest: serde_json::Value =
        serde_json::from_str(CAPABILITY_MANIFEST).expect("the compiled-in manifest is valid JSON");
    let declared_permissions = manifest
        .get("permissions")
        .and_then(serde_json::Value::as_array)
        .map(|permissions| {
            permissions
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let execution_capable_permissions = declared_permissions
        .iter()
        .filter(|permission| {
            EXECUTION_CAPABLE_PREFIXES
                .iter()
                .any(|prefix| permission.starts_with(prefix))
        })
        .cloned()
        .collect();
    SystemIntegration {
        declared_permissions,
        execution_capable_permissions,
        login_startup: IntegrationFact {
            state: IntegrationState::NotRegistered,
            detail: "This app registers no login item, launch agent, or startup service, and \
                      it must stay unregistered: nothing it declares can install one."
                .to_owned(),
        },
        background_service: IntegrationFact {
            state: IntegrationState::SeparateProcess,
            detail: format!(
                "This app installs and starts no background service and spawns no helper \
                 process. The Abbey daemon ({}) is a separate owner-only process that you \
                 start yourself; this window is only a client of it.",
                ACTIVE.daemon_binary_name()
            ),
        },
        window_close: IntegrationFact {
            state: IntegrationState::DaemonOwned,
            detail: "Closing this window does not cancel daemon-owned work. Runs and their \
                     lifecycle events live in the daemon's own durable store and continue \
                     there."
                .to_owned(),
        },
    }
}

/// Unix IPC blocks; keep the tool inventory read off the UI thread.
async fn tools_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| {
            IpcError::new(
                crate::ipc::IpcErrorKind::Transport,
                "tool inventory request worker was interrupted; retry the read",
            )
        })?
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

#[tauri::command]
pub async fn app_model_grants() -> Result<V3CapabilitySet, IpcError> {
    model_task(backend::model_grants).await
}

#[tauri::command]
pub async fn app_model_details(query: V3ResourceQuery) -> Result<V3ModelDetails, IpcError> {
    model_task(move || backend::model_details(query)).await
}

#[tauri::command]
pub async fn app_model_download(action: V3ModelAction) -> Result<V3OperationStatus, IpcError> {
    model_task(move || backend::model_download(action)).await
}

#[tauri::command]
pub async fn app_model_load(action: V3ModelAction) -> Result<V3OperationStatus, IpcError> {
    model_task(move || backend::model_load(action)).await
}

#[tauri::command]
pub async fn app_model_unload(action: V3ModelAction) -> Result<V3OperationStatus, IpcError> {
    model_task(move || backend::model_unload(action)).await
}

#[tauri::command]
pub async fn app_model_download_status(
    query: V3ResourceQuery,
) -> Result<V3OperationStatus, IpcError> {
    model_task(move || backend::model_download_status(query)).await
}

#[tauri::command]
pub async fn app_model_lifecycle_status(
    query: V3ResourceQuery,
) -> Result<V3OperationStatus, IpcError> {
    model_task(move || backend::model_lifecycle_status(query)).await
}

// Unix IPC blocks; keep both inventory and lifecycle exchanges off the UI thread.
async fn model_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| {
            IpcError::new(
                crate::ipc::IpcErrorKind::Transport,
                "model request worker was interrupted; check existing operation status",
            )
        })?
}

// Separate explicit v4 conversation authority; no execution or arbitrary IPC.
#[tauri::command]
pub async fn app_conversation_grants() -> Result<V4CapabilitySet, IpcError> {
    conversation_task(backend::conversation_grants).await
}
#[tauri::command]
pub async fn app_conversations_list(
    query: V4ConversationQuery,
) -> Result<V4ConversationPage, IpcError> {
    conversation_task(move || backend::conversations_list(query)).await
}
#[tauri::command]
pub async fn app_conversations_search(
    query: V4ConversationQuery,
) -> Result<V4ConversationPage, IpcError> {
    conversation_task(move || backend::conversations_search(query)).await
}
#[tauri::command]
pub async fn app_conversation_read(query: V4ResourceQuery) -> Result<V4Conversation, IpcError> {
    conversation_task(move || backend::conversation_read(query)).await
}
#[tauri::command]
pub async fn app_conversation_create(
    query: V4ConversationCreate,
) -> Result<V4Conversation, IpcError> {
    conversation_task(move || backend::conversation_create(query)).await
}
#[tauri::command]
pub async fn app_conversation_rename(
    query: V4ConversationRename,
) -> Result<V4Conversation, IpcError> {
    conversation_task(move || backend::conversation_rename(query)).await
}
#[tauri::command]
pub async fn app_conversation_archive(query: V4ResourceQuery) -> Result<V4Conversation, IpcError> {
    conversation_task(move || backend::conversation_archive(query)).await
}
#[tauri::command]
pub async fn app_conversation_delete(query: V4ResourceQuery) -> Result<(), IpcError> {
    conversation_task(move || backend::conversation_delete(query)).await
}
#[tauri::command]
pub async fn app_conversation_messages(query: V4MessageQuery) -> Result<V4MessagePage, IpcError> {
    conversation_task(move || backend::conversation_messages(query)).await
}
#[tauri::command]
pub async fn app_conversation_content(query: V4ContentQuery) -> Result<V4ContentPage, IpcError> {
    conversation_task(move || backend::conversation_content(query)).await
}
#[tauri::command]
pub async fn app_conversation_routes() -> Result<Vec<V4Route>, IpcError> {
    conversation_task(backend::conversation_routes).await
}
#[tauri::command]
pub async fn app_conversation_submit_turn(
    request: V4SubmitTurn,
) -> Result<V4TurnSubmission, IpcError> {
    conversation_task(move || backend::conversation_submit_turn(request)).await
}
#[tauri::command]
pub async fn app_conversation_cancel_turn(
    conversation_id: ConversationId,
    run_id: RunId,
) -> Result<RunState, IpcError> {
    conversation_task(move || backend::conversation_cancel_turn(conversation_id, run_id)).await
}
#[tauri::command]
pub async fn app_conversation_lookup_turn(
    conversation_id: ConversationId,
    idempotency_key: IdempotencyKey,
) -> Result<Option<V4TurnSubmission>, IpcError> {
    conversation_task(move || backend::conversation_lookup_turn(conversation_id, idempotency_key))
        .await
}
#[tauri::command]
pub async fn app_conversation_update(
    conversation_id: ConversationId,
    patch: V4ConversationPatch,
) -> Result<V4Conversation, IpcError> {
    conversation_task(move || backend::conversation_update(conversation_id, patch)).await
}
async fn conversation_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| {
            IpcError::new(
                crate::ipc::IpcErrorKind::Transport,
                "conversation outcome unknown: request worker interrupted; reconcile through reads",
            )
        })?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The permissions this build reports are the ones the shipped capability
    /// manifest declares — read from that manifest, not restated by hand.
    #[test]
    fn the_reported_permissions_are_the_manifests_own() {
        const CAPABILITY_MANIFEST: &str = include_str!("../capabilities/default.json");
        let integration = app_system_integration();
        assert_eq!(
            integration.declared_permissions,
            vec!["core:default".to_owned(), "updater:default".to_owned()]
        );
        // And the manifest itself is what the report is derived from.
        let manifest: serde_json::Value =
            serde_json::from_str(CAPABILITY_MANIFEST).expect("valid manifest");
        let declared = manifest
            .get("permissions")
            .and_then(serde_json::Value::as_array)
            .expect("permissions array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(integration.declared_permissions, declared);
    }

    /// No declared permission may reach the filesystem, a shell, a process, or
    /// the OS opener. This is the check behind the sentence the Settings
    /// surface prints, so the sentence cannot drift away from the manifest.
    #[test]
    fn no_declared_permission_can_execute() {
        let integration = app_system_integration();
        assert!(
            integration.execution_capable_permissions.is_empty(),
            "unexpected execution-capable permission: {:?}",
            integration.execution_capable_permissions
        );
        for prefix in ["fs:", "shell:", "process:", "opener:"] {
            assert!(
                !integration
                    .declared_permissions
                    .iter()
                    .any(|permission| permission.starts_with(prefix)),
                "{prefix} permission is declared"
            );
        }
    }

    /// The three host-integration facts are the ones this build guarantees:
    /// nothing is registered at login, no service is installed or started, and
    /// closing the window cancels nothing.
    #[test]
    fn the_reported_host_facts_are_the_shipped_ones() {
        let integration = app_system_integration();
        assert_eq!(
            integration.login_startup.state,
            IntegrationState::NotRegistered
        );
        assert_eq!(
            integration.background_service.state,
            IntegrationState::SeparateProcess
        );
        assert_eq!(
            integration.window_close.state,
            IntegrationState::DaemonOwned
        );
        for fact in [
            &integration.login_startup,
            &integration.background_service,
            &integration.window_close,
        ] {
            assert!(!fact.detail.trim().is_empty(), "every fact explains itself");
            // The report must not read as an instruction to do any of it.
            for forbidden in ["install it", "enable it", "start it", "turn it on"] {
                assert!(
                    !fact.detail.to_ascii_lowercase().contains(forbidden),
                    "a host fact must not invite an action: {forbidden}"
                );
            }
        }
        // The daemon is named as the separate owner of background work.
        assert!(
            integration
                .background_service
                .detail
                .contains(ACTIVE.daemon_binary_name()),
            "the daemon binary must be named: {}",
            integration.background_service.detail
        );
    }
}
