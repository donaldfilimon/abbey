use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use thiserror::Error;

use crate::app_core::{
    AppCommand, AppEvent, AppService, V3CapabilitySet, V3Command, V3ErrorCode, V3Event,
};

use super::config::DaemonConfig;
use super::protocol::{
    CURRENT_PROTOCOL_VERSION, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope,
    SUPPORTED_PROTOCOL_VERSIONS, V3RequestEnvelope, V3ResponseEnvelope,
};

const MAX_REQUEST_ID_LEN: usize = 128;

/// App-core adapter boundary. Only read-only operations can cross this trait.
pub trait ReadOnlyHandler: Send + Sync + 'static {
    fn handle(&self, command: AppCommand) -> Result<AppEvent, String>;
}

impl ReadOnlyHandler for AppService {
    fn handle(&self, command: AppCommand) -> Result<AppEvent, String> {
        AppService::handle(self, command).map_err(|error| error.to_string())
    }
}

/// Version-aware daemon application boundary with stable, non-sensitive errors.
pub trait DaemonHandler: Send + Sync + 'static {
    fn supports_version(&self, version: u16) -> bool;
    fn handle_versioned(
        &self,
        version: u16,
        command: AppCommand,
    ) -> Result<AppEvent, HandlerFailure>;

    /// Whether this handler owns any protocol-v3 authority.
    fn supports_v3(&self) -> bool {
        false
    }

    /// Prove an echoed grant set is a subset of startup-owned authority and
    /// contains the exact grant required by this non-negotiation command.
    fn authorizes_v3(&self, _grants: &V3CapabilitySet, _command: &V3Command) -> bool {
        false
    }

    /// Handle one already authenticated and structurally validated v3 command.
    fn handle_v3(&self, _command: V3Command) -> Result<V3Event, HandlerFailure> {
        Err(HandlerFailure::new(
            "capability_denied",
            "protocol-v3 authority is unavailable",
        ))
    }
}

impl<T: ReadOnlyHandler> DaemonHandler for T {
    fn supports_version(&self, version: u16) -> bool {
        version == PROTOCOL_VERSION
    }

    fn handle_versioned(
        &self,
        _version: u16,
        command: AppCommand,
    ) -> Result<AppEvent, HandlerFailure> {
        self.handle(command)
            .map_err(|_| HandlerFailure::new("handler_failed", "request handling failed"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlerFailure {
    code: &'static str,
    message: &'static str,
}

impl HandlerFailure {
    #[must_use]
    pub const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }

    #[must_use]
    pub const fn message(self) -> &'static str {
        self.message
    }
}

#[derive(Clone, Debug, Default)]
pub struct Shutdown(Arc<AtomicBool>);

impl Shutdown {
    pub fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct DaemonServer<H> {
    config: DaemonConfig,
    handler: H,
}

impl<H: DaemonHandler> DaemonServer<H> {
    pub fn new(config: DaemonConfig, handler: H) -> Self {
        Self { config, handler }
    }

    #[cfg(unix)]
    pub fn serve(self, shutdown: Shutdown) -> Result<(), ServerError> {
        unix::serve(self.config, self.handler, shutdown)
    }

    #[cfg(not(unix))]
    pub fn serve(self, _shutdown: Shutdown) -> Result<(), ServerError> {
        Err(ServerError::UnsupportedPlatform)
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error(
        "abbeyd local transport is not implemented on this platform; named-pipe support is required"
    )]
    UnsupportedPlatform,
    #[error("socket path has no parent directory: {0}")]
    MissingSocketParent(PathBuf),
    #[error("socket directory must be owned by the current user: {0}")]
    SocketDirectoryOwner(PathBuf),
    #[error("socket directory must not grant group or other permissions: {0}")]
    SocketDirectoryPermissions(PathBuf),
    #[error("socket directory must be a real directory, not a symlink: {0}")]
    SocketDirectoryType(PathBuf),
    #[error("socket path already exists or is not a stale Abbey-owned socket: {0}")]
    SocketPathConflict(PathBuf),
    #[error("daemon I/O failed during {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

fn dispatch_authenticated<H: DaemonHandler>(
    request: RequestEnvelope,
    handler: &H,
) -> ResponseEnvelope {
    if !valid_request_id(&request.request_id) {
        return ResponseEnvelope::error("", "invalid_request_id", "request_id is invalid");
    }
    if !SUPPORTED_PROTOCOL_VERSIONS.contains(&request.version)
        || !handler.supports_version(request.version)
    {
        return ResponseEnvelope::error_for(
            CURRENT_PROTOCOL_VERSION,
            request.request_id,
            "unsupported_version",
            "protocol version is unsupported",
        );
    }
    if request.command.minimum_protocol_version() > request.version {
        return ResponseEnvelope::error_for(
            request.version,
            request.request_id,
            "unsupported_command",
            "command is unavailable in this protocol version",
        );
    }
    if request.command.validate().is_err() {
        return ResponseEnvelope::error_for(
            request.version,
            request.request_id,
            "invalid_command",
            "command payload is invalid",
        );
    }

    let result = handler.handle_versioned(request.version, request.command);
    match result {
        Ok(event) => ResponseEnvelope::ok_for(request.version, request.request_id, event),
        Err(failure) => ResponseEnvelope::error_for(
            request.version,
            request.request_id,
            failure.code,
            failure.message,
        ),
    }
}

fn dispatch_authenticated_v3<H: DaemonHandler>(
    request: V3RequestEnvelope,
    handler: &H,
) -> V3ResponseEnvelope {
    if !valid_request_id(&request.request_id) {
        return V3ResponseEnvelope::error("", V3ErrorCode::InvalidCommand, "request_id is invalid");
    }
    if request.version != crate::app_core::APP_PROTOCOL_V3
        || request.schema_version != crate::app_core::APP_SCHEMA_V3
        || !handler.supports_v3()
    {
        return V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::UnsupportedVersion,
            "protocol version is unsupported",
        );
    }
    if request.grants.validate().is_err()
        || (matches!(request.command, V3Command::Negotiate(_))
            && !request.grants.as_slice().is_empty())
    {
        return V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::InvalidCommand,
            "grant declaration is invalid",
        );
    }
    if !request.grants.permits(&request.command) {
        return V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::CapabilityDenied,
            "command lacks its advertised capability grant",
        );
    }
    if !matches!(request.command, V3Command::Negotiate(_))
        && !handler.authorizes_v3(&request.grants, &request.command)
    {
        return V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::CapabilityDenied,
            "advertised grants exceed daemon authority",
        );
    }
    if request.command.validate().is_err() {
        return V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::InvalidCommand,
            "command payload is invalid",
        );
    }

    match handler.handle_v3(request.command) {
        Ok(event) if event.validate().is_ok() => V3ResponseEnvelope::ok(request.request_id, event),
        Ok(_) => V3ResponseEnvelope::error(
            request.request_id,
            V3ErrorCode::Internal,
            "handler returned an invalid event",
        ),
        Err(failure) => V3ResponseEnvelope::error(
            request.request_id,
            v3_error_code(failure.code),
            failure.message,
        ),
    }
}

fn v3_error_code(code: &str) -> V3ErrorCode {
    match code {
        "capability_denied" => V3ErrorCode::CapabilityDenied,
        "invalid_command" => V3ErrorCode::InvalidCommand,
        "not_found" => V3ErrorCode::NotFound,
        "conflict" => V3ErrorCode::Conflict,
        "cancelled" => V3ErrorCode::Cancelled,
        "deadline_exceeded" => V3ErrorCode::DeadlineExceeded,
        "budget_exceeded" => V3ErrorCode::BudgetExceeded,
        "response_too_large" => V3ErrorCode::ResponseTooLarge,
        _ => V3ErrorCode::Internal,
    }
}

fn valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_LEN
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

struct AuthenticatedRateLimiter {
    limit: super::config::AuthenticatedRateLimit,
    window_started: Instant,
    accepted: u16,
}

impl AuthenticatedRateLimiter {
    fn new(limit: super::config::AuthenticatedRateLimit) -> Self {
        Self {
            limit,
            window_started: Instant::now(),
            accepted: 0,
        }
    }

    fn admit(&mut self) -> bool {
        if self.window_started.elapsed() >= self.limit.window {
            self.window_started = Instant::now();
            self.accepted = 0;
        }
        if self.accepted >= self.limit.requests {
            return false;
        }
        self.accepted += 1;
        true
    }
}

#[cfg(unix)]
#[path = "server/unix.rs"]
mod unix;

#[cfg(all(test, unix))]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::app_core::{AppCommand, AppEvent, ClaimsQuery, RuntimeState};
    use crate::daemon::ResponsePayload;

    const TEST_BEARER: &str = "0123456789abcdef0123456789abcdef";

    struct Handler;

    impl ReadOnlyHandler for Handler {
        fn handle(&self, command: AppCommand) -> Result<AppEvent, String> {
            AppService::default()
                .handle(command)
                .map_err(|error| error.to_string())
        }
    }

    #[test]
    fn authenticated_request_round_trips_and_server_tears_down() {
        let harness = Harness::start();
        let response = harness.request(request(TEST_BEARER, PROTOCOL_VERSION));
        assert!(matches!(
            response.payload,
            ResponsePayload::Ok {
                event: AppEvent::Status(status)
            } if status.state == RuntimeState::Ready
        ));
        let path = harness.socket.clone();
        harness.stop();
        assert!(!path.exists(), "socket must be removed on clean teardown");
    }

    #[test]
    fn wrong_bearer_and_version_fail_closed() {
        let harness = Harness::start();
        let unauthorized = harness.request(request(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            PROTOCOL_VERSION,
        ));
        assert_error(&unauthorized, "unauthorized");
        let hidden_version = harness.request(request(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            CURRENT_PROTOCOL_VERSION + 99,
        ));
        assert_error(&hidden_version, "unauthorized");
        let incompatible = harness.request(request(TEST_BEARER, PROTOCOL_VERSION + 1));
        assert_error(&incompatible, "unsupported_version");
        harness.stop();
    }

    #[test]
    fn only_authenticated_requests_consume_the_bounded_rate_limit() {
        let harness = Harness::start_with_rate(1);
        for _ in 0..3 {
            let response = harness.request(request(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                PROTOCOL_VERSION,
            ));
            assert_error(&response, "unauthorized");
        }
        let admitted = harness.request(request(TEST_BEARER, PROTOCOL_VERSION));
        assert!(matches!(admitted.payload, ResponsePayload::Ok { .. }));
        let limited = harness.request(request(TEST_BEARER, CURRENT_PROTOCOL_VERSION));
        assert_error(&limited, "rate_limited");
        assert_eq!(limited.version, CURRENT_PROTOCOL_VERSION);
        harness.stop();
    }

    #[test]
    fn an_accepted_connection_waits_for_a_delayed_partial_frame() {
        let harness = Harness::start();
        let mut stream = UnixStream::connect(&harness.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let bytes = serde_json::to_vec(&request(TEST_BEARER, PROTOCOL_VERSION)).unwrap();
        let prefix = (bytes.len() as u32).to_be_bytes();
        stream.write_all(&prefix[..2]).unwrap();

        // `for_test` polls accept every 5 ms and gives each connection a
        // 300 ms read deadline. The partial prefix makes the accepted stream
        // enter `read_exact` before the frame is complete. Without restoring
        // blocking mode, the inherited nonblocking stream returns `WouldBlock`
        // here instead of honoring its bounded deadline.
        thread::sleep(Duration::from_millis(100));

        let mut frame = Vec::with_capacity(2 + bytes.len());
        frame.extend_from_slice(&prefix[2..]);
        frame.extend_from_slice(&bytes);
        stream.write_all(&frame).unwrap();

        let response = read_response(&mut stream);
        assert!(matches!(response.payload, ResponsePayload::Ok { .. }));
        harness.stop();
    }

    #[test]
    fn malformed_and_oversize_frames_are_rejected() {
        let harness = Harness::start();
        let malformed = harness.raw_frame(b"not-json", Some(8));
        assert_error(&malformed, "malformed_request");
        let oversize = harness.raw_frame(&[], Some((64 * 1024 + 1) as u32));
        assert_error(&oversize, "frame_too_large");
        harness.stop();
    }

    #[test]
    fn reflected_request_ids_use_a_bounded_ascii_grammar() {
        let harness = Harness::start();
        for request_id in ["has space", "line\nbreak", "unicode-λ"] {
            let response = harness.request(RequestEnvelope {
                version: PROTOCOL_VERSION,
                request_id: request_id.into(),
                bearer: TEST_BEARER.into(),
                command: AppCommand::Status,
            });
            assert_error(&response, "invalid_request_id");
            assert!(response.request_id.is_empty());
        }
        let response = harness.request(RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: "x".repeat(MAX_REQUEST_ID_LEN + 1),
            bearer: TEST_BEARER.into(),
            command: AppCommand::Status,
        });
        assert_error(&response, "invalid_request_id");
        harness.stop();
    }

    #[test]
    fn oversized_v2_response_fallback_retains_requested_version() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let response = ResponseEnvelope::error_for(
            CURRENT_PROTOCOL_VERSION,
            "v2-request",
            "large",
            "x".repeat(2_048),
        );
        unix::write_wire_response(&mut writer, unix::WireResponse::Legacy(response), 512).unwrap();
        let response = read_response(&mut reader);
        assert_eq!(response.version, CURRENT_PROTOCOL_VERSION);
        assert_error(&response, "response_too_large");
    }

    #[test]
    fn insecure_socket_directory_is_rejected() {
        let root = scratch_dir("insecure");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = DaemonConfig::for_test(root.join("abbeyd.sock"), TEST_BEARER.as_bytes());
        let result = DaemonServer::new(config, Handler).serve(Shutdown::default());
        assert!(matches!(
            result,
            Err(ServerError::SocketDirectoryPermissions(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_error(response: &ResponseEnvelope, expected: &str) {
        match &response.payload {
            ResponsePayload::Error { code, .. } => assert_eq!(code, expected),
            ResponsePayload::Ok { .. } => panic!("expected error response"),
        }
    }

    fn request(bearer: &str, version: u16) -> RequestEnvelope {
        RequestEnvelope {
            version,
            request_id: "test-request".into(),
            bearer: bearer.into(),
            command: AppCommand::Status,
        }
    }

    #[test]
    fn claims_query_uses_app_core_validation() {
        let harness = Harness::start();
        let response = harness.request(RequestEnvelope {
            version: PROTOCOL_VERSION,
            request_id: "claims-request".into(),
            bearer: TEST_BEARER.into(),
            command: AppCommand::Claims(ClaimsQuery {
                status: None,
                contains: Some("\n".into()),
            }),
        });
        assert_error(&response, "invalid_command");
        harness.stop();
    }

    struct Harness {
        root: PathBuf,
        socket: PathBuf,
        shutdown: Shutdown,
        thread: thread::JoinHandle<Result<(), ServerError>>,
    }

    impl Harness {
        fn start() -> Self {
            Self::start_with_rate(64)
        }

        fn start_with_rate(requests: u16) -> Self {
            let root = scratch_dir("server");
            let socket = root.join("abbeyd.sock");
            let mut config = DaemonConfig::for_test(socket.clone(), TEST_BEARER.as_bytes());
            config.authenticated_rate_limit =
                crate::daemon::AuthenticatedRateLimit::new(requests, Duration::from_secs(60))
                    .unwrap();
            let shutdown = Shutdown::default();
            let server_shutdown = shutdown.clone();
            let thread =
                thread::spawn(move || DaemonServer::new(config, Handler).serve(server_shutdown));
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if let Ok(metadata) = std::fs::metadata(&socket) {
                    let mode = metadata.permissions().mode();
                    if metadata.file_type().is_socket() && mode & 0o077 == 0 {
                        break;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "daemon socket was not created with owner-only permissions"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Self {
                root,
                socket,
                shutdown,
                thread,
            }
        }

        fn request(&self, request: RequestEnvelope) -> ResponseEnvelope {
            let bytes = serde_json::to_vec(&request).unwrap();
            self.raw_frame(&bytes, None)
        }

        fn raw_frame(&self, bytes: &[u8], declared: Option<u32>) -> ResponseEnvelope {
            let mut stream = UnixStream::connect(&self.socket).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut frame = Vec::with_capacity(4 + bytes.len());
            frame.extend_from_slice(&declared.unwrap_or(bytes.len() as u32).to_be_bytes());
            frame.extend_from_slice(bytes);
            stream.write_all(&frame).unwrap();
            read_response(&mut stream)
        }

        fn stop(self) {
            self.shutdown.request();
            self.thread.join().unwrap().unwrap();
            std::fs::remove_dir_all(self.root).unwrap();
        }
    }

    fn read_response(stream: &mut UnixStream) -> ResponseEnvelope {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut bytes = vec![0_u8; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut bytes).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn scratch_dir(label: &str) -> PathBuf {
        // Darwin's sockaddr_un path is short; the usual per-user temporary
        // directory can consume most of it before the test's own name begins.
        let path = PathBuf::from("/tmp").join(format!(
            "abd-{label}-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
