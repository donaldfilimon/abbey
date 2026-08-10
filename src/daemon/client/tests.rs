//! Real-socket client tests, split out of `client.rs` to keep that file
//! under the gate's hard 1000-line ceiling.

use std::io::{Read as _, Write as _};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::app_core::{
    AppContext, AppEvent, AppService, ApprovalKind, ApprovalRequest, BackendSelection, ClaimsQuery,
    ClaimsSnapshot, ConversationId, IdempotencyKey, RunEventPage, RunEventRecord, RunId,
    RunLifecycleEvent, RunMode, RunRequest, RunRouteCapability, RunSnapshot, RunState,
    RunSubmission, RunSubmissionDisposition, RuntimeState,
};
use crate::daemon::{DaemonServer, Shutdown};

const TEST_BEARER: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn real_scratch_server_round_trip() {
    let harness = RealServer::start();
    let event = DaemonClient::new(harness.config.clone())
        .request(AppCommand::Status)
        .unwrap();
    assert!(matches!(
        event,
        AppEvent::Status(status) if status.state == RuntimeState::Ready
    ));
    harness.stop();
}

#[test]
fn peer_close_during_connection_handoff_is_one_stable_bounded_error() {
    let root = scratch_dir("handoff-close");
    let mut config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    config.client_handoff_barrier = Some(barrier.clone());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream);
        // Release the client only after the accepted peer is closed. Darwin
        // may fail timeout configuration with EINVAL; other Unix hosts may
        // reach the first write and report EPIPE. Neither platform detail is
        // part of the client contract.
        barrier.wait();
    });

    let started = Instant::now();
    let error = DaemonClient::new(config.clone())
        .request(AppCommand::Status)
        .unwrap_err();
    assert!(matches!(error, ClientError::ConnectionHandoff));
    assert_eq!(
        error.to_string(),
        "abbeyd connection closed before request handoff completed"
    );
    assert!(started.elapsed() < config.read_timeout + Duration::from_secs(1));
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn read_only_request_downgrades_once_but_mutation_never_replays() {
    let root = scratch_dir("downgrade");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let first_request = read_request(&mut first);
        assert_eq!(first_request.version, CURRENT_PROTOCOL_VERSION);
        let unsupported = ResponseEnvelope::error_for(
            PROTOCOL_VERSION,
            first_request.request_id,
            "unsupported_version",
            "protocol version is unsupported",
        );
        first.write_all(&encoded_response(unsupported)).unwrap();

        let (mut second, _) = listener.accept().unwrap();
        let second_request = read_request(&mut second);
        assert_eq!(second_request.version, PROTOCOL_VERSION);
        let event = AppService::default()
            .handle(second_request.command)
            .unwrap();
        second
            .write_all(&encoded_response(ResponseEnvelope::ok_for(
                PROTOCOL_VERSION,
                second_request.request_id,
                event,
            )))
            .unwrap();
    });
    let event = DaemonClient::new(config)
        .request(AppCommand::Status)
        .unwrap();
    assert!(
        matches!(event, AppEvent::Status(status) if status.protocol_version == PROTOCOL_VERSION)
    );
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        assert_eq!(request.version, CURRENT_PROTOCOL_VERSION);
        encoded_response(ResponseEnvelope::error_for(
            PROTOCOL_VERSION,
            request.request_id,
            "unsupported_version",
            "protocol version is unsupported",
        ))
    });
    let request = RunRequest {
        idempotency_key: "mutation-no-replay".parse::<IdempotencyKey>().unwrap(),
        conversation_id: None,
        mode: RunMode::OneShot,
        backend: BackendSelection::Abi,
        input: "bounded request".into(),
        labels: Vec::new(),
    };
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::SubmitRun(request)),
        Err(ClientError::Daemon { code, .. }) if code == "unsupported_version"
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_cross_kind_and_approval_events() {
    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Ok {
                event: AppEvent::Claims(ClaimsSnapshot {
                    claims: Vec::new(),
                    matched: 0,
                }),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::UnexpectedEvent { .. })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Ok {
                event: AppService::default().handle(AppCommand::Status).unwrap(),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Claims(ClaimsQuery::default())),
        Err(ClientError::UnexpectedEvent { .. })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Ok {
                event: AppEvent::ApprovalRequested(ApprovalRequest {
                    run_id: RunId::new(),
                    kind: ApprovalKind::ProcessExecution,
                    summary: "must remain outside the read-only client".into(),
                }),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::UnexpectedEvent { .. })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validates_status_and_claims_invariants() {
    let (config, thread, root) = fake_server(|request| {
        let mut status = v2_status();
        status.schema_version += 1;
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Ok {
                event: AppEvent::Status(status),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::InvalidRuntimeStatus(_))
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        let response = ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Ok {
                event: AppEvent::Status(v2_status()),
            },
        };
        let mut value = serde_json::to_value(response).unwrap();
        value["payload"]["event"]["payload"]["capabilities"]["capabilities"] =
            serde_json::json!(["read_status"]);
        framed(&serde_json::to_vec(&value).unwrap())
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::InvalidRuntimeStatus(_))
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Ok {
                event: AppEvent::Claims(ClaimsSnapshot {
                    claims: Vec::new(),
                    matched: 1,
                }),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Claims(ClaimsQuery::default())),
        Err(ClientError::InvalidClaimsSnapshot)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_mismatched_protocol_version_and_request_id() {
    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version + 1,
            request_id: request.request_id,
            payload: ResponsePayload::Error {
                code: "ignored".into(),
                message: "ignored".into(),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::ProtocolMismatch { .. })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: "different-request".into(),
            payload: ResponsePayload::Error {
                code: "ignored".into(),
                message: "ignored".into(),
            },
        })
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::RequestIdMismatch)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|_request| {
        encoded_response(ResponseEnvelope::error_for(
            PROTOCOL_VERSION,
            "different-request",
            "unsupported_version",
            "protocol version is unsupported",
        ))
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::RequestIdMismatch)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn run_responses_must_match_submission_identity_and_requested_page_limit() {
    let submitted = RunRequest {
        idempotency_key: "expected-submission".parse().unwrap(),
        conversation_id: None,
        mode: RunMode::OneShot,
        backend: BackendSelection::Abi,
        input: "bounded request".into(),
        labels: Vec::new(),
    };
    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope::ok_for(
            request.version,
            request.request_id,
            AppEvent::RunSubmitted(RunSubmission {
                disposition: RunSubmissionDisposition::Enqueued,
                run: run_snapshot("different-submission", None),
            }),
        ))
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::SubmitRun(submitted)),
        Err(ClientError::InvalidRunResponse)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let expected_conversation = ConversationId::new();
    let different_conversation = ConversationId::new();
    let submitted = RunRequest {
        idempotency_key: "conversation-bound".parse().unwrap(),
        conversation_id: Some(expected_conversation),
        mode: RunMode::OneShot,
        backend: BackendSelection::Abi,
        input: "bounded request".into(),
        labels: Vec::new(),
    };
    let (config, thread, root) = fake_server(move |request| {
        encoded_response(ResponseEnvelope::ok_for(
            request.version,
            request.request_id,
            AppEvent::RunSubmitted(RunSubmission {
                disposition: RunSubmissionDisposition::Enqueued,
                run: run_snapshot("conversation-bound", Some(different_conversation)),
            }),
        ))
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::SubmitRun(submitted)),
        Err(ClientError::InvalidRunResponse)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let run_id = RunId::new();
    let response_id = run_id.clone();
    let (config, thread, root) = fake_server(move |request| {
        let events = [RunLifecycleEvent::Queued, RunLifecycleEvent::Starting]
            .into_iter()
            .enumerate()
            .map(|(offset, event)| RunEventRecord {
                run_id: response_id.clone(),
                sequence: u64::try_from(offset + 1).unwrap(),
                recorded_at: "2026-08-08T00:00:00Z".into(),
                event,
            })
            .collect();
        encoded_response(ResponseEnvelope::ok_for(
            request.version,
            request.request_id,
            AppEvent::RunEvents(RunEventPage {
                run_id: response_id,
                events,
                after_sequence: 0,
                next_after_sequence: 2,
                through_sequence: 2,
                has_more: false,
            }),
        ))
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::RunEvents(crate::app_core::RunEventsQuery {
            run_id,
            after_sequence: 0,
            through_sequence: None,
            limit: 1,
        })),
        Err(ClientError::InvalidRunResponse)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_oversized_truncated_and_malformed_responses() {
    let (config, thread, root) =
        fake_server(|_request| ((64 * 1024 + 1) as u32).to_be_bytes().to_vec());
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::ResponseTooLarge)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|_request| {
        let mut bytes = 12_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"short");
        bytes
    });
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::Read(_))
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|_request| framed(b"not-json"));
    assert!(matches!(
        DaemonClient::new(config).request(AppCommand::Status),
        Err(ClientError::MalformedResponse)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unreachable_socket_is_bounded_connect_error() {
    let root = scratch_dir("unreachable");
    let config = test_config(root.join("missing.sock"));
    let before = Instant::now();
    assert!(matches!(
        DaemonClient::new(config.clone()).request(AppCommand::Status),
        Err(ClientError::Connect { .. } | ClientError::ConnectTimeout { .. })
    ));
    assert!(before.elapsed() < config.read_timeout + Duration::from_secs(1));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn debug_and_errors_do_not_reveal_bearer() {
    let root = scratch_dir("redaction");
    let config = test_config(root.join("missing.sock"));
    let client = DaemonClient::new(config);
    assert!(!format!("{client:?}").contains(TEST_BEARER));
    let error = client.request(AppCommand::Status).unwrap_err();
    assert!(!format!("{error:?}").contains(TEST_BEARER));
    assert!(!error.to_string().contains(TEST_BEARER));
    std::fs::remove_dir_all(root).unwrap();

    let (config, thread, root) = fake_server(|request| {
        encoded_response(ResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            payload: ResponsePayload::Error {
                code: format!("denied-{TEST_BEARER}"),
                message: format!("peer echoed {TEST_BEARER}"),
            },
        })
    });
    let error = DaemonClient::new(config)
        .request(AppCommand::Status)
        .unwrap_err();
    assert!(matches!(&error, ClientError::Daemon { .. }));
    assert!(!format!("{error:?}").contains(TEST_BEARER));
    assert!(!error.to_string().contains(TEST_BEARER));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

fn v2_status() -> RuntimeStatus {
    let route = RunRouteCapability {
        backend: BackendSelection::Abi,
        modes: vec![RunMode::OneShot, RunMode::Background],
    };
    AppContext::runtime_v2(vec![route])
        .unwrap()
        .status()
        .clone()
}

fn run_snapshot(key: &str, conversation_id: Option<ConversationId>) -> RunSnapshot {
    RunSnapshot {
        run_id: RunId::new(),
        conversation_id,
        idempotency_key: key.parse().unwrap(),
        state: RunState::Queued,
        created_at: "2026-08-08T00:00:00Z".into(),
        updated_at: "2026-08-08T00:00:00Z".into(),
        failure: None,
        event_count: 1,
    }
}

struct RealServer {
    root: PathBuf,
    config: DaemonConfig,
    shutdown: Shutdown,
    thread: thread::JoinHandle<Result<(), crate::daemon::ServerError>>,
}

impl RealServer {
    fn start() -> Self {
        let root = scratch_dir("real");
        let mut config = test_config(root.join("abbeyd.sock"));
        config.read_timeout = Duration::from_secs(2);
        config.write_timeout = Duration::from_secs(2);
        let shutdown = Shutdown::default();
        let server_shutdown = shutdown.clone();
        let server_config = config.clone();
        let thread = thread::spawn(move || {
            DaemonServer::new(server_config, AppService::default()).serve(server_shutdown)
        });
        wait_for_socket(&config.socket_path);
        Self {
            root,
            config,
            shutdown,
            thread,
        }
    }

    fn stop(self) {
        self.shutdown.request();
        self.thread.join().unwrap().unwrap();
        std::fs::remove_dir_all(self.root).unwrap();
    }
}

fn fake_server<F>(responder: F) -> (DaemonConfig, thread::JoinHandle<()>, PathBuf)
where
    F: FnOnce(RequestEnvelope) -> Vec<u8> + Send + 'static,
{
    let root = scratch_dir("fake");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        stream.write_all(&responder(request)).unwrap();
        stream.flush().unwrap();
    });
    (config, thread, root)
}

fn read_request(stream: &mut UnixStream) -> RequestEnvelope {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).unwrap();
    let mut bytes = vec![0_u8; u32::from_be_bytes(prefix) as usize];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn encoded_response(response: ResponseEnvelope) -> Vec<u8> {
    framed(&serde_json::to_vec(&response).unwrap())
}

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

fn test_config(socket_path: PathBuf) -> DaemonConfig {
    DaemonConfig::for_test(socket_path, TEST_BEARER.as_bytes())
}

fn wait_for_socket(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(metadata) = std::fs::metadata(path) {
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

fn scratch_dir(label: &str) -> PathBuf {
    let root = PathBuf::from("/tmp").join(format!(
        "adc-{label}-{}-{}",
        std::process::id(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    root
}
