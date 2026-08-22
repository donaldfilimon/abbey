//! Startup-owned signed model registry and durable lifecycle operations.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use abi_model_runtime::{DevicePreference, LoadConfig, LocalModelProvider};
use abi_models::{
    AcceptanceLedger, ChunkTransport, HttpTransport, ModelRegistry, PublisherTrustStore,
    ResumableDownload, StorageRoot,
};
use serde::Deserialize;
use serde_json::json;

use crate::app_core::{
    V3EntityRecord, V3Event, V3ModelAction, V3OperationState, V3OperationStatus, V3ResourceQuery,
};
use crate::runtime::{
    AuditMetadata, ModelOperationKind, ModelOperationRecord, ModelOperationState, NewAuditEvent,
    NewModelOperation, RuntimeStore, StoreError,
};

use super::{HandlerFailure, invalid_command_failure};

const CONFIG_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_REGISTRY_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupDocument {
    version: u32,
    registry_path: PathBuf,
    publisher_keys: BTreeMap<String, String>,
    acceptance_ledger_path: PathBuf,
    storage_root: PathBuf,
    principal: String,
    device: String,
    #[serde(default = "default_max_artifact_bytes")]
    max_artifact_bytes: u64,
}

const fn default_max_artifact_bytes() -> u64 {
    DEFAULT_MAX_ARTIFACT_BYTES
}

#[derive(Clone)]
pub(in crate::daemon) struct ModelLifecycleAuthority {
    inner: Arc<Inner>,
}

struct Inner {
    registry: ModelRegistry,
    ledger: AcceptanceLedger,
    storage: StorageRoot,
    principal: String,
    device: DevicePreference,
    max_artifact_bytes: u64,
    transport: Arc<dyn ChunkTransport + Send + Sync>,
    store: Arc<RuntimeStore>,
    active: Mutex<HashSet<ActiveKey>>,
    loaded: Mutex<BTreeMap<ModelKey, Arc<LocalModelProvider>>>,
}

struct ModelRuntimeBinding {
    registry: ModelRegistry,
    ledger: AcceptanceLedger,
    storage: StorageRoot,
    principal: String,
    device: DevicePreference,
    max_artifact_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ModelKey {
    model_id: String,
    revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ActiveKey {
    model_id: String,
    revision: String,
}

impl ModelLifecycleAuthority {
    /// Open one exact, owner-only startup document and verify its signed registry.
    pub(in crate::daemon) fn open(
        config_path: &Path,
        workspace: &Path,
        store: Arc<RuntimeStore>,
    ) -> Result<Self, ()> {
        validate_private_file(config_path)?;
        let document: StartupDocument =
            serde_json::from_str(&read_bounded(config_path, MAX_CONFIG_BYTES)?).map_err(|_| ())?;
        if document.version != CONFIG_VERSION
            || !document.registry_path.is_absolute()
            || !document.acceptance_ledger_path.is_absolute()
            || !document.storage_root.is_absolute()
            || document.publisher_keys.is_empty()
            || document.publisher_keys.len() > 64
            || document.principal.trim().is_empty()
            || document.principal.len() > 256
            || document.principal.chars().any(char::is_control)
            || document.max_artifact_bytes == 0
            || document.max_artifact_bytes > abi_models::download::DEFAULT_MAX_ARTIFACT_BYTES
        {
            return Err(());
        }
        let mut trust = PublisherTrustStore::new();
        for (key_id, public_key) in document.publisher_keys {
            trust.insert_hex(key_id, &public_key).map_err(|_| ())?;
        }
        let registry_document = read_bounded(&document.registry_path, MAX_REGISTRY_BYTES)?;
        let registry = ModelRegistry::from_signed_json_array(
            &document.registry_path.display().to_string(),
            &registry_document,
            &trust,
        )
        .map_err(|_| ())?;
        if registry.is_empty() {
            return Err(());
        }
        for model_id in registry.ids() {
            let manifest = registry.manifest(model_id).map_err(|_| ())?;
            let record = V3EntityRecord {
                id: model_id.to_owned(),
                label: model_label(model_id, manifest.architecture.as_str()),
                state: V3OperationState::Available,
            };
            record.validate().map_err(|_| ())?;
            V3ModelAction {
                model_id: model_id.to_owned(),
                revision: manifest.revision.as_str().to_owned(),
                operation_id: "validation".to_owned(),
            }
            .validate()
            .map_err(|_| ())?;
        }
        validate_optional_private_file(&document.acceptance_ledger_path)?;
        let ledger = AcceptanceLedger::load(&document.acceptance_ledger_path).map_err(|_| ())?;
        let storage = StorageRoot::at(document.storage_root);
        storage.reject_inside(workspace).map_err(|_| ())?;
        storage
            .reject_inside(Path::new(env!("CARGO_MANIFEST_DIR")))
            .map_err(|_| ())?;
        let device = parse_device(&document.device)?;
        Ok(Self::from_parts(
            ModelRuntimeBinding {
                registry,
                ledger,
                storage,
                principal: document.principal,
                device,
                max_artifact_bytes: document.max_artifact_bytes,
            },
            Arc::new(HttpTransport::new()),
            store,
        ))
    }

    fn from_parts(
        binding: ModelRuntimeBinding,
        transport: Arc<dyn ChunkTransport + Send + Sync>,
        store: Arc<RuntimeStore>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                registry: binding.registry,
                ledger: binding.ledger,
                storage: binding.storage,
                principal: binding.principal,
                device: binding.device,
                max_artifact_bytes: binding.max_artifact_bytes,
                transport,
                store,
                active: Mutex::new(HashSet::new()),
                loaded: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub(super) fn inventory(&self) -> Vec<V3EntityRecord> {
        self.inner
            .registry
            .ids()
            .filter_map(|model_id| {
                let manifest = self.inner.registry.get(model_id)?;
                Some(V3EntityRecord {
                    id: model_id.to_owned(),
                    label: model_label(model_id, manifest.architecture.as_str()),
                    state: V3OperationState::Available,
                })
            })
            .collect()
    }

    pub(super) fn download(&self, action: V3ModelAction) -> Result<V3Event, HandlerFailure> {
        self.validate_action(&action)?;
        self.inner
            .registry
            .resolve_for(&action.model_id, &self.inner.ledger, &self.inner.principal)
            .map_err(|_| model_denied_failure())?;
        self.start(
            action,
            ModelOperationKind::Download,
            "download",
            Self::run_download,
        )
    }

    pub(super) fn load(&self, action: V3ModelAction) -> Result<V3Event, HandlerFailure> {
        self.validate_action(&action)?;
        self.inner
            .registry
            .resolve_for(&action.model_id, &self.inner.ledger, &self.inner.principal)
            .map_err(|_| model_denied_failure())?;
        self.start(action, ModelOperationKind::Load, "load", Self::run_load)
    }

    pub(super) fn unload(&self, action: V3ModelAction) -> Result<V3Event, HandlerFailure> {
        self.validate_action(&action)?;
        let active_key = self.acquire_active(&action)?;
        let result = (|| {
            let operation = self.create_operation(&action, ModelOperationKind::Unload)?;
            self.audit(&operation).map_err(|_| runtime_failure())?;
            self.transition(&operation.operation_id, ModelOperationState::Running, 0)?;
            let key = model_key(&action);
            let removed = self
                .inner
                .loaded
                .lock()
                .map_err(|_| runtime_failure())?
                .remove(&key)
                .is_some();
            let state = if removed {
                ModelOperationState::Succeeded
            } else {
                ModelOperationState::Failed
            };
            let progress = if removed { 10_000 } else { 0 };
            let terminal = self.transition(&operation.operation_id, state, progress)?;
            self.audit(&terminal).map_err(|_| runtime_failure())?;
            Ok(V3Event::ModelStatus(status(terminal)))
        })();
        self.release_active(&active_key);
        result
    }

    pub(super) fn download_status(
        &self,
        query: V3ResourceQuery,
    ) -> Result<V3Event, HandlerFailure> {
        query.validate().map_err(|_| invalid_command_failure())?;
        let operation = self
            .inner
            .store
            .model_operation(&query.resource_id)
            .map_err(|_| runtime_failure())?
            .ok_or_else(not_found_failure)?;
        if operation.kind != ModelOperationKind::Download {
            return Err(invalid_command_failure());
        }
        Ok(V3Event::ModelStatus(status(operation)))
    }

    pub(super) fn inference_status(
        &self,
        query: V3ResourceQuery,
    ) -> Result<V3Event, HandlerFailure> {
        query.validate().map_err(|_| invalid_command_failure())?;
        if let Some(operation) = self
            .inner
            .store
            .model_operation(&query.resource_id)
            .map_err(|_| runtime_failure())?
        {
            if operation.kind == ModelOperationKind::Download {
                return Err(invalid_command_failure());
            }
            return Ok(V3Event::ModelStatus(status(operation)));
        }
        let loaded = self.inner.loaded.lock().map_err(|_| runtime_failure())?;
        let (key, provider) = loaded
            .iter()
            .find(|(key, _)| key.model_id == query.resource_id)
            .ok_or_else(not_found_failure)?;
        let report = provider
            .last_inference_report()
            .map_err(|_| runtime_failure())?;
        Ok(V3Event::ModelStatus(V3OperationStatus {
            operation_id: format!("inference:{}", key.model_id),
            resource_id: key.model_id.clone(),
            state: if report.is_some() {
                V3OperationState::Succeeded
            } else {
                V3OperationState::Available
            },
            progress_basis_points: if report.is_some() { 10_000 } else { 0 },
        }))
    }

    fn start(
        &self,
        action: V3ModelAction,
        kind: ModelOperationKind,
        active_kind: &'static str,
        operation: fn(Arc<Inner>, V3ModelAction, ActiveKey),
    ) -> Result<V3Event, HandlerFailure> {
        let active_key = self.acquire_active(&action)?;
        let created = match self.create_operation(&action, kind) {
            Ok(record) => record,
            Err(error) => {
                self.release_active(&active_key);
                return Err(error);
            }
        };
        if self.audit(&created).is_err() {
            let _ = self.transition(
                &created.operation_id,
                ModelOperationState::Failed,
                created.progress_basis_points,
            );
            self.release_active(&active_key);
            return Err(runtime_failure());
        }
        let inner = Arc::clone(&self.inner);
        let thread_action = action;
        let thread_key = active_key.clone();
        if std::thread::Builder::new()
            .name(format!("abbey-model-{active_kind}"))
            .spawn(move || operation(inner, thread_action, thread_key))
            .is_err()
        {
            let failed = self.transition(
                &created.operation_id,
                ModelOperationState::Failed,
                created.progress_basis_points,
            )?;
            self.audit(&failed).map_err(|_| runtime_failure())?;
            self.release_active(&active_key);
            return Ok(V3Event::ModelStatus(status(failed)));
        }
        Ok(V3Event::ModelStatus(status(created)))
    }

    fn run_download(inner: Arc<Inner>, action: V3ModelAction, active: ActiveKey) {
        let authority = Self { inner };
        let result = authority.download_all(&action);
        authority.release_active(&active);
        authority.finish_background(&action.operation_id, result);
    }

    fn run_load(inner: Arc<Inner>, action: V3ModelAction, active: ActiveKey) {
        let authority = Self { inner };
        let result = authority.load_exact(&action);
        authority.release_active(&active);
        authority.finish_background(&action.operation_id, result);
    }

    fn download_all(&self, action: &V3ModelAction) -> Result<(), ()> {
        self.transition(&action.operation_id, ModelOperationState::Running, 0)
            .map_err(|_| ())?;
        let manifest = self.inner.registry.get(&action.model_id).ok_or(())?;
        let count = manifest.artifacts.len();
        for (index, artifact) in manifest.artifacts.iter().enumerate() {
            let destination = self.inner.storage.artifact_path(manifest, artifact);
            ResumableDownload::new(artifact, destination)
                .with_max_size(self.inner.max_artifact_bytes)
                .run(self.inner.transport.as_ref())
                .map_err(|_| ())?;
            let completed = u32::try_from(index + 1).map_err(|_| ())?;
            let total = u32::try_from(count).map_err(|_| ())?;
            let progress = u16::try_from((u64::from(completed) * 10_000) / u64::from(total))
                .map_err(|_| ())?;
            self.transition(&action.operation_id, ModelOperationState::Running, progress)
                .map_err(|_| ())?;
        }
        Ok(())
    }

    fn load_exact(&self, action: &V3ModelAction) -> Result<(), ()> {
        self.transition(&action.operation_id, ModelOperationState::Running, 0)
            .map_err(|_| ())?;
        let key = model_key(action);
        {
            let loaded = self.inner.loaded.lock().map_err(|_| ())?;
            if loaded.contains_key(&key) {
                return Err(());
            }
        }
        let provider = LocalModelProvider::load(
            &self.inner.registry,
            &self.inner.ledger,
            &self.inner.storage,
            &action.model_id,
            &self.inner.principal,
            LoadConfig::new(self.inner.device).with_max_model_bytes(self.inner.max_artifact_bytes),
        )
        .map_err(|_| ())?;
        self.inner
            .loaded
            .lock()
            .map_err(|_| ())?
            .insert(key, Arc::new(provider));
        Ok(())
    }

    fn finish_background(&self, operation_id: &str, result: Result<(), ()>) {
        let state = if result.is_ok() {
            ModelOperationState::Succeeded
        } else {
            ModelOperationState::Failed
        };
        let progress = if result.is_ok() {
            10_000
        } else {
            self.inner
                .store
                .model_operation(operation_id)
                .ok()
                .flatten()
                .map_or(0, |record| record.progress_basis_points)
        };
        if let Ok(record) = self.transition(operation_id, state, progress) {
            let _ = self.audit(&record);
        }
    }

    fn create_operation(
        &self,
        action: &V3ModelAction,
        kind: ModelOperationKind,
    ) -> Result<ModelOperationRecord, HandlerFailure> {
        self.inner
            .store
            .create_model_operation(NewModelOperation {
                operation_id: action.operation_id.clone(),
                model_id: action.model_id.clone(),
                revision: action.revision.clone(),
                kind,
                created_at_ms: now_ms().map_err(|_| runtime_failure())?,
            })
            .map_err(map_store_error)
    }

    fn transition(
        &self,
        operation_id: &str,
        state: ModelOperationState,
        progress: u16,
    ) -> Result<ModelOperationRecord, HandlerFailure> {
        self.inner
            .store
            .transition_model_operation(
                operation_id,
                state,
                progress,
                now_ms().map_err(|_| runtime_failure())?,
            )
            .map_err(map_store_error)
    }

    fn validate_action(&self, action: &V3ModelAction) -> Result<(), HandlerFailure> {
        action.validate().map_err(|_| invalid_command_failure())?;
        self.validate_model_revision(&action.model_id, &action.revision)
    }

    fn validate_model_revision(
        &self,
        model_id: &str,
        revision: &str,
    ) -> Result<(), HandlerFailure> {
        let manifest = self
            .inner
            .registry
            .get(model_id)
            .ok_or_else(not_found_failure)?;
        if manifest.revision.as_str() != revision {
            return Err(invalid_command_failure());
        }
        Ok(())
    }

    fn audit(&self, operation: &ModelOperationRecord) -> Result<(), StoreError> {
        self.inner.store.record_audit(NewAuditEvent {
            run_id: None,
            action: "v3_model_lifecycle".to_owned(),
            outcome: operation.state.as_audit_str().to_owned(),
            metadata: AuditMetadata::new(json!({
                "operation_id": operation.operation_id,
                "model_id": operation.model_id,
                "revision": operation.revision,
                "kind": operation.kind.as_audit_str(),
                "progress_basis_points": operation.progress_basis_points,
                "requested_device": self.inner.device.as_str(),
                "fallback_allowed": false
            }))?,
        })?;
        Ok(())
    }

    fn release_active(&self, key: &ActiveKey) {
        if let Ok(mut active) = self.inner.active.lock() {
            active.remove(key);
        }
    }

    fn acquire_active(&self, action: &V3ModelAction) -> Result<ActiveKey, HandlerFailure> {
        self.acquire_exact(&action.model_id, &action.revision)
    }

    fn acquire_exact(&self, model_id: &str, revision: &str) -> Result<ActiveKey, HandlerFailure> {
        let key = ActiveKey {
            model_id: model_id.to_owned(),
            revision: revision.to_owned(),
        };
        let mut active = self.inner.active.lock().map_err(|_| runtime_failure())?;
        if !active.insert(key.clone()) {
            return Err(conflict_failure());
        }
        Ok(key)
    }
}

impl ModelOperationKind {
    const fn as_audit_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Load => "load",
            Self::Unload => "unload",
        }
    }
}

impl ModelOperationState {
    const fn as_audit_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

fn status(record: ModelOperationRecord) -> V3OperationStatus {
    V3OperationStatus {
        operation_id: record.operation_id,
        resource_id: record.model_id,
        state: match record.state {
            ModelOperationState::Queued => V3OperationState::Queued,
            ModelOperationState::Running => V3OperationState::Running,
            ModelOperationState::Succeeded => V3OperationState::Succeeded,
            ModelOperationState::Failed => V3OperationState::Failed,
            ModelOperationState::Cancelled => V3OperationState::Cancelled,
        },
        progress_basis_points: record.progress_basis_points,
    }
}

fn model_key(action: &V3ModelAction) -> ModelKey {
    ModelKey {
        model_id: action.model_id.clone(),
        revision: action.revision.clone(),
    }
}

fn model_label(model_id: &str, architecture: &str) -> String {
    let label = format!("{model_id} ({architecture})");
    label.chars().take(256).collect()
}

fn parse_device(value: &str) -> Result<DevicePreference, ()> {
    match value {
        "cpu" => Ok(DevicePreference::Cpu),
        "metal" => Ok(DevicePreference::Metal),
        "cuda" => Ok(DevicePreference::Cuda),
        _ => Err(()),
    }
}

fn read_bounded(path: &Path, maximum: u64) -> Result<String, ()> {
    let metadata = std::fs::metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(());
    }
    std::fs::read_to_string(path).map_err(|_| ())
}

#[cfg(unix)]
fn validate_private_file(path: &Path) -> Result<(), ()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !path.is_absolute()
        || !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(());
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_file(_path: &Path) -> Result<(), ()> {
    Err(())
}

fn validate_optional_private_file(path: &Path) -> Result<(), ()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => validate_private_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(()),
    }
}

fn now_ms() -> Result<u64, ()> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ())?;
    u64::try_from(elapsed.as_millis()).map_err(|_| ())
}

fn map_store_error(error: StoreError) -> HandlerFailure {
    match error {
        StoreError::ModelOperationNotFound(_) => not_found_failure(),
        StoreError::ModelOperationConflict => conflict_failure(),
        StoreError::ModelOperationCapacity => HandlerFailure::new(
            "resource_exhausted",
            "model operation ledger reached its bounded capacity",
        ),
        StoreError::InvalidInput(_) => invalid_command_failure(),
        _ => runtime_failure(),
    }
}

const fn not_found_failure() -> HandlerFailure {
    HandlerFailure::new("not_found", "model resource was not found")
}

const fn conflict_failure() -> HandlerFailure {
    HandlerFailure::new("conflict", "model operation conflicts with durable state")
}

const fn model_denied_failure() -> HandlerFailure {
    HandlerFailure::new(
        "model_not_authorized",
        "model license acceptance does not authorize this exact model revision",
    )
}

const fn runtime_failure() -> HandlerFailure {
    HandlerFailure::new(
        "runtime_unavailable",
        "model runtime operation is unavailable",
    )
}

mod model_inference;

#[cfg(test)]
#[path = "model_lifecycle/tests.rs"]
mod tests;
