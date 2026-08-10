use super::*;
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct ScratchState {
    root: ScratchDir,
    state: AbbeyState,
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "abbey-conversation-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl ScratchState {
    fn new(tag: &str, per_cwd: bool) -> Self {
        let root = ScratchDir::new(tag);
        let state_dir = root.path().join("state");
        let cwd = root.path().join("private-project");
        fs::create_dir(&state_dir).unwrap();
        fs::create_dir(&cwd).unwrap();
        let state = AbbeyState {
            chat_file: state_dir.join("chat-id"),
            model_file: state_dir.join("model"),
            history_file: state_dir.join("history.log"),
            cwd_dir: state_dir.join("by-cwd"),
            state_dir,
            per_cwd,
            cwd,
        };
        fs::create_dir(&state.cwd_dir).unwrap();
        Self { root, state }
    }

    fn pending(&self) -> PathBuf {
        self.state
            .state_dir
            .join("daemon")
            .join(JOURNAL_DIR)
            .join(JOURNAL_PENDING)
    }

    fn store(&self) -> RuntimeStore {
        open_metadata_store(&self.state).unwrap()
    }
}

#[test]
fn canonical_resolution_is_exact_cwd_first_and_never_falls_back_on_divergence() {
    let scratch = ScratchState::new("canonical-read-order", true);
    save_chat(&scratch.state, "canonical-private-id").unwrap();

    fs::write(&scratch.state.chat_file, b"divergent-global-id\n").unwrap();
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("canonical-private-id"),
        "an authenticated cwd selection must not consult the global mirror"
    );

    fs::write(&scratch.state.chat_file, b"canonical-private-id\n").unwrap();
    fs::write(scratch.state.active_chat_file(), b"divergent-cwd-id\n").unwrap();
    let error = read_chat(&scratch.state).unwrap_err();
    assert!(error.to_string().contains("diverged"));
    assert_eq!(
        fs::read_to_string(&scratch.state.chat_file).unwrap(),
        "canonical-private-id\n",
        "a valid global mirror must not rescue a corrupt selected cwd mirror"
    );
}

#[test]
fn cwd_tombstone_continues_to_global_and_global_tombstone_resolves_none() {
    let scratch = ScratchState::new("canonical-tombstones", true);
    save_chat(&scratch.state, "global-fallback-private-id").unwrap();

    clear_chat(&scratch.state, false).unwrap();
    fs::write(
        scratch.state.active_chat_file(),
        vec![b'x'; MAX_ID_FILE_BYTES + 1],
    )
    .unwrap();
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("global-fallback-private-id")
    );

    fs::remove_file(scratch.state.active_chat_file()).unwrap();
    clear_chat(&scratch.state, true).unwrap();
    fs::write(&scratch.state.chat_file, vec![b'x'; MAX_ID_FILE_BYTES + 1]).unwrap();
    assert_eq!(read_chat(&scratch.state).unwrap(), None);
}

#[test]
fn untracked_scopes_keep_bounded_legacy_cwd_then_global_compatibility() {
    let scratch = ScratchState::new("legacy-read-order", true);
    fs::write(scratch.state.active_chat_file(), b"legacy-cwd-id\n").unwrap();
    fs::write(&scratch.state.chat_file, b"legacy-global-id\n").unwrap();
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("legacy-cwd-id")
    );

    fs::remove_file(scratch.state.active_chat_file()).unwrap();
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("legacy-global-id")
    );

    let global_only = ScratchState::new("legacy-global-only", false);
    fs::write(&global_only.state.chat_file, b"legacy-global-only-id\n").unwrap();
    fs::write(
        global_only
            .state
            .cwd_dir
            .join(AbbeyState::cwd_key(&global_only.state.cwd)),
        b"must-not-be-read\n",
    )
    .unwrap();
    assert_eq!(
        read_chat(&global_only.state).unwrap().as_deref(),
        Some("legacy-global-only-id")
    );
}

#[cfg(unix)]
#[test]
fn selected_mirror_corruption_is_fatal_without_global_fallback() {
    use std::os::unix::fs::symlink;

    for (tag, corrupt) in [
        ("missing", 0_u8),
        ("empty", 1),
        ("invalid-utf8", 2),
        ("invalid-id", 3),
        ("oversize", 4),
        ("symlink", 5),
        ("identity-mismatch", 6),
    ] {
        let scratch = ScratchState::new(tag, true);
        save_chat(&scratch.state, "selected-private-id").unwrap();
        let active = scratch.state.active_chat_file();
        match corrupt {
            0 => fs::remove_file(&active).unwrap(),
            1 => fs::write(&active, b"").unwrap(),
            2 => fs::write(&active, [0xff, 0xfe]).unwrap(),
            3 => fs::write(&active, b"bad\0id\n").unwrap(),
            4 => fs::write(&active, vec![b'x'; MAX_ID_FILE_BYTES + 1]).unwrap(),
            5 => {
                fs::remove_file(&active).unwrap();
                symlink(&scratch.state.chat_file, &active).unwrap();
            }
            6 => fs::write(&active, b"different-valid-id\n").unwrap(),
            _ => unreachable!(),
        }

        assert!(
            read_chat(&scratch.state).is_err(),
            "{tag} selected mirror corruption must fail closed"
        );
        assert_eq!(
            fs::read_to_string(&scratch.state.chat_file).unwrap(),
            "selected-private-id\n",
            "{tag} must not fall back to the valid global mirror"
        );
    }
}

#[test]
fn changed_chat_override_cannot_reinterpret_a_canonical_global_selection() {
    let mut scratch = ScratchState::new("changed-override", false);
    scratch.state.chat_file = scratch.state.state_dir.join("saved-chat-override");
    save_chat(&scratch.state, "override-private-id").unwrap();

    let mut reopened = scratch.state.clone();
    reopened.chat_file = reopened.state_dir.join("changed-chat-override");
    fs::write(&reopened.chat_file, b"reinterpreted-private-id\n").unwrap();
    let error = read_chat(&reopened).unwrap_err();
    assert!(error.to_string().contains("diverged"));
    assert_eq!(
        fs::read_to_string(&scratch.state.chat_file).unwrap(),
        "override-private-id\n"
    );
}

#[test]
fn uncommitted_journal_is_discarded_without_mirror_writes() {
    let scratch = ScratchState::new("uncommitted", true);
    validate_layout(&scratch.state).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, "uncommitted-private-id").unwrap();
    write_pending(&journal, &plan).unwrap();
    drop(journal);

    let journal = lock_journal(&scratch.state).unwrap();
    recover_pending(&scratch.state, &journal).unwrap();

    assert!(!scratch.pending().exists());
    assert!(!scratch.state.chat_file.exists());
    assert!(!scratch.state.active_chat_file().exists());
    assert!(!scratch.state.history_file.exists());
}

#[test]
fn committed_plan_recovers_mirrors_and_history_exactly_once() {
    let scratch = ScratchState::new("committed", true);
    validate_layout(&scratch.state).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, "committed-private-id").unwrap();
    write_pending(&journal, &plan).unwrap();
    let commit = scratch
        .store()
        .save_conversation_identity(
            &plan.edition_slug,
            &plan.scopes().unwrap(),
            &plan.external_id,
            &plan.mutation_token,
        )
        .unwrap();
    // Model the hardest replay point: one mirror already reached its desired
    // bytes, but the journal still says the whole transaction is pending.
    let global = plan
        .targets
        .iter()
        .find(|target| target.role == TargetRole::Global)
        .unwrap();
    atomic_replace(
        &scratch.state.chat_file,
        &desired_bytes(TargetRole::Global, &plan, &commit, None).unwrap(),
    )
    .unwrap();
    assert_eq!(
        path_from_hex(&global.path_hex).unwrap(),
        resolved_mirror_path(&scratch.state, &scratch.state.chat_file).unwrap()
    );
    drop(journal);

    let journal = lock_journal(&scratch.state).unwrap();
    recover_pending(&scratch.state, &journal).unwrap();
    // Model a crash after every mirror (including history) reached disk but
    // before pending.json was removed. Replay must recognize all after-images
    // and must not append the history line again.
    write_pending(&journal, &plan).unwrap();
    recover_pending(&scratch.state, &journal).unwrap();

    assert!(!scratch.pending().exists());
    assert_eq!(
        fs::read_to_string(&scratch.state.chat_file).unwrap(),
        "committed-private-id\n"
    );
    assert_eq!(
        fs::read_to_string(scratch.state.active_chat_file()).unwrap(),
        "committed-private-id\n"
    );
    let history = fs::read_to_string(&scratch.state.history_file).unwrap();
    assert_eq!(history.matches("committed-private-id").count(), 1);
    assert_eq!(history.lines().count(), 1);
}

#[test]
fn mirror_divergence_fails_closed_without_overwrite() {
    let scratch = ScratchState::new("diverged", true);
    validate_layout(&scratch.state).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, "canonical-private-id").unwrap();
    write_pending(&journal, &plan).unwrap();
    scratch
        .store()
        .save_conversation_identity(
            &plan.edition_slug,
            &plan.scopes().unwrap(),
            &plan.external_id,
            &plan.mutation_token,
        )
        .unwrap();
    fs::write(&scratch.state.chat_file, b"out-of-band-value\n").unwrap();
    drop(journal);

    let journal = lock_journal(&scratch.state).unwrap();
    let error = recover_pending(&scratch.state, &journal).unwrap_err();
    assert!(error.to_string().contains("diverged"));
    assert_eq!(
        fs::read(&scratch.state.chat_file).unwrap(),
        b"out-of-band-value\n"
    );
    assert!(scratch.pending().exists());
}

#[test]
fn canonical_store_and_diagnostics_exclude_raw_identity_canaries() {
    let scratch = ScratchState::new("redaction", true);
    let private_id = "raw-provider-secret-canary";
    save_chat(&scratch.state, private_id).unwrap();

    let scopes = [
        ConversationIdentityScope::working_directory(&scratch.state.cwd),
        ConversationIdentityScope::global(),
    ];
    let marker = scratch.store().current_identity_commit().unwrap().unwrap();
    assert_eq!(marker.scope_sha256, scopes[0].as_sha256());
    assert_eq!(marker.scope_set_sha256.len(), 64);
    for entry in fs::read_dir(scratch.state.state_dir.join("daemon")).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(
                !fs::read(path)
                    .unwrap()
                    .windows(private_id.len())
                    .any(|window| window == private_id.as_bytes())
            );
        }
    }
    let error = save_chat(&scratch.state, "bad\nsecret-canary").unwrap_err();
    assert!(!error.to_string().contains("secret-canary"));
    assert!(!scratch.pending().exists());
}

#[test]
fn concurrent_saves_serialize_canonical_and_mirror_updates() {
    let scratch = ScratchState::new("concurrent", true);
    let state = Arc::new(scratch.state.clone());
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for id in ["parallel-private-a", "parallel-private-b"] {
        let state = Arc::clone(&state);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            save_chat(&state, id).unwrap();
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }

    let history = fs::read_to_string(&state.history_file).unwrap();
    assert_eq!(history.matches("parallel-private-a").count(), 1);
    assert_eq!(history.matches("parallel-private-b").count(), 1);
    assert_eq!(history.lines().count(), 2);
    let active = read_first_line_bounded(&state.active_chat_file())
        .unwrap()
        .unwrap();
    let global = read_first_line_bounded(&state.chat_file).unwrap().unwrap();
    assert_eq!(active, global);
    assert!(matches!(
        active.as_str(),
        "parallel-private-a" | "parallel-private-b"
    ));
}

#[test]
fn canonical_clear_current_and_all_keep_existing_mirror_parity() {
    let scratch = ScratchState::new("clear-parity", true);
    save_chat(&scratch.state, "clear-private-id").unwrap();
    let other = scratch.state.cwd_dir.join("other-project");
    fs::write(&other, b"other-private-id\n").unwrap();

    clear_chat(&scratch.state, false).unwrap();
    assert!(!scratch.state.active_chat_file().exists());
    assert!(scratch.state.chat_file.exists());
    assert!(scratch.state.chat_file.with_extension("export").exists());
    assert!(other.exists());
    assert!(scratch.state.history_file.exists());

    clear_chat(&scratch.state, true).unwrap();
    assert!(!scratch.state.chat_file.exists());
    assert!(!scratch.state.chat_file.with_extension("export").exists());
    assert!(
        fs::read_dir(&scratch.state.cwd_dir)
            .unwrap()
            .next()
            .is_none()
    );
    assert!(scratch.state.history_file.exists());
}

#[test]
fn history_and_compact_recover_committed_plan_before_observation() {
    let scratch = ScratchState::new("history-recovery", true);
    validate_layout(&scratch.state).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, "history-recovery-private-id").unwrap();
    write_pending(&journal, &plan).unwrap();
    scratch
        .store()
        .save_conversation_identity(
            &plan.edition_slug,
            &plan.scopes().unwrap(),
            &plan.external_id,
            &plan.mutation_token,
        )
        .unwrap();
    drop(journal);

    let entries = history(&scratch.state, 10).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].chat_id, "history-recovery-private-id");
    assert!(!scratch.pending().exists());
    assert_eq!(compact_history(&scratch.state, 1).unwrap(), 1);
    assert_eq!(
        fs::read_to_string(&scratch.state.history_file)
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn compact_and_save_serialize_without_divergence_or_partial_history() {
    let scratch = ScratchState::new("compact-save", true);
    save_chat(&scratch.state, "history-seed-private-id").unwrap();
    let state = Arc::new(scratch.state.clone());
    let barrier = Arc::new(Barrier::new(3));
    let save_state = Arc::clone(&state);
    let save_barrier = Arc::clone(&barrier);
    let save = std::thread::spawn(move || {
        save_barrier.wait();
        save_chat(&save_state, "history-next-private-id")
    });
    let compact_state = Arc::clone(&state);
    let compact_barrier = Arc::clone(&barrier);
    let compact = std::thread::spawn(move || {
        compact_barrier.wait();
        compact_history(&compact_state, 1)
    });
    barrier.wait();
    save.join().unwrap().unwrap();
    compact.join().unwrap().unwrap();

    let text = fs::read_to_string(&state.history_file).unwrap();
    assert!(matches!(text.lines().count(), 1 | 2));
    assert!(text.ends_with('\n'));
    assert!(text.lines().all(|line| line.split('\t').count() == 3));
    assert!(!scratch.pending().exists());
}

#[test]
fn legacy_capture_guard_serializes_concurrent_save_projection() {
    let scratch = ScratchState::new("capture-guard", true);
    save_chat(&scratch.state, "capture-old-private-id").unwrap();
    let guard = lock_legacy_capture(&scratch.state.state_dir).unwrap();
    let state = scratch.state.clone();
    let (sent, received) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = save_chat(&state, "capture-new-private-id");
        sent.send(result).unwrap();
    });
    assert!(matches!(
        received.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(
        fs::read_to_string(&scratch.state.chat_file).unwrap(),
        "capture-old-private-id\n"
    );
    assert_eq!(
        fs::read_to_string(&scratch.state.history_file)
            .unwrap()
            .lines()
            .count(),
        1
    );
    drop(guard);
    received
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(
        fs::read_to_string(&scratch.state.chat_file).unwrap(),
        "capture-new-private-id\n"
    );
}

#[test]
#[allow(unsafe_code)]
fn recovery_failure_refuses_inherited_cursor_identity() {
    let _environment = ENV_LOCK.lock().unwrap();
    let scratch = ScratchState::new("cursor-recovery", true);
    validate_layout(&scratch.state).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, "cursor-canonical-private-id").unwrap();
    write_pending(&journal, &plan).unwrap();
    scratch
        .store()
        .save_conversation_identity(
            &plan.edition_slug,
            &plan.scopes().unwrap(),
            &plan.external_id,
            &plan.mutation_token,
        )
        .unwrap();
    fs::write(&scratch.state.chat_file, b"divergent-private-id\n").unwrap();
    drop(journal);

    let original = std::env::var_os("CURSOR_AGENT_CHAT_ID");
    unsafe { std::env::set_var("CURSOR_AGENT_CHAT_ID", "inherited-private-id") };
    assert!(
        scratch
            .state
            .resolve_chat_for(crate::agent::AgentBackend::Cursor)
            .is_err(),
        "canonical recovery corruption must remain visible to provider callers"
    );
    assert_eq!(
        scratch
            .state
            .read_chat_for(crate::agent::AgentBackend::Cursor),
        None
    );
    match original {
        Some(value) => unsafe { std::env::set_var("CURSOR_AGENT_CHAT_ID", value) },
        None => unsafe { std::env::remove_var("CURSOR_AGENT_CHAT_ID") },
    }
}

#[cfg(unix)]
#[test]
fn external_overrides_recover_from_journal_after_environment_resolution_changes() {
    use std::os::unix::fs::PermissionsExt as _;

    let mut scratch = ScratchState::new("override", true);
    let external = scratch.root.path().join("external-overrides");
    fs::create_dir(&external).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
    let external_chat = external.join("custom-chat");
    let external_history = external.join("custom-history");
    scratch.state.chat_file = external_chat.clone();
    scratch.state.history_file = external_history.clone();
    validate_layout(&scratch.state).unwrap();

    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, "external-private-id").unwrap();
    write_pending(&journal, &plan).unwrap();
    scratch
        .store()
        .save_conversation_identity(
            &plan.edition_slug,
            &plan.scopes().unwrap(),
            &plan.external_id,
            &plan.mutation_token,
        )
        .unwrap();
    drop(journal);

    // A later process has no CHAT/HISTORY override. Recovery must replay the
    // exact validated targets retained in the private journal, not reinterpret
    // the transaction through today's environment.
    scratch.state.chat_file = scratch.state.state_dir.join("chat-id");
    scratch.state.history_file = scratch.state.state_dir.join("history.log");
    let journal = lock_journal(&scratch.state).unwrap();
    recover_pending(&scratch.state, &journal).unwrap();
    assert_eq!(
        fs::read_to_string(external_chat).unwrap(),
        "external-private-id\n"
    );
    assert_eq!(
        fs::read_to_string(external.join("custom-chat.export")).unwrap(),
        "ABBEY_CHAT_ID='external-private-id'\n"
    );
    assert!(
        fs::read_to_string(external_history)
            .unwrap()
            .contains("external-private-id")
    );
    assert!(!scratch.state.chat_file.exists());
    assert!(!scratch.state.history_file.exists());
}

#[test]
fn aliased_and_reserved_targets_fail_before_canonical_commit() {
    for (role, target) in [
        ("chat", "pending.json"),
        ("history", "lock"),
        ("model", "runtime.sqlite"),
        ("chat", "legacy-conversation-backups"),
    ] {
        let mut scratch = ScratchState::new("reserved", true);
        let reserved = scratch.state.state_dir.join("daemon").join(target);
        match role {
            "chat" => scratch.state.chat_file = reserved,
            "history" => scratch.state.history_file = reserved,
            "model" => scratch.state.model_file = reserved,
            _ => unreachable!(),
        }
        let error = save_chat(&scratch.state, "reserved-private-id").unwrap_err();
        assert!(error.to_string().contains("runtime subtree"));
        assert!(
            !scratch
                .state
                .state_dir
                .join("daemon/runtime.sqlite")
                .exists()
        );
    }

    let mut alias = ScratchState::new("aliased", true);
    alias.state.chat_file = alias.state.state_dir.join("same");
    alias.state.history_file = alias.state.state_dir.join("same.export");
    let error = save_chat(&alias.state, "alias-private-id").unwrap_err();
    assert!(error.to_string().contains("pairwise distinct"));
    assert!(!alias.state.state_dir.join("daemon/runtime.sqlite").exists());

    let mut model = ScratchState::new("model-alias", true);
    model.state.model_file = model.state.chat_file.clone();
    let error = write_model(&model.state, "private-model").unwrap_err();
    assert!(error.to_string().contains("must be distinct"));
    assert!(!model.state.chat_file.exists());
}

#[cfg(unix)]
#[test]
fn symlink_override_fails_closed_without_mutating_its_target() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let mut scratch = ScratchState::new("symlink", true);
    let external = scratch.root.path().join("external-symlink");
    fs::create_dir(&external).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
    let target = external.join("real-chat");
    fs::write(&target, b"unchanged\n").unwrap();
    let link = external.join("chat-link");
    symlink(&target, &link).unwrap();
    scratch.state.chat_file = link;

    assert!(save_chat(&scratch.state, "symlink-private-id").is_err());
    assert_eq!(fs::read(target).unwrap(), b"unchanged\n");
    assert!(
        !scratch
            .state
            .state_dir
            .join("daemon/runtime.sqlite")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn ancestor_symlink_cannot_alias_the_reserved_journal_subtree() {
    use std::os::unix::fs::symlink;

    let mut scratch = ScratchState::new("ancestor-symlink", true);
    read_chat(&scratch.state).unwrap();
    let alias = scratch.state.state_dir.join("daemon-alias");
    symlink(scratch.state.state_dir.join("daemon"), &alias).unwrap();
    scratch.state.chat_file = alias.join(JOURNAL_DIR).join(JOURNAL_PENDING);

    let error = save_chat(&scratch.state, "ancestor-symlink-private-id").unwrap_err();
    assert!(error.to_string().contains("runtime subtree"));
    assert!(!scratch.pending().exists());
}

#[test]
fn shell_export_quotes_provider_metacharacters_and_single_quotes() {
    let scratch = ScratchState::new("shell-quote", true);
    let id = "provider $(touch nope); 'quoted' value";
    save_chat(&scratch.state, id).unwrap();
    let export = fs::read_to_string(scratch.state.chat_file.with_extension("export")).unwrap();
    assert_eq!(
        export,
        "ABBEY_CHAT_ID='provider $(touch nope); '\\''quoted'\\'' value'\n"
    );
    assert!(!export.starts_with("ABBEY_CHAT_ID=provider"));
}

#[test]
fn control_character_cwd_fails_before_canonical_commit() {
    let mut scratch = ScratchState::new("cwd-control", true);
    scratch.state.cwd = scratch.root.path().join("private-\u{1b}[31m-cwd");
    let error = save_chat(&scratch.state, "cwd-private-id").unwrap_err();
    assert!(error.to_string().contains("working directory"));
    assert!(!scratch.state.history_file.exists());
    assert!(
        !scratch
            .state
            .state_dir
            .join("daemon/runtime.sqlite")
            .exists()
    );
}

#[test]
fn journal_debug_redacts_raw_identity_and_working_directory() {
    let plan = JournalPlan {
        schema_version: JOURNAL_SCHEMA,
        mutation_token: "private-token".into(),
        edition_slug: crate::edition::ACTIVE.slug().into(),
        external_id: "private-conversation".into(),
        cwd_hex: lower_hex(b"/private/workspace"),
        cwd_display: "/private/workspace".into(),
        per_cwd: true,
        targets: Vec::new(),
    };
    let debug = format!("{plan:?}");
    assert!(!debug.contains("private-token"));
    assert!(!debug.contains("private-conversation"));
    assert!(!debug.contains("private/workspace"));
    assert!(debug.contains("[REDACTED]"));
}

#[cfg(unix)]
#[test]
fn journal_and_mirror_artifacts_are_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = ScratchState::new("permissions", true);
    save_chat(&scratch.state, "permission-private-id").unwrap();
    let journal = scratch.state.state_dir.join("daemon").join(JOURNAL_DIR);
    for path in [
        scratch.state.state_dir.clone(),
        scratch.state.cwd_dir.clone(),
        scratch.state.state_dir.join("daemon"),
        journal.clone(),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    for path in [
        journal.join(JOURNAL_LOCK),
        scratch.state.chat_file.clone(),
        scratch.state.chat_file.with_extension("export"),
        scratch.state.history_file.clone(),
        scratch.state.active_chat_file(),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
