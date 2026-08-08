//! Real-binary proof for Abbey's authenticated read-only daemon CLI.

#![cfg(unix)]

use abbey::app_core::{AppEvent, ClaimStatus, Edition, RuntimeState};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-daemon-cli-test-bearer-0001";

struct Harness {
    root: PathBuf,
    socket: PathBuf,
    child: Child,
}

impl Harness {
    fn start() -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "abbey-dcli-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir(&root).expect("create daemon CLI scratch directory");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("make daemon CLI scratch directory private");
        let socket = root.join("abbeyd.sock");
        let child = Command::new(ABBEYD_BIN)
            .env("ABBEY_STATE_DIR", &root)
            .env("ABBEYD_SOCKET_PATH", &socket)
            .env("ABBEYD_BEARER_TOKEN", BEARER)
            .spawn()
            .expect("start abbeyd");

        let deadline = Instant::now() + Duration::from_secs(3);
        while !socket.exists() {
            assert!(Instant::now() < deadline, "abbeyd socket was not created");
            std::thread::sleep(Duration::from_millis(10));
        }
        Self {
            root,
            socket,
            child,
        }
    }

    fn abbey(&self, bearer: &str, args: &[&str]) -> std::process::Output {
        self.command(args)
            .env("ABBEYD_BEARER_TOKEN", bearer)
            .output()
            .expect("run abbey daemon command")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(ABBEY_BIN);
        command
            .args(args)
            .env("ABBEY_STATE_DIR", &self.root)
            .env("ABBEYD_SOCKET_PATH", &self.socket)
            .env_remove("ABBEYD_BEARER_TOKEN")
            .env_remove("ABBEYD_BEARER_TOKEN_FILE")
            // A local control-plane query must not resolve a model executor.
            .env("ABBEY_AGENT_BIN", self.root.join("missing-agent"));
        command
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
fn status_and_filtered_claims_round_trip_through_real_binaries() {
    let harness = Harness::start();

    let human = harness.abbey(BEARER, &["daemon", "status"]);
    assert_success(&human, "daemon human status");
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("abbeyd: ready (standard edition)"));
    assert!(human.contains("capabilities: read_status, read_claims"));

    let status = harness.abbey(BEARER, &["daemon", "status", "--json"]);
    assert_success(&status, "daemon status");
    let status_event: AppEvent = serde_json::from_slice(&status.stdout).unwrap();
    let AppEvent::Status(status) = status_event else {
        panic!("daemon status returned the wrong event");
    };
    assert_eq!(status.edition, Edition::Standard);
    assert_eq!(status.state, RuntimeState::Ready);

    let claims = harness.abbey(
        BEARER,
        &[
            "daemon",
            "claims",
            "--status",
            "blocked",
            "--contains",
            "linux",
            "--json",
        ],
    );
    assert_success(&claims, "daemon claims");
    let claims_event: AppEvent = serde_json::from_slice(&claims.stdout).unwrap();
    let AppEvent::Claims(snapshot) = claims_event else {
        panic!("daemon claims returned the wrong event");
    };
    assert_eq!(snapshot.matched, 1);
    assert_eq!(snapshot.claims[0].status, ClaimStatus::Blocked);

    let proposed = harness.abbey(
        BEARER,
        &[
            "daemon",
            "claims",
            "--status",
            "proposed",
            "--contains",
            "desktop",
            "--json",
        ],
    );
    assert_success(&proposed, "daemon proposed claims");
    let event: AppEvent = serde_json::from_slice(&proposed.stdout).unwrap();
    let AppEvent::Claims(snapshot) = event else {
        panic!("daemon proposed claims returned the wrong event");
    };
    assert_eq!(snapshot.matched, 1);
    assert_eq!(snapshot.claims[0].status, ClaimStatus::Proposed);
}

#[test]
fn authentication_failure_does_not_disclose_local_secrets() {
    let harness = Harness::start();
    let wrong_bearer = "abbey-daemon-cli-test-bearer-wrong";
    let output = harness.abbey(wrong_bearer, &["daemon", "status"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(BEARER), "correct bearer leaked: {stderr}");
    assert!(
        !stderr.contains(wrong_bearer),
        "supplied bearer leaked: {stderr}"
    );
    assert!(
        !stderr.contains(path_text(&harness.socket)),
        "socket path leaked: {stderr}"
    );
    assert!(stderr.contains("authentication failed"), "stderr: {stderr}");

    let missing = harness
        .command(&["daemon", "status"])
        .output()
        .expect("run daemon command without bearer");
    assert!(!missing.status.success());
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_stderr.contains("set exactly one"));
    assert!(!missing_stderr.contains(path_text(&harness.socket)));

    let bearer_file = harness.root.join("bearer");
    std::fs::write(&bearer_file, BEARER).unwrap();
    std::fs::set_permissions(&bearer_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let conflicting = harness
        .command(&["daemon", "status"])
        .env("ABBEYD_BEARER_TOKEN", wrong_bearer)
        .env("ABBEYD_BEARER_TOKEN_FILE", &bearer_file)
        .output()
        .expect("run daemon command with conflicting bearer sources");
    assert!(!conflicting.status.success());
    let conflicting_stderr = String::from_utf8_lossy(&conflicting.stderr);
    assert!(conflicting_stderr.contains("cannot both be set"));
    assert!(!conflicting_stderr.contains(BEARER));
    assert!(!conflicting_stderr.contains(wrong_bearer));
}

fn assert_success(output: &std::process::Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("scratch socket path must be UTF-8")
}
