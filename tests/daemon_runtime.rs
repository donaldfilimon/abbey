//! Real-binary proof for authenticated protocol-v2 run control.

#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use abbey::app_core::{
    APP_PROTOCOL_VERSION, AppCommand, AppEvent, BackendSelection, IdempotencyKey, RunEventsQuery,
    RunId, RunMode, RunQuery, RunRequest, RunState, RunSubmissionDisposition,
};
use abbey::daemon::{BearerSecret, DaemonClient, DaemonConfig};
use abbey::edition;

const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-runtime-v2-test-bearer-00000001";
const PRIVATE_PROMPT: &str = "private-runtime-prompt-do-not-echo";
const PRIVATE_OUTPUT: &str = "private-provider-output-do-not-echo";

struct Harness {
    root: PathBuf,
    socket: PathBuf,
    provider: PathBuf,
    launches: PathBuf,
    provider_pid: PathBuf,
    daemon_stdout: PathBuf,
    daemon_stderr: PathBuf,
    child: Child,
}

impl Harness {
    fn start() -> Self {
        let root = scratch("runtime-v2");
        let socket = root.join("abbeyd.sock");
        let provider = root.join("abi-provider");
        let launches = root.join("launches.log");
        let provider_pid = root.join("provider.pid");
        let daemon_stdout = root.join("daemon.stdout");
        let daemon_stderr = root.join("daemon.stderr");
        write_provider(&provider, &launches, &provider_pid);
        let child = spawn_daemon(&root, &socket, &provider, &daemon_stdout, &daemon_stderr);
        let harness = Self {
            root,
            socket,
            provider,
            launches,
            provider_pid,
            daemon_stdout,
            daemon_stderr,
            child,
        };
        harness.wait_ready();
        harness
    }

    fn client(&self) -> DaemonClient {
        DaemonClient::new(DaemonConfig::local(
            &self.socket,
            BearerSecret::parse(BEARER).unwrap(),
        ))
    }

    fn restart(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
        self.child = spawn_daemon(
            &self.root,
            &self.socket,
            &self.provider,
            &self.daemon_stdout,
            &self.daemon_stderr,
        );
        self.wait_ready();
    }

    fn terminate(&mut self) {
        let pid = nix::unistd::Pid::from_raw(i32::try_from(self.child.id()).unwrap());
        nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "abbeyd SIGTERM shutdown failed: {status}");
                return;
            }
            assert!(Instant::now() < deadline, "abbeyd ignored SIGTERM");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.socket.exists() && self.client().request(AppCommand::Status).is_ok() {
                return;
            }
            assert!(Instant::now() < deadline, "abbeyd did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_terminal(&self, run_id: &RunId) -> abbey::app_core::RunSnapshot {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let event = self
                .client()
                .request(AppCommand::GetRun(RunQuery {
                    run_id: run_id.clone(),
                }))
                .unwrap();
            let AppEvent::RunStatus(snapshot) = event else {
                panic!("expected run status");
            };
            if snapshot.state.is_terminal() {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "run did not become terminal");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_daemon_did_not_disclose(&self, values: &[&str]) {
        let mut output = fs::read_to_string(&self.daemon_stdout).unwrap_or_default();
        output.push_str(&fs::read_to_string(&self.daemon_stderr).unwrap_or_default());
        for value in values {
            assert!(
                !output.contains(value),
                "daemon output disclosed private data"
            );
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
fn real_daemon_v2_is_idempotent_paged_cancellable_and_reopenable() {
    let mut harness = Harness::start();
    let client = harness.client();

    let AppEvent::Status(status) = client.request(AppCommand::Status).unwrap() else {
        panic!("expected runtime status");
    };
    assert_eq!(status.protocol_version, APP_PROTOCOL_VERSION);
    assert!(
        status
            .run_routes
            .iter()
            .any(|route| route.backend == BackendSelection::Abi)
    );

    let initial_request = request("real-v2-idempotent", PRIVATE_PROMPT);
    let AppEvent::RunSubmitted(first) = client
        .request(AppCommand::SubmitRun(initial_request.clone()))
        .unwrap()
    else {
        panic!("expected run submission");
    };
    assert_eq!(first.disposition, RunSubmissionDisposition::Enqueued);
    let run_id = first.run.run_id.clone();
    assert_eq!(harness.wait_terminal(&run_id).state, RunState::Succeeded);
    let first_provider_pid = wait_for_pid(&harness.provider_pid);

    let AppEvent::RunSubmitted(duplicate) = client
        .request(AppCommand::SubmitRun(initial_request))
        .unwrap()
    else {
        panic!("expected duplicate run submission");
    };
    assert_eq!(duplicate.disposition, RunSubmissionDisposition::Existing);
    assert_eq!(duplicate.run.run_id, run_id);
    assert_eq!(line_count(&harness.launches), 1);

    let mut after = 0;
    let mut through = None;
    let mut observed = Vec::new();
    loop {
        let AppEvent::RunEvents(page) = client
            .request(AppCommand::RunEvents(RunEventsQuery {
                run_id: run_id.clone(),
                after_sequence: after,
                through_sequence: through,
                limit: 2,
            }))
            .unwrap()
        else {
            panic!("expected run event page");
        };
        let encoded = serde_json::to_string(&page).unwrap();
        for private in [
            PRIVATE_PROMPT,
            PRIVATE_OUTPUT,
            BEARER,
            harness.provider.to_str().unwrap(),
        ] {
            assert!(
                !encoded.contains(private),
                "event page disclosed private data"
            );
        }
        through = Some(page.through_sequence);
        after = page.next_after_sequence;
        observed.extend(page.events);
        if !page.has_more {
            break;
        }
    }
    assert!(!observed.is_empty());
    assert_eq!(after, through.unwrap());

    fs::remove_file(&harness.provider_pid).unwrap();
    let sleeping = request("real-v2-cancel", "sleep-until-cancelled");
    let AppEvent::RunSubmitted(sleeping) = client.request(AppCommand::SubmitRun(sleeping)).unwrap()
    else {
        panic!("expected sleeping run submission");
    };
    let sleeping_id = sleeping.run.run_id;
    let provider_pid = wait_for_pid(&harness.provider_pid);
    assert_ne!(provider_pid, first_provider_pid);
    assert_process_group_alive(provider_pid);
    let AppEvent::CancellationAcknowledged(acknowledged) = client
        .request(AppCommand::CancelRun(RunQuery {
            run_id: sleeping_id.clone(),
        }))
        .unwrap()
    else {
        panic!("expected cancellation acknowledgement");
    };
    assert!(matches!(
        acknowledged.state,
        RunState::CancelRequested | RunState::Cancelled
    ));
    assert_eq!(
        harness.wait_terminal(&sleeping_id).state,
        RunState::Cancelled
    );
    wait_process_group_absent(provider_pid);

    fs::remove_file(&harness.provider_pid).unwrap();
    let shutdown_run = request("real-v2-shutdown", "sleep-until-cancelled");
    let AppEvent::RunSubmitted(shutdown_run) =
        client.request(AppCommand::SubmitRun(shutdown_run)).unwrap()
    else {
        panic!("expected shutdown run submission");
    };
    let shutdown_id = shutdown_run.run.run_id;
    let shutdown_provider_pid = wait_for_pid(&harness.provider_pid);
    assert_ne!(shutdown_provider_pid, provider_pid);
    assert_process_group_alive(shutdown_provider_pid);
    harness.terminate();
    wait_process_group_absent(shutdown_provider_pid);

    harness.restart();
    let reopened = harness.wait_terminal(&run_id);
    assert_eq!(reopened.state, RunState::Succeeded);
    let AppEvent::RunEvents(reopened_page) = harness
        .client()
        .request(AppCommand::RunEvents(RunEventsQuery {
            run_id,
            after_sequence: 0,
            through_sequence: None,
            limit: 16,
        }))
        .unwrap()
    else {
        panic!("expected reopened event page");
    };
    assert!(!reopened_page.events.is_empty());
    assert_eq!(
        harness.wait_terminal(&shutdown_id).state,
        RunState::Cancelled
    );
    harness.assert_daemon_did_not_disclose(&[
        PRIVATE_PROMPT,
        PRIVATE_OUTPUT,
        BEARER,
        harness.provider.to_str().unwrap(),
    ]);
}

fn request(key: &str, input: &str) -> RunRequest {
    RunRequest {
        idempotency_key: key.parse::<IdempotencyKey>().unwrap(),
        conversation_id: None,
        mode: RunMode::Background,
        backend: BackendSelection::Abi,
        input: input.into(),
        labels: vec!["inert-label".into()],
    }
}

fn spawn_daemon(
    root: &Path,
    socket: &Path,
    provider: &Path,
    stdout: &Path,
    stderr: &Path,
) -> Child {
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(stdout)
        .unwrap();
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(stderr)
        .unwrap();
    Command::new(ABBEYD_BIN)
        .env(edition::ACTIVE.state_dir_env(), root)
        .env(edition::ACTIVE.config_path_env(), root.join("config.toml"))
        .env(edition::ACTIVE.daemon_socket_env(), socket)
        .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
        .env("ABBEY_MEMORY_BACKEND", "sqlite")
        .env(edition::ACTIVE.scoped_env("ABI_BIN"), provider)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap()
}

fn write_provider(path: &Path, launches: &Path, pid_file: &Path) {
    let body = format!(
        "#!/bin/sh\n\
         printf 'launch\\n' >> '{}'\n\
         printf '%s\\n' \"$$\" > '{}'\n\
         last=''\n\
         for arg in \"$@\"; do last=\"$arg\"; done\n\
         case \"$last\" in *sleep-until-cancelled*) /bin/sleep 30 ;; esac\n\
         printf '{}\\n'\n\
         printf '{}\\n' >&2\n",
        launches.display(),
        pid_file.display(),
        PRIVATE_OUTPUT,
        PRIVATE_OUTPUT,
    );
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn wait_for_pid(path: &Path) -> nix::unistd::Pid {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(value) = fs::read_to_string(path)
            && let Ok(raw) = value.trim().parse::<i32>()
        {
            return nix::unistd::Pid::from_raw(raw);
        }
        assert!(Instant::now() < deadline, "provider pid was not recorded");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_process_group_alive(pid: nix::unistd::Pid) {
    assert!(
        nix::sys::signal::killpg(pid, None).is_ok(),
        "provider process group was not alive"
    );
}

fn wait_process_group_absent(pid: nix::unistd::Pid) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while nix::sys::signal::killpg(pid, None).is_ok() {
        assert!(
            Instant::now() < deadline,
            "provider process group remained alive"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn line_count(path: &Path) -> usize {
    fs::read_to_string(path).unwrap_or_default().lines().count()
}

fn scratch(label: &str) -> PathBuf {
    let root = PathBuf::from("/tmp").join(format!(
        "abbey-{label}-{}-{}",
        std::process::id(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    root
}
