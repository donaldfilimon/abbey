use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use thiserror::Error;

use crate::app_core::{AppCommand, AppEvent, AppService};

use super::config::DaemonConfig;
use super::protocol::{
    CURRENT_PROTOCOL_VERSION, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope,
    SUPPORTED_PROTOCOL_VERSIONS,
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
mod unix {
    use std::fs;
    use std::io::{self, Read as _, Write as _};
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;

    use super::*;

    pub(super) fn serve<H: DaemonHandler>(
        config: DaemonConfig,
        handler: H,
        shutdown: Shutdown,
    ) -> Result<(), ServerError> {
        prepare_private_directory(&config.socket_path)?;
        remove_stale_socket(&config.socket_path)?;

        let listener =
            UnixListener::bind(&config.socket_path).map_err(|source| ServerError::Io {
                operation: "bind socket",
                path: config.socket_path.clone(),
                source,
            })?;
        let _socket_guard = SocketGuard(config.socket_path.clone());
        fs::set_permissions(&config.socket_path, fs::Permissions::from_mode(0o600)).map_err(
            |source| ServerError::Io {
                operation: "set socket permissions",
                path: config.socket_path.clone(),
                source,
            },
        )?;
        listener
            .set_nonblocking(true)
            .map_err(|source| ServerError::Io {
                operation: "configure listener",
                path: config.socket_path.clone(),
                source,
            })?;

        let mut limiter = AuthenticatedRateLimiter::new(config.authenticated_rate_limit);

        // Exactly one request is handled at a time. This intentionally creates
        // no user-space connection queue; per-connection deadlines bound idle
        // occupancy until a dedicated concurrency policy is introduced.
        while !shutdown.requested() {
            match listener.accept() {
                Ok((stream, _address)) => {
                    handle_connection(stream, &config, &handler, &mut limiter);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(config.accept_poll_interval);
                }
                Err(source) => {
                    return Err(ServerError::Io {
                        operation: "accept connection",
                        path: config.socket_path.clone(),
                        source,
                    });
                }
            }
        }
        Ok(())
    }

    fn handle_connection<H: DaemonHandler>(
        mut stream: UnixStream,
        config: &DaemonConfig,
        handler: &H,
        limiter: &mut AuthenticatedRateLimiter,
    ) {
        if stream.set_read_timeout(Some(config.read_timeout)).is_err()
            || stream
                .set_write_timeout(Some(config.write_timeout))
                .is_err()
        {
            return;
        }

        let response = match read_frame(&mut stream, config.max_frame_len) {
            Ok(bytes) => match authenticate_frame(&bytes, config) {
                FrameAuthentication::Authenticated {
                    response_version,
                    request_id,
                } => {
                    if !limiter.admit() {
                        ResponseEnvelope::error_for(
                            response_version,
                            request_id,
                            "rate_limited",
                            "authenticated request rate limit exceeded",
                        )
                    } else {
                        match serde_json::from_slice::<RequestEnvelope>(&bytes) {
                            Ok(request) => dispatch_authenticated(request, handler),
                            Err(_) => ResponseEnvelope::error(
                                "",
                                "malformed_request",
                                "request is not valid JSON",
                            ),
                        }
                    }
                }
                FrameAuthentication::Unauthorized {
                    response_version,
                    request_id,
                } => ResponseEnvelope::error_for(
                    response_version,
                    request_id,
                    "unauthorized",
                    "authentication failed",
                ),
                FrameAuthentication::Malformed => {
                    ResponseEnvelope::error("", "malformed_request", "request is not valid JSON")
                }
            },
            Err(FrameError::Oversize) => {
                ResponseEnvelope::error("", "frame_too_large", "frame exceeds configured limit")
            }
            Err(FrameError::Empty) => {
                ResponseEnvelope::error("", "malformed_request", "frame must not be empty")
            }
            Err(FrameError::Io) => return,
        };
        let _ = write_response(&mut stream, response, config.max_frame_len);
    }

    enum FrameAuthentication {
        Authenticated {
            response_version: u16,
            request_id: String,
        },
        Unauthorized {
            response_version: u16,
            request_id: String,
        },
        Malformed,
    }

    fn authenticate_frame(bytes: &[u8], config: &DaemonConfig) -> FrameAuthentication {
        let value = match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(value) => value,
            Err(_) => return FrameAuthentication::Malformed,
        };
        let candidate = value
            .as_object()
            .and_then(|object| object.get("bearer"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let response_version = value
            .as_object()
            .and_then(|object| object.get("version"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u16::try_from(version).ok())
            .unwrap_or(PROTOCOL_VERSION);
        let request_id = value
            .as_object()
            .and_then(|object| object.get("request_id"))
            .and_then(serde_json::Value::as_str)
            .filter(|request_id| valid_request_id(request_id))
            .unwrap_or_default()
            .to_owned();
        if config.bearer.matches(candidate.as_bytes()) {
            let response_version = Some(response_version)
                .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
                .unwrap_or(CURRENT_PROTOCOL_VERSION);
            FrameAuthentication::Authenticated {
                response_version,
                request_id,
            }
        } else {
            // Echoing the caller's envelope version lets either compatible
            // client decode the generic denial without disclosing which
            // versions or capabilities the daemon actually supports.
            FrameAuthentication::Unauthorized {
                response_version,
                request_id,
            }
        }
    }

    fn read_frame(stream: &mut UnixStream, max: usize) -> Result<Vec<u8>, FrameError> {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).map_err(|_| FrameError::Io)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 {
            return Err(FrameError::Empty);
        }
        if length > max {
            return Err(FrameError::Oversize);
        }
        let mut bytes = vec![0_u8; length];
        stream.read_exact(&mut bytes).map_err(|_| FrameError::Io)?;
        Ok(bytes)
    }

    pub(super) fn write_response(
        stream: &mut UnixStream,
        response: ResponseEnvelope,
        max: usize,
    ) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;
        if bytes.len() > max {
            bytes = serde_json::to_vec(&ResponseEnvelope::error_for(
                response.version,
                response.request_id,
                "response_too_large",
                "handler response exceeds configured limit",
            ))
            .map_err(io::Error::other)?;
        }
        let length = u32::try_from(bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "response too large"))?;
        stream.write_all(&length.to_be_bytes())?;
        stream.write_all(&bytes)?;
        stream.flush()
    }

    fn prepare_private_directory(socket_path: &Path) -> Result<(), ServerError> {
        let parent = socket_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| ServerError::MissingSocketParent(socket_path.to_owned()))?;
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|source| ServerError::Io {
                operation: "create socket directory",
                path: parent.to_owned(),
                source,
            })?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
                ServerError::Io {
                    operation: "set socket directory permissions",
                    path: parent.to_owned(),
                    source,
                }
            })?;
        }
        let metadata = fs::symlink_metadata(parent).map_err(|source| ServerError::Io {
            operation: "inspect socket directory",
            path: parent.to_owned(),
            source,
        })?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(ServerError::SocketDirectoryType(parent.to_owned()));
        }
        if metadata.uid() != nix::unistd::Uid::effective().as_raw() {
            return Err(ServerError::SocketDirectoryOwner(parent.to_owned()));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(ServerError::SocketDirectoryPermissions(parent.to_owned()));
        }
        Ok(())
    }

    fn remove_stale_socket(path: &Path) -> Result<(), ServerError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(ServerError::Io {
                    operation: "inspect socket path",
                    path: path.to_owned(),
                    source,
                });
            }
        };
        if !metadata.file_type().is_socket()
            || metadata.uid() != nix::unistd::Uid::effective().as_raw()
            || UnixStream::connect(path).is_ok()
        {
            return Err(ServerError::SocketPathConflict(path.to_owned()));
        }
        fs::remove_file(path).map_err(|source| ServerError::Io {
            operation: "remove stale socket",
            path: path.to_owned(),
            source,
        })
    }

    struct SocketGuard(PathBuf);

    impl Drop for SocketGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    enum FrameError {
        Empty,
        Oversize,
        Io,
    }
}

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
        unix::write_response(&mut writer, response, 512).unwrap();
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
