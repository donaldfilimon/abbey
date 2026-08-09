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
