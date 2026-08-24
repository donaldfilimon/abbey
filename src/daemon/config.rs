use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

pub const DEFAULT_MAX_FRAME_LEN: usize = super::federation::MAX_FRAME_BYTES;
const MIN_BEARER_LEN: usize = 32;
const MAX_BEARER_LEN: usize = 4096;
const MAX_AUTHENTICATED_REQUESTS: u16 = 1_024;
const MIN_RATE_WINDOW: Duration = Duration::from_millis(10);
const MAX_RATE_WINDOW: Duration = Duration::from_secs(60);

/// Authentication material whose `Debug` implementation is always redacted.
#[derive(Clone)]
pub struct BearerSecret(String);

impl BearerSecret {
    pub fn parse(value: impl AsRef<[u8]>) -> Result<Self, ConfigError> {
        let value = trim_line_ending(value.as_ref());
        if !(MIN_BEARER_LEN..=MAX_BEARER_LEN).contains(&value.len()) {
            return Err(ConfigError::BearerLength);
        }
        let value = std::str::from_utf8(value).map_err(|_| ConfigError::BearerNotUtf8)?;
        if value.chars().any(char::is_control) {
            return Err(ConfigError::BearerControlCharacter);
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn matches(&self, candidate: &[u8]) -> bool {
        // Compare all bytes when lengths match rather than returning on the
        // first mismatch. This is a small local-IPC hardening measure; it is
        // not advertised as a general-purpose constant-time primitive.
        if self.0.len() != candidate.len() {
            return false;
        }
        self.0
            .as_bytes()
            .iter()
            .zip(candidate)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BearerSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerSecret([REDACTED])")
    }
}

/// Fixed-memory rate limit applied only after successful authentication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthenticatedRateLimit {
    pub(crate) requests: u16,
    pub(crate) window: Duration,
}

impl AuthenticatedRateLimit {
    pub fn new(requests: u16, window: Duration) -> Result<Self, ConfigError> {
        if requests == 0
            || requests > MAX_AUTHENTICATED_REQUESTS
            || !(MIN_RATE_WINDOW..=MAX_RATE_WINDOW).contains(&window)
        {
            return Err(ConfigError::InvalidRateLimit);
        }
        Ok(Self { requests, window })
    }
}

impl Default for AuthenticatedRateLimit {
    fn default() -> Self {
        Self {
            requests: 64,
            window: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub bearer: BearerSecret,
    pub max_frame_len: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub accept_poll_interval: Duration,
    pub authenticated_rate_limit: AuthenticatedRateLimit,
    #[cfg(test)]
    pub(crate) client_handoff_barrier: Option<std::sync::Arc<std::sync::Barrier>>,
}

impl DaemonConfig {
    /// Construct a local IPC configuration from already-validated bearer
    /// material using the same bounded production defaults as [`Self::from_env`].
    #[must_use]
    pub fn local(socket_path: impl Into<PathBuf>, bearer: BearerSecret) -> Self {
        Self {
            socket_path: socket_path.into(),
            bearer,
            max_frame_len: DEFAULT_MAX_FRAME_LEN,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            accept_poll_interval: Duration::from_millis(25),
            authenticated_rate_limit: AuthenticatedRateLimit::default(),
            #[cfg(test)]
            client_handoff_barrier: None,
        }
    }

    /// Load a fail-closed daemon configuration.
    ///
    /// The inline-token and token-file variables are mutually exclusive.
    /// Private key material is never read from Abbey's TOML config.
    ///
    /// The variable *names* belong to the active edition (safe:
    /// `ABBEYD_BEARER_TOKEN` / `ABBEYD_BEARER_TOKEN_FILE`), so one exported
    /// bearer secret never authenticates against the other edition's daemon.
    pub fn from_env() -> Result<Self, ConfigError> {
        let edition = crate::edition::ACTIVE;
        let socket_path = std::env::var_os(edition.daemon_socket_env())
            .map(PathBuf::from)
            .unwrap_or_else(default_socket_path);
        let bearer_env = std::env::var_os(edition.daemon_bearer_env());
        let bearer_file = std::env::var_os(edition.daemon_bearer_file_env());

        let bearer = match (bearer_env, bearer_file) {
            (Some(_), Some(_)) => return Err(ConfigError::ConflictingBearerSources),
            (Some(value), None) => {
                let value = value
                    .into_string()
                    .map_err(|_| ConfigError::BearerNotUtf8)?;
                BearerSecret::parse(value)
            }
            (None, Some(path)) => load_bearer_file(Path::new(&path)),
            (None, None) => Err(ConfigError::MissingBearer),
        }?;

        Ok(Self {
            socket_path,
            bearer,
            max_frame_len: DEFAULT_MAX_FRAME_LEN,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            accept_poll_interval: Duration::from_millis(25),
            authenticated_rate_limit: AuthenticatedRateLimit::default(),
            #[cfg(test)]
            client_handoff_barrier: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(socket_path: PathBuf, bearer: &[u8]) -> Self {
        let mut config = Self::local(
            socket_path,
            BearerSecret::parse(bearer).expect("valid test bearer"),
        );
        config.read_timeout = Duration::from_millis(300);
        config.write_timeout = Duration::from_millis(300);
        config.accept_poll_interval = Duration::from_millis(5);
        config
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(
        "set exactly one of {} or {}",
        crate::edition::ACTIVE.daemon_bearer_env(),
        crate::edition::ACTIVE.daemon_bearer_file_env()
    )]
    MissingBearer,
    #[error(
        "{} and {} cannot both be set",
        crate::edition::ACTIVE.daemon_bearer_env(),
        crate::edition::ACTIVE.daemon_bearer_file_env()
    )]
    ConflictingBearerSources,
    #[error("daemon bearer token must be valid UTF-8")]
    BearerNotUtf8,
    #[error("daemon bearer token must contain 32 through 4096 bytes")]
    BearerLength,
    #[error("daemon bearer token must not contain control characters")]
    BearerControlCharacter,
    #[error("authenticated daemon rate limit is outside its supported bounds")]
    InvalidRateLimit,
    #[error("bearer file must be a regular file, not a symlink: {0}")]
    BearerFileType(PathBuf),
    #[error("bearer file must be owned by the current user: {0}")]
    BearerFileOwner(PathBuf),
    #[error("bearer file must not grant group or other permissions: {0}")]
    BearerFilePermissions(PathBuf),
    #[error("cannot inspect bearer file {path}: {source}")]
    InspectBearer {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read bearer file {path}: {source}")]
    ReadBearer {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn default_socket_path() -> PathBuf {
    let edition = crate::edition::ACTIVE;
    if let Some(root) = std::env::var_os(edition.state_dir_env()) {
        return edition.daemon_socket_path(Path::new(&root));
    }
    let root = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(edition.slug());
    edition.daemon_socket_path(&root)
}

fn trim_line_ending(mut bytes: &[u8]) -> &[u8] {
    if bytes.ends_with(b"\n") {
        bytes = &bytes[..bytes.len() - 1];
    }
    if bytes.ends_with(b"\r") {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

#[cfg(unix)]
fn load_bearer_file(path: &Path) -> Result<BearerSecret, ConfigError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let metadata =
        std::fs::symlink_metadata(path).map_err(|source| ConfigError::InspectBearer {
            path: path.to_owned(),
            source,
        })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ConfigError::BearerFileType(path.to_owned()));
    }
    if metadata.uid() != nix::unistd::Uid::effective().as_raw() {
        return Err(ConfigError::BearerFileOwner(path.to_owned()));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(ConfigError::BearerFilePermissions(path.to_owned()));
    }
    let bytes = std::fs::read(path).map_err(|source| ConfigError::ReadBearer {
        path: path.to_owned(),
        source,
    })?;
    BearerSecret::parse(bytes)
}

#[cfg(not(unix))]
fn load_bearer_file(path: &Path) -> Result<BearerSecret, ConfigError> {
    let _ = path;
    Err(ConfigError::BearerFilePermissions(path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_secret() {
        let secret = BearerSecret::parse(b"0123456789abcdef0123456789abcdef").unwrap();
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, "BearerSecret([REDACTED])");
    }

    #[test]
    fn authenticated_rate_limit_is_bounded() {
        assert!(AuthenticatedRateLimit::new(0, Duration::from_secs(1)).is_err());
        assert!(AuthenticatedRateLimit::new(1_025, Duration::from_secs(1)).is_err());
        assert!(AuthenticatedRateLimit::new(1, Duration::from_millis(9)).is_err());
        assert!(AuthenticatedRateLimit::new(1, Duration::from_secs(61)).is_err());
        assert!(AuthenticatedRateLimit::new(1, Duration::from_secs(1)).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn bearer_file_rejects_group_permissions() {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let root = scratch_dir("bearer-permissions");
        let path = root.join("bearer");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"0123456789abcdef0123456789abcdef").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(
            load_bearer_file(&path),
            Err(ConfigError::BearerFilePermissions(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn owner_only_bearer_file_rejects_non_utf8() {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let root = scratch_dir("bearer-utf8");
        let path = root.join("bearer");
        let mut bytes = vec![b'a'; MIN_BEARER_LEN];
        bytes[0] = 0xff;
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            load_bearer_file(&path),
            Err(ConfigError::BearerNotUtf8)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn scratch_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "abbeyd-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
}
