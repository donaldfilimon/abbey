//! Bounded client for Abbey's authenticated local daemon transport.

use std::fmt;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::app_core::{
    APP_PROTOCOL_VERSION, APP_SCHEMA_VERSION, AppCommand, AppEvent, CapabilitySet, ClaimsSnapshot,
    RuntimeStatus,
};

use super::{DaemonConfig, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, ResponsePayload};

/// Object-oriented client for one configured Abbey daemon installation.
#[derive(Clone)]
pub struct DaemonClient {
    config: DaemonConfig,
}

impl DaemonClient {
    #[must_use]
    pub fn new(config: DaemonConfig) -> Self {
        Self { config }
    }

    /// Send one typed, read-only application command.
    #[cfg(unix)]
    pub fn request(&self, command: AppCommand) -> Result<AppEvent, ClientError> {
        unix::request(&self.config, command)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn request(&self, _command: AppCommand) -> Result<AppEvent, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }
}

impl fmt::Debug for DaemonClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DaemonClient")
            .field("socket_path", &self.config.socket_path)
            .field("bearer", &"[REDACTED]")
            .field("max_frame_len", &self.config.max_frame_len)
            .field("read_timeout", &self.config.read_timeout)
            .field("write_timeout", &self.config.write_timeout)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(
        "abbeyd client transport is not implemented on this platform; named-pipe support is required"
    )]
    UnsupportedPlatform,
    #[error("timed out connecting to abbeyd at {path}")]
    ConnectTimeout { path: PathBuf },
    #[error("cannot connect to abbeyd at {path}: {source}")]
    Connect { path: PathBuf, source: io::Error },
    #[error("cannot configure abbeyd socket: {0}")]
    Configure(io::Error),
    #[error("cannot serialize abbeyd request: {0}")]
    Serialize(serde_json::Error),
    #[error("abbeyd request exceeds the configured frame limit")]
    RequestTooLarge,
    #[error("cannot write abbeyd request: {0}")]
    Write(io::Error),
    #[error("cannot read abbeyd response: {0}")]
    Read(io::Error),
    #[error("abbeyd returned an empty frame")]
    EmptyResponse,
    #[error("abbeyd response exceeds the configured frame limit")]
    ResponseTooLarge,
    #[error("abbeyd returned malformed response JSON")]
    MalformedResponse,
    #[error("abbeyd protocol mismatch: expected {expected}, received {received}")]
    ProtocolMismatch { expected: u16, received: u16 },
    #[error("abbeyd response request_id does not match the request")]
    RequestIdMismatch,
    #[error("abbeyd returned {received} for a {expected} request")]
    UnexpectedEvent {
        expected: &'static str,
        received: &'static str,
    },
    #[error("abbeyd returned invalid runtime status: {0}")]
    InvalidRuntimeStatus(&'static str),
    #[error("abbeyd returned an inconsistent claims snapshot")]
    InvalidClaimsSnapshot,
    #[error("abbeyd rejected the request ({code}): {message}")]
    Daemon { code: String, message: String },
    #[error("abbeyd connection worker stopped unexpectedly")]
    ConnectWorkerStopped,
}

#[cfg(unix)]
mod unix {
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    pub(super) fn request(
        config: &DaemonConfig,
        command: AppCommand,
    ) -> Result<AppEvent, ClientError> {
        let expected_event = ExpectedEvent::for_command(&command);
        let request_id = uuid::Uuid::new_v4().to_string();
        let bearer = config.bearer.as_str().to_owned();
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            bearer,
            command,
        };
        let bytes = serde_json::to_vec(&request).map_err(ClientError::Serialize)?;
        if bytes.is_empty() || bytes.len() > config.max_frame_len {
            return Err(ClientError::RequestTooLarge);
        }

        let mut stream = connect(config)?;
        stream
            .set_read_timeout(Some(config.read_timeout))
            .map_err(ClientError::Configure)?;
        stream
            .set_write_timeout(Some(config.write_timeout))
            .map_err(ClientError::Configure)?;

        let length = u32::try_from(bytes.len()).map_err(|_| ClientError::RequestTooLarge)?;
        stream
            .write_all(&length.to_be_bytes())
            .and_then(|()| stream.write_all(&bytes))
            .and_then(|()| stream.flush())
            .map_err(ClientError::Write)?;

        let response = read_response(&mut stream, config.max_frame_len)?;
        if response.version != PROTOCOL_VERSION {
            return Err(ClientError::ProtocolMismatch {
                expected: PROTOCOL_VERSION,
                received: response.version,
            });
        }
        if response.request_id != request_id {
            return Err(ClientError::RequestIdMismatch);
        }
        match response.payload {
            ResponsePayload::Ok { event } => validate_event(expected_event, event),
            ResponsePayload::Error { code, message } => Err(ClientError::Daemon {
                code: redact_bearer(code, config.bearer.as_str()),
                message: redact_bearer(message, config.bearer.as_str()),
            }),
        }
    }

    #[derive(Clone, Copy)]
    enum ExpectedEvent {
        Status,
        Claims,
    }

    impl ExpectedEvent {
        fn for_command(command: &AppCommand) -> Self {
            match command {
                AppCommand::Status => Self::Status,
                AppCommand::Claims(_) => Self::Claims,
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Status => "status event",
                Self::Claims => "claims event",
            }
        }
    }

    fn validate_event(expected: ExpectedEvent, event: AppEvent) -> Result<AppEvent, ClientError> {
        match (expected, &event) {
            (ExpectedEvent::Status, AppEvent::Status(status)) => validate_status(status)?,
            (ExpectedEvent::Claims, AppEvent::Claims(snapshot)) => validate_claims(snapshot)?,
            (_, received) => {
                return Err(ClientError::UnexpectedEvent {
                    expected: expected.name(),
                    received: event_name(received),
                });
            }
        }
        Ok(event)
    }

    fn validate_status(status: &RuntimeStatus) -> Result<(), ClientError> {
        if status.protocol_version != APP_PROTOCOL_VERSION {
            return Err(ClientError::InvalidRuntimeStatus(
                "application protocol version does not match",
            ));
        }
        if status.schema_version != APP_SCHEMA_VERSION {
            return Err(ClientError::InvalidRuntimeStatus(
                "application schema version does not match",
            ));
        }
        status
            .capabilities
            .validate()
            .map_err(|_| ClientError::InvalidRuntimeStatus("capability set is invalid"))?;
        if status.capabilities != CapabilitySet::standard() {
            return Err(ClientError::InvalidRuntimeStatus(
                "read-only capability set is not supported",
            ));
        }
        Ok(())
    }

    fn validate_claims(snapshot: &ClaimsSnapshot) -> Result<(), ClientError> {
        if snapshot.matched != snapshot.claims.len() {
            return Err(ClientError::InvalidClaimsSnapshot);
        }
        Ok(())
    }

    fn event_name(event: &AppEvent) -> &'static str {
        match event {
            AppEvent::Status(_) => "status event",
            AppEvent::Claims(_) => "claims event",
            AppEvent::ApprovalRequested(_) => "approval request",
        }
    }

    fn redact_bearer(value: String, bearer: &str) -> String {
        value.replace(bearer, "[REDACTED]")
    }

    fn connect(config: &DaemonConfig) -> Result<UnixStream, ClientError> {
        let path = config.socket_path.clone();
        let worker_path = path.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        // `recv_timeout` strictly bounds the caller. A timed-out worker owns
        // only the public socket path and exits when the kernel connect call
        // returns; it holds neither the client nor its bearer. Local UDS
        // missing-socket behavior is covered by the bounded-connect test.
        thread::spawn(move || {
            let _ = sender.send(UnixStream::connect(worker_path));
        });
        match receiver.recv_timeout(config.read_timeout) {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(source)) => Err(ClientError::Connect { path, source }),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ClientError::ConnectTimeout { path }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ClientError::ConnectWorkerStopped),
        }
    }

    fn read_response(
        stream: &mut UnixStream,
        max_frame_len: usize,
    ) -> Result<ResponseEnvelope, ClientError> {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).map_err(ClientError::Read)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 {
            return Err(ClientError::EmptyResponse);
        }
        if length > max_frame_len {
            return Err(ClientError::ResponseTooLarge);
        }
        let mut bytes = vec![0_u8; length];
        stream.read_exact(&mut bytes).map_err(ClientError::Read)?;
        serde_json::from_slice(&bytes).map_err(|_| ClientError::MalformedResponse)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::app_core::{
        AppEvent, AppService, ApprovalKind, ApprovalRequest, ClaimsQuery, ClaimsSnapshot, RunId,
        RuntimeState,
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
    fn rejects_cross_kind_and_approval_events() {
        let (config, thread, root) = fake_server(|request| {
            encoded_response(ResponseEnvelope {
                version: PROTOCOL_VERSION,
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
                version: PROTOCOL_VERSION,
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
                version: PROTOCOL_VERSION,
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
            let AppEvent::Status(mut status) =
                AppService::default().handle(AppCommand::Status).unwrap()
            else {
                unreachable!();
            };
            status.schema_version += 1;
            encoded_response(ResponseEnvelope {
                version: PROTOCOL_VERSION,
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
                version: PROTOCOL_VERSION,
                request_id: request.request_id,
                payload: ResponsePayload::Ok {
                    event: AppService::default().handle(AppCommand::Status).unwrap(),
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
                version: PROTOCOL_VERSION,
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
                version: PROTOCOL_VERSION + 1,
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

        let (config, thread, root) = fake_server(|_request| {
            encoded_response(ResponseEnvelope {
                version: PROTOCOL_VERSION,
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
                version: PROTOCOL_VERSION,
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

    struct RealServer {
        root: PathBuf,
        config: DaemonConfig,
        shutdown: Shutdown,
        thread: thread::JoinHandle<Result<(), crate::daemon::ServerError>>,
    }

    impl RealServer {
        fn start() -> Self {
            let root = scratch_dir("real");
            let config = test_config(root.join("abbeyd.sock"));
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
        while !path.exists() {
            assert!(Instant::now() < deadline, "daemon socket was not created");
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
}
