use super::*;
use crate::app_core::{
    AppCapability, IdempotencyKey, RunEventsQuery, RunQuery, RunRequest, RunState, RuntimeStatus,
    V3Capability, V3CapabilitySet, V3Command, V3ErrorCode, V3Event, V3GrantRequest, V3PageQuery,
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
    let daemon_config = crate::daemon::DaemonConfig::for_test(socket.clone(), BEARER.as_bytes());
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
                requested: V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels]).unwrap(),
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
    let request_id = request.request_id.clone();
    wire_exchange(socket, &request, &request_id)
}

fn wire_v3_request(
    socket: &Path,
    request: crate::daemon::V3RequestEnvelope,
) -> crate::daemon::V3ResponseEnvelope {
    let request_id = request.request_id.clone();
    wire_exchange(socket, &request, &request_id)
}

// This is a scheduler budget for a process-backed integration fixture, not a
// product latency claim. The daemon intentionally serves one connection at a
// time, and the full parallel suite can deschedule its server thread long
// enough for the former one-second client timeout to expire spuriously.
const WIRE_IO_TIMEOUT: Duration = Duration::from_secs(5);

fn wire_exchange<T, R>(socket: &Path, request: &T, request_id: &str) -> R
where
    T: serde::Serialize,
    R: serde::de::DeserializeOwned,
{
    let mut stream = UnixStream::connect(socket)
        .unwrap_or_else(|error| panic!("connect for wire request {request_id:?}: {error}"));
    stream
        .set_read_timeout(Some(WIRE_IO_TIMEOUT))
        .unwrap_or_else(|error| {
            panic!("set read timeout for wire request {request_id:?}: {error}")
        });
    stream
        .set_write_timeout(Some(WIRE_IO_TIMEOUT))
        .unwrap_or_else(|error| {
            panic!("set write timeout for wire request {request_id:?}: {error}")
        });
    let bytes = serde_json::to_vec(request)
        .unwrap_or_else(|error| panic!("encode wire request {request_id:?}: {error}"));
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&bytes);
    stream
        .write_all(&frame)
        .unwrap_or_else(|error| panic!("write wire request {request_id:?}: {error}"));
    stream
        .flush()
        .unwrap_or_else(|error| panic!("flush wire request {request_id:?}: {error}"));
    let mut prefix = [0_u8; 4];
    stream
        .read_exact(&mut prefix)
        .unwrap_or_else(|error| panic!("read wire response prefix for {request_id:?}: {error}"));
    let response_len = u32::from_be_bytes(prefix) as usize;
    let mut response = vec![0_u8; response_len];
    stream.read_exact(&mut response).unwrap_or_else(|error| {
        panic!("read {response_len}-byte wire response body for {request_id:?}: {error}")
    });
    serde_json::from_slice(&response)
        .unwrap_or_else(|error| panic!("decode wire response for {request_id:?}: {error}"))
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
