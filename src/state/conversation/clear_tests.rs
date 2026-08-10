use super::*;
use std::sync::{Arc, Barrier};

struct Scratch {
    root: PathBuf,
    state: AbbeyState,
}

impl Scratch {
    fn new(tag: &str, per_cwd: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "abbey-clear-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let state_dir = root.join("state");
        let cwd = root.join("private-cwd");
        fs::create_dir_all(state_dir.join("by-cwd")).unwrap();
        fs::create_dir(&cwd).unwrap();
        Self {
            state: AbbeyState {
                chat_file: state_dir.join("chat-id"),
                model_file: state_dir.join("model"),
                history_file: state_dir.join("history.log"),
                cwd_dir: state_dir.join("by-cwd"),
                state_dir,
                per_cwd,
                cwd,
            },
            root,
        }
    }

    fn pending(&self) -> PathBuf {
        self.state
            .state_dir
            .join("daemon")
            .join(JOURNAL_DIR)
            .join(JOURNAL_PENDING)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn rebind(plan: &mut ClearPlan) {
    let nonce = plan.mutation_token.split_once(':').unwrap().0.to_owned();
    plan.mutation_token = format!("{nonce}:{}", plan_binding_digest(plan, &nonce));
}

#[test]
fn uncommitted_clear_plan_is_discarded_without_removal() {
    let scratch = Scratch::new("uncommitted", true);
    save_chat(&scratch.state, "uncommitted-clear-secret").unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, false).unwrap();
    write_plan(&journal, &plan).unwrap();
    drop(journal);

    let journal = lock_journal(&scratch.state).unwrap();
    super::recover_pending(
        &scratch.state,
        &journal,
        &fs::read(scratch.pending()).unwrap(),
    )
    .unwrap();
    assert!(scratch.state.active_chat_file().exists());
    assert!(!scratch.pending().exists());
}

#[test]
fn committed_scope_clear_recovers_once_and_preserves_global_history() {
    let scratch = Scratch::new("scope-recover", true);
    save_chat(&scratch.state, "scope-clear-secret").unwrap();
    let history = fs::read(&scratch.state.history_file).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, false).unwrap();
    write_plan(&journal, &plan).unwrap();
    let store = open_metadata_store(&scratch.state).unwrap();
    let scopes = plan.scopes().unwrap().unwrap();
    store
        .clear_conversation_identity(&plan.edition_slug, Some(&scopes), &plan.mutation_token)
        .unwrap();
    drop(journal);

    read_chat(&scratch.state).unwrap();
    read_chat(&scratch.state).unwrap();
    assert!(!scratch.state.active_chat_file().exists());
    assert_eq!(
        fs::read_to_string(&scratch.state.chat_file).unwrap(),
        "scope-clear-secret\n"
    );
    assert_eq!(fs::read(&scratch.state.history_file).unwrap(), history);
    assert!(!scratch.pending().exists());
}

#[test]
fn clear_all_recovers_partial_removal_and_preserves_retained_data() {
    let scratch = Scratch::new("all-partial", true);
    save_chat(&scratch.state, "all-clear-secret").unwrap();
    let other = scratch.state.cwd_dir.join("other-project");
    fs::write(&other, b"other-secret\n").unwrap();
    let history = fs::read(&scratch.state.history_file).unwrap();
    let journal = lock_journal(&scratch.state).unwrap();
    let plan = prepare_plan(&scratch.state, true).unwrap();
    write_plan(&journal, &plan).unwrap();
    let store = open_metadata_store(&scratch.state).unwrap();
    let commit = store
        .clear_conversation_identity(&plan.edition_slug, None, &plan.mutation_token)
        .unwrap();
    ensure_commit_effect(&store, &plan, &commit).unwrap();
    let first = path_from_hex(&plan.targets[0].path_hex).unwrap();
    if first.exists() {
        remove_mirror(&first).unwrap();
    }
    drop(journal);

    read_chat(&scratch.state).unwrap();
    assert!(!scratch.state.chat_file.exists());
    assert!(!scratch.state.chat_file.with_extension("export").exists());
    assert!(
        fs::read_dir(&scratch.state.cwd_dir)
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(fs::read(&scratch.state.history_file).unwrap(), history);
    assert_eq!(store.current_identity_commit().unwrap(), Some(commit));
}

#[test]
fn tombstones_refuse_stale_mirrors_and_save_supersedes_them() {
    let scratch = Scratch::new("stale-read", true);
    save_chat(&scratch.state, "stale-clear-secret").unwrap();
    let store = open_metadata_store(&scratch.state).unwrap();
    let cwd = ConversationIdentityScope::working_directory(&scratch.state.cwd);
    store
        .clear_conversation_identity(
            crate::edition::ACTIVE.slug(),
            Some(std::slice::from_ref(&cwd)),
            "direct-scope-clear",
        )
        .unwrap();
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("stale-clear-secret"),
        "tombstoned cwd must fall back to the still-current global scope"
    );
    store
        .clear_conversation_identity(crate::edition::ACTIVE.slug(), None, "direct-all-clear")
        .unwrap();
    assert_eq!(read_chat(&scratch.state).unwrap(), None);
    save_chat(&scratch.state, "resaved-secret").unwrap();
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("resaved-secret")
    );
}

#[test]
fn concurrent_clear_and_save_serialize_without_resurrection() {
    let scratch = Scratch::new("clear-save-race", true);
    save_chat(&scratch.state, "race-old-secret").unwrap();
    let state = Arc::new(scratch.state.clone());
    let barrier = Arc::new(Barrier::new(3));
    let clear_state = Arc::clone(&state);
    let clear_barrier = Arc::clone(&barrier);
    let clear = std::thread::spawn(move || {
        clear_barrier.wait();
        clear_chat(&clear_state, false)
    });
    let save_state = Arc::clone(&state);
    let save_barrier = Arc::clone(&barrier);
    let save = std::thread::spawn(move || {
        save_barrier.wait();
        save_chat(&save_state, "race-new-secret")
    });
    barrier.wait();
    clear.join().unwrap().unwrap();
    save.join().unwrap().unwrap();
    assert_eq!(
        read_chat(&state).unwrap().as_deref(),
        Some("race-new-secret")
    );
    assert!(!scratch.pending().exists());
}

#[test]
fn malformed_roles_and_omitted_inventory_fail_closed() {
    let scratch = Scratch::new("tampered-plan", true);
    save_chat(&scratch.state, "tampered-clear-secret").unwrap();
    fs::write(scratch.state.cwd_dir.join("other"), b"other\n").unwrap();

    let mut role = prepare_plan(&scratch.state, true).unwrap();
    role.targets[0].role = ClearTargetRole::Active;
    rebind(&mut role);
    assert!(validate_plan(&scratch.state, &role).is_err());

    let mut omitted = prepare_plan(&scratch.state, true).unwrap();
    let cwd = omitted
        .targets
        .iter()
        .position(|target| target.role == ClearTargetRole::Cwd)
        .unwrap();
    omitted.targets.remove(cwd);
    rebind(&mut omitted);
    let error = validate_plan(&scratch.state, &omitted).unwrap_err();
    assert!(error.to_string().contains("omits"));
}

#[test]
fn lossy_cwd_key_collision_never_deletes_another_scope_mirror() {
    for (first_id, second_id) in [
        ("collision-first", "collision-second"),
        ("collision-shared", "collision-shared"),
    ] {
        let scratch = Scratch::new("cwd-key-collision", true);
        let nested = scratch.root.join("a/b");
        let underscored = scratch.root.join("a_b");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(&underscored).unwrap();
        assert_eq!(
            AbbeyState::cwd_key(&nested),
            AbbeyState::cwd_key(&underscored)
        );

        let mut first = scratch.state.clone();
        first.cwd = nested;
        save_chat(&first, first_id).unwrap();
        let mut second = scratch.state.clone();
        second.cwd = underscored;
        save_chat(&second, second_id).unwrap();
        let shared_path = second.active_chat_file();
        let before = fs::read(&shared_path).unwrap();
        let before_commit = open_metadata_store(&second)
            .unwrap()
            .current_identity_commit()
            .unwrap();

        assert!(clear_chat(&first, false).is_err());
        assert_eq!(fs::read(&shared_path).unwrap(), before);
        assert_eq!(
            open_metadata_store(&second)
                .unwrap()
                .current_identity_commit()
                .unwrap(),
            before_commit
        );
        assert_eq!(read_chat(&second).unwrap().as_deref(), Some(second_id));
    }
}

#[test]
fn legacy_untracked_clear_cuts_forward_and_global_clear_preserves_cwd() {
    let empty = Scratch::new("empty-cutover", true);
    clear_chat(&empty.state, false).unwrap();
    assert!(!empty.state.active_chat_file().exists());
    let empty_scope = ConversationIdentityScope::working_directory(&empty.state.cwd);
    assert_eq!(
        open_metadata_store(&empty.state)
            .unwrap()
            .identity_scope_state(crate::edition::ACTIVE.slug(), &empty_scope, None)
            .unwrap(),
        IdentityScopeState::Tombstoned
    );

    let legacy = Scratch::new("legacy-cutover", true);
    fs::write(legacy.state.active_chat_file(), b"legacy-secret\n").unwrap();
    fs::write(&legacy.state.chat_file, b"legacy-secret\n").unwrap();
    clear_chat(&legacy.state, false).unwrap();
    assert!(!legacy.state.active_chat_file().exists());
    assert_eq!(
        fs::read_to_string(&legacy.state.chat_file).unwrap(),
        "legacy-secret\n"
    );
    let cwd = ConversationIdentityScope::working_directory(&legacy.state.cwd);
    assert_eq!(
        open_metadata_store(&legacy.state)
            .unwrap()
            .identity_scope_state(crate::edition::ACTIVE.slug(), &cwd, Some("legacy-secret"))
            .unwrap(),
        IdentityScopeState::Tombstoned
    );

    let scratch = Scratch::new("global-preserves-cwd", true);
    save_chat(&scratch.state, "global-clear-secret").unwrap();
    let cwd_file = scratch.state.active_chat_file();
    let mut global = scratch.state.clone();
    global.per_cwd = false;
    clear_chat(&global, false).unwrap();
    assert!(!global.chat_file.exists());
    assert!(!global.chat_file.with_extension("export").exists());
    assert_eq!(
        fs::read_to_string(&cwd_file).unwrap(),
        "global-clear-secret\n"
    );
    assert_eq!(
        read_chat(&scratch.state).unwrap().as_deref(),
        Some("global-clear-secret")
    );
}

#[test]
fn clear_journal_debug_and_database_exclude_raw_path_canaries() {
    let scratch = Scratch::new("clear-redaction", true);
    save_chat(&scratch.state, "clear-redaction-secret").unwrap();
    let plan = prepare_plan(&scratch.state, true).unwrap();
    let debug = format!("{plan:?}");
    assert!(!debug.contains("clear-redaction-secret"));
    assert!(!debug.contains("private-cwd"));
    clear_chat(&scratch.state, true).unwrap();
    for entry in fs::read_dir(scratch.state.state_dir.join("daemon")).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = fs::read(path).unwrap();
            for canary in [b"clear-redaction-secret".as_slice(), b"private-cwd"] {
                assert!(!bytes.windows(canary.len()).any(|window| window == canary));
            }
        }
    }
}

#[test]
fn deleted_clear_effect_rows_never_resurrect_recreated_stale_mirrors() {
    let scope = Scratch::new("deleted-scope-effect", true);
    save_chat(&scope.state, "deleted-scope-secret").unwrap();
    clear_chat(&scope.state, false).unwrap();
    fs::write(scope.state.active_chat_file(), b"deleted-scope-secret\n").unwrap();
    let database = RuntimeStore::path_for_state_dir(&scope.state.state_dir.join("daemon"));
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute("DELETE FROM conversation_identity_tombstones", [])
        .unwrap();
    assert!(read_chat(&scope.state).is_err());

    let all = Scratch::new("deleted-all-effect", true);
    save_chat(&all.state, "deleted-all-secret").unwrap();
    clear_chat(&all.state, true).unwrap();
    fs::write(&all.state.chat_file, b"deleted-all-secret\n").unwrap();
    let database = RuntimeStore::path_for_state_dir(&all.state.state_dir.join("daemon"));
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute("DELETE FROM conversation_identity_clear_all", [])
        .unwrap();
    assert!(read_chat(&all.state).is_err());

    let forged = Scratch::new("forged-future-selection", true);
    save_chat(&forged.state, "forged-future-secret").unwrap();
    clear_chat(&forged.state, true).unwrap();
    fs::write(&forged.state.chat_file, b"forged-future-secret\n").unwrap();
    let database = RuntimeStore::path_for_state_dir(&forged.state.state_dir.join("daemon"));
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "INSERT INTO conversation_identity_scopes(
                    edition_sha256, scope_sha256, alias_sha256, conversation_id,
                    revision, updated_at
                 )
                 SELECT m.edition_sha256, s.scope_sha256, m.alias_sha256,
                        m.conversation_id, 999, '2026-08-09T00:00:00.000Z'
                 FROM conversation_identity_mutations m
                 JOIN conversation_identity_mutation_scopes s
                   ON s.mutation_sha256=m.mutation_sha256
                 WHERE m.operation='save' AND s.scope_sha256=?1",
            [ConversationIdentityScope::global().as_sha256()],
        )
        .unwrap();
    drop(connection);
    assert!(read_chat(&forged.state).is_err());
}
