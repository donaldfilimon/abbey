//! Real-process proof for startup-owned signed model lifecycle authority.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use abbey::app_core::{
    V3Capability, V3CapabilitySet, V3ErrorCode, V3ModelAction, V3ModelDevice,
    V3ModelInferenceRequest, V3OperationState, V3PageQuery, V3ResourceQuery,
};
use abbey::daemon::{BearerSecret, ClientError, DaemonClient, DaemonConfig, V3DaemonSession};
use abbey::edition;
use abi_models::{AcceptanceLedger, ModelManifest, Sha256Digest, StorageRoot, hash_file};
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::json;

const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-model-lifecycle-bearer-00000001";
const MODEL_ID: &str = "fixture-bigram";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
const PRINCIPAL: &str = "operator@example.invalid";
/// Bounded attempts at one exact-model inference run on a Metal device.
///
/// The daemon caps a single run at one second, and Candle compiles its Metal
/// shader source on first use. One attempt may therefore be refused by the
/// daemon's own run budget on a cold device; the compiled library is cached
/// in-process, so a later attempt answers.
const METAL_INFERENCE_ATTEMPTS: usize = 3;

/// How long the Metal branch waits for one inference attempt.
///
/// The daemon's own one-second run budget is not the only bound. The pinned
/// ABI revision's `RunContext::finish` reports `StopReason::Completed`
/// without consulting the elapsed duration, so a cold Metal compile that
/// happens *before the first event* is not caught by the budget at all: the
/// client stops waiting first. The default five-second read deadline is
/// therefore the bound that actually decides a cold attempt, and it is too
/// tight for a first-use shader compile.
///
/// This is a *client* deadline, raised only for the Metal branch of this
/// test. It does not change the daemon's budget, does not change the
/// fail-closed guarantee (a host with no Metal device still refuses the load
/// and the inference), and does not change the default cohort at all.
const METAL_READ_TIMEOUT: Duration = Duration::from_secs(60);

struct Harness {
    root: PathBuf,
    state: PathBuf,
    workspace: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    child: Child,
}

impl Harness {
    /// Start a daemon whose startup document selects the CPU device.
    fn start() -> Self {
        Self::start_with_device("cpu")
    }

    /// Start a daemon whose startup document selects an exact device.
    ///
    /// The device string is the daemon's only device authority: it is parsed
    /// once at startup and handed to ABI's `LocalModelProvider::load`, which
    /// fails closed on an unavailable device instead of substituting another
    /// one. `cpu`, `metal`, and `cuda` are the only accepted values.
    fn start_with_device(device: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "abbey-daemon-model-lifecycle-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let state = root.join("state");
        let workspace = root.join("workspace");
        let storage = StorageRoot::at(root.join("models"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manifest = write_artifacts_and_manifest(&storage, &root);
        let acceptance = root.join("acceptance.jsonl");
        AcceptanceLedger::load(&acceptance)
            .unwrap()
            .accept(&manifest, PRINCIPAL)
            .unwrap();
        std::fs::set_permissions(&acceptance, std::fs::Permissions::from_mode(0o600)).unwrap();

        let signing = SigningKey::from_bytes(&[31; 32]);
        let manifest_json = manifest.to_json().unwrap();
        let signature = signing.sign(manifest_json.as_bytes());
        let registry = root.join("registry.json");
        std::fs::write(
            &registry,
            json!([{
                "version": abi_models::SIGNED_MANIFEST_VERSION,
                "key_id": "fixture-publisher",
                "manifest_json": manifest_json,
                "signature": hex(&signature.to_bytes())
            }])
            .to_string(),
        )
        .unwrap();
        let config = root.join("model-runtime.json");
        std::fs::write(
            &config,
            json!({
                "version": 1,
                "registry_path": registry,
                "publisher_keys": {
                    "fixture-publisher": hex(&signing.verifying_key().to_bytes())
                },
                "acceptance_ledger_path": acceptance,
                "storage_root": storage.path(),
                "principal": PRINCIPAL,
                "device": device,
                "max_artifact_bytes": 1024 * 1024
            })
            .to_string(),
        )
        .unwrap();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
        let socket = root.join("abbeyd.sock");
        let child = spawn_daemon(&state, &workspace, &socket, &config);
        let mut harness = Self {
            root,
            state,
            workspace,
            socket,
            config,
            child,
        };
        harness.wait_ready();
        harness
    }

    fn restart(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        let _ = std::fs::remove_file(&self.socket);
        self.child = spawn_daemon(&self.state, &self.workspace, &self.socket, &self.config);
        self.wait_ready();
    }

    fn wait_ready(&mut self) {
        // UnixListener::bind (src/daemon/server/unix.rs) creates the socket path
        // at bind(2) before it calls listen(2); a connect attempted in that gap
        // gets ECONNREFUSED even though the path already exists. Probe with a
        // real (immediately dropped) connect instead of just checking the path.
        let deadline = Instant::now() + Duration::from_secs(3);
        while UnixStream::connect(&self.socket).is_err() {
            assert!(
                Instant::now() < deadline,
                "abbeyd did not start accepting connections"
            );
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "abbeyd exited before creating its socket"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn session(&self) -> V3DaemonSession {
        self.session_with_read_timeout(Duration::from_secs(5))
    }

    /// A session with an explicit client read deadline.
    ///
    /// The deadline is a *client* bound, not the daemon's run budget: it
    /// decides how long the client waits for a response that the daemon has
    /// already begun producing. The Metal branch needs a longer one because
    /// a cold device compiles its shader before the daemon's first event.
    fn session_with_read_timeout(&self, read_timeout: Duration) -> V3DaemonSession {
        let requested = V3CapabilitySet::from_sorted(vec![
            V3Capability::ReadModels,
            V3Capability::DownloadModels,
            V3Capability::ManageModels,
            V3Capability::InferModels,
        ])
        .unwrap();
        let mut config =
            DaemonConfig::local(self.socket.clone(), BearerSecret::parse(BEARER).unwrap());
        config.read_timeout = read_timeout;
        DaemonClient::new(config).negotiate_v3(requested).unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn real_daemon_downloads_loads_reports_unloads_and_reopens_exact_model_state() {
    let mut harness = Harness::start();
    let session = harness.session();
    assert_eq!(
        session.negotiation().granted.as_slice(),
        &[
            V3Capability::ReadModels,
            V3Capability::DownloadModels,
            V3Capability::ManageModels,
            V3Capability::InferModels,
        ]
    );
    let models = session.list_models(V3PageQuery::default()).unwrap();
    assert_eq!(models.records.len(), 1);
    assert_eq!(models.records[0].id, MODEL_ID);

    assert_eq!(
        session
            .download_model(action("download-real-1"))
            .unwrap()
            .state,
        V3OperationState::Queued
    );
    assert_eq!(
        wait_for(&session, "download-real-1", true).state,
        V3OperationState::Succeeded
    );
    assert_eq!(
        session.load_model(action("load-real-1")).unwrap().state,
        V3OperationState::Queued
    );
    assert_eq!(
        wait_for(&session, "load-real-1", false).state,
        V3OperationState::Succeeded
    );
    let readiness = session
        .inference_status(V3ResourceQuery {
            resource_id: MODEL_ID.to_owned(),
        })
        .unwrap();
    assert_eq!(readiness.state, V3OperationState::Available);
    assert_eq!(readiness.progress_basis_points, 0);
    let inference_request = V3ModelInferenceRequest::new(MODEL_ID, REVISION, "hello", 4).unwrap();
    let inference = session.infer_model(inference_request.clone()).unwrap();
    assert_eq!(inference.output, "world again");
    assert_eq!(inference.requested_device, V3ModelDevice::Cpu);
    assert_eq!(inference.executed_device, V3ModelDevice::Cpu);
    assert!(inference.native_operations > 0);
    assert!(!inference.fallback_used);
    assert!(!inference.mixed_execution);
    assert_eq!(
        session.unload_model(action("unload-real-1")).unwrap().state,
        V3OperationState::Succeeded
    );
    assert!(matches!(
        session.infer_model(inference_request.clone()),
        Err(ClientError::DaemonV3 {
            code: V3ErrorCode::NotFound,
            ..
        })
    ));
    drop(session);

    harness.restart();
    let reopened = harness.session();
    assert_eq!(
        reopened
            .model_download_status(V3ResourceQuery {
                resource_id: "download-real-1".to_owned(),
            })
            .unwrap()
            .state,
        V3OperationState::Succeeded
    );
    assert!(matches!(
        reopened.inference_status(V3ResourceQuery {
            resource_id: MODEL_ID.to_owned(),
        }),
        Err(ClientError::DaemonV3 {
            code: V3ErrorCode::NotFound,
            ..
        })
    ));
    assert!(matches!(
        reopened.infer_model(inference_request),
        Err(ClientError::DaemonV3 {
            code: V3ErrorCode::NotFound,
            ..
        })
    ));

    let database = std::fs::read(harness.state.join("daemon/runtime.sqlite")).unwrap();
    let rendered = String::from_utf8_lossy(&database);
    assert!(!rendered.contains("models.example.invalid"));
    assert!(!rendered.contains(PRINCIPAL));
    assert!(!rendered.contains("hello"));
    assert!(!rendered.contains("world again"));
}

/// Real-process proof that the startup-selected Metal device never silently
/// falls back to some other device.
///
/// The daemon's startup document is the only device authority: the requested
/// device is parsed once at startup and handed to ABI's
/// `LocalModelProvider::load`. A real Metal device exists only when Abbey is
/// built with `--features accel` (which turns on ABI's
/// `abi-model-runtime/metal`) *and* the host actually exposes one. Every
/// other host must fail closed: the model is never advertised as available,
/// inference is refused, and no CPU execution stands in for the requested
/// device. The branch is decided at runtime by what the daemon reports, so
/// the same assertions hold in every build cohort.
#[test]
fn real_daemon_runs_the_exact_fixture_on_metal_without_falling_back() {
    let harness = Harness::start_with_device("metal");
    // A cold Metal device compiles its shader before the daemon's first
    // event, and the pinned ABI's run budget is not consulted at that
    // point, so the *client* deadline is the bound that decides a cold
    // attempt. Give this branch a deadline that can outlast a first-use
    // shader compile. The daemon's own budget, the fail-closed guarantee,
    // and the default cohort are unchanged.
    let session = harness.session_with_read_timeout(METAL_READ_TIMEOUT);

    // The startup document is the only device authority: the daemon must
    // report exactly the configured Metal device before anything is loaded.
    let details = session
        .model_details(V3ResourceQuery {
            resource_id: MODEL_ID.to_owned(),
        })
        .unwrap();
    assert_eq!(details.model_id, MODEL_ID);
    assert_eq!(details.revision, REVISION);
    assert_eq!(details.configured_device, V3ModelDevice::Metal);
    assert!(details.license_accepted);
    assert!(!details.loaded);

    assert_eq!(
        session
            .download_model(action("download-metal-1"))
            .unwrap()
            .state,
        V3OperationState::Queued
    );
    assert_eq!(
        wait_for(&session, "download-metal-1", true).state,
        V3OperationState::Succeeded
    );

    assert_eq!(
        session.load_model(action("load-metal-1")).unwrap().state,
        V3OperationState::Queued
    );
    let loaded = wait_for(&session, "load-metal-1", false);
    assert!(
        matches!(
            loaded.state,
            V3OperationState::Succeeded | V3OperationState::Failed
        ),
        "the Metal load must reach a terminal state, saw {:?}",
        loaded.state
    );

    let request = V3ModelInferenceRequest::new(MODEL_ID, REVISION, "hello", 4).unwrap();
    match loaded.state {
        V3OperationState::Succeeded => {
            eprintln!(
                "metal branch: native — a real Metal device was compiled in and initialized, so the \
                 daemon loaded the exact fixture on Metal; native inference evidence is required next"
            );
            let readiness = session
                .inference_status(V3ResourceQuery {
                    resource_id: MODEL_ID.to_owned(),
                })
                .unwrap();
            assert_eq!(readiness.state, V3OperationState::Available);
            assert_eq!(readiness.progress_basis_points, 0);
            // The daemon bounds one exact-model run at a single second
            // (src/daemon/runtime_v3/model_lifecycle/model_inference.rs). Candle
            // compiles its Metal shader source on first use and caches the
            // compiled library in-process, so a cold Metal device can overrun
            // that ceiling once: the daemon then refuses to report the run
            // rather than substituting a device. Re-issue the identical request
            // a bounded number of times so a warm run can answer; every
            // budget refusals use the typed BudgetExceeded code; other daemon
            // failures remain fatal. Every attempt that answers must carry
            // native Metal evidence,
            // and a run that never answers fails the test rather than
            // downgrading the assertion.
            //
            // The client deadline is what decides a cold attempt, because the
            // pinned ABI revision's `RunContext::finish` reports completion
            // without consulting the elapsed duration, and the cold compile
            // happens before the daemon's first event. This branch uses
            // `METAL_READ_TIMEOUT` for that reason. A client read deadline is
            // still retried the same way, because it is the same "not
            // answered in time" outcome; a client that can never get an
            // answer still fails the test.
            let mut answered = None;
            let mut refusals: Vec<String> = Vec::new();
            for _ in 0..METAL_INFERENCE_ATTEMPTS {
                match session.infer_model(request.clone()) {
                    Ok(result) => {
                        answered = Some(result);
                        break;
                    }
                    Err(ClientError::DaemonV3 {
                        code: V3ErrorCode::BudgetExceeded,
                        message,
                    }) => refusals.push(format!("budget_exceeded: {message}")),
                    Err(ClientError::Read(source))
                        if matches!(
                            source.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        refusals.push(format!("client read deadline elapsed: {source}"));
                    }
                    Err(other) => {
                        panic!("unexpected exact-model inference failure: {other:?}");
                    }
                }
            }
            let inference = answered.unwrap_or_else(|| {
                panic!(
                    "the daemon refused every bounded exact-model inference attempt: {refusals:?}"
                )
            });
            eprintln!(
                "metal inference: {} bounded cold-start refusal(s)",
                refusals.len()
            );
            eprintln!(
                "metal branch: output={:?} requested_device={:?} executed_device={:?} \
                 native_operations={} fallback_used={} mixed_execution={}",
                inference.output,
                inference.requested_device,
                inference.executed_device,
                inference.native_operations,
                inference.fallback_used,
                inference.mixed_execution,
            );
            assert_eq!(inference.output, "world again");
            assert_eq!(inference.requested_device, V3ModelDevice::Metal);
            assert_eq!(inference.executed_device, V3ModelDevice::Metal);
            assert!(inference.native_operations > 0);
            assert!(!inference.fallback_used);
            assert!(!inference.mixed_execution);
        }
        V3OperationState::Failed => {
            eprintln!(
                "metal branch: fail-closed — no Metal device was available in this build or on this \
                 host, so the daemon refused to load the model instead of substituting a device"
            );
            // The model is not advertised as available.
            assert!(matches!(
                session.inference_status(V3ResourceQuery {
                    resource_id: MODEL_ID.to_owned(),
                }),
                Err(ClientError::DaemonV3 {
                    code: V3ErrorCode::NotFound,
                    ..
                })
            ));
            // Inference is refused for a model that was never loaded.
            assert!(matches!(
                session.infer_model(request.clone()),
                Err(ClientError::DaemonV3 {
                    code: V3ErrorCode::NotFound,
                    ..
                })
            ));
            // Nothing was loaded, so no CPU execution stood in for Metal.
            let details = session
                .model_details(V3ResourceQuery {
                    resource_id: MODEL_ID.to_owned(),
                })
                .unwrap();
            assert!(!details.loaded);
            assert_eq!(details.configured_device, V3ModelDevice::Metal);
        }
        other => panic!("unexpected model load state: {other:?}"),
    }

    // Operation ids are globally single-use: the terminal load id cannot drive
    // a second load, so a failed Metal attempt is never silently replayed.
    assert!(matches!(
        session.load_model(action("load-metal-1")),
        Err(ClientError::DaemonV3 {
            code: V3ErrorCode::Conflict,
            ..
        })
    ));

    // The durable store keeps neither the prompt nor the output.
    let database = std::fs::read(harness.state.join("daemon/runtime.sqlite")).unwrap();
    let rendered = String::from_utf8_lossy(&database);
    assert!(!rendered.contains("models.example.invalid"));
    assert!(!rendered.contains(PRINCIPAL));
    assert!(!rendered.contains("hello"));
    assert!(!rendered.contains("world again"));
}

fn wait_for(
    session: &V3DaemonSession,
    operation_id: &str,
    download: bool,
) -> abbey::app_core::V3OperationStatus {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let query = V3ResourceQuery {
            resource_id: operation_id.to_owned(),
        };
        let status = if download {
            session.model_download_status(query)
        } else {
            session.inference_status(query)
        }
        .unwrap();
        if matches!(
            status.state,
            V3OperationState::Succeeded | V3OperationState::Failed
        ) {
            return status;
        }
        assert!(Instant::now() < deadline, "model operation timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn action(operation_id: &str) -> V3ModelAction {
    V3ModelAction {
        model_id: MODEL_ID.to_owned(),
        revision: REVISION.to_owned(),
        operation_id: operation_id.to_owned(),
    }
}

fn spawn_daemon(state: &Path, workspace: &Path, socket: &Path, config: &Path) -> Child {
    Command::new(ABBEYD_BIN)
        .current_dir(workspace)
        // This fixture owns exactly one signed lifecycle catalog. Keep model
        // providers and read-only manifest catalogs exported by the outer gate
        // from silently extending that inventory.
        .env_remove(edition::ACTIVE.scoped_env("ABI_BIN"))
        .env_remove(edition::ACTIVE.scoped_env("MODEL_MANIFEST_DIR"))
        .env_remove(edition::ACTIVE.scoped_env("MODEL_RUNTIME_CONFIG"))
        .env_remove("ABI_MODELS_DIR")
        .env(edition::ACTIVE.state_dir_env(), state)
        .env(
            edition::ACTIVE.config_path_env(),
            workspace.join("abbey.toml"),
        )
        .env(edition::ACTIVE.daemon_socket_env(), socket)
        .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
        .env(edition::ACTIVE.scoped_env("MODEL_RUNTIME_CONFIG"), config)
        .env("ABBEY_MEMORY_BACKEND", "sqlite")
        .spawn()
        .unwrap()
}

fn write_artifacts_and_manifest(storage: &StorageRoot, root: &Path) -> ModelManifest {
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    let source_weights = source.join("model.safetensors");
    let source_tokenizer = source.join("tokenizer.json");
    std::fs::write(&source_weights, safetensors_fixture()).unwrap();
    std::fs::write(&source_tokenizer, tokenizer_fixture()).unwrap();
    let manifest = ModelManifest::from_json(
        "real daemon fixture",
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
                    "sha256": hash_file(&source_weights).unwrap().to_hex(),
                    "size_bytes": std::fs::metadata(&source_weights).unwrap().len(),
                    "url": format!("https://models.example.invalid/{REVISION}/model.safetensors")
                },
                {
                    "path": "tokenizer.json",
                    "kind": "tokenizer",
                    "sha256": hash_file(&source_tokenizer).unwrap().to_hex(),
                    "size_bytes": std::fs::metadata(&source_tokenizer).unwrap().len(),
                    "url": format!("https://models.example.invalid/{REVISION}/tokenizer.json")
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    for (artifact, source) in manifest
        .artifacts
        .iter()
        .zip([source_weights, source_tokenizer])
    {
        let destination = storage.artifact_path(&manifest, artifact);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(source, destination).unwrap();
    }
    manifest
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

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}
