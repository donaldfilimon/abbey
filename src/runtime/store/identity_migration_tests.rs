use super::*;
use std::path::Path;

#[test]
fn canonical_scope_selection_authenticates_v3_unordered_scope_membership() {
    let dir = tempfile_dir("v3-unordered-scope-membership");
    let path = RuntimeStore::path_for_state_dir(&dir);
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    crate::runtime::migrations::apply_through_v3(&mut conn, "2026-08-09T00:00:00.000Z").unwrap();
    let edition = edition_sha256("abbey").unwrap();
    let global = ConversationIdentityScope::global();
    let cwd = ConversationIdentityScope::working_directory(Path::new("/private/v3-primary"));
    let external = external_identity("v3-unordered-secret").unwrap();
    let timestamp = "2026-08-09T00:00:01.000Z";
    conn.execute(
        "INSERT INTO conversations(id, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![external.conversation_id.as_str(), timestamp],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_identity_aliases(
            alias_sha256, conversation_id, origin, created_at
         ) VALUES (?1, ?2, 'runtime_v3', ?3)",
        params![
            external.alias_sha256,
            external.conversation_id.as_str(),
            timestamp
        ],
    )
    .unwrap();

    // Deliberately store the secondary scope first while the v3 commit's
    // authenticated scope-set digest retains primary-first order.
    for scope in [&cwd, &global] {
        conn.execute(
            "INSERT INTO conversation_identity_scopes(
                edition_sha256, scope_sha256, alias_sha256, conversation_id,
                revision, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            params![
                edition,
                scope.as_sha256(),
                external.alias_sha256,
                external.conversation_id.as_str(),
                timestamp
            ],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO conversation_identity_commit(
            singleton, revision, operation, edition_sha256, scope_sha256,
            scope_set_sha256, alias_sha256, conversation_id, mutation_sha256, committed_at
         ) VALUES (1, 1, 'save', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            edition,
            global.as_sha256(),
            scope_set_sha256(&[global.clone(), cwd.clone()]).unwrap(),
            external.alias_sha256,
            external.conversation_id.as_str(),
            mutation_sha256("v3-unordered-token").unwrap(),
            timestamp
        ],
    )
    .unwrap();
    drop(conn);

    let store = RuntimeStore::open_metadata(&path).unwrap();
    let mutation_scope_order = {
        let conn = store.conn.lock().unwrap();
        let mut statement = conn
            .prepare(
                "SELECT scope_sha256 FROM conversation_identity_mutation_scopes ORDER BY rowid",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(
        mutation_scope_order,
        vec![cwd.as_sha256().to_owned(), global.as_sha256().to_owned()]
    );
    for scope in [&cwd, &global] {
        assert_eq!(
            store.identity_scope_selection("abbey", scope).unwrap(),
            IdentityScopeSelection::Selected {
                alias_sha256: external.alias_sha256.clone(),
                conversation_id: external.conversation_id.clone(),
            }
        );
    }
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM conversation_identity_mutation_scopes WHERE scope_sha256=?1",
            [cwd.as_sha256()],
        )
        .unwrap();
    }
    for scope in [&cwd, &global] {
        assert!(matches!(
            store.identity_scope_selection("abbey", scope),
            Err(StoreError::CorruptData(_))
        ));
    }
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

fn tempfile_dir(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "abbey-runtime-identity-migration-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
