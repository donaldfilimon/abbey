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

    let listener = UnixListener::bind(&config.socket_path).map_err(|source| ServerError::Io {
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
    // The listener is nonblocking so the accept loop can observe shutdown.
    // On BSD-family hosts an accepted stream can retain that mode, which
    // would make `read_exact` fail with `WouldBlock` if accept wins the
    // scheduling race against the client's first write. Per-connection I/O
    // is synchronous and bounded by the deadlines below, so restore the
    // stream's blocking contract before reading the frame.
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(config.read_timeout)).is_err()
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
