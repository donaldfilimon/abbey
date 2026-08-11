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
use super::runtime_v3::{MemoryEffectRoute, V3RuntimeAuthority};
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
            let v3 = V3RuntimeAuthority::from_provider_routes(
                executor.routes(),
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
mod tests {
    use super::*;
    use crate::app_core::{
        AppCapability, IdempotencyKey, RunEventsQuery, RunQuery, RunRequest, RunState,
        RuntimeStatus, V3Capability, V3CapabilitySet, V3Command, V3ErrorCode, V3Event,
        V3GrantRequest, V3PageQuery,
    };
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::str::FromStr as _;
    use std::thread;
    use std::time::{Duration, Instant};

    struct Harness {
        root: PathBuf,
        handler: RuntimeHandler,
    }

    impl Harness {
        fn start(script: &str) -> Self {
            let root = scratch("handler");
            let workspace = root.join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            let executable = root.join("abi-provider");
            write_script(&executable, script);
            let config = RuntimeDaemonConfig::new(&root, &workspace).bind_abi_local(&executable);
            let handler = RuntimeHandler::start(config).unwrap();
            Self { root, handler }
        }

        fn call(&self, command: AppCommand) -> Result<AppEvent, HandlerFailure> {
            self.handler.handle_versioned(APP_PROTOCOL_VERSION, command)
        }

        fn submit(&self, key: &str, input: &str) -> crate::app_core::RunId {
            let event = self
                .call(AppCommand::SubmitRun(request(key, input)))
                .unwrap();
            let AppEvent::RunSubmitted(submission) = event else {
                panic!("expected run submission");
            };
            submission.run.run_id
        }

        fn terminal(&self, run_id: &crate::app_core::RunId) -> crate::app_core::RunSnapshot {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let AppEvent::RunStatus(snapshot) = self
                    .call(AppCommand::GetRun(RunQuery {
                        run_id: run_id.clone(),
                    }))
                    .unwrap()
                else {
                    panic!("expected run status");
                };
                if snapshot.state.is_terminal() {
                    return snapshot;
                }
                assert!(Instant::now() < deadline, "run did not finish");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = self.handler.manager.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn v1_stays_read_only_while_v2_advertises_only_bound_routes() {
        let harness = Harness::start("exit 0");
        let AppEvent::Status(v1) = harness
            .handler
            .handle_versioned(APP_PROTOCOL_V1, AppCommand::Status)
            .unwrap()
        else {
            panic!("expected v1 status");
        };
        assert_eq!(v1.protocol_version, APP_PROTOCOL_V1);
        assert!(v1.run_routes.is_empty());

        let AppEvent::Status(RuntimeStatus {
            protocol_version,
            run_routes,
            ..
        }) = harness.call(AppCommand::Status).unwrap()
        else {
            panic!("expected v2 status");
        };
        assert_eq!(protocol_version, APP_PROTOCOL_VERSION);
        assert_eq!(run_routes, vec![route(BackendSelection::Abi)]);

        let error = harness
            .handler
            .handle_versioned(
                APP_PROTOCOL_V1,
                AppCommand::SubmitRun(request("v1:rejected", "not executed")),
            )
            .unwrap_err();
        assert_eq!(error.code(), "invalid_command");
    }

    #[test]
    fn daemon_starts_without_provider_and_omits_only_submission_authority() {
        let root = scratch("no-provider");
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let handler = RuntimeHandler::start(RuntimeDaemonConfig::new(&root, &workspace)).unwrap();
        let AppEvent::Status(status) = handler
            .handle_versioned(APP_PROTOCOL_VERSION, AppCommand::Status)
            .unwrap()
        else {
            panic!("expected v2 status");
        };
        assert!(status.run_routes.is_empty());
        assert!(!status.capabilities.contains(AppCapability::SubmitRun));
        assert!(status.capabilities.contains(AppCapability::ReadRun));
        assert!(status.capabilities.contains(AppCapability::CancelRun));
        let error = handler
            .handle_versioned(
                APP_PROTOCOL_VERSION,
                AppCommand::SubmitRun(request("no-provider:submit", "do not spawn")),
            )
            .unwrap_err();
        assert_eq!(error.code(), "unsupported_route");
        drop(handler);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn submit_status_and_events_use_canonical_redacted_projections() {
        let harness = Harness::start("printf 'ok'");
        let run_id = harness.submit("run:success", "private prompt");
        let snapshot = harness.terminal(&run_id);
        assert_eq!(snapshot.state, RunState::Succeeded);
        let page = harness
            .call(AppCommand::RunEvents(RunEventsQuery {
                run_id: run_id.clone(),
                after_sequence: 0,
                through_sequence: None,
                limit: 16,
            }))
            .unwrap();
        let rendered = serde_json::to_string(&page).unwrap();
        assert!(!rendered.contains("private prompt"));
        let AppEvent::RunEvents(page) = page else {
            panic!("expected run events");
        };
        assert_eq!(page.run_id, run_id);
        assert!(!page.events.is_empty());
        assert!(!page.has_more);
    }

    #[test]
    fn cancellation_acknowledges_and_reaches_a_durable_terminal_state() {
        let harness = Harness::start("sleep 5");
        let run_id = harness.submit("run:cancel", "cancel this bounded run");
        let event = harness
            .call(AppCommand::CancelRun(RunQuery {
                run_id: run_id.clone(),
            }))
            .unwrap();
        let AppEvent::CancellationAcknowledged(snapshot) = event else {
            panic!("expected cancellation acknowledgement");
        };
        assert!(matches!(
            snapshot.state,
            RunState::CancelRequested | RunState::Cancelled
        ));
        assert_eq!(harness.terminal(&run_id).state, RunState::Cancelled);
    }

    #[test]
    fn route_and_store_failures_are_stable_and_redacted() {
        let harness = Harness::start("printf '%s' \"$*\" >&2; exit 9");
        let mut unsupported = request("route:unsupported", "secret-input");
        unsupported.backend = BackendSelection::Cursor;
        let error = harness
            .call(AppCommand::SubmitRun(unsupported))
            .unwrap_err();
        assert_eq!(error.code(), "unsupported_route");
        assert!(!format!("{error:?}").contains("secret-input"));

        let missing = crate::app_core::RunId::new();
        let error = harness
            .call(AppCommand::GetRun(RunQuery { run_id: missing }))
            .unwrap_err();
        assert_eq!(error.code(), "run_not_found");

        let run_id = harness.submit("run:failure", "never disclose me");
        let snapshot = harness.terminal(&run_id);
        assert_eq!(snapshot.state, RunState::Failed);
        let rendered = serde_json::to_string(&snapshot).unwrap();
        assert!(!rendered.contains("never disclose me"));
        assert!(!rendered.contains("complete --model"));
    }

    #[test]
    fn authenticated_unix_server_accepts_v1_v2_and_separate_v3_envelopes() {
        const BEARER: &str = "runtime-handler-wire-bearer-000001";

        let root = scratch("wire");
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let executable = root.join("abi-provider");
        write_script(&executable, "printf 'ok'");
        let handler = RuntimeHandler::start(
            RuntimeDaemonConfig::new(&root, &workspace).bind_abi_local(&executable),
        )
        .unwrap();
        let socket = root.join("abbeyd.sock");
        let daemon_config =
            crate::daemon::DaemonConfig::for_test(socket.clone(), BEARER.as_bytes());
        let shutdown = crate::daemon::Shutdown::default();
        let server_shutdown = shutdown.clone();
        let server = thread::spawn(move || {
            crate::daemon::DaemonServer::new(daemon_config, handler).serve(server_shutdown)
        });
        wait_for_socket(&socket);

        let unauthorized = wire_v3_request(
            &socket,
            crate::daemon::V3RequestEnvelope {
                version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                request_id: "v3-unauthorized".into(),
                bearer: "wrong-bearer-00000000000000000000".into(),
                grants: V3CapabilitySet::deny_all(),
                command: V3Command::Negotiate(V3GrantRequest {
                    supported_versions: vec![3],
                    requested: V3CapabilitySet::deny_all(),
                }),
            },
        );
        assert!(matches!(
            unauthorized.payload,
            crate::daemon::V3ResponsePayload::Error { error }
                if error.code == V3ErrorCode::Unauthorized
        ));

        let wrong_schema = wire_v3_request(
            &socket,
            crate::daemon::V3RequestEnvelope {
                version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3 + 1,
                request_id: "v3-wrong-schema".into(),
                bearer: BEARER.into(),
                grants: V3CapabilitySet::deny_all(),
                command: V3Command::Negotiate(V3GrantRequest {
                    supported_versions: vec![3],
                    requested: V3CapabilitySet::deny_all(),
                }),
            },
        );
        assert!(matches!(
            wrong_schema.payload,
            crate::daemon::V3ResponsePayload::Error { error }
                if error.code == V3ErrorCode::UnsupportedVersion
        ));

        let v1 = wire_request(
            &socket,
            crate::daemon::RequestEnvelope {
                version: APP_PROTOCOL_V1,
                request_id: "v1-status".into(),
                bearer: BEARER.into(),
                command: AppCommand::Status,
            },
        );
        let crate::daemon::ResponsePayload::Ok {
            event: AppEvent::Status(v1),
        } = v1.payload
        else {
            panic!("expected v1 status");
        };
        assert_eq!(v1.protocol_version, APP_PROTOCOL_V1);
        assert!(v1.run_routes.is_empty());

        let submitted = wire_request(
            &socket,
            crate::daemon::RequestEnvelope {
                version: APP_PROTOCOL_VERSION,
                request_id: "v2-submit".into(),
                bearer: BEARER.into(),
                command: AppCommand::SubmitRun(request("wire:submit", "wire private prompt")),
            },
        );
        assert_eq!(submitted.version, APP_PROTOCOL_VERSION);
        let crate::daemon::ResponsePayload::Ok {
            event: AppEvent::RunSubmitted(submission),
        } = submitted.payload
        else {
            panic!("expected v2 submission");
        };
        let run_id = submission.run.run_id;

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let response = wire_request(
                &socket,
                crate::daemon::RequestEnvelope {
                    version: APP_PROTOCOL_VERSION,
                    request_id: "v2-get".into(),
                    bearer: BEARER.into(),
                    command: AppCommand::GetRun(RunQuery {
                        run_id: run_id.clone(),
                    }),
                },
            );
            let crate::daemon::ResponsePayload::Ok {
                event: AppEvent::RunStatus(snapshot),
            } = response.payload
            else {
                panic!("expected v2 run status");
            };
            if snapshot.state.is_terminal() {
                assert_eq!(snapshot.state, RunState::Succeeded);
                break;
            }
            assert!(Instant::now() < deadline, "wire run did not finish");
            thread::sleep(Duration::from_millis(10));
        }

        let events = wire_request(
            &socket,
            crate::daemon::RequestEnvelope {
                version: APP_PROTOCOL_VERSION,
                request_id: "v2-events".into(),
                bearer: BEARER.into(),
                command: AppCommand::RunEvents(RunEventsQuery {
                    run_id,
                    after_sequence: 0,
                    through_sequence: None,
                    limit: 16,
                }),
            },
        );
        let rendered = serde_json::to_string(&events).unwrap();
        assert!(matches!(
            events.payload,
            crate::daemon::ResponsePayload::Ok {
                event: AppEvent::RunEvents(_)
            }
        ));
        assert!(!rendered.contains("wire private prompt"));

        let negotiated = wire_v3_request(
            &socket,
            crate::daemon::V3RequestEnvelope {
                version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                request_id: "v3-negotiate".into(),
                bearer: BEARER.into(),
                grants: V3CapabilitySet::deny_all(),
                command: V3Command::Negotiate(V3GrantRequest {
                    supported_versions: vec![3],
                    requested: V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels])
                        .unwrap(),
                }),
            },
        );
        assert!(matches!(
            negotiated.payload,
            crate::daemon::V3ResponsePayload::Ok {
                event: V3Event::Negotiated(_)
            }
        ));

        let models = wire_v3_request(
            &socket,
            crate::daemon::V3RequestEnvelope {
                version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                request_id: "v3-models".into(),
                bearer: BEARER.into(),
                grants: V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap(),
                command: V3Command::ListModels(V3PageQuery::default()),
            },
        );
        assert!(matches!(
            models.payload,
            crate::daemon::V3ResponsePayload::Ok {
                event: V3Event::Models(_)
            }
        ));

        let missing_grant = wire_v3_request(
            &socket,
            crate::daemon::V3RequestEnvelope {
                version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                request_id: "v3-missing-grant".into(),
                bearer: BEARER.into(),
                grants: V3CapabilitySet::deny_all(),
                command: V3Command::ListModels(V3PageQuery::default()),
            },
        );
        assert!(matches!(
            missing_grant.payload,
            crate::daemon::V3ResponsePayload::Error { error }
                if error.code == V3ErrorCode::CapabilityDenied
        ));

        let denied = wire_v3_request(
            &socket,
            crate::daemon::V3RequestEnvelope {
                version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                request_id: "v3-denied".into(),
                bearer: BEARER.into(),
                grants: V3CapabilitySet::from_sorted(vec![
                    V3Capability::ReadModels,
                    V3Capability::PollEvents,
                ])
                .unwrap(),
                command: V3Command::ListModels(V3PageQuery::default()),
            },
        );
        assert!(matches!(
            denied.payload,
            crate::daemon::V3ResponsePayload::Error { error }
                if error.code == V3ErrorCode::CapabilityDenied
        ));

        shutdown.request();
        server.join().unwrap().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    fn request(key: &str, input: &str) -> RunRequest {
        RunRequest {
            idempotency_key: IdempotencyKey::from_str(key).unwrap(),
            conversation_id: None,
            mode: RunMode::OneShot,
            backend: BackendSelection::Abi,
            input: input.into(),
            labels: Vec::new(),
        }
    }

    fn write_script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn wait_for_socket(socket: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(metadata) = std::fs::metadata(socket) {
                let mode = metadata.permissions().mode();
                if metadata.file_type().is_socket() && mode & 0o077 == 0 {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "daemon socket was not created with owner-only permissions"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wire_request(
        socket: &Path,
        request: crate::daemon::RequestEnvelope,
    ) -> crate::daemon::ResponseEnvelope {
        wire_exchange(socket, &request)
    }

    fn wire_v3_request(
        socket: &Path,
        request: crate::daemon::V3RequestEnvelope,
    ) -> crate::daemon::V3ResponseEnvelope {
        wire_exchange(socket, &request)
    }

    fn wire_exchange<T, R>(socket: &Path, request: &T) -> R
    where
        T: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let bytes = serde_json::to_vec(request).unwrap();
        let mut frame = Vec::with_capacity(4 + bytes.len());
        frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        frame.extend_from_slice(&bytes);
        stream.write_all(&frame).unwrap();
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut response = vec![0_u8; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut response).unwrap();
        serde_json::from_slice(&response).unwrap()
    }

    fn scratch(label: &str) -> PathBuf {
        let path = PathBuf::from("/tmp").join(format!(
            "abbey-runtime-handler-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
