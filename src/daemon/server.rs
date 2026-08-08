use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;

use crate::app_core::{AppCommand, AppEvent, AppService};

use super::config::DaemonConfig;
use super::protocol::{PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope};

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

impl<H: ReadOnlyHandler> DaemonServer<H> {
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

fn dispatch<H: ReadOnlyHandler>(
    request: RequestEnvelope,
    config: &DaemonConfig,
    handler: &H,
) -> ResponseEnvelope {
    if request.request_id.is_empty() || request.request_id.len() > MAX_REQUEST_ID_LEN {
        return ResponseEnvelope::error("", "invalid_request_id", "request_id is invalid");
    }
    if request.version != PROTOCOL_VERSION {
        return ResponseEnvelope::error(
            request.request_id,
            "unsupported_version",
            format!("supported protocol version is {PROTOCOL_VERSION}"),
        );
    }
    if !config.bearer.matches(request.bearer.as_bytes()) {
        return ResponseEnvelope::error(
            request.request_id,
            "unauthorized",
            "authentication failed",
        );
    }

    let result = handler.handle(request.command);
    match result {
        Ok(event) => ResponseEnvelope::ok(request.request_id, event),
        Err(message) => ResponseEnvelope::error(
            request.request_id,
            "handler_failed",
            bounded_message(message),
        ),
    }
}

fn bounded_message(mut message: String) -> String {
    const MAX_ERROR_LEN: usize = 512;
    if message.len() <= MAX_ERROR_LEN {
        return message;
    }
    let mut boundary = MAX_ERROR_LEN;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message.truncate(boundary);
    message.push('…');
    message
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::{self, Read as _, Write as _};
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;

    use super::*;

    pub(super) fn serve<H: ReadOnlyHandler>(
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

        // Exactly one request is handled at a time. This intentionally creates
        // no user-space connection queue; per-connection deadlines bound idle
        // occupancy until a dedicated concurrency policy is introduced.
        while !shutdown.requested() {
            match listener.accept() {
                Ok((stream, _address)) => handle_connection(stream, &config, &handler),
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

    fn handle_connection<H: ReadOnlyHandler>(
        mut stream: UnixStream,
        config: &DaemonConfig,
        handler: &H,
    ) {
        if stream.set_read_timeout(Some(config.read_timeout)).is_err()
            || stream
                .set_write_timeout(Some(config.write_timeout))
                .is_err()
        {
            return;
        }

        let response = match read_frame(&mut stream, config.max_frame_len) {
            Ok(bytes) => match serde_json::from_slice::<RequestEnvelope>(&bytes) {
                Ok(request) => dispatch(request, config, handler),
                Err(_) => {
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

    fn write_response(
        stream: &mut UnixStream,
        response: ResponseEnvelope,
        max: usize,
    ) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;
        if bytes.len() > max {
            bytes = serde_json::to_vec(&ResponseEnvelope::error(
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
    use std::os::unix::fs::PermissionsExt as _;
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
        let incompatible = harness.request(request(TEST_BEARER, PROTOCOL_VERSION + 1));
        assert_error(&incompatible, "unsupported_version");
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
        assert_error(&response, "handler_failed");
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
            let root = scratch_dir("server");
            let socket = root.join("abbeyd.sock");
            let config = DaemonConfig::for_test(socket.clone(), TEST_BEARER.as_bytes());
            let shutdown = Shutdown::default();
            let server_shutdown = shutdown.clone();
            let thread =
                thread::spawn(move || DaemonServer::new(config, Handler).serve(server_shutdown));
            let deadline = Instant::now() + Duration::from_secs(2);
            while !socket.exists() {
                assert!(Instant::now() < deadline, "daemon socket was not created");
                thread::sleep(Duration::from_millis(5));
            }
            let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "socket must be owner-only");
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
            stream
                .write_all(&declared.unwrap_or(bytes.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(bytes).unwrap();
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
