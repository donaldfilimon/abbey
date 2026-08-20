//! Protocol-v2 runtime plus the first deny-by-default protocol-v3 authority.

use std::sync::Arc;

use abi_agent_runtime::RunBudget;

use crate::app_core::{
    APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, AppCommand, AppContext, AppEvent, AppService,
    BackendSelection, RunMode, RunRouteCapability, RunSubmission, RunSubmissionDisposition,
    V3Command, V3Event,
};
use crate::runtime::{
    FixedProviderKind, FixedRecipeProvider, ManagerError, ModelProviderExecutor, ProviderRoute,
    RunManager, RuntimeStore, StoreError, SubmitDisposition, SystemClock,
};

use super::runtime_config::{RuntimeConfigError, RuntimeDaemonConfig, open_private_store};
use super::runtime_v3::{MemoryEffectRoute, V3RuntimeAuthority, model_inventory};
use super::server::{DaemonHandler, HandlerFailure};

/// Authenticated v1/v2 lifecycle plus narrowly scoped v3 runtime authority.
pub struct RuntimeHandler {
    readonly_v1: AppService,
    runtime_v2: AppService,
    store: Arc<RuntimeStore>,
    manager: RunManager<ModelProviderExecutor, SystemClock>,
    routes: Vec<RunRouteCapability>,
    v3: V3RuntimeAuthority,
}
impl RuntimeHandler {
    pub fn start(config: RuntimeDaemonConfig) -> Result<Self, RuntimeConfigError> {
        #[cfg(not(unix))]
        {
            let _ = config;
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
            let memory = MemoryEffectRoute::new(parts.state_root.clone(), parts.memory_backend);
            let manifest_models = model_inventory::build(parts.model_manifest_dir.as_deref())
                .map_err(|_| RuntimeConfigError::ModelManifests)?;
            let v3 = V3RuntimeAuthority::from_provider_routes(
                executor.routes(),
                manifest_models,
                Arc::clone(&store),
                memory,
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
                v3,
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
}

fn provider_budget(limits: crate::runtime::DelegatedLimits) -> RunBudget {
    RunBudget::unlimited()
        .with_max_events(8)
        .with_max_output_tokens(1)
        .with_max_duration(limits.timeout)
}

fn route(backend: BackendSelection) -> RunRouteCapability {
    RunRouteCapability {
        backend,
        modes: vec![RunMode::OneShot, RunMode::Background],
    }
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
        | StoreError::ToolExecutionConflict => invalid_command_failure(),
        StoreError::ToolApprovalNotFound(_) | StoreError::ToolExecutionNotFound(_) => {
            not_found_failure()
        }
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
