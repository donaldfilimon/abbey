//! Real-process proof for canonical-first conversation identity mirror recovery.

#![cfg(unix)]

use rusqlite::Connection;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const RAW_ID: &str = "private-provider-identity-canary";
const BARRIER_TIMEOUT: Duration = Duration::from_secs(2);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "abbey-conversation-cutover-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ProcessFixture {
    _scratch: Scratch,
    root: PathBuf,
    state: PathBuf,
    journal: PathBuf,
    cwd: PathBuf,
    config: PathBuf,
    agent: PathBuf,
    inherited_path: Option<OsString>,
}

impl ProcessFixture {
    fn new(cwd_label: &str) -> Self {
        let scratch = Scratch::new();
        let root = scratch.0.join(format!(
            "edition-{}-{}",
            abbey::edition::ACTIVE.slug(),
            uuid::Uuid::new_v4()
        ));
        let state = root.join("state");
        let journal = state.join("daemon/conversation-mirror-journal");
        let cwd = root.join("workspace").join(cwd_label);
        let config = root.join("config").join("config.toml");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let agent = fake_agent(&root);
        Self {
            _scratch: scratch,
            root,
            state,
            journal,
            cwd,
            config,
            agent,
            inherited_path: std::env::var_os("PATH"),
        }
    }

    fn run(
        &self,
        args: &[&str],
        failpoint: Option<&str>,
        chat_override: Option<&Path>,
        history_override: Option<&Path>,
    ) -> Output {
        let mut command = Command::new(ABBEY_BIN);
        command
            .env_clear()
            .args(args)
            .current_dir(&self.cwd)
            .env(abbey::edition::ACTIVE.state_dir_env(), &self.state)
            .env(abbey::edition::ACTIVE.config_path_env(), &self.config)
            .env("ABBEY_AGENT", &self.agent)
            .env("ABBEY_TEST_AGENT_COUNT", self.agent.with_extension("count"))
            .env("ABBEY_PER_CWD", "1");
        if let Some(path) = &self.inherited_path {
            command.env("PATH", path);
        }
        if let Some(failpoint) = failpoint {
            command.env("ABBEY_TEST_CONVERSATION_FAILPOINT", failpoint);
        }
        if let Some(path) = chat_override {
            command.env(abbey::edition::ACTIVE.scoped_env("CHAT_FILE"), path);
        }
        if let Some(path) = history_override {
            command.env(abbey::edition::ACTIVE.scoped_env("HISTORY_FILE"), path);
        }
        // `output` is the first barrier: the failpoint process has terminated
        // and all inherited descriptors are closed before the caller observes
        // canonical or mirror state.
        command.output().expect("spawn real abbey binary")
    }

    fn invocation_count(&self) -> usize {
        invocation_count(&self.agent)
    }

    fn wait_for_committed_operation(&self, expected: &str) {
        wait_until("canonical conversation commit marker", || {
            let database = self.state.join("daemon/runtime.sqlite");
            if !database.is_file() {
                return false;
            }
            let Ok(connection) = Connection::open(database) else {
                return false;
            };
            connection
                .query_row(
                    "SELECT operation FROM conversation_identity_commit WHERE singleton=1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .is_ok_and(|operation| operation == expected)
        });
    }

    fn wait_for_pending(&self, expected: bool) {
        wait_until("conversation pending marker state", || {
            self.journal.join("pending.json").is_file() == expected
        });
    }
}

fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + BARRIER_TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn real_cli_recovers_committed_conversation_mirrors_after_failpoint() {
    let fixture = ProcessFixture::new("private-cwd-canary");

    let crashed = fixture.run(&["create-chat"], Some("after_canonical_commit"), None, None);
    assert_eq!(crashed.status.code(), Some(86));
    assert!(!String::from_utf8_lossy(&crashed.stderr).contains(RAW_ID));
    assert_eq!(fixture.invocation_count(), 1);

    fixture.wait_for_committed_operation("save");
    fixture.wait_for_pending(true);
    assert!(!fixture.state.join("chat-id").exists());
    assert!(!fixture.state.join("history.log").exists());

    // A recovery failure must stop before a second provider create-chat side
    // effect. Restore the snapshotted before-image, then let the history reader
    // prove that it participates in recovery too.
    fs::write(fixture.state.join("chat-id"), b"out-of-band-divergence\n").unwrap();
    let refused = fixture.run(&["create-chat"], None, None, None);
    assert!(!refused.status.success());
    assert_eq!(fixture.invocation_count(), 1);
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(RAW_ID));
    fixture.wait_for_pending(true);
    fs::remove_file(fixture.state.join("chat-id")).unwrap();

    let recovered_history = fixture.run(&["history", "10"], None, None, None);
    assert!(
        recovered_history.status.success(),
        "{:?}",
        recovered_history.stderr
    );
    assert!(String::from_utf8_lossy(&recovered_history.stdout).contains(RAW_ID));
    fixture.wait_for_pending(false);
    let recovered = fixture.run(&["chat-id"], None, None, None);
    assert!(recovered.status.success(), "{:?}", recovered.stderr);
    assert_eq!(String::from_utf8_lossy(&recovered.stdout).trim(), RAW_ID);
    fixture.wait_for_pending(false);

    let reread = fixture.run(&["chat-id"], None, None, None);
    assert!(reread.status.success());
    assert_eq!(String::from_utf8_lossy(&reread.stdout).trim(), RAW_ID);
    assert_eq!(
        fs::read_to_string(fixture.state.join("chat-id")).unwrap(),
        format!("{RAW_ID}\n")
    );
    assert_eq!(
        fs::read_to_string(fixture.state.join("chat-id.export")).unwrap(),
        format!("ABBEY_CHAT_ID='{RAW_ID}'\n")
    );
    let per_cwd = fs::read_dir(fixture.state.join("by-cwd"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(per_cwd.len(), 1);
    assert_eq!(
        fs::read_to_string(&per_cwd[0]).unwrap(),
        format!("{RAW_ID}\n")
    );
    let history = fs::read_to_string(fixture.state.join("history.log")).unwrap();
    assert_eq!(history.matches(RAW_ID).count(), 1);
    assert_eq!(history.matches("private-cwd-canary").count(), 1);
    assert_eq!(history.lines().count(), 1);

    let database = fixture.state.join("daemon/runtime.sqlite");
    let connection = Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM conversation_identity_scopes",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
        2,
        "per-cwd save must atomically bind cwd and global fallback scopes"
    );
    drop(connection);
    assert_tree_excludes(&fixture.state.join("daemon"), RAW_ID.as_bytes());
    assert_tree_excludes(&fixture.state.join("daemon"), b"private-cwd-canary");
    assert_owner_only(&fixture.state, &per_cwd[0], &fixture.journal);

    prove_external_override_recovery_and_unsafe_parent_rejection();
}

#[test]
fn real_cli_clear_tombstones_recover_across_each_failpoint() {
    prove_clear_failpoint("after_clear_journal_prepare", false);
    prove_clear_failpoint("after_clear_canonical_commit", false);
    prove_clear_failpoint("after_clear_first_removal", true);
}

#[test]
fn shared_state_control_reproduces_the_missing_pending_marker_symptom() {
    let fixture = ProcessFixture::new("shared-state-cwd");

    let crashed = fixture.run(&["create-chat"], Some("after_canonical_commit"), None, None);
    assert_eq!(crashed.status.code(), Some(86));
    fixture.wait_for_committed_operation("save");
    fixture.wait_for_pending(true);

    // This is the negative control: deliberately reuse the *same* fixture.
    // The sibling is allowed to recover the committed journal and therefore
    // consumes the marker that the historical assertion expected to observe.
    let sibling = fixture.run(&["history", "10"], None, None, None);
    assert!(sibling.status.success(), "{:?}", sibling.stderr);
    fixture.wait_for_pending(false);
}

fn prove_clear_failpoint(failpoint: &str, all: bool) {
    let fixture = ProcessFixture::new(&format!("clear-{failpoint}-cwd"));
    let seeded = fixture.run(&["create-chat"], None, None, None);
    assert!(seeded.status.success(), "{:?}", seeded.stderr);
    let history = fs::read(fixture.state.join("history.log")).unwrap();
    let by_cwd = fs::read_dir(fixture.state.join("by-cwd"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(by_cwd.len(), 1);
    if all {
        fs::write(
            fixture.state.join("by-cwd/other-project"),
            b"other-private-id\n",
        )
        .unwrap();
    }

    let args = if all {
        vec!["clear", "--all"]
    } else {
        vec!["clear"]
    };
    let crashed = fixture.run(&args, Some(failpoint), None, None);
    assert_eq!(crashed.status.code(), Some(86));
    assert!(!String::from_utf8_lossy(&crashed.stderr).contains(RAW_ID));
    let expected = match failpoint {
        "after_clear_journal_prepare" => "save",
        _ if all => "clear_all",
        _ => "clear_scope",
    };
    fixture.wait_for_committed_operation(expected);
    fixture.wait_for_pending(true);

    let recovered = fixture.run(&["chat-id"], None, None, None);
    if all {
        assert_eq!(recovered.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&recovered.stderr).contains(RAW_ID));
        assert!(!fixture.state.join("chat-id").exists());
        assert!(!fixture.state.join("chat-id.export").exists());
        assert!(
            fs::read_dir(fixture.state.join("by-cwd"))
                .unwrap()
                .next()
                .is_none()
        );
    } else {
        assert!(recovered.status.success(), "{:?}", recovered.stderr);
        assert_eq!(String::from_utf8_lossy(&recovered.stdout).trim(), RAW_ID);
        if failpoint == "after_clear_journal_prepare" {
            assert!(by_cwd[0].exists());
        } else {
            assert!(!by_cwd[0].exists());
        }
        assert_eq!(
            fs::read_to_string(fixture.state.join("chat-id")).unwrap(),
            format!("{RAW_ID}\n")
        );
    }
    assert_eq!(
        fs::read(fixture.state.join("history.log")).unwrap(),
        history
    );
    fixture.wait_for_pending(false);

    let connection = Connection::open(fixture.state.join("daemon/runtime.sqlite")).unwrap();
    let operation = connection
        .query_row(
            "SELECT operation FROM conversation_identity_commit WHERE singleton=1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(operation, expected);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM conversations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM conversation_identity_aliases",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
        1
    );
    drop(connection);
    assert_tree_excludes(&fixture.state.join("daemon"), RAW_ID.as_bytes());
    assert_tree_excludes(
        &fixture.state.join("daemon"),
        format!("clear-{failpoint}-cwd").as_bytes(),
    );
}

fn prove_external_override_recovery_and_unsafe_parent_rejection() {
    let fixture = ProcessFixture::new("override-cwd");
    let external = fixture.root.join("external");
    fs::create_dir(&external).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
    let chat = external.join("custom-chat");
    let history = external.join("custom-history");
    let crashed = fixture.run(
        &["create-chat"],
        Some("after_canonical_commit"),
        Some(&chat),
        Some(&history),
    );
    assert_eq!(crashed.status.code(), Some(86));
    fixture.wait_for_committed_operation("save");
    fixture.wait_for_pending(true);

    // Reopen without either override: the committed plan owns its exact
    // validated targets and must not be reinterpreted through current env.
    let recovered = fixture.run(&["history", "10"], None, None, None);
    assert!(recovered.status.success(), "{:?}", recovered.stderr);
    fixture.wait_for_pending(false);
    assert_eq!(fs::read_to_string(chat).unwrap(), format!("{RAW_ID}\n"));
    assert!(fs::read_to_string(history).unwrap().contains(RAW_ID));
    assert!(!fixture.state.join("chat-id").exists());
    assert!(!fixture.state.join("history.log").exists());

    let unsafe_parent = fixture.root.join("unsafe-parent");
    fs::create_dir(&unsafe_parent).unwrap();
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777)).unwrap();
    let before = fixture.invocation_count();
    let rejected = fixture.run(
        &["create-chat"],
        None,
        Some(&unsafe_parent.join("chat-id")),
        None,
    );
    assert!(!rejected.status.success());
    assert_eq!(fixture.invocation_count(), before);
}

fn fake_agent(root: &Path) -> PathBuf {
    let path = root.join("cursor-agent-fixture");
    fs::write(
        &path,
        format!("#!/bin/sh\nif [ \"$1\" = create-chat ]; then printf 'x\\n' >> \"$ABBEY_TEST_AGENT_COUNT\"; printf '%s\\n' '{RAW_ID}'; exit 0; fi\nexit 64\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn invocation_count(agent: &Path) -> usize {
    fs::read_to_string(agent.with_extension("count"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

fn assert_tree_excludes(root: &Path, canary: &[u8]) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                let bytes = fs::read(entry.path()).unwrap();
                assert!(
                    !bytes.windows(canary.len()).any(|window| window == canary),
                    "canonical daemon state retained a raw canary"
                );
            }
        }
    }
}

fn assert_owner_only(state: &Path, per_cwd: &Path, journal: &Path) {
    for directory in [
        state.to_path_buf(),
        state.join("by-cwd"),
        state.join("daemon"),
        journal.to_path_buf(),
    ] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    for file in [
        state.join("chat-id"),
        state.join("chat-id.export"),
        state.join("history.log"),
        per_cwd.to_path_buf(),
        journal.join("lock"),
    ] {
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
