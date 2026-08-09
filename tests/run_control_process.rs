//! Real-binary parity proof for CLI and TUI-style /daemon run routing.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use abbey::app_core::RunState;
use abbey::edition;
use abbey::run_control::RunControlView;

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-run-control-process-bearer-0001";
const PRIVATE_PROMPT: &str = "private-run-control-prompt";

struct Harness {
    root: PathBuf,
    socket: PathBuf,
    provider: PathBuf,
    child: Child,
}

impl Harness {
    fn start() -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "abbey-run-control-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("abbeyd.sock");
        let provider = root.join("abi-provider");
        fs::write(
            &provider,
            "#!/bin/sh\ncase \"$*\" in *sleep-until-cancelled*) /bin/sleep 30 ;; esac\nprintf 'private-provider-output\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
        let child = Command::new(ABBEYD_BIN)
            .env(edition::ACTIVE.state_dir_env(), &root)
            .env(edition::ACTIVE.daemon_socket_env(), &socket)
            .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
            .env(edition::ACTIVE.scoped_env("ABI_BIN"), &provider)
            .spawn()
            .unwrap();
        let harness = Self {
            root,
            socket,
            provider,
            child,
        };
        harness.wait_ready();
        harness
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(ABBEY_BIN)
            .args(args)
            .env(edition::ACTIVE.state_dir_env(), &self.root)
            .env(edition::ACTIVE.daemon_socket_env(), &self.socket)
            .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
            .env("ABBEY_AGENT_BIN", self.root.join("missing-agent"))
            .output()
            .unwrap()
    }

    fn view(&self, args: &[&str]) -> RunControlView {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.contains(PRIVATE_PROMPT));
        assert!(!stdout.contains(BEARER));
        assert!(!stdout.contains("private-provider-output"));
        assert!(!stdout.contains(self.provider.to_str().unwrap()));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains(BEARER));
        assert!(!stderr.contains(self.provider.to_str().unwrap()));
        serde_json::from_str(&stdout).unwrap()
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let output = self.run(&["daemon", "status", "--json"]);
            if output.status.success() {
                return;
            }
            assert!(Instant::now() < deadline, "abbeyd did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_terminal(&self, run_id: &str) -> RunControlView {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let view = self.view(&["daemon", "run", "status", run_id, "--json"]);
            if view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.state.is_terminal())
            {
                return view;
            }
            assert!(Instant::now() < deadline, "run did not become terminal");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cli_and_slash_use_the_same_commands_reducer_and_sanitized_view() {
    let harness = Harness::start();
    let submitted = harness.view(&[
        "daemon",
        "run",
        "submit",
        "--backend",
        "abi",
        "--idempotency-key",
        "process-parity-request",
        "--json",
        PRIVATE_PROMPT,
    ]);
    let run_id = submitted.snapshot.as_ref().unwrap().run_id.to_string();

    let duplicate = harness.view(&[
        "/daemon",
        "run",
        "submit",
        "--backend",
        "abi",
        "--idempotency-key",
        "process-parity-request",
        "--json",
        PRIVATE_PROMPT,
    ]);
    assert_eq!(
        duplicate.snapshot.as_ref().unwrap().run_id.to_string(),
        run_id
    );

    let terminal = harness.wait_terminal(&run_id);
    assert_eq!(
        terminal.snapshot.as_ref().unwrap().state,
        RunState::Succeeded
    );
    let slash_status = harness.view(&["/daemon", "run", "status", &run_id, "--json"]);
    assert_eq!(slash_status, terminal);
    let human = harness.run(&["daemon", "run", "status", &run_id]);
    assert!(human.status.success());
    let human_output = String::from_utf8(human.stdout).unwrap();
    assert!(human_output.contains("state: succeeded"));
    for private in [
        PRIVATE_PROMPT,
        BEARER,
        "private-provider-output",
        harness.provider.to_str().unwrap(),
    ] {
        assert!(!human_output.contains(private));
    }

    let cli_events = harness.view(&["daemon", "run", "events", &run_id, "--limit", "2", "--json"]);
    let slash_events = harness.view(&[
        "/daemon", "run", "events", &run_id, "--limit", "2", "--json",
    ]);
    assert_eq!(slash_events, cli_events);
    let page = &cli_events.event_pages[0];
    assert_eq!(page.after_sequence, 0);
    assert!(page.through_sequence >= page.next_after_sequence);

    let cancellable = harness.view(&[
        "daemon",
        "run",
        "submit",
        "--backend",
        "abi",
        "--idempotency-key",
        "process-cancel-request",
        "--json",
        "sleep-until-cancelled",
    ]);
    let cancellable_id = cancellable.snapshot.as_ref().unwrap().run_id.to_string();
    let cancelled = harness.view(&["daemon", "run", "cancel", &cancellable_id, "--json"]);
    assert!(matches!(
        cancelled.snapshot.as_ref().unwrap().state,
        RunState::CancelRequested | RunState::Cancelled
    ));
    let slash_cancelled = harness.view(&["/daemon", "run", "status", &cancellable_id, "--json"]);
    assert!(matches!(
        slash_cancelled.snapshot.as_ref().unwrap().state,
        RunState::CancelRequested | RunState::Cancelled
    ));
}
