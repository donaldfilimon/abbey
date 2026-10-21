//! Protocol-v2 runtime, deny-by-default protocol-v3 authority, and store-backed
//! protocol-v4 records. V4 turns admit through the shared run manager; providers
//! run only when [`crate::runtime::Executor::execute_v4`] is bound.

use std::sync::Arc;

use abi_agent_runtime::RunBudget;

use crate::app_core::{
    APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, AppCommand, AppContext, AppEvent, AppService,
    BackendSelection, RunMode, RunRouteCapability, RunSubmission, RunSubmissionDisposition,
    V3Command, V3Event, V4Command, V4Event, V4Route,
};
use crate::runtime::{
    FixedProviderKind, FixedRecipeProvider, ManagerError, ModelProviderExecutor, ProviderRoute,
    RunManager, RuntimeStore, StoreError, SubmitDisposition, SystemClock, v4_route_id,
};

use super::runtime_config::{RuntimeConfigError, RuntimeDaemonConfig, open_private_store};
use super::runtime_v3::model_lifecycle::ModelLifecycleAuthority;
use super::runtime_v3::{MemoryEffectRoute, V3RuntimeAuthority, model_inventory};
use super::server::{DaemonHandler, HandlerFailure};
use super::{FederationRequest, FederationResponse, FederationService};

/// Authenticated v1/v2 lifecycle plus narrowly scoped v3 runtime authority.
pub struct RuntimeHandler {
    readonly_v1: AppService,
    runtime_v2: AppService,
    store: Arc<RuntimeStore>,
    manager: RunManager<ModelProviderExecutor, SystemClock>,
    routes: Vec<RunRouteCapability>,
    v4_routes: Vec<V4Route>,
    v3: V3RuntimeAuthority,
    federation: FederationService,
}
impl RuntimeHandler {
    pub fn start(config: RuntimeDaemonConfig) -> Result<Self, RuntimeConfigError> {
        #[cfg(not(unix))]
        {
            let _ = config;
            // Windows named-pipe + JobObject stubs: refuse daemon runtime handler
            // construction on non-Unix. Desktop and CLI clients already gate on
            // transport; this ensures no in-process bypass on Windows builds.
            return Err(RuntimeConfigError::UnsupportedPlatform);
        }

        #[cfg(unix)]
        {
            let parts = config.parts();
            let store = Arc::new(open_private_store(&parts.state_root)?);
            let mut provider_routes = Vec::with_capacity(2);
            if let Some(executable) = parts.abi_binary {
                let provider = FixedRecipeProvider::new(
                    FixedProviderKind::AbiLocal,
                    executable,
                    &parts.workspace,
                    "local",
                    parts.delegated,
                )
                .map_err(|_| RuntimeConfigError::ProviderBinding)?;
                provider_routes.push(
                    ProviderRoute::new(
                        BackendSelection::Abi,
                        "local",
                        Arc::new(provider),
                        provider_budget(parts.delegated),
                    )
                    .map_err(|_| RuntimeConfigError::Routes)?,
                );
            }
            if let Some(executable) = parts.foundation_models_binary {
                let provider = FixedRecipeProvider::new(
                    FixedProviderKind::FoundationModels,
                    executable,
                    &parts.workspace,
                    "system",
                    parts.delegated,
                )
                .map_err(|_| RuntimeConfigError::ProviderBinding)?;
                provider_routes.push(
                    ProviderRoute::new(
                        BackendSelection::FoundationModels,
                        "system",
                        Arc::new(provider),
                        provider_budget(parts.delegated),
                    )
                    .map_err(|_| RuntimeConfigError::Routes)?,
                );
            }
            let executor = if provider_routes.is_empty() {
                ModelProviderExecutor::deny_all()
            } else {
                ModelProviderExecutor::new(provider_routes)
                    .map_err(|_| RuntimeConfigError::Routes)?
            };
            let routes = executor
                .routes()
                .map(|provider| route(provider.backend()))
                .collect::<Vec<_>>();
            let v4_routes = executor
                .routes()
                .map(|provider| v4_route(provider.backend(), provider.model()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| RuntimeConfigError::Routes)?;
            let memory = MemoryEffectRoute::new(parts.state_root.clone(), parts.memory_backend);
            let manifest_models = model_inventory::build(parts.model_manifest_dir.as_deref())
                .map_err(|_| RuntimeConfigError::ModelManifests)?;
            let model_lifecycle = parts
                .model_runtime_config
                .as_deref()
                .map(|path| {
                    ModelLifecycleAuthority::open(path, &parts.workspace, Arc::clone(&store))
                })
                .transpose()
                .map_err(|_| RuntimeConfigError::ModelRuntime)?;
            let v3 = V3RuntimeAuthority::from_provider_routes(
                executor.routes(),
                manifest_models,
                Arc::clone(&store),
                memory,
                model_lifecycle,
            )
            .map_err(|_| RuntimeConfigError::Routes)?;
            let context =
                AppContext::runtime_v2(routes.clone()).map_err(|_| RuntimeConfigError::Routes)?;
            let manager = RunManager::start(
                Arc::clone(&store),
                Arc::new(executor),
                Arc::new(SystemClock),
                parts.manager,
            );
            Ok(Self {
                readonly_v1: AppService::default(),
                runtime_v2: AppService::new(context),
                store,
                manager,
                routes,
                v4_routes,
                v3,
                federation: FederationService,
            })
        }
    }
    fn handle_runtime(&self, command: AppCommand) -> Result<AppEvent, HandlerFailure> {
        command.validate().map_err(|_| invalid_command_failure())?;
        match command {
            // `ReadRoutes` needs no runtime route, store, or provider — it is a
            // read of the same append-only audit log the v1 service reads, so
            // it joins the pure app-core arm rather than getting a route here.
            AppCommand::Status | AppCommand::Claims(_) | AppCommand::ReadRoutes(_) => self
                .runtime_v2
                .handle(command)
                .map_err(|_| internal_failure()),
            AppCommand::SubmitRun(request) => {
                if !route_permits(&self.routes, request.backend, request.mode) {
                    return Err(HandlerFailure::new(
                        "unsupported_route",
                        "requested runtime route is unavailable",
                    ));
                }
                let submitted = self.manager.submit(request).map_err(map_manager_error)?;
                let snapshot = self.snapshot(&submitted.run.id)?;
                let disposition = match submitted.disposition {
                    SubmitDisposition::Enqueued => RunSubmissionDisposition::Enqueued,
                    SubmitDisposition::Existing => RunSubmissionDisposition::Existing,
                    SubmitDisposition::QueueFull => RunSubmissionDisposition::QueueFull,
                };
                let event = RunSubmission {
                    disposition,
                    run: snapshot,
                };
                event.validate().map_err(|_| internal_failure())?;
                Ok(AppEvent::RunSubmitted(event))
            }
            AppCommand::GetRun(query) => Ok(AppEvent::RunStatus(self.snapshot(&query.run_id)?)),
            AppCommand::CancelRun(query) => {
                self.manager
                    .cancel(&query.run_id)
                    .map_err(map_manager_error)?;
                Ok(AppEvent::CancellationAcknowledged(
                    self.snapshot(&query.run_id)?,
                ))
            }
            AppCommand::RunEvents(query) => {
                let page = self
                    .store
                    .run_events_page(
                        &query.run_id,
                        query.after_sequence,
                        query.through_sequence,
                        query.limit,
                    )
                    .map_err(map_store_error)?;
                Ok(AppEvent::RunEvents(page))
            }
        }
    }
    fn snapshot(
        &self,
        run_id: &crate::app_core::RunId,
    ) -> Result<crate::app_core::RunSnapshot, HandlerFailure> {
        self.store
            .run_snapshot(run_id)
            .map_err(map_store_error)?
            .ok_or_else(not_found_failure)
    }
}
impl DaemonHandler for RuntimeHandler {
    fn supports_version(&self, version: u16) -> bool {
        matches!(version, APP_PROTOCOL_V1 | APP_PROTOCOL_VERSION)
    }

    fn handle_versioned(
        &self,
        version: u16,
        command: AppCommand,
    ) -> Result<AppEvent, HandlerFailure> {
        match version {
            APP_PROTOCOL_V1 => self
                .readonly_v1
                .handle(command)
                .map_err(|_| invalid_command_failure()),
            APP_PROTOCOL_VERSION => self.handle_runtime(command),
            _ => Err(HandlerFailure::new(
                "unsupported_version",
                "protocol version is unsupported",
            )),
        }
    }

    fn supports_v3(&self) -> bool {
        true
    }

    fn authorizes_v3(
        &self,
        grants: &crate::app_core::V3CapabilitySet,
        command: &V3Command,
    ) -> bool {
        self.v3.authorizes(grants, command)
    }

    fn handle_v3(&self, command: V3Command) -> Result<V3Event, HandlerFailure> {
        self.v3.handle(command)
    }

    fn supports_v4(&self) -> bool {
        true
    }

    fn authorizes_v4(
        &self,
        grants: &crate::app_core::V4CapabilitySet,
        command: &V4Command,
    ) -> bool {
        super::runtime_v4::authorizes(grants, command)
    }

    fn handle_v4(&self, command: V4Command) -> Result<V4Event, HandlerFailure> {
        match command {
            V4Command::Negotiate(request) => super::runtime_v4::negotiate(request, true),
            V4Command::CancelTurn(conversation_id, run_id) => {
                // Validate the durable v4 association BEFORE touching RunManager.
                // A valid legacy run ID or a different conversation confers no authority.
                self.store
                    .v4_require_turn(&conversation_id, &run_id)
                    .map_err(map_store_error)?;
                let run = self.manager.cancel(&run_id).map_err(map_manager_error)?;
                let event = V4Event::TurnCancelled(conversation_id, run_id, run.status);
                event.validate().map_err(|_| internal_failure())?;
                Ok(event)
            }
            V4Command::SubmitTurn(request) => {
                let submission = self.manager.submit_v4(request).map_err(map_manager_error)?;
                let event = V4Event::TurnSubmitted(submission);
                event.validate().map_err(|_| internal_failure())?;
                Ok(event)
            }
            V4Command::ListRoutes => {
                let event = V4Event::Routes(self.v4_routes.clone());
                event.validate().map_err(|_| internal_failure())?;
                Ok(event)
            }
            other => {
                let elapsed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| internal_failure())?;
                let now_ms = u64::try_from(elapsed.as_millis()).map_err(|_| internal_failure())?;
                super::runtime_v4::handle(&self.store, other, now_ms)
            }
        }
    }

    fn supports_federation(&self) -> bool {
        true
    }

    fn handle_federation(&self, request: FederationRequest) -> FederationResponse {
        let request_id = request.request_id.clone();
        match self.federation.handle(request) {
            Ok(result) => FederationResponse::ok(request_id, result),
            Err(error) => FederationResponse::error(request_id, error.code, error.message),
        }
    }
}

/// Per-turn provider ceilings shared by legacy and v4 chat runs: large enough
/// for an ordinary streaming reply, small enough to stop a runaway provider.
const TURN_MAX_EVENTS: u64 = 4096;
const TURN_MAX_OUTPUT_TOKENS: u64 = 1024;

fn provider_budget(limits: crate::runtime::DelegatedLimits) -> RunBudget {
    RunBudget::unlimited()
        .with_max_events(TURN_MAX_EVENTS)
        .with_max_output_tokens(TURN_MAX_OUTPUT_TOKENS)
        .with_max_duration(limits.timeout)
}

fn route(backend: BackendSelection) -> RunRouteCapability {
    RunRouteCapability {
        backend,
        modes: vec![RunMode::OneShot, RunMode::Background],
    }
}

fn v4_route(backend: BackendSelection, model: &str) -> Result<V4Route, ()> {
    let (backend_name, label) = match backend {
        BackendSelection::Abi => ("abi", "ABI local"),
        BackendSelection::FoundationModels => ("foundation_models", "Foundation Models"),
        BackendSelection::Cursor | BackendSelection::Grok => return Err(()),
    };
    let route = V4Route {
        route_id: v4_route_id(backend, model),
        backend: backend_name.into(),
        label: format!("{label} ({model})"),
        available: true,
    };
    route.validate().map_err(|_| ())?;
    Ok(route)
}

fn route_permits(routes: &[RunRouteCapability], backend: BackendSelection, mode: RunMode) -> bool {
    routes
        .iter()
        .any(|route| route.backend == backend && route.modes.binary_search(&mode).is_ok())
}

fn map_manager_error(error: ManagerError) -> HandlerFailure {
    match error {
        ManagerError::InvalidRequest(_) => invalid_command_failure(),
        ManagerError::ShuttingDown | ManagerError::WorkerPanicked => internal_failure(),
        ManagerError::Store(error) => map_store_error(error),
    }
}
fn map_store_error(error: StoreError) -> HandlerFailure {
    match error {
        StoreError::RunNotFound(_) => not_found_failure(),
        StoreError::IdempotencyConflict => HandlerFailure::new(
            "idempotency_conflict",
            "idempotency key belongs to a different request",
        ),
        StoreError::InvalidInput(_)
        | StoreError::InvalidAuditMetadata(_)
        | StoreError::ConversationNotFound(_)
        | StoreError::UnexpectedStatus { .. }
        | StoreError::InvalidTransition { .. }
        | StoreError::TerminalRun { .. }
        | StoreError::ToolApprovalConflict
        | StoreError::ToolApprovalDigestMismatch
        | StoreError::ToolExecutionConflict
        | StoreError::ModelOperationConflict
        | StoreError::ModelOperationCapacity => invalid_command_failure(),
        StoreError::ToolApprovalNotFound(_) | StoreError::ToolExecutionNotFound(_) => {
            not_found_failure()
        }
        StoreError::AssistantNotFound => HandlerFailure::new("not_found", "resource was not found"),
        StoreError::AssistantConflict => {
            HandlerFailure::new("conflict", "resource conflicts with stored state")
        }
        StoreError::ModelOperationNotFound(_) => not_found_failure(),
        StoreError::CorruptData(_)
        | StoreError::Migration(_)
        | StoreError::Database(_)
        | StoreError::Io(_)
        | StoreError::Json(_) => internal_failure(),
    }
}

const fn invalid_command_failure() -> HandlerFailure {
    HandlerFailure::new("invalid_command", "command payload is invalid")
}

const fn not_found_failure() -> HandlerFailure {
    HandlerFailure::new("run_not_found", "run was not found")
}

const fn internal_failure() -> HandlerFailure {
    HandlerFailure::new("runtime_unavailable", "runtime operation is unavailable")
}

#[cfg(all(test, unix))]
mod tests;
