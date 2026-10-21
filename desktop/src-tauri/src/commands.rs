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
use abbey::effective_model::EffectiveModel;

use crate::backend;
use crate::ipc::{
    BundleIdentity, ConnectionInfo, IntegrationFact, IntegrationState, IpcError, IpcErrorKind,
    SystemIntegration,
};

/// `ReadStatus` — process identity, build stamp, and granted capabilities.
#[tauri::command]
pub fn app_status() -> Result<RuntimeStatus, IpcError> {
    backend::status()
}

/// The effective model — the model a turn would use right now, resolved with
/// Abbey's shared spawn-path rules. Read-only: it reports; it does not act.
///
/// The read is governed by the `ReadRoutes` grant, and by nothing else. The
/// answer is a projection of the routes the connected runtime publishes, and
/// the protocol-v4 client refuses the request outright when that grant was
/// not negotiated. There is no separate effective-model capability, because
/// there is no separate effective-model authority.
///
/// The read opens its own read-only session rather than borrowing the
/// conversation session: the conversation session negotiates write and submit
/// authority because the Chat surface needs it, and a diagnosis of "which
/// model would a turn use" must not depend on whether the client may mutate.
///
/// Three ways to ask:
///
/// * **A route id** — the answer is *that* route, which is what the Chat
///   surface's current selection is. This is the honest answer to "the model
///   a turn would use right now" once a route is selected.
/// * **Nothing** — the answer is the runtime's first available route, which
///   is the route a turn uses when no other one is selected.
/// * **A backend and a model** — the answer is that named pair, resolved
///   with the same rules. This is an explicit request for a specific pair,
///   not a report of what a turn would use, and it is labeled as such.
#[tauri::command]
pub fn app_effective_model(
    route_id: Option<String>,
    backend: Option<String>,
    model: Option<String>,
) -> Result<EffectiveModel, IpcError> {
    match (route_id, backend, model) {
        (None, Some(backend), Some(model)) => resolve(&backend, &model),
        (None, None, None) => runtime_effective_model(None),
        (Some(route_id), None, None) => runtime_effective_model(Some(&route_id)),
        _ => Err(IpcError::new(
            IpcErrorKind::Rejected,
            "ask for a route id, for a backend and model together, or for neither",
        )),
    }
}

/// The runtime's route, resolved with Abbey's shared rules.
///
/// The runtime owns the routes its turns run on, so this is the route the
/// desktop's own turns use — not the desktop process's own environment.
///
/// With a `route_id`, that exact route is the answer: it is the route the
/// Chat surface has selected, and the Chat surface submits turns on
/// whichever route is selected. Resolving the *first* available route
/// instead would report a model the desktop will not use, which is exactly
/// the disagreement this command exists to prevent. A route id the runtime
/// does not publish, or one it publishes as unavailable, is refused rather
/// than silently replaced with a different route.
fn runtime_effective_model(route_id: Option<&str>) -> Result<EffectiveModel, IpcError> {
    if backend::bearer_source().is_none() {
        return Err(IpcError::new(
            IpcErrorKind::Rejected,
            "no runtime route is published: this client executes no turns without a daemon",
        ));
    }
    let routes = backend::published_routes()?;
    let route = match route_id {
        Some(route_id) => routes
            .iter()
            .find(|route| route.route_id == route_id)
            .ok_or_else(|| {
                IpcError::new(
                    IpcErrorKind::NotFound,
                    format!("the runtime publishes no route {route_id:?}"),
                )
            })?,
        None => routes.iter().find(|route| route.available).ok_or_else(|| {
            IpcError::new(
                IpcErrorKind::NotFound,
                "the runtime publishes no available route",
            )
        })?,
    };
    if !route.available {
        return Err(IpcError::new(
            IpcErrorKind::Rejected,
            format!("the runtime publishes route {:?} as unavailable", route.route_id),
        ));
    }
    resolve_for_route(&route.backend, &route.model)
}

/// Resolve one backend/model pair with Abbey's shared rules.
///
/// An unknown backend label is refused rather than guessed: the client must
/// not invent a backend the user did not name.
fn resolve(backend: &str, model: &str) -> Result<EffectiveModel, IpcError> {
    EffectiveModel::resolve(backend, model).ok_or_else(|| {
        IpcError::new(
            IpcErrorKind::Rejected,
            format!("unknown backend {backend:?}"),
        )
    })
}

/// Resolve a route the runtime published, with Abbey's shared rules.
///
/// The route's backend is already a canonical label and its model is already
/// the exact id the provider receives, so this is the shared resolution and
/// not a second grammar. The answer records that the runtime's route was
/// the authority.
fn resolve_for_route(backend: &str, model: &str) -> Result<EffectiveModel, IpcError> {
    EffectiveModel::resolve_for_route(backend, model).ok_or_else(|| {
        IpcError::new(
            IpcErrorKind::Rejected,
            format!("unknown backend {backend:?}"),
        )
    })
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
    use abbey::effective_model::EffectiveModelSource;

    /// An explicit backend and model resolve with Abbey's shared rules — the
    /// same rules `abbey doctor` and the TUI doctor panel apply, so the
    /// surfaces cannot disagree.
    #[test]
    fn an_explicit_backend_and_model_resolve_with_the_shared_rules() {
        let ollama = app_effective_model(None, Some("ollama".into()), Some("gemma".into()))
            .expect("a known backend resolves");
        assert_eq!(ollama.backend, "ollama");
        assert_eq!(ollama.raw, "gemma");
        assert_eq!(ollama.resolved, "gemma4:26b-mlx");

        // The alias table, the fm vocabulary, and the abi vocabulary all
        // resolve exactly as they do for the CLI.
        let cursor = app_effective_model(None, Some("cursor-agent".into()), Some("opus".into()))
            .expect("a known backend resolves");
        assert_eq!(cursor.resolved, "claude-opus-5-thinking-high");

        let fm = app_effective_model(
            None,
            Some("fm".into()),
            Some("claude-opus-5-thinking-high".into()),
        )
        .expect("a known backend resolves");
        assert_eq!(fm.resolved, "system");

        let abi = app_effective_model(None, Some("abi".into()), Some("fable".into()))
            .expect("a known backend resolves");
        assert_eq!(abi.resolved, "local");
    }

    /// An unknown backend label is refused, never guessed into one.
    #[test]
    fn an_unknown_backend_is_refused_rather_than_guessed() {
        let error = app_effective_model(None, Some("nonsense".into()), Some("gemma".into()))
            .expect_err("an unknown backend resolves to nothing");
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        assert!(error.message.contains("unknown backend"));
    }

    /// Half a request is refused: a backend without a model (or the reverse)
    /// would otherwise resolve against an invented model.
    #[test]
    fn a_half_requested_pair_is_refused() {
        let error = app_effective_model(None, Some("ollama".into()), None)
            .expect_err("half a request is refused");
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        assert!(error.message.contains("ask for a route id"));
    }

    /// A route id together with a backend or a model is refused: they are
    /// two different questions, and answering one with the other would
    /// report a model the route does not use.
    #[test]
    fn a_route_id_combined_with_a_named_pair_is_refused() {
        let error = app_effective_model(Some("abi.local".into()), Some("abi".into()), None)
            .expect_err("a route id and a half pair are refused");
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        assert!(error.message.contains("ask for a route id"));
    }

    /// Without a daemon there is no route, so there is no effective model to
    /// report — and the window does not substitute one of its own.
    #[test]
    fn without_a_daemon_there_is_no_runtime_route_to_report() {
        if backend::bearer_source().is_some() {
            // A daemon is configured in this environment; the live daemon
            // proof covers the daemon-backed path.
            return;
        }
        let error = app_effective_model(None, None, None).expect_err("no daemon means no runtime route");
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        assert!(error.message.contains("no runtime route is published"));
    }

    /// A route id the runtime does not publish is refused, never replaced
    /// with a different route. Reporting the runtime's first available route
    /// instead would answer a different question than the one asked.
    #[test]
    fn an_unpublished_route_id_is_refused_rather_than_replaced() {
        if backend::bearer_source().is_some() {
            // A daemon is configured in this environment; the live daemon
            // proof covers the daemon-backed path.
            return;
        }
        let error = app_effective_model(Some("no.such.route".into()), None, None)
            .expect_err("an unpublished route is refused");
        // Without a daemon there is no published route list at all, so the
        // refusal is the same one a missing daemon produces.
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        assert!(error.message.contains("no runtime route is published"));
    }

    /// Against a real daemon the effective model is the runtime's own
    /// published route, resolved with Abbey's shared rules — never a value the
    /// desktop process made up about itself.
    #[cfg(unix)]
    #[test]
    fn live_daemon_desktop_effective_model_comes_from_the_runtime_route() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_DAEMON").is_none() {
            return;
        }
        let routes = backend::published_routes().expect("the daemon publishes its routes");
        let route = routes
            .iter()
            .find(|route| route.available)
            .expect("an available route");
        // The scratch daemon binds the ABI local provider.
        assert_eq!(route.backend, "abi");
        assert_eq!(route.model, "local");
        let effective = app_effective_model(None, None, None).expect("the runtime route resolves");
        assert_eq!(effective.backend, route.backend);
        assert_eq!(effective.raw, route.model);
        assert_eq!(effective.resolved, "local");
    }

    /// A *selected* route is the route a turn runs on, so the effective model
    /// must be that route's model — not the runtime's first available one.
    #[cfg(unix)]
    #[test]
    fn live_daemon_the_selected_route_answers_the_effective_model() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_DAEMON").is_none() {
            return;
        }
        let routes = backend::published_routes().expect("the daemon publishes its routes");
        let route = routes
            .iter()
            .find(|route| route.available)
            .expect("an available route");
        // Asking for that exact route must answer with that route's model.
        let effective = app_effective_model(Some(route.route_id.clone()), None, None)
            .expect("the selected route resolves");
        assert_eq!(effective.backend, route.backend);
        assert_eq!(effective.raw, route.model);
        assert_eq!(effective.resolved, "local");
        // And it is labeled as the runtime's route, not as a local guess.
        assert_eq!(effective.source, EffectiveModelSource::RuntimeRoute);
    }

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
