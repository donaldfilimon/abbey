//! Real-process proof for approved safe memory execution and crash recovery.

#![cfg(all(unix, not(feature = "personal-edition")))]

use abbey::app_core::{
    V3Capability, V3CapabilitySet, V3OperationState, V3ToolCall, V3ToolDecision, V3ToolInvocation,
};
use abbey::daemon::{BearerSecret, ClientError, DaemonClient, DaemonConfig, V3DaemonSession};
use abbey::edition;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-daemon-tool-execution-bearer-0001";

struct Harness {
    root: PathBuf,
    socket: PathBuf,
    backend: &'static str,
    child: Option<Child>,
}

impl Harness {
    fn start(backend: &'static str, failpoint: Option<&str>) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "abbey-daemon-tool-execution-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("abbeyd.sock");
        let mut harness = Self {
            root,
            socket,
            backend,
            child: None,
        };
        harness.spawn(failpoint);
        harness
    }

    fn spawn(&mut self, failpoint: Option<&str>) {
        let _ = std::fs::remove_file(&self.socket);
        let mut daemon = Command::new(ABBEYD_BIN);
        daemon
            .current_dir(&self.root)
            .env(edition::ACTIVE.state_dir_env(), &self.root)
            .env(
                edition::ACTIVE.config_path_env(),
                self.root.join("config.toml"),
            )
            .env(edition::ACTIVE.daemon_socket_env(), &self.socket)
            .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
            .env("ABBEY_MEMORY_BACKEND", self.backend)
            .env_remove("ABBEY_TEST_TOOL_EXECUTION_FAILPOINT");
        if let Some(failpoint) = failpoint {
            daemon.env("ABBEY_TEST_TOOL_EXECUTION_FAILPOINT", failpoint);
        }
        self.child = Some(daemon.spawn().expect("start real abbeyd"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.socket.exists() {
            assert!(Instant::now() < deadline, "abbeyd socket was not created");
            assert!(
                self.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                "abbeyd exited before creating its socket"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn restart(&mut self) {
        if let Some(mut child) = self.child.take()
            && child.try_wait().unwrap().is_none()
        {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        self.spawn(None);
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "failpoint daemon did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn session(&self) -> V3DaemonSession {
        let requested = V3CapabilitySet::from_sorted(vec![
            V3Capability::InvokeTools,
            V3Capability::DecideToolApprovals,
        ])
        .unwrap();
        DaemonClient::new(DaemonConfig::local(
            self.socket.clone(),
            BearerSecret::parse(BEARER).unwrap(),
        ))
        .negotiate_v3(requested)
        .expect("negotiate safe tool execution")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(ABBEY_BIN);
        command
            .args(args)
            .current_dir(&self.root)
            .env(edition::ACTIVE.state_dir_env(), &self.root)
            .env(
                edition::ACTIVE.config_path_env(),
                self.root.join("config.toml"),
            )
            .env("ABBEY_MEMORY_BACKEND", self.backend)
            .env_remove(edition::ACTIVE.daemon_bearer_env())
            .env_remove(edition::ACTIVE.daemon_bearer_file_env())
            .env_remove("ABBEY_TEST_TOOL_EXECUTION_FAILPOINT");
        command
    }

    fn seed_memory(&self, summary: &str) -> String {
        let output = self
            .command(&["memory", "put", summary])
            .output()
            .expect("seed memory through the real CLI");
        assert!(output.status.success(), "memory put failed: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn memory_is_obsolete(&self, record_id: &str) -> bool {
        let output = self
            .command(&["memory", "get", record_id])
            .output()
            .expect("read memory through the real CLI");
        assert!(output.status.success(), "memory get failed: {output:?}");
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["obsolete"]
            .as_bool()
            .unwrap()
    }

    fn runtime_database(&self) -> PathBuf {
        self.root.join("daemon/runtime.sqlite")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn call(call_id: &str, record_id: &str) -> V3ToolCall {
    V3ToolCall {
        tool_id: "abbey_memory_mark_obsolete".to_owned(),
        call_id: call_id.to_owned(),
        input: serde_json::json!({"record_id": record_id}),
    }
}

fn approve(session: &V3DaemonSession, call: &V3ToolCall, decision_id: &str) {
    let V3ToolInvocation::ApprovalRequired(pending) = session
        .request_tool(call.clone())
        .expect("first exact request must become pending")
    else {
        panic!("mutation executed before approval");
    };
    session
        .approve_tool(V3ToolDecision {
            call_id: call.call_id.clone(),
            call_digest: pending.call_digest,
            decision_id: decision_id.to_owned(),
        })
        .expect("approve exact pending call");
}

#[test]
fn approved_exact_resubmission_executes_through_the_configured_memory_backend() {
    let backends = if cfg!(feature = "wdbx") {
        vec!["sqlite", "wdbx"]
    } else {
        vec!["sqlite"]
    };
    for backend in backends {
        let mut harness = Harness::start(backend, None);
        let record_id = harness.seed_memory(&format!("served effect through {backend}"));
        let call = call(&format!("served-{backend}-call"), &record_id);
        let session = harness.session();
        approve(&session, &call, &format!("served-{backend}-decision"));
        assert!(!harness.memory_is_obsolete(&record_id));
        drop(session);
        harness.restart();
        let session = harness.session();

        let V3ToolInvocation::Completed(result) = session
            .request_tool(call.clone())
            .expect("approved exact resubmission must complete")
        else {
            panic!("approved resubmission returned another approval");
        };
        assert_eq!(result.state, V3OperationState::Succeeded);
        assert_eq!(result.output["record_id"], record_id);
        assert!(harness.memory_is_obsolete(&record_id));

        let connection = rusqlite::Connection::open(harness.runtime_database()).unwrap();
        let (approval_state, execution_state, digest_length): (String, String, i64) = connection
            .query_row(
                "SELECT a.state, e.state, length(e.result_digest)
                 FROM tool_approvals a JOIN tool_executions e USING(call_id)
                 WHERE a.call_id=?1",
                [&call.call_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(approval_state, "consumed");
        assert_eq!(execution_state, "succeeded");
        assert_eq!(digest_length, 64);
        let audit = connection
            .prepare(
                "SELECT metadata_json FROM audit_events
                 WHERE json_extract(metadata_json, '$.call_id')=?1 ORDER BY id",
            )
            .unwrap()
            .query_map([&call.call_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(audit.len(), 2);
        assert!(audit.iter().all(|metadata| {
            !metadata.contains(&record_id) && !metadata.contains("served effect through")
        }));
    }
}

#[test]
fn prepared_effect_failpoints_reopen_as_interrupted_and_require_a_fresh_call() {
    for (failpoint, effect_happened) in [("after_prepare", false), ("after_effect", true)] {
        let mut harness = Harness::start("sqlite", Some(failpoint));
        let record_id = harness.seed_memory(&format!("crash proof {failpoint}"));
        let original = call(&format!("crash-{failpoint}-call"), &record_id);
        let session = harness.session();
        approve(&session, &original, &format!("crash-{failpoint}-decision"));
        assert!(session.request_tool(original.clone()).is_err());
        let status = harness.wait_for_exit();
        assert_eq!(status.code(), Some(86));

        let connection = rusqlite::Connection::open(harness.runtime_database()).unwrap();
        let before_reopen: (String, String, Option<String>) = connection
            .query_row(
                "SELECT a.state, e.state, e.result_digest
                 FROM tool_approvals a JOIN tool_executions e USING(call_id)
                 WHERE a.call_id=?1",
                [&original.call_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(before_reopen.0, "consumed");
        assert_eq!(before_reopen.1, "prepared");
        assert_eq!(before_reopen.2, None);
        drop(connection);
        assert_eq!(harness.memory_is_obsolete(&record_id), effect_happened);

        harness.restart();
        let reopened = harness.session();
        assert!(matches!(
            reopened.request_tool(original.clone()),
            Err(ClientError::DaemonV3 { .. })
        ));
        let connection = rusqlite::Connection::open(harness.runtime_database()).unwrap();
        let (state, digest): (String, Option<String>) = connection
            .query_row(
                "SELECT state, result_digest FROM tool_executions WHERE call_id=?1",
                [&original.call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "interrupted");
        assert_eq!(digest, None);
        drop(connection);

        let fresh = call(&format!("fresh-{failpoint}-call"), &record_id);
        approve(&reopened, &fresh, &format!("fresh-{failpoint}-decision"));
        let V3ToolInvocation::Completed(result) = reopened
            .request_tool(fresh)
            .expect("fresh exact call may retry an interrupted idempotent effect")
        else {
            panic!("fresh approved retry did not complete");
        };
        assert_eq!(result.state, V3OperationState::Succeeded);
        assert!(harness.memory_is_obsolete(&record_id));
    }
}
