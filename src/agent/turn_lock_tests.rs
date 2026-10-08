//! A held private turn lock must not keep an owned resilient run alive.

use super::{AgentBackend, AgentConfig, run_resilient};
use crate::state::AbbeyState;
use crate::stream::StreamTap;
use fs4::fs_std::FileExt as _;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);
const CHAT: &str = "fixture-held-turn";

struct Fixture {
    root: PathBuf,
    transcript: PathBuf,
    cfg: AgentConfig,
    state: AbbeyState,
    tap: StreamTap,
    _events: Receiver<crate::stream::StreamEvent>,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "abbey-turn-lock-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("state/by-cwd")).unwrap();
        let program = root.join("executor");
        let quoted = format!("'{}'", root.to_string_lossy().replace('\'', "'\\''"));
        fs::write(&program, format!(
            "#!/bin/sh\nprintf 'called\\n' > {quoted}/executor.called\nprintf 'owned-answer\\n'\n"
        )).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let state = AbbeyState {
            state_dir: root.join("state"),
            chat_file: root.join("state/chat-id"),
            model_file: root.join("state/model"),
            history_file: root.join("state/history.log"),
            cwd_dir: root.join("state/by-cwd"),
            per_cwd: false,
            cwd: root.clone(),
        };
        state.save_chat(CHAT).unwrap();
        let (tx, events) = mpsc::channel();
        let tap = StreamTap::new(tx);
        let mut cfg =
            AgentConfig::fixed_provider_recipe(program, AgentBackend::Abi, "local".into());
        cfg.no_resume = false;
        cfg.transcript_dir = Some(root.join("transcripts"));
        cfg.stream = Some(tap.clone());
        fs::create_dir_all(cfg.transcript_dir.as_ref().unwrap()).unwrap();
        let transcript = cfg.transcript_path(CHAT).unwrap();
        fs::write(&transcript, b"unchanged-seed\n").unwrap();
        Self {
            root,
            transcript,
            cfg,
            state,
            tap,
            _events: events,
        }
    }

    fn hold_turn(&self) -> File {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(self.transcript.with_extension("transcript.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();
        lock
    }

    fn spawn(&self) -> (Receiver<anyhow::Result<i32>>, Receiver<()>, JoinHandle<()>) {
        let cfg = self.cfg.clone();
        let state = self.state.clone();
        let (out, result) = mpsc::channel();
        let (started, start) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = started.send(());
            let value = run_resilient(&cfg, &state, false, &["private fixture prompt".into()]);
            let _ = out.send(value);
        });
        (result, start, worker)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn held_turn_control_joins_then_runs_after_lock_release() {
    let fixture = Fixture::new("control");
    let held = fixture.hold_turn();
    let (result, start, worker) = fixture.spawn();
    let started = start.recv_timeout(WAIT).is_ok();
    let pending = matches!(
        result.recv_timeout(Duration::from_millis(200)),
        Err(RecvTimeoutError::Timeout)
    );
    let absent_before_release = !fixture.root.join("executor.called").exists();
    fs4::fs_std::FileExt::unlock(&held).unwrap();
    drop(held);
    let outcome = result.recv_timeout(WAIT).unwrap();
    worker.join().unwrap();
    assert!(
        started && pending && absent_before_release,
        "resilient control did not wait on the actual held turn lock"
    );
    assert_eq!(outcome.unwrap(), 0);
    assert!(fixture.root.join("executor.called").exists());
    assert!(
        fs::read_to_string(&fixture.transcript)
            .unwrap()
            .contains("owned-answer")
    );
}

#[test]
fn held_turn_cancellation_completes_before_lock_release_without_generation() {
    let fixture = Fixture::new("cancel");
    let held = fixture.hold_turn();
    let (result, start, worker) = fixture.spawn();
    let started = start.recv_timeout(WAIT).is_ok();
    let pending = matches!(
        result.recv_timeout(Duration::from_millis(200)),
        Err(RecvTimeoutError::Timeout)
    );
    fixture.tap.cancel.cancel();
    let before_release = result.recv_timeout(WAIT).ok();
    let joined_before_release = before_release.is_some();
    let mut worker = Some(worker);
    if joined_before_release {
        worker.take().unwrap().join().unwrap();
    }
    let unchanged_before_release = !fixture.root.join("executor.called").exists()
        && fs::read(&fixture.transcript).unwrap() == b"unchanged-seed\n";
    // Release only after recording product completion. Old blocking flock
    // code can then finish naturally before its regression assertion fails.
    fs4::fs_std::FileExt::unlock(&held).unwrap();
    drop(held);
    let outcome = before_release.or_else(|| result.recv_timeout(WAIT).ok());
    if let Some(worker) = worker {
        worker.join().unwrap();
    }
    assert!(
        started && pending,
        "run did not remain pending under the actual held turn lock"
    );
    assert!(
        joined_before_release,
        "owned cancellation waited for somebody else's turn lock to be released: {outcome:?}"
    );
    assert!(
        unchanged_before_release,
        "cancelled lock waiter generated or changed transcript state"
    );
    assert!(
        !fixture.root.join("executor.called").exists(),
        "cancelled waiter spawned generation after lock release"
    );
    assert!(
        outcome.is_some_and(|value| !matches!(value, Ok(0))),
        "cancelled lock waiter reported success"
    );
}
