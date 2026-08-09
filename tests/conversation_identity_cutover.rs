//! Real-process proof for canonical-first conversation identity mirror recovery.

#![cfg(unix)]

use rusqlite::Connection;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const RAW_ID: &str = "private-provider-identity-canary";

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

#[test]
fn real_cli_recovers_committed_conversation_mirrors_after_failpoint() {
    let scratch = Scratch::new();
    let state = scratch.0.join("state");
    let cwd = scratch.0.join("private-cwd-canary");
    fs::create_dir(&state).unwrap();
    fs::create_dir(&cwd).unwrap();
    let agent = fake_agent(&scratch.0);

    let crashed = run(&state, &cwd, &agent, &["create-chat"], true, None, None);
    assert_eq!(crashed.status.code(), Some(86));
    assert!(!String::from_utf8_lossy(&crashed.stderr).contains(RAW_ID));
    assert_eq!(invocation_count(&agent), 1);

    let journal = state.join("daemon/conversation-mirror-journal");
    assert!(journal.join("pending.json").is_file());
    assert!(!state.join("chat-id").exists());
    assert!(!state.join("history.log").exists());

    // A recovery failure must stop before a second provider create-chat side
    // effect. Restore the snapshotted before-image, then let the history reader
    // prove that it participates in recovery too.
    fs::write(state.join("chat-id"), b"out-of-band-divergence\n").unwrap();
    let refused = run(&state, &cwd, &agent, &["create-chat"], false, None, None);
    assert!(!refused.status.success());
    assert_eq!(invocation_count(&agent), 1);
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(RAW_ID));
    fs::remove_file(state.join("chat-id")).unwrap();

    let recovered_history = run(&state, &cwd, &agent, &["history", "10"], false, None, None);
    assert!(
        recovered_history.status.success(),
        "{:?}",
        recovered_history.stderr
    );
    assert!(String::from_utf8_lossy(&recovered_history.stdout).contains(RAW_ID));
    let recovered = run(&state, &cwd, &agent, &["chat-id"], false, None, None);
    assert!(recovered.status.success(), "{:?}", recovered.stderr);
    assert_eq!(String::from_utf8_lossy(&recovered.stdout).trim(), RAW_ID);
    assert!(!journal.join("pending.json").exists());

    let reread = run(&state, &cwd, &agent, &["chat-id"], false, None, None);
    assert!(reread.status.success());
    assert_eq!(String::from_utf8_lossy(&reread.stdout).trim(), RAW_ID);
    assert_eq!(
        fs::read_to_string(state.join("chat-id")).unwrap(),
        format!("{RAW_ID}\n")
    );
    assert_eq!(
        fs::read_to_string(state.join("chat-id.export")).unwrap(),
        format!("ABBEY_CHAT_ID='{RAW_ID}'\n")
    );
    let per_cwd = fs::read_dir(state.join("by-cwd"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(per_cwd.len(), 1);
    assert_eq!(
        fs::read_to_string(&per_cwd[0]).unwrap(),
        format!("{RAW_ID}\n")
    );
    let history = fs::read_to_string(state.join("history.log")).unwrap();
    assert_eq!(history.matches(RAW_ID).count(), 1);
    assert_eq!(history.matches("private-cwd-canary").count(), 1);
    assert_eq!(history.lines().count(), 1);

    let database = state.join("daemon/runtime.sqlite");
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
    assert_tree_excludes(&state.join("daemon"), RAW_ID.as_bytes());
    assert_tree_excludes(&state.join("daemon"), b"private-cwd-canary");
    assert_owner_only(&state, &per_cwd[0], &journal);

    prove_external_override_recovery_and_unsafe_parent_rejection();
}

fn prove_external_override_recovery_and_unsafe_parent_rejection() {
    let scratch = Scratch::new();
    let state = scratch.0.join("state");
    let cwd = scratch.0.join("override-cwd");
    let external = scratch.0.join("external");
    fs::create_dir(&state).unwrap();
    fs::create_dir(&cwd).unwrap();
    fs::create_dir(&external).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
    let agent = fake_agent(&scratch.0);
    let chat = external.join("custom-chat");
    let history = external.join("custom-history");
    let crashed = run(
        &state,
        &cwd,
        &agent,
        &["create-chat"],
        true,
        Some(&chat),
        Some(&history),
    );
    assert_eq!(crashed.status.code(), Some(86));

    // Reopen without either override: the committed plan owns its exact
    // validated targets and must not be reinterpreted through current env.
    let recovered = run(&state, &cwd, &agent, &["history", "10"], false, None, None);
    assert!(recovered.status.success(), "{:?}", recovered.stderr);
    assert_eq!(fs::read_to_string(chat).unwrap(), format!("{RAW_ID}\n"));
    assert!(fs::read_to_string(history).unwrap().contains(RAW_ID));
    assert!(!state.join("chat-id").exists());
    assert!(!state.join("history.log").exists());

    let unsafe_parent = scratch.0.join("unsafe-parent");
    fs::create_dir(&unsafe_parent).unwrap();
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777)).unwrap();
    let before = invocation_count(&agent);
    let rejected = run(
        &state,
        &cwd,
        &agent,
        &["create-chat"],
        false,
        Some(&unsafe_parent.join("chat-id")),
        None,
    );
    assert!(!rejected.status.success());
    assert_eq!(invocation_count(&agent), before);
}

fn run(
    state: &Path,
    cwd: &Path,
    agent: &Path,
    args: &[&str],
    failpoint: bool,
    chat_override: Option<&Path>,
    history_override: Option<&Path>,
) -> Output {
    let mut command = Command::new(ABBEY_BIN);
    command
        .args(args)
        .current_dir(cwd)
        .env(abbey::edition::ACTIVE.state_dir_env(), state)
        .env("ABBEY_AGENT", agent)
        .env("ABBEY_TEST_AGENT_COUNT", agent.with_extension("count"))
        .env("ABBEY_PER_CWD", "1")
        .env_remove("CURSOR_AGENT_CHAT_ID")
        .env_remove(abbey::edition::ACTIVE.scoped_env("MODEL_FILE"));
    if failpoint {
        command.env(
            "ABBEY_TEST_CONVERSATION_FAILPOINT",
            "after_canonical_commit",
        );
    } else {
        command.env_remove("ABBEY_TEST_CONVERSATION_FAILPOINT");
    }
    if let Some(path) = chat_override {
        command.env(abbey::edition::ACTIVE.scoped_env("CHAT_FILE"), path);
    } else {
        command.env_remove(abbey::edition::ACTIVE.scoped_env("CHAT_FILE"));
    }
    if let Some(path) = history_override {
        command.env(abbey::edition::ACTIVE.scoped_env("HISTORY_FILE"), path);
    } else {
        command.env_remove(abbey::edition::ACTIVE.scoped_env("HISTORY_FILE"));
    }
    command.output().expect("spawn real abbey binary")
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
