//! Hermetic regressions through the existing owned execution entries.
//! No global environment writes, live providers or production test hooks.

use crate::agent::{AgentBackend, AgentConfig};
use crate::roles::WorkerRole;
use crate::state::AbbeyState;
use crate::stream::StreamTap;
use crate::subagents::{LaneKind, LanePlan};
use nix::errno::Errno;
use nix::sys::signal::{kill, killpg};
use nix::unistd::{Pid, getpgid};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const OBSERVATION_BOUND: Duration = Duration::from_secs(10);

struct Fixture {
    root: PathBuf,
    program: PathBuf,
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

impl Fixture {
    fn new(label: &str, held: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "abbey-owned-current-api-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("state/by-cwd")).unwrap();
        let program = root.join("check.sh");
        let script = if held {
            format!(
                r#"#!/bin/sh
fixture_root={}
if [ "$1" = fixture-leaf ]; then
  trap 'exit 0' TERM INT
  printf '%s\n' "$$" > "$fixture_root/leaf.pid"
  while [ ! -e "$fixture_root/release" ]; do /bin/sleep 0.02; done
  exit 0
fi
"$0" fixture-leaf &
leaf=$!
trap 'kill -TERM "$leaf" 2>/dev/null || :; wait "$leaf" 2>/dev/null || :; exit 130' TERM INT
printf '%s\n' "$$" > "$fixture_root/parent.pid"
while [ ! -s "$fixture_root/leaf.pid" ]; do /bin/sleep 0.02; done
printf 'ready\n' > "$fixture_root/ready"
while [ ! -e "$fixture_root/release" ]; do /bin/sleep 0.02; done
wait "$leaf"
printf 'fixture-success\n'
"#,
                shell_quote(&root)
            )
        } else {
            "#!/bin/sh\nprintf 'fixture-success\\n'\n".into()
        };
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root, program }
    }

    fn cfg(&self, backend: AgentBackend, tap: StreamTap) -> AgentConfig {
        let mut cfg =
            AgentConfig::fixed_provider_recipe(self.program.clone(), backend, "local".into());
        cfg.stream = Some(tap);
        cfg
    }

    fn state(&self) -> AbbeyState {
        AbbeyState {
            state_dir: self.root.join("state"),
            chat_file: self.root.join("state/chat-id"),
            model_file: self.root.join("state/model"),
            history_file: self.root.join("state/history.log"),
            cwd_dir: self.root.join("state/by-cwd"),
            per_cwd: false,
            cwd: self.root.clone(),
        }
    }

    fn wait_ready(&self) -> bool {
        let deadline = Instant::now() + OBSERVATION_BOUND;
        while Instant::now() < deadline {
            if self.root.join("ready").is_file() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn pid(&self, name: &str) -> Option<i32> {
        fs::read_to_string(self.root.join(name))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    fn release(&self) {
        fs::write(self.root.join("release"), b"fixture cleanup only\n").unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join("release"), b"fixture cleanup only\n");
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn tap() -> (StreamTap, mpsc::Receiver<crate::stream::StreamEvent>) {
    let (events, receiver) = mpsc::channel();
    (StreamTap::new(events), receiver)
}

fn absent(pid: Option<i32>) -> bool {
    pid.is_some_and(|pid| matches!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH)))
}

/// The fixture is released only after recording product completion and process
/// observations. On old code the release then permits natural join, so RED does
/// not strand the fixture or wait out a 30-minute product timeout.
fn observe_owned_cancel(
    fixture: &Fixture,
    tap: &StreamTap,
    operation: impl FnOnce() -> (bool, String) + Send + 'static,
) {
    let (done, outcome) = mpsc::channel();
    let owner = thread::spawn(move || {
        let _ = done.send(operation());
    });
    let ready = fixture.wait_ready();
    let parent = fixture.pid("parent.pid");
    let leaf = fixture.pid("leaf.pid");
    let group = parent.and_then(|pid| getpgid(Some(Pid::from_raw(pid))).ok());
    tap.cancel.cancel();
    let before_release = outcome.recv_timeout(OBSERVATION_BOUND).ok();
    let returned_before_release = before_release.is_some();
    let pids_gone_before_release = absent(parent) && absent(leaf);
    let owned_group_before_cancel = parent.is_some_and(|pid| group == Some(Pid::from_raw(pid)));
    let group_gone_before_release =
        parent.is_some_and(|pid| matches!(killpg(Pid::from_raw(pid), None), Err(Errno::ESRCH)));
    fixture.release();
    let final_outcome = before_release.or_else(|| outcome.recv_timeout(OBSERVATION_BOUND).ok());
    owner.join().unwrap();
    assert!(
        ready && parent.is_some() && leaf.is_some(),
        "private child and descendant never reached the held boundary"
    );
    assert!(
        returned_before_release,
        "cfg.stream cancellation did not complete before fixture release: {final_outcome:?}"
    );
    assert!(
        owned_group_before_cancel,
        "held process had no independently owned supervisor group"
    );
    assert!(
        pids_gone_before_release && group_gone_before_release,
        "completion preceded observed parent/descendant/group cleanup"
    );
    assert!(
        final_outcome.is_some_and(|(success, _)| !success),
        "cancelled work reported success"
    );
}

fn peer_plan(program: PathBuf) -> LanePlan {
    LanePlan {
        name: "owned-peer".into(),
        kind: LaneKind::Peer,
        role: WorkerRole::Max,
        persona_label: "abbey".into(),
        model: "local".into(),
        focus: None,
        peer_bin: Some("codex".into()),
        peer_path: Some(program),
    }
}

fn gate_args() -> Vec<String> {
    [
        "run",
        "--gate-only",
        "--max-rounds",
        "1",
        "--max-minutes",
        "1",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[test]
fn current_api_create_chat_control() {
    let fixture = Fixture::new("chat-control", false);
    let (tap, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Cursor, tap);
    assert_eq!(cfg.create_chat().unwrap(), "fixture-success");
}

#[test]
fn current_api_create_chat_cancellation_joins_before_generation() {
    let fixture = Fixture::new("chat-cancel", true);
    let (tap, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Cursor, tap.clone());
    observe_owned_cancel(&fixture, &tap, move || {
        let result = cfg.create_chat();
        (result.is_ok(), format!("{result:?}"))
    });
}

#[test]
fn current_api_peer_lane_control() {
    let fixture = Fixture::new("peer-control", false);
    let (tap, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, tap);
    let results = crate::subagents::run_plans(
        &cfg,
        &[peer_plan(fixture.program.clone())],
        "fixture prompt",
        1,
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].exit, 0);
    assert_eq!(results[0].stdout.trim(), "fixture-success");
}

#[test]
fn current_api_peer_lane_cancellation_joins_owned_descendant() {
    let fixture = Fixture::new("peer-cancel", true);
    let (tap, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, tap.clone());
    let plans = vec![peer_plan(fixture.program.clone())];
    observe_owned_cancel(&fixture, &tap, move || {
        let results = crate::subagents::run_plans(&cfg, &plans, "fixture prompt", 1);
        let success = !results.is_empty() && results.iter().all(|result| result.exit == 0);
        (success, format!("{results:?}"))
    });
}

#[test]
fn current_api_improve_gate_control() {
    assert!(
        std::env::var_os("ABBEY_CHECK_CMD").is_none(),
        "run this private gate fixture with ABBEY_CHECK_CMD absent; no test mutates global env"
    );
    let fixture = Fixture::new("gate-control", false);
    let (tap, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, tap);
    assert_eq!(
        crate::improve::dispatch(&cfg, &fixture.state(), &gate_args()).unwrap(),
        0
    );
}

#[test]
fn current_api_improve_gate_cancellation_joins_owned_descendant() {
    assert!(
        std::env::var_os("ABBEY_CHECK_CMD").is_none(),
        "run this private gate fixture with ABBEY_CHECK_CMD absent; no test mutates global env"
    );
    let fixture = Fixture::new("gate-cancel", true);
    let (tap, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, tap.clone());
    let state = fixture.state();
    observe_owned_cancel(&fixture, &tap, move || {
        let result = crate::improve::dispatch(&cfg, &state, &gate_args());
        let success = matches!(&result, Ok(0));
        (success, format!("{result:?}"))
    });
}

#[path = "owned_cancel_tests/diagnostic_tests.rs"]
mod diagnostic_tests;
