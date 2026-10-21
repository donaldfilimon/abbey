//! Enumerated reads, signed-model lifecycle and daemon-only conversation metadata/history.
//! No generic effect, inference, shell, credentials, or paths cross IPC.

use abbey::app_core::{
    ClaimsQuery, ClaimsSnapshot, ConversationId, RouteAuditPage, RouteAuditQuery, RunEventPage,
    RunEventsQuery, RunId, RunQuery, RunSnapshot, RunState, RuntimeStatus, V3CapabilitySet,
    V3EntityPage, V3EntityRecord, V3ModelAction, V3ModelDetails, V3OperationStatus, V3PageQuery,
    V3ResourceQuery, V3SearchRequest, V3StableClaim, V4CapabilitySet, V4ContentPage,
    V4ContentQuery, V4Conversation, V4ConversationCreate, V4ConversationPage, V4ConversationQuery,
    V4ConversationRename, V4MessagePage, V4MessageQuery, V4ResourceQuery, V4Route, V4SubmitTurn,
    V4TurnSubmission,
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
