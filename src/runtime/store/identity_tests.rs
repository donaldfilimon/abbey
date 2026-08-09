use super::*;
use crate::app_core::{IdempotencyKey, RunState};
use crate::runtime::{NewRun, NewRunEvent};
use std::path::Path;

#[test]
fn identity_save_is_token_idempotent_revisioned_and_reopens_opaque() {
    let dir = tempfile_dir("save-reopen");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let store = RuntimeStore::open_metadata(&path).unwrap();
    let scope = ConversationIdentityScope::working_directory(Path::new("/secret/project"));
    let first = store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&scope),
            "raw-secret-chat",
            "mutation-one",
        )
        .unwrap();
    let repeated = store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&scope),
            "raw-secret-chat",
            "mutation-one",
        )
        .unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.revision, 1);
    assert_eq!(first.operation, IdentityOperation::Save);
    assert!(first.matches_save_scopes(
        "abbey",
        std::slice::from_ref(&scope),
        "raw-secret-chat",
        "mutation-one"
    ));
    drop(store);

    let reopened = RuntimeStore::open_metadata(&path).unwrap();
    assert_eq!(reopened.current_identity_commit().unwrap(), Some(first));
    let second = reopened
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&scope),
            "second-secret",
            "mutation-two",
        )
        .unwrap();
    assert_eq!(second.revision, 2);
    assert!(format!("{second:?}").contains("mutation_sha256"));
    assert!(!format!("{second:?}").contains("second-secret"));
    drop(reopened);

    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path()).unwrap();
        for secret in [
            b"raw-secret-chat".as_slice(),
            b"second-secret",
            b"/secret/project",
        ] {
            assert!(!bytes.windows(secret.len()).any(|window| window == secret));
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn identity_multi_scope_save_is_one_atomic_revision() {
    let dir = tempfile_dir("multi-scope");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let store = RuntimeStore::open_metadata(&path).unwrap();
    let scopes = [
        ConversationIdentityScope::working_directory(Path::new("/private/worktree")),
        ConversationIdentityScope::global(),
    ];
    let commit = store
        .save_conversation_identity("abbey", &scopes, "shared-chat", "multi-token")
        .unwrap();
    assert_eq!(commit.revision, 1);
    assert!(commit.matches_save_scopes("abbey", &scopes, "shared-chat", "multi-token"));
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(DISTINCT revision) FROM conversation_identity_scopes",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM conversation_identity_scopes",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        2
    );
    drop(conn);
    assert!(matches!(
        store.save_conversation_identity(
            "abbey",
            &[scopes[0].clone(), scopes[0].clone()],
            "shared-chat",
            "duplicate-token"
        ),
        Err(StoreError::InvalidInput(_))
    ));
    assert_eq!(store.current_identity_commit().unwrap().unwrap(), commit);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn identity_scope_and_edition_are_isolated_and_token_reuse_fails_closed() {
    let dir = tempfile_dir("scope-edition");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let global = ConversationIdentityScope::global();
    let cwd = ConversationIdentityScope::working_directory(Path::new("/different"));
    store
        .save_conversation_identity("abbey", std::slice::from_ref(&global), "chat-a", "token-a")
        .unwrap();
    let conn = store.conn.lock().unwrap();
    let safe = edition_sha256("abbey").unwrap();
    let personal = edition_sha256("abbey-personal").unwrap();
    assert_eq!(scope_count(&conn, &safe, &cwd), 0);
    assert_eq!(scope_count(&conn, &personal, &global), 0);
    drop(conn);
    assert!(matches!(
        store.save_conversation_identity("abbey", std::slice::from_ref(&cwd), "chat-b", "token-a"),
        Err(StoreError::InvalidInput(_))
    ));
    assert_eq!(
        store.current_identity_commit().unwrap().unwrap().revision,
        1
    );
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn identity_native_collision_rolls_back_without_marker_or_scope() {
    let dir = tempfile_dir("collision");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let store = RuntimeStore::open_metadata(&path).unwrap();
    let external = external_identity("collision-secret").unwrap();
    store
        .create_conversation(&external.conversation_id)
        .unwrap();
    assert!(matches!(
        store.save_conversation_identity(
            "abbey",
            &[ConversationIdentityScope::global()],
            "collision-secret",
            "collision-token"
        ),
        Err(StoreError::InvalidInput(_))
    ));
    assert!(store.current_identity_commit().unwrap().is_none());
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM conversation_identity_scopes",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(conn);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn metadata_open_migrates_without_recovering_interrupted_runs() {
    let dir = tempfile_dir("metadata-open");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let store = RuntimeStore::open(&path).unwrap();
    let run = store
        .create_or_get_run(NewRun {
            conversation_id: None,
            idempotency_key: IdempotencyKey::new(),
            request_digest: "a".repeat(64),
        })
        .unwrap();
    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Starting,
            NewRunEvent {
                kind: "run_starting".into(),
                payload: serde_json::json!({}),
            },
        )
        .unwrap();
    drop(store);

    let metadata = RuntimeStore::open_metadata(&path).unwrap();
    assert_eq!(metadata.recovered_runs(), 0);
    assert_eq!(
        metadata.get_run(&run.id).unwrap().unwrap().status,
        RunState::Starting
    );
    drop(metadata);
    assert_eq!(RuntimeStore::open(&path).unwrap().recovered_runs(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn identity_commit_rejects_tampered_digests_and_timestamps() {
    let dir = tempfile_dir("commit-corruption");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let store = RuntimeStore::open_metadata(&path).unwrap();
    store
        .save_conversation_identity(
            "abbey",
            &[ConversationIdentityScope::global()],
            "private-chat",
            "private-token",
        )
        .unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        conn.execute(
            "UPDATE conversation_identity_commit SET mutation_sha256='ABC' WHERE singleton=1",
            [],
        )
        .unwrap();
    }
    assert!(matches!(
        store.current_identity_commit(),
        Err(StoreError::CorruptData(_))
    ));
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversation_identity_commit
             SET mutation_sha256=?1, committed_at='2026-08-09T12:00:00-04:00'
             WHERE singleton=1",
            [mutation_sha256("private-token").unwrap()],
        )
        .unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints=OFF;")
            .unwrap();
    }
    assert!(matches!(
        store.current_identity_commit(),
        Err(StoreError::CorruptData(_))
    ));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn scope_clear_is_idempotent_tombstoned_and_superseded_by_save() {
    let dir = tempfile_dir("scope-clear");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let cwd = ConversationIdentityScope::working_directory(Path::new("/private/clear-cwd"));
    let global = ConversationIdentityScope::global();
    store
        .save_conversation_identity(
            "abbey",
            &[cwd.clone(), global.clone()],
            "clear-secret",
            "save-token",
        )
        .unwrap();
    let clear = store
        .clear_conversation_identity("abbey", Some(std::slice::from_ref(&cwd)), "clear-token")
        .unwrap();
    assert_eq!(clear.operation, IdentityOperation::ClearScope);
    assert!(clear.matches_clear_scopes("abbey", std::slice::from_ref(&cwd), "clear-token"));
    assert_eq!(
        store
            .identity_scope_state("abbey", &cwd, Some("clear-secret"))
            .unwrap(),
        IdentityScopeState::Tombstoned
    );
    assert_eq!(
        store
            .identity_scope_state("abbey", &global, Some("clear-secret"))
            .unwrap(),
        IdentityScopeState::Current
    );
    assert_eq!(
        store
            .clear_conversation_identity("abbey", Some(std::slice::from_ref(&cwd)), "clear-token")
            .unwrap(),
        clear
    );
    store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&cwd),
            "clear-secret",
            "resave-token",
        )
        .unwrap();
    assert_eq!(
        store
            .identity_scope_state("abbey", &cwd, Some("clear-secret"))
            .unwrap(),
        IdentityScopeState::Current
    );
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM conversation_identity_aliases",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM conversations", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(conn);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn clear_all_is_edition_scoped_and_rejects_token_or_effect_tampering() {
    let dir = tempfile_dir("clear-all");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let global = ConversationIdentityScope::global();
    store
        .save_conversation_identity(
            "abbey-personal",
            std::slice::from_ref(&global),
            "personal-secret",
            "personal-save",
        )
        .unwrap();
    store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "safe-secret",
            "safe-save",
        )
        .unwrap();
    let clear = store
        .clear_conversation_identity("abbey", None, "clear-all-token")
        .unwrap();
    assert!(clear.matches_clear_all("abbey", "clear-all-token"));
    assert_eq!(
        store
            .identity_scope_state("abbey", &global, Some("safe-secret"))
            .unwrap(),
        IdentityScopeState::Tombstoned
    );
    assert_eq!(
        store
            .identity_scope_state("abbey-personal", &global, Some("personal-secret"))
            .unwrap(),
        IdentityScopeState::Current
    );
    assert!(matches!(
        store.clear_conversation_identity(
            "abbey",
            Some(std::slice::from_ref(&global)),
            "clear-all-token"
        ),
        Err(StoreError::InvalidInput(_))
    ));

    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM conversation_identity_clear_all WHERE edition_sha256=?1",
            [edition_sha256("abbey").unwrap()],
        )
        .unwrap();
    }
    assert!(matches!(
        store.clear_conversation_identity("abbey", None, "clear-all-token"),
        Err(StoreError::CorruptData(_))
    ));
    assert!(matches!(
        store.verify_clear_conversation_identity("abbey", None, &clear),
        Err(StoreError::CorruptData(_))
    ));
    assert!(matches!(
        store.identity_scope_state("abbey", &global, Some("safe-secret")),
        Err(StoreError::CorruptData(_))
    ));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn clear_all_marker_digest_tampering_is_rejected() {
    let dir = tempfile_dir("clear-marker-tamper");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    store
        .clear_conversation_identity("abbey", None, "clear-token")
        .unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversation_identity_commit SET scope_sha256=?1 WHERE singleton=1",
            ["b".repeat(64)],
        )
        .unwrap();
    }
    assert!(matches!(
        store.current_identity_commit(),
        Err(StoreError::CorruptData(_))
    ));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn historical_mutation_tokens_cannot_replay_across_save_and_clear() {
    let dir = tempfile_dir("token-replay");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let global = ConversationIdentityScope::global();
    store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "replay-secret",
            "historical-save-token",
        )
        .unwrap();
    store
        .clear_conversation_identity(
            "abbey",
            Some(std::slice::from_ref(&global)),
            "historical-clear-token",
        )
        .unwrap();
    assert!(matches!(
        store.save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "replay-secret",
            "historical-save-token"
        ),
        Err(StoreError::InvalidInput(_))
    ));
    let current = store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "current-secret",
            "current-save-token",
        )
        .unwrap();
    assert!(matches!(
        store.clear_conversation_identity(
            "abbey",
            Some(std::slice::from_ref(&global)),
            "historical-clear-token"
        ),
        Err(StoreError::InvalidInput(_))
    ));
    assert_eq!(store.current_identity_commit().unwrap(), Some(current));
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM conversation_identity_mutations",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        3
    );
    drop(conn);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_scope_tombstone_cannot_resurrect_a_stale_mirror() {
    let dir = tempfile_dir("missing-scope-tombstone");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let global = ConversationIdentityScope::global();
    store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "stale-secret",
            "save-token",
        )
        .unwrap();
    store
        .clear_conversation_identity("abbey", Some(std::slice::from_ref(&global)), "clear-token")
        .unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute("DELETE FROM conversation_identity_tombstones", [])
            .unwrap();
    }
    assert!(matches!(
        store.identity_scope_state("abbey", &global, Some("stale-secret")),
        Err(StoreError::CorruptData(_))
    ));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_saved_selection_cannot_fall_back_to_an_untracked_mirror() {
    let dir = tempfile_dir("missing-save-selection");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let global = ConversationIdentityScope::global();
    store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "saved-secret",
            "save-token",
        )
        .unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute("DELETE FROM conversation_identity_scopes", [])
            .unwrap();
    }
    assert!(matches!(
        store.identity_scope_state("abbey", &global, Some("saved-secret")),
        Err(StoreError::CorruptData(_))
    ));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn forged_future_selection_cannot_outrank_an_intact_clear_receipt() {
    let dir = tempfile_dir("forged-future-selection");
    let store = RuntimeStore::open_metadata(&RuntimeStore::path_for_state_dir(&dir)).unwrap();
    let global = ConversationIdentityScope::global();
    store
        .save_conversation_identity(
            "abbey",
            std::slice::from_ref(&global),
            "forged-secret",
            "save-token",
        )
        .unwrap();
    let (alias, conversation) = {
        let conn = store.conn.lock().unwrap();
        conn.query_row(
            "SELECT alias_sha256, conversation_id FROM conversation_identity_scopes",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap()
    };
    store
        .clear_conversation_identity("abbey", None, "clear-token")
        .unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO conversation_identity_scopes(
                edition_sha256, scope_sha256, alias_sha256, conversation_id,
                revision, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 999, '2026-08-09T00:00:00.000Z')",
            params![
                edition_sha256("abbey").unwrap(),
                global.as_sha256(),
                alias,
                conversation
            ],
        )
        .unwrap();
    }
    assert!(matches!(
        store.identity_scope_state("abbey", &global, Some("forged-secret")),
        Err(StoreError::CorruptData(_))
    ));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn v3_multiple_cwd_selections_remain_authenticated_after_v4_migration() {
    let dir = tempfile_dir("v3-multi-cwd-migration");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    crate::runtime::migrations::apply_through_v3(&mut conn, "2026-08-09T00:00:00.000Z").unwrap();
    let edition = edition_sha256("abbey").unwrap();
    let cwd_a = ConversationIdentityScope::working_directory(Path::new("/private/cwd-a"));
    let cwd_b = ConversationIdentityScope::working_directory(Path::new("/private/cwd-b"));
    let global = ConversationIdentityScope::global();
    let identity_a = external_identity("v3-secret-a").unwrap();
    let identity_b = external_identity("v3-secret-b").unwrap();
    for (identity, timestamp) in [
        (&identity_a, "2026-08-09T00:00:01.000Z"),
        (&identity_b, "2026-08-09T00:00:02.000Z"),
    ] {
        conn.execute(
            "INSERT INTO conversations(id, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![identity.conversation_id.as_str(), timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_identity_aliases(
                alias_sha256, conversation_id, origin, created_at
             ) VALUES (?1, ?2, 'runtime_v3', ?3)",
            params![
                identity.alias_sha256,
                identity.conversation_id.as_str(),
                timestamp
            ],
        )
        .unwrap();
    }
    for (scope, identity, revision, timestamp) in [
        (&cwd_a, &identity_a, 1_i64, "2026-08-09T00:00:01.000Z"),
        (&cwd_b, &identity_b, 2_i64, "2026-08-09T00:00:02.000Z"),
        (&global, &identity_b, 2_i64, "2026-08-09T00:00:02.000Z"),
    ] {
        conn.execute(
            "INSERT INTO conversation_identity_scopes(
                edition_sha256, scope_sha256, alias_sha256, conversation_id,
                revision, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                edition,
                scope.as_sha256(),
                identity.alias_sha256,
                identity.conversation_id.as_str(),
                revision,
                timestamp
            ],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO conversation_identity_commit(
            singleton, revision, operation, edition_sha256, scope_sha256,
            scope_set_sha256, alias_sha256, conversation_id, mutation_sha256, committed_at
         ) VALUES (1, 2, 'save', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            edition,
            cwd_b.as_sha256(),
            scope_set_sha256(&[cwd_b.clone(), global.clone()]).unwrap(),
            identity_b.alias_sha256,
            identity_b.conversation_id.as_str(),
            mutation_sha256("v3-current-token").unwrap(),
            "2026-08-09T00:00:02.000Z"
        ],
    )
    .unwrap();
    drop(conn);

    let store = RuntimeStore::open_metadata(&path).unwrap();
    for (scope, candidate) in [
        (&cwd_a, "v3-secret-a"),
        (&cwd_b, "v3-secret-b"),
        (&global, "v3-secret-b"),
    ] {
        assert_eq!(
            store
                .identity_scope_state("abbey", scope, Some(candidate))
                .unwrap(),
            IdentityScopeState::Current
        );
    }
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

fn tempfile_dir(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "abbey-runtime-identity-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn scope_count(
    conn: &rusqlite::Connection,
    edition_sha256: &str,
    scope: &ConversationIdentityScope,
) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM conversation_identity_scopes
         WHERE edition_sha256=?1 AND scope_sha256=?2",
        params![edition_sha256, scope.as_sha256()],
        |row| row.get(0),
    )
    .unwrap()
}
