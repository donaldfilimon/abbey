//! Startup-owned local model manifest inventory.
//!
//! Projects `abi_models::ModelRegistry` identities into sanitized protocol-v3
//! entity records with size-checked artifact **presence** only. Nothing here
//! hashes, downloads, loads, resolves licenses, or infers — an absent or
//! short artifact simply reports [`V3OperationState::NotDownloaded`]. The
//! manifest directory and the storage root are both resolved once at daemon
//! startup; requests cannot select either.

use std::path::Path;

use abi_models::{Artifact, ModelManifest, ModelRegistry, StorageRoot};

use crate::app_core::{V3EntityRecord, V3OperationState};

/// A manifest directory that fails to validate refuses daemon startup rather
/// than serving a partial or guessed inventory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ModelInventoryError;

/// Build the manifest-derived model inventory, or an empty inventory when no
/// manifest directory was bound at startup.
pub(crate) fn build(
    manifest_dir: Option<&Path>,
) -> Result<Vec<V3EntityRecord>, ModelInventoryError> {
    let Some(dir) = manifest_dir else {
        return Ok(Vec::new());
    };
    let registry = ModelRegistry::from_dir(dir).map_err(|_| ModelInventoryError)?;
    let storage = StorageRoot::resolve().map_err(|_| ModelInventoryError)?;
    let mut records = Vec::with_capacity(registry.len());
    for id in registry.ids() {
        let manifest = registry.manifest(id).map_err(|_| ModelInventoryError)?;
        let present = manifest
            .artifacts
            .iter()
            .all(|artifact| artifact_present(&storage, manifest, artifact));
        let (state, summary) = if present {
            (V3OperationState::Available, "artifacts present")
        } else {
            (V3OperationState::NotDownloaded, "not downloaded")
        };
        let revision = manifest.revision.as_str();
        let short_revision = &revision[..revision.len().min(12)];
        let record = V3EntityRecord {
            id: format!("abi-model:{id}"),
            label: format!(
                "local model {id} {}@{short_revision} ({summary})",
                manifest.repository
            ),
            state,
        };
        // A manifest identity that cannot be represented on the sanitized
        // wire refuses startup instead of being silently skipped or renamed.
        record.validate().map_err(|_| ModelInventoryError)?;
        records.push(record);
    }
    Ok(records)
}

/// Presence is a regular file with the exact manifest size — deliberately not
/// a digest check, which would read every artifact byte at daemon startup.
fn artifact_present(storage: &StorageRoot, manifest: &ModelManifest, artifact: &Artifact) -> bool {
    std::fs::metadata(storage.artifact_path(manifest, artifact))
        .map(|metadata| metadata.is_file() && metadata.len() == artifact.size_bytes)
        .unwrap_or(false)
}

#[cfg(test)]
// `std::env::set_var` is unsafe in edition 2024 (it races other threads), and
// these tests pin the `ABI_MODELS_DIR` storage root under a serializing lock.
// Test-only: the crate otherwise denies `unsafe_code`.
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn manifest_json(id: &str, weights_size: u64) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "repository": "example/{id}",
                "revision": "{REVISION}",
                "architecture": "llama",
                "license": "apache-2.0",
                "license_sha256": "{DIGEST}",
                "modalities": ["text"],
                "tensor_format": "gguf",
                "quantizations": ["q4_k_m"],
                "context": {{ "max_context_tokens": 4096, "max_output_tokens": 1024 }},
                "artifacts": [
                    {{
                        "path": "weights.gguf",
                        "kind": "weights",
                        "sha256": "{DIGEST}",
                        "size_bytes": {weights_size},
                        "url": "https://example.invalid/{REVISION}/weights.gguf"
                    }},
                    {{
                        "path": "tokenizer.json",
                        "kind": "tokenizer",
                        "sha256": "{DIGEST}",
                        "size_bytes": 4,
                        "url": "https://example.invalid/{REVISION}/tokenizer.json"
                    }}
                ]
            }}"#
        )
    }

    struct Scratch {
        root: PathBuf,
        previous_storage: Option<std::ffi::OsString>,
    }

    // `ABI_MODELS_DIR` is process-global; serialize the tests that set it.
    static STORAGE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl Scratch {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "abbey-model-inventory-{label}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(root.join("manifests")).unwrap();
            std::fs::create_dir_all(root.join("storage")).unwrap();
            let previous_storage = std::env::var_os("ABI_MODELS_DIR");
            // SAFETY: guarded by STORAGE_ENV_LOCK in every caller.
            unsafe { std::env::set_var("ABI_MODELS_DIR", root.join("storage")) };
            Self {
                root,
                previous_storage,
            }
        }

        fn manifest_dir(&self) -> PathBuf {
            self.root.join("manifests")
        }

        fn write_manifest(&self, name: &str, contents: &str) {
            std::fs::write(self.manifest_dir().join(name), contents).unwrap();
        }

        fn write_artifact(&self, repository_slug: &str, path: &str, bytes: &[u8]) {
            let file = self
                .root
                .join("storage")
                .join(repository_slug)
                .join(REVISION)
                .join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // SAFETY: guarded by STORAGE_ENV_LOCK in every caller.
            unsafe {
                match self.previous_storage.take() {
                    Some(previous) => std::env::set_var("ABI_MODELS_DIR", previous),
                    None => std::env::remove_var("ABI_MODELS_DIR"),
                }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn an_unbound_manifest_dir_yields_an_empty_inventory_without_io() {
        assert_eq!(build(None), Ok(Vec::new()));
    }

    #[test]
    fn present_and_absent_artifacts_split_available_from_not_downloaded() {
        let _guard = STORAGE_ENV_LOCK.lock().unwrap();
        let scratch = Scratch::new("presence");
        scratch.write_manifest("downloaded.json", &manifest_json("downloaded", 8));
        scratch.write_manifest("missing.json", &manifest_json("missing", 8));
        scratch.write_artifact("example__downloaded", "weights.gguf", b"12345678");
        scratch.write_artifact("example__downloaded", "tokenizer.json", b"abcd");

        let records = build(Some(&scratch.manifest_dir())).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "abi-model:downloaded");
        assert_eq!(records[0].state, V3OperationState::Available);
        assert!(records[0].label.contains("artifacts present"));
        assert_eq!(records[1].id, "abi-model:missing");
        assert_eq!(records[1].state, V3OperationState::NotDownloaded);
        assert!(records[1].label.contains("not downloaded"));
    }

    #[test]
    fn a_size_mismatch_is_not_downloaded_rather_than_present() {
        let _guard = STORAGE_ENV_LOCK.lock().unwrap();
        let scratch = Scratch::new("size-mismatch");
        scratch.write_manifest("truncated.json", &manifest_json("truncated", 8));
        scratch.write_artifact("example__truncated", "weights.gguf", b"123");
        scratch.write_artifact("example__truncated", "tokenizer.json", b"abcd");

        let records = build(Some(&scratch.manifest_dir())).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, V3OperationState::NotDownloaded);
    }

    #[test]
    fn a_malformed_manifest_refuses_the_whole_inventory() {
        let _guard = STORAGE_ENV_LOCK.lock().unwrap();
        let scratch = Scratch::new("malformed");
        scratch.write_manifest("good.json", &manifest_json("good", 8));
        scratch.write_manifest("bad.json", "{ not json");

        assert_eq!(
            build(Some(&scratch.manifest_dir())),
            Err(ModelInventoryError)
        );
    }

    #[test]
    fn a_manifest_id_outside_the_sanitized_wire_charset_refuses_startup() {
        let _guard = STORAGE_ENV_LOCK.lock().unwrap();
        let scratch = Scratch::new("bad-id");
        scratch.write_manifest("bad-id.json", &manifest_json("bad/id", 8));

        assert_eq!(
            build(Some(&scratch.manifest_dir())),
            Err(ModelInventoryError)
        );
    }

    #[test]
    fn a_missing_manifest_directory_refuses_startup() {
        let _guard = STORAGE_ENV_LOCK.lock().unwrap();
        let scratch = Scratch::new("missing-dir");
        let missing = scratch.root.join("does-not-exist");

        assert_eq!(build(Some(&missing)), Err(ModelInventoryError));
    }
}
