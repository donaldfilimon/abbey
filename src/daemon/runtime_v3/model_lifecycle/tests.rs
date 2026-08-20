
use super::*;
use abi_models::{Chunk, ModelManifest, Sha256Digest};
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Barrier;
use std::time::{Duration, Instant};

const MODEL_ID: &str = "fixture-bigram";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
const PRINCIPAL: &str = "operator@example.invalid";

#[derive(Clone)]
struct FakeTransport {
    bodies: Arc<BTreeMap<String, Vec<u8>>>,
}

#[derive(Clone)]
struct BlockingTransport {
    inner: FakeTransport,
    first: Arc<AtomicBool>,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl ChunkTransport for BlockingTransport {
    fn fetch(
        &self,
        url: &str,
        offset: u64,
        max_len: u64,
    ) -> Result<Chunk, abi_models::ModelError> {
        if self.first.swap(false, Ordering::AcqRel) {
            self.entered.wait();
            self.release.wait();
        }
        self.inner.fetch(url, offset, max_len)
    }
}

impl ChunkTransport for FakeTransport {
    fn fetch(&self, url: &str, offset: u64, max_len: u64) -> Result<Chunk, abi_models::ModelError> {
        let body = self
            .bodies
            .get(url)
            .ok_or_else(|| abi_models::ModelError::Transport {
                url: url.to_owned(),
                detail: "fixture URL is absent".to_owned(),
            })?;
        let start = usize::try_from(offset).unwrap();
        let maximum = usize::try_from(max_len).unwrap();
        let end = start.saturating_add(maximum).min(body.len());
        Ok(Chunk {
            bytes: body[start..end].to_vec(),
            total_len: u64::try_from(body.len()).unwrap(),
        })
    }
}

struct Fixture {
    root: PathBuf,
    authority: ModelLifecycleAuthority,
    store: Arc<RuntimeStore>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture(accepted: bool) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "abbey-v3-model-lifecycle-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let weights = safetensors_fixture();
    let tokenizer = tokenizer_fixture();
    let weights_url = format!("https://models.example.invalid/{REVISION}/model.safetensors");
    let tokenizer_url = format!("https://models.example.invalid/{REVISION}/tokenizer.json");
    let manifest = ModelManifest::from_json(
        "fixture",
        &json!({
            "id": MODEL_ID,
            "repository": "abbey-fixtures/bigram",
            "revision": REVISION,
            "architecture": abi_model_runtime::FIXTURE_BIGRAM_ARCHITECTURE,
            "license": "fixture-only-1.0",
            "license_sha256": Sha256Digest::from_bytes([7; 32]).to_hex(),
            "modalities": ["text"],
            "tensor_format": "safetensors",
            "quantizations": ["f32"],
            "context": {"max_context_tokens": 8, "max_output_tokens": 4},
            "artifacts": [
                {
                    "path": "model.safetensors",
                    "kind": "weights",
                    "sha256": sha256(&weights),
                    "size_bytes": weights.len(),
                    "url": weights_url
                },
                {
                    "path": "tokenizer.json",
                    "kind": "tokenizer",
                    "sha256": sha256(&tokenizer),
                    "size_bytes": tokenizer.len(),
                    "url": tokenizer_url
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    let mut registry = ModelRegistry::new();
    registry.insert(manifest.clone()).unwrap();
    let mut ledger = AcceptanceLedger::in_memory();
    if accepted {
        ledger.accept(&manifest, PRINCIPAL).unwrap();
    }
    let transport = FakeTransport {
        bodies: Arc::new(BTreeMap::from([
            (weights_url, weights),
            (tokenizer_url, tokenizer),
        ])),
    };
    let store = Arc::new(RuntimeStore::open(&root.join("runtime.sqlite")).unwrap());
    let authority = ModelLifecycleAuthority::from_parts(
        ModelRuntimeBinding {
            registry,
            ledger,
            storage: StorageRoot::at(root.join("external-models")),
            principal: PRINCIPAL.to_owned(),
            device: DevicePreference::Cpu,
            max_artifact_bytes: 1024 * 1024,
        },
        Arc::new(transport),
        Arc::clone(&store),
    );
    Fixture {
        root,
        authority,
        store,
    }
}

#[test]
fn exact_accepted_model_downloads_loads_reports_and_unloads() {
    let fixture = fixture(true);
    assert_eq!(fixture.authority.inventory()[0].id, MODEL_ID);
    let download = action("download-1");
    assert!(matches!(
        fixture.authority.download(download.clone()).unwrap(),
        V3Event::ModelStatus(V3OperationStatus {
            state: V3OperationState::Queued,
            ..
        })
    ));
    let downloaded = wait_for(&fixture.authority, "download-1", true);
    assert_eq!(downloaded.state, V3OperationState::Succeeded);
    assert_eq!(downloaded.progress_basis_points, 10_000);

    let load = action("load-1");
    assert!(matches!(
        fixture.authority.load(load).unwrap(),
        V3Event::ModelStatus(V3OperationStatus {
            state: V3OperationState::Queued,
            ..
        })
    ));
    let loaded = wait_for(&fixture.authority, "load-1", false);
    assert_eq!(loaded.state, V3OperationState::Succeeded);
    let V3Event::ModelStatus(inference) = fixture
        .authority
        .inference_status(V3ResourceQuery {
            resource_id: MODEL_ID.to_owned(),
        })
        .unwrap()
    else {
        panic!("expected model readiness status")
    };
    assert_eq!(inference.state, V3OperationState::Available);
    assert_eq!(inference.progress_basis_points, 0);

    let V3Event::ModelStatus(unloaded) = fixture.authority.unload(action("unload-1")).unwrap()
    else {
        panic!("expected unload status")
    };
    assert_eq!(unloaded.state, V3OperationState::Succeeded);
    assert_eq!(unloaded.progress_basis_points, 10_000);
    assert_eq!(
        fixture
            .store
            .audit_events_for_run(None)
            .unwrap()
            .iter()
            .filter(|event| event.action == "v3_model_lifecycle")
            .count(),
        6
    );
}

#[test]
fn unaccepted_revision_and_reused_operation_ids_fail_closed() {
    let unaccepted = fixture(false);
    assert_eq!(
        unaccepted
            .authority
            .download(action("denied-1"))
            .unwrap_err()
            .code(),
        "model_not_authorized"
    );
    let fixture = fixture(true);
    let mut wrong = action("wrong-revision");
    wrong.revision = "f".repeat(40);
    assert_eq!(
        fixture.authority.download(wrong).unwrap_err().code(),
        "invalid_command"
    );
    fixture.authority.download(action("single-use")).unwrap();
    assert_eq!(
        fixture
            .authority
            .load(action("single-use"))
            .unwrap_err()
            .code(),
        "conflict"
    );
    let _ = wait_for(&fixture.authority, "single-use", true);
}

#[test]
fn one_model_revision_cannot_download_and_load_concurrently() {
    let mut fixture = fixture(true);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    Arc::get_mut(&mut fixture.authority.inner)
        .expect("fixture authority is uniquely owned")
        .transport = Arc::new(BlockingTransport {
        inner: FakeTransport {
            bodies: Arc::clone(
                &fixture
                    .authority
                    .inner
                    .transport
                    .as_any_fixture_bodies(),
            ),
        },
        first: Arc::new(AtomicBool::new(true)),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    fixture.authority.download(action("blocked-download")).unwrap();
    entered.wait();
    assert_eq!(
        fixture
            .authority
            .load(action("overlapping-load"))
            .unwrap_err()
            .code(),
        "conflict"
    );
    release.wait();
    assert_eq!(
        wait_for(&fixture.authority, "blocked-download", true).state,
        V3OperationState::Succeeded
    );
}

#[cfg(unix)]
#[test]
fn owner_only_startup_document_verifies_the_publisher_signature() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = std::env::temp_dir().join(format!(
        "abbey-v3-model-config-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let workspace = root.join("workspace");
    let storage = root.join("external-models");
    std::fs::create_dir_all(&workspace).unwrap();
    let manifest = signed_fixture_manifest();
    let signing = SigningKey::from_bytes(&[23; 32]);
    let signature = signing.sign(manifest.as_bytes());
    let registry_path = root.join("registry.json");
    std::fs::write(
        &registry_path,
        json!([{
            "version": abi_models::SIGNED_MANIFEST_VERSION,
            "key_id": "fixture-publisher",
            "manifest_json": manifest,
            "signature": hex(&signature.to_bytes())
        }])
        .to_string(),
    )
    .unwrap();
    let config_path = root.join("model-runtime.json");
    std::fs::write(
        &config_path,
        json!({
            "version": CONFIG_VERSION,
            "registry_path": registry_path,
            "publisher_keys": {
                "fixture-publisher": hex(&signing.verifying_key().to_bytes())
            },
            "acceptance_ledger_path": root.join("accepted.jsonl"),
            "storage_root": storage,
            "principal": PRINCIPAL,
            "device": "cpu",
            "max_artifact_bytes": 1024
        })
        .to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let store = Arc::new(RuntimeStore::open(&root.join("runtime.sqlite")).unwrap());
    let authority = ModelLifecycleAuthority::open(&config_path, &workspace, store).unwrap();
    assert_eq!(authority.inventory()[0].id, MODEL_ID);

    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        ModelLifecycleAuthority::open(
            &config_path,
            &workspace,
            Arc::new(RuntimeStore::open(&root.join("second.sqlite")).unwrap())
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn wait_for(
    authority: &ModelLifecycleAuthority,
    operation_id: &str,
    download: bool,
) -> V3OperationStatus {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let event = if download {
            authority.download_status(V3ResourceQuery {
                resource_id: operation_id.to_owned(),
            })
        } else {
            authority.inference_status(V3ResourceQuery {
                resource_id: operation_id.to_owned(),
            })
        }
        .unwrap();
        let V3Event::ModelStatus(status) = event else {
            panic!("expected model status")
        };
        if matches!(
            status.state,
            V3OperationState::Succeeded | V3OperationState::Failed
        ) {
            return status;
        }
        assert!(Instant::now() < deadline, "model operation timed out");
        std::thread::yield_now();
    }
}

fn action(operation_id: &str) -> V3ModelAction {
    V3ModelAction {
        model_id: MODEL_ID.to_owned(),
        revision: REVISION.to_owned(),
        operation_id: operation_id.to_owned(),
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn signed_fixture_manifest() -> String {
    json!({
        "id": MODEL_ID,
        "repository": "abbey-fixtures/bigram",
        "revision": REVISION,
        "architecture": abi_model_runtime::FIXTURE_BIGRAM_ARCHITECTURE,
        "license": "fixture-only-1.0",
        "license_sha256": Sha256Digest::from_bytes([7; 32]).to_hex(),
        "modalities": ["text"],
        "tensor_format": "safetensors",
        "quantizations": ["f32"],
        "context": {"max_context_tokens": 8, "max_output_tokens": 4},
        "artifacts": [
            {
                "path": "model.safetensors",
                "kind": "weights",
                "sha256": sha256(b"weights"),
                "size_bytes": 7,
                "url": format!("https://models.example.invalid/{REVISION}/model.safetensors")
            },
            {
                "path": "tokenizer.json",
                "kind": "tokenizer",
                "sha256": sha256(b"tokenizer"),
                "size_bytes": 9,
                "url": format!("https://models.example.invalid/{REVISION}/tokenizer.json")
            }
        ]
    })
    .to_string()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn tokenizer_fixture() -> Vec<u8> {
    br#"{"version":"1.0","truncation":null,"padding":null,"added_tokens":[],"normalizer":null,"pre_tokenizer":{"type":"WhitespaceSplit"},"post_processor":null,"decoder":null,"model":{"type":"WordLevel","vocab":{"<unk>":0,"<eos>":1,"hello":2,"world":3,"again":4},"unk_token":"<unk>"}}"#.to_vec()
}

fn safetensors_fixture() -> Vec<u8> {
    let mut logits = vec![-100.0f32; 25];
    for token in 0..5 {
        logits[token * 5 + 1] = 10.0;
    }
    logits[2 * 5 + 3] = 20.0;
    logits[3 * 5 + 4] = 20.0;
    logits[4 * 5 + 1] = 20.0;
    let mut header =
        br#"{"transition":{"dtype":"F32","shape":[5,5],"data_offsets":[0,100]}}"#.to_vec();
    while !header.len().is_multiple_of(8) {
        header.push(b' ');
    }
    let mut bytes = u64::try_from(header.len()).unwrap().to_le_bytes().to_vec();
    bytes.extend(header);
    for value in logits {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}
