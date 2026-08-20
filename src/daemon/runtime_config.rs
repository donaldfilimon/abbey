//! Startup-only authority and private state configuration for daemon run control.

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::runtime::{DelegatedLimits, RunManagerConfig, RuntimeStore, prepare_legacy_import};

/// Startup-owned runtime authority. Requests cannot modify any field.
pub struct RuntimeDaemonConfig {
    state_root: PathBuf,
    workspace: PathBuf,
    memory_backend: String,
    abi_binary: Option<PathBuf>,
    foundation_models_binary: Option<PathBuf>,
    model_manifest_dir: Option<PathBuf>,
    manager: RunManagerConfig,
    delegated: DelegatedLimits,
}

impl RuntimeDaemonConfig {
    #[must_use]
    pub fn new(state_root: impl Into<PathBuf>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
            workspace: workspace.into(),
            memory_backend: "sqlite".to_owned(),
            abi_binary: None,
            foundation_models_binary: None,
            model_manifest_dir: None,
            manager: RunManagerConfig::default(),
            delegated: DelegatedLimits::default(),
        }
    }

    /// Resolve edition-scoped storage and fixed provider routes once at startup.
    pub fn from_env() -> Result<Self, RuntimeConfigError> {
        #[cfg(not(unix))]
        return Err(RuntimeConfigError::UnsupportedPlatform);

        #[cfg(unix)]
        {
            let edition = crate::edition::ACTIVE;
            let state_root = std::env::var_os(edition.state_dir_env())
                .map(PathBuf::from)
                .or_else(|| edition.default_state_root())
                .ok_or(RuntimeConfigError::MissingStateRoot)?;
            let workspace = std::env::current_dir().map_err(|_| RuntimeConfigError::Workspace)?;
            let mut config = Self::new(state_root, workspace);
            config.memory_backend = crate::config::AbbeyConfig::load()
                .map_err(|_| RuntimeConfigError::MemoryConfiguration)?
                .memory_backend;
            if let Some(path) = std::env::var_os(edition.scoped_env("ABI_BIN")) {
                config.abi_binary = Some(PathBuf::from(path));
            }
            if let Some(path) = std::env::var_os(edition.scoped_env("MODEL_MANIFEST_DIR")) {
                config.model_manifest_dir = Some(PathBuf::from(path));
            }
            let foundation_models = PathBuf::from("/usr/bin/fm");
            if foundation_models.is_file() {
                config.foundation_models_binary = Some(foundation_models);
            }
            Ok(config)
        }
    }

    #[must_use]
    pub fn bind_abi_local(mut self, executable: impl Into<PathBuf>) -> Self {
        self.abi_binary = Some(executable.into());
        self
    }

    #[must_use]
    pub fn bind_foundation_models(mut self, executable: impl Into<PathBuf>) -> Self {
        self.foundation_models_binary = Some(executable.into());
        self
    }

    /// Bind the startup-owned local model manifest directory served read-only
    /// through protocol-v3 model inventory. Requests cannot select or change it.
    #[must_use]
    pub fn bind_model_manifest_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.model_manifest_dir = Some(dir.into());
        self
    }

    /// Bind the startup-owned memory backend used by served safe effects.
    #[must_use]
    pub fn bind_memory_backend(mut self, backend: impl Into<String>) -> Self {
        self.memory_backend = backend.into().trim().to_ascii_lowercase();
        self
    }

    pub fn with_manager_config(
        mut self,
        manager: RunManagerConfig,
    ) -> Result<Self, RuntimeConfigError> {
        if manager.queue_capacity != manager.effective_queue_capacity() {
            return Err(RuntimeConfigError::ManagerLimits);
        }
        self.manager = manager;
        Ok(self)
    }

    pub fn with_delegated_limits(
        mut self,
        delegated: DelegatedLimits,
    ) -> Result<Self, RuntimeConfigError> {
        delegated
            .validate()
            .map_err(|_| RuntimeConfigError::DelegatedLimits)?;
        self.delegated = delegated;
        Ok(self)
    }

    pub(crate) fn parts(self) -> RuntimeConfigParts {
        RuntimeConfigParts {
            state_root: self.state_root,
            workspace: self.workspace,
            memory_backend: self.memory_backend,
            abi_binary: self.abi_binary,
            foundation_models_binary: self.foundation_models_binary,
            model_manifest_dir: self.model_manifest_dir,
            manager: self.manager,
            delegated: self.delegated,
        }
    }
}

impl fmt::Debug for RuntimeDaemonConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeDaemonConfig")
            .field("state_root", &"[REDACTED]")
            .field("workspace", &"[REDACTED]")
            .field("memory_backend", &self.memory_backend)
            .field("abi_binary", &self.abi_binary.as_ref().map(|_| "[BOUND]"))
            .field(
                "foundation_models_binary",
                &self.foundation_models_binary.as_ref().map(|_| "[BOUND]"),
            )
            .field(
                "model_manifest_dir",
                &self.model_manifest_dir.as_ref().map(|_| "[BOUND]"),
            )
            .field("manager", &self.manager)
            .field("delegated", &self.delegated)
            .finish()
    }
}

pub(crate) struct RuntimeConfigParts {
    pub state_root: PathBuf,
    pub workspace: PathBuf,
    pub memory_backend: String,
    pub abi_binary: Option<PathBuf>,
    pub foundation_models_binary: Option<PathBuf>,
    pub model_manifest_dir: Option<PathBuf>,
    pub manager: RunManagerConfig,
    pub delegated: DelegatedLimits,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeConfigError {
    #[error("daemon runtime is not implemented on this platform")]
    UnsupportedPlatform,
    #[error("daemon runtime state root is unavailable")]
    MissingStateRoot,
    #[error("daemon runtime workspace is unavailable")]
    Workspace,
    #[error("daemon memory configuration is unavailable")]
    MemoryConfiguration,
    #[error("daemon runtime state directory is not private")]
    StateDirectory,
    #[error("daemon runtime database path is not private")]
    DatabasePath,
    #[error("daemon runtime manager limits are invalid")]
    ManagerLimits,
    #[error("daemon delegated process limits are invalid")]
    DelegatedLimits,
    #[error("daemon delegated provider binding is invalid")]
    ProviderBinding,
    #[error("daemon model manifest directory is invalid or unreadable")]
    ModelManifests,
    #[error("daemon runtime route declaration is invalid")]
    Routes,
    #[error("daemon runtime database could not be opened")]
    Store,
    #[error("daemon legacy conversation metadata could not be safely retained")]
    LegacyMetadata,
}

#[cfg(unix)]
pub(crate) fn open_private_store(state_root: &Path) -> Result<RuntimeStore, RuntimeConfigError> {
    use std::fs;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let runtime_dir = state_root.join("daemon");
    match fs::symlink_metadata(&runtime_dir) {
        Ok(metadata) => validate_private_directory(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&runtime_dir).map_err(|_| RuntimeConfigError::StateDirectory)?;
            fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
                .map_err(|_| RuntimeConfigError::StateDirectory)?;
            let metadata = fs::symlink_metadata(&runtime_dir)
                .map_err(|_| RuntimeConfigError::StateDirectory)?;
            validate_private_directory(&metadata)?;
        }
        Err(_) => return Err(RuntimeConfigError::StateDirectory),
    }

    let database = RuntimeStore::path_for_state_dir(&runtime_dir);
    match fs::symlink_metadata(&database) {
        Ok(metadata) => validate_private_database(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&database)
            {
                Ok(file) => drop(file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(&database)
                        .map_err(|_| RuntimeConfigError::DatabasePath)?;
                    validate_private_database(&metadata)?;
                }
                Err(_) => return Err(RuntimeConfigError::DatabasePath),
            }
        }
        Err(_) => return Err(RuntimeConfigError::DatabasePath),
    }

    // Recover any canonical identity commit and hold the same projection lock
    // across legacy capture + SQLite import. Per-file atomic renames alone do
    // not prevent a stable but mixed cross-file snapshot during a save.
    let _conversation_guard = crate::state::lock_legacy_capture(state_root)
        .map_err(|_| RuntimeConfigError::LegacyMetadata)?;
    let legacy = prepare_legacy_import(state_root, &runtime_dir)
        .map_err(|_| RuntimeConfigError::LegacyMetadata)?;

    let store = RuntimeStore::open_with_legacy_private(&database, legacy.as_ref())
        .map_err(|_| RuntimeConfigError::Store)?;
    let metadata = fs::symlink_metadata(&database).map_err(|_| RuntimeConfigError::DatabasePath)?;
    validate_private_database(&metadata)?;
    Ok(store)
}

#[cfg(unix)]
fn validate_private_directory(metadata: &std::fs::Metadata) -> Result<(), RuntimeConfigError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(RuntimeConfigError::StateDirectory);
    }
    Ok(())
}

#[cfg(unix)]
fn validate_private_database(metadata: &std::fs::Metadata) -> Result<(), RuntimeConfigError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(RuntimeConfigError::DatabasePath);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_every_authority_path() {
        let config = RuntimeDaemonConfig::new("/secret/state", "/secret/workspace")
            .bind_abi_local("/secret/abi")
            .bind_foundation_models("/secret/fm")
            .bind_model_manifest_dir("/secret/manifests");
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("/secret"));
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("[BOUND]"));
    }

    #[test]
    fn manager_and_process_limits_reject_implicit_clamping() {
        assert_eq!(
            RuntimeDaemonConfig::new("state", "workspace")
                .with_manager_config(RunManagerConfig { queue_capacity: 0 })
                .unwrap_err(),
            RuntimeConfigError::ManagerLimits
        );
        let invalid = DelegatedLimits {
            stdout_bytes: 0,
            ..DelegatedLimits::default()
        };
        assert_eq!(
            RuntimeDaemonConfig::new("state", "workspace")
                .with_delegated_limits(invalid)
                .unwrap_err(),
            RuntimeConfigError::DelegatedLimits
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_store_rejects_insecure_directory_and_symlink_database() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let root = scratch("private-store");
        let daemon = root.join("daemon");
        std::fs::create_dir(&daemon).unwrap();
        std::fs::set_permissions(&daemon, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = match open_private_store(&root) {
            Ok(_) => panic!("insecure runtime directory was accepted"),
            Err(error) => error,
        };
        assert_eq!(error, RuntimeConfigError::StateDirectory);

        std::fs::set_permissions(&daemon, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.join("target");
        std::fs::write(&target, b"not a database").unwrap();
        symlink(&target, RuntimeStore::path_for_state_dir(&daemon)).unwrap();
        let error = match open_private_store(&root) {
            Ok(_) => panic!("symlink runtime database was accepted"),
            Err(error) => error,
        };
        assert_eq!(error, RuntimeConfigError::DatabasePath);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn new_runtime_store_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = scratch("owner-only-store");
        let store = open_private_store(&root).unwrap();
        drop(store);
        let daemon = root.join("daemon");
        let directory_mode = std::fs::metadata(&daemon).unwrap().permissions().mode();
        let database_mode = std::fs::metadata(RuntimeStore::path_for_state_dir(&daemon))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(directory_mode & 0o077, 0);
        assert_eq!(database_mode & 0o077, 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn scratch(label: &str) -> PathBuf {
        let path = PathBuf::from("/tmp").join(format!(
            "abbey-runtime-config-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
}
