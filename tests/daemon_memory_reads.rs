//! Real-process proof for bounded, edition-scoped protocol-v3 memory reads.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use abbey::app_core::{
    V3Capability, V3CapabilitySet, V3ErrorCode, V3PageQuery, V3ResourceQuery, V3SearchRequest,
};
use abbey::daemon::{BearerSecret, ClientError, DaemonClient, DaemonConfig, V3DaemonSession};
use abbey::edition;

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-daemon-memory-read-bearer-0001";
const SPACE_ID: &str = "memory-v1-summary";

struct Harness {
    root: PathBuf,
    socket: PathBuf,
    backend: &'static str,
    child: Child,
}

impl Harness {
    fn start(backend: &'static str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "abbey-daemon-memory-read-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("abbeyd.sock");
        let child = Command::new(ABBEYD_BIN)
            .current_dir(&root)
            .env(edition::ACTIVE.state_dir_env(), &root)
            .env(edition::ACTIVE.config_path_env(), root.join("config.toml"))
            .env(edition::ACTIVE.daemon_socket_env(), &socket)
            .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
            .env("ABBEY_MEMORY_BACKEND", backend)
            .spawn()
            .expect("start real abbeyd");
        let mut harness = Self {
            root,
            socket,
            backend,
            child,
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while !harness.socket.exists() {
            assert!(Instant::now() < deadline, "abbeyd socket was not created");
            assert!(
                harness.child.try_wait().unwrap().is_none(),
                "abbeyd exited before creating its socket"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        harness
    }

    fn session(&self) -> V3DaemonSession {
        let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadMemory]).unwrap();
        DaemonClient::new(DaemonConfig::local(
            self.socket.clone(),
            BearerSecret::parse(BEARER).unwrap(),
        ))
        .negotiate_v3(requested)
        .expect("negotiate memory reads")
    }

    fn seed(&self, summary: &str, payload: &str, provenance: &str) -> String {
        let output = Command::new(ABBEY_BIN)
            .args([
                "memory",
                "put",
                summary,
                "--payload",
                payload,
                "--provenance",
                provenance,
                "--source-ref",
                "/private/source/canary",
            ])
            .current_dir(&self.root)
            .env(edition::ACTIVE.state_dir_env(), &self.root)
            .env(
                edition::ACTIVE.config_path_env(),
                self.root.join("config.toml"),
            )
            .env("ABBEY_MEMORY_BACKEND", self.backend)
            .output()
            .expect("seed memory through real abbey");
        assert!(output.status.success(), "memory seed failed: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
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
fn real_daemon_serves_only_sanitized_fixed_snapshot_memory_reads() {
    let backends = if cfg!(feature = "wdbx") {
        vec!["sqlite", "wdbx"]
    } else {
        vec!["sqlite"]
    };
    for backend in backends {
        let harness = Harness::start(backend);
        let raw_id = harness.seed(
            "deploy from cwd=/Users/alice/private safely",
            "RAW_PAYLOAD_CANARY",
            "RAW_PROVENANCE_CANARY",
        );
        let session = harness.session();
        assert_eq!(
            session.negotiation().granted.as_slice(),
            &[V3Capability::ReadMemory]
        );

        let spaces = session
            .list_memory_spaces(V3PageQuery::default())
            .expect("list memory spaces");
        assert_eq!(spaces.through, 1);
        assert_eq!(spaces.records[0].id, SPACE_ID);

        let first = session
            .search_memory(V3SearchRequest {
                space_id: SPACE_ID.to_owned(),
                query: "deploy".to_owned(),
                page: V3PageQuery {
                    after: 0,
                    through: None,
                    limit: 1,
                },
            })
            .expect("search sanitized summaries");
        let snapshot_through = first.through;
        assert_eq!(first.records.len(), 1);
        assert_eq!(first.records[0].label, "deploy from [path] safely");
        assert!(first.records[0].id.starts_with("memory-"));
        assert_ne!(first.records[0].id, raw_id);

        let metadata = session
            .read_memory_metadata(V3ResourceQuery {
                resource_id: first.records[0].id.clone(),
            })
            .expect("read sanitized metadata");
        assert_eq!(metadata, first.records[0]);

        let payload_search = session
            .search_memory(V3SearchRequest {
                space_id: SPACE_ID.to_owned(),
                query: "RAW_PAYLOAD_CANARY".to_owned(),
                page: V3PageQuery::default(),
            })
            .expect("payload does not participate in served search");
        assert!(payload_search.records.is_empty());
        let hidden_path_search = session
            .search_memory(V3SearchRequest {
                space_id: SPACE_ID.to_owned(),
                query: "alice".to_owned(),
                page: V3PageQuery::default(),
            })
            .expect("redacted paths do not participate in served search");
        assert!(hidden_path_search.records.is_empty());

        harness.seed("deploy after snapshot", "NEW_PAYLOAD", "NEW_PROVENANCE");
        let continuation = session
            .search_memory(V3SearchRequest {
                space_id: SPACE_ID.to_owned(),
                query: "deploy".to_owned(),
                page: V3PageQuery {
                    after: 1,
                    through: Some(snapshot_through),
                    limit: 32,
                },
            })
            .expect("continue the original fixed snapshot");
        assert!(continuation.records.is_empty());

        let rendered = serde_json::to_string(&(spaces, first, metadata, continuation)).unwrap();
        for private in [
            raw_id.as_str(),
            "RAW_PAYLOAD_CANARY",
            "RAW_PROVENANCE_CANARY",
            "/private/source/canary",
            "/Users/alice/private",
        ] {
            assert!(
                !rendered.contains(private),
                "served response leaked {private}"
            );
        }

        let future = session
            .search_memory(V3SearchRequest {
                space_id: SPACE_ID.to_owned(),
                query: "deploy".to_owned(),
                page: V3PageQuery {
                    after: 0,
                    through: Some(snapshot_through + 10),
                    limit: 1,
                },
            })
            .unwrap_err();
        assert!(matches!(
            future,
            ClientError::DaemonV3 {
                code: V3ErrorCode::InvalidCommand,
                ..
            }
        ));
    }
}
