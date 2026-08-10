use super::*;
use std::path::{Path, PathBuf};

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "abbey-runtime-{test}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn scratch_store(test: &str) -> (ScratchDir, RuntimeStore) {
    let dir = ScratchDir::new(test);
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(dir.path())).unwrap();
    (dir, store)
}

fn new_run(key: &str) -> NewRun {
    NewRun {
        conversation_id: None,
        idempotency_key: key.parse().unwrap(),
        request_digest: "a".repeat(64),
    }
}

fn event(kind: &str) -> NewRunEvent {
    NewRunEvent {
        kind: kind.into(),
        payload: serde_json::json!({"safe": true}),
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(unix)]
fn private_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::create_dir(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn failed_event(code: &str, message: &str) -> NewRunEvent {
    NewRunEvent {
        kind: "run_failed".into(),
        payload: serde_json::json!({"code": code, "message": message}),
    }
}

#[test]
fn uses_separate_runtime_database_with_required_pragmas() {
    let (dir, store) = scratch_store("pragmas");
    assert_eq!(
        RuntimeStore::path_for_state_dir(dir.path()),
        dir.path().join("runtime.sqlite")
    );
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[cfg(unix)]
#[test]
fn legacy_metadata_backup_and_schema_v2_import_are_exact_and_idempotent() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = ScratchDir::new("legacy-import");
    let runtime_dir = root.path().join("daemon");
    private_dir(&runtime_dir);
    let by_cwd = root.path().join("by-cwd");
    private_dir(&by_cwd);
    write_private(
        &root.path().join("history.log"),
        b"2026-08-08T03:02:03+02:00\tlegacy-a\t/private/project-a\n\
2026-08-08T02:03:04Z\tlegacy-a\t/private/project-b\n\
2026-08-08T04:05:06Z\tlegacy-b\n",
    );
    write_private(&root.path().join("chat-id"), b"legacy-a\n");
    write_private(
        &root.path().join("chat-id.export"),
        b"export-only-secret=must-never-import\n",
    );
    write_private(&by_cwd.join("private_project_c"), b"legacy-c\n");

    let prepared = crate::runtime::legacy::prepare(root.path(), &runtime_dir)
        .unwrap()
        .unwrap();
    assert_eq!(prepared.source_count, 4);
    assert_eq!(prepared.entries.len(), 5);
    assert_eq!(prepared.skipped_count, 0);
    let debug = format!("{prepared:?}");
    for private in ["legacy-a", "legacy-b", "legacy-c", "/private"] {
        assert!(!debug.contains(private));
    }

    let backup = runtime_dir
        .join("legacy-conversation-backups")
        .join(format!("v1-{}", prepared.snapshot_sha256));
    assert_eq!(
        std::fs::read(backup.join("history.log")).unwrap(),
        std::fs::read(root.path().join("history.log")).unwrap()
    );
    assert_eq!(
        std::fs::read(backup.join("by-cwd/private_project_c")).unwrap(),
        b"legacy-c\n"
    );
    assert_eq!(
        std::fs::metadata(&backup).unwrap().permissions().mode() & 0o077,
        0
    );
    assert_eq!(
        std::fs::metadata(backup.join("history.log"))
            .unwrap()
            .permissions()
            .mode()
            & 0o077,
        0
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["snapshot_sha256"], prepared.snapshot_sha256);
    assert_eq!(manifest["captured_at"], prepared.captured_at);
    let roles = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["source_role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        vec!["by_cwd", "chat_id", "backup_only_export", "history"]
    );
    assert!(manifest["files"].as_array().unwrap().iter().all(|file| {
        file["sha256"]
            .as_str()
            .is_some_and(|value| value.len() == 64)
            && file["path_hex"].as_str().is_some()
    }));

    let database = RuntimeStore::path_for_state_dir(&runtime_dir);
    let store = RuntimeStore::open_with_legacy(&database, Some(&prepared)).unwrap();
    assert!(store.legacy_imported());
    {
        let conn = store.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            4
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name='conversation_identity_mutations'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM legacy_conversation_imports",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM legacy_conversation_aliases",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM legacy_conversation_entries",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            5
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM conversation_backends", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM runs", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        let mapped = crate::runtime::legacy::legacy_conversation_id("legacy-a");
        assert_eq!(mapped.as_str().as_bytes()[14], b'8');
        let envelope = conn
            .query_row(
                "SELECT created_at, updated_at FROM conversations WHERE id=?1",
                [mapped.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(envelope.0, "2026-08-08T01:02:03.000000000Z");
        assert_eq!(envelope.1, "2026-08-08T02:03:04.000000000Z");
        let direct = crate::runtime::legacy::legacy_conversation_id("legacy-c");
        let direct_envelope = conn
            .query_row(
                "SELECT created_at, updated_at FROM conversations WHERE id=?1",
                [direct.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(
            direct_envelope,
            (prepared.captured_at.clone(), prepared.captured_at.clone())
        );
        let columns: String = conn
            .query_row(
                "SELECT group_concat(name, ',') FROM pragma_table_info('legacy_conversation_entries')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        for forbidden in ["legacy_id", "cwd", "source_locator"] {
            assert!(!columns.split(',').any(|column| column == forbidden));
        }
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM legacy_conversation_entries WHERE source_kind='history' AND observed_at='2026-08-08T04:05:06.000000000Z'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    drop(store);

    let prepared_again = crate::runtime::legacy::prepare(root.path(), &runtime_dir)
        .unwrap()
        .unwrap();
    assert_eq!(prepared_again.captured_at, prepared.captured_at);
    assert_eq!(prepared_again.snapshot_sha256, prepared.snapshot_sha256);
    let reopened = RuntimeStore::open_with_legacy(&database, Some(&prepared_again)).unwrap();
    assert!(!reopened.legacy_imported());
    let conn = reopened.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM legacy_conversation_imports",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM legacy_conversation_entries",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        5
    );
    drop(conn);
    drop(reopened);
    let database_bytes = std::fs::read(&database).unwrap();
    for forbidden in [
        b"legacy-a".as_slice(),
        b"legacy-b".as_slice(),
        b"legacy-c".as_slice(),
        b"/private/project".as_slice(),
        b"export-only-secret".as_slice(),
        b"chat-id.export".as_slice(),
    ] {
        assert!(
            !database_bytes
                .windows(forbidden.len())
                .any(|window| window == forbidden)
        );
    }
}

#[cfg(unix)]
#[test]
fn legacy_import_rolls_back_on_native_conversation_id_collision() {
    let root = ScratchDir::new("legacy-native-collision");
    let runtime_dir = root.path().join("daemon");
    private_dir(&runtime_dir);
    write_private(&root.path().join("chat-id"), b"native-collision-secret\n");
    let prepared = crate::runtime::legacy::prepare(root.path(), &runtime_dir)
        .unwrap()
        .unwrap();
    let database = RuntimeStore::path_for_state_dir(&runtime_dir);
    let native_id = crate::runtime::legacy::legacy_conversation_id("native-collision-secret");
    let native = RuntimeStore::open(&database).unwrap();
    native.create_conversation(&native_id).unwrap();
    drop(native);

    assert!(matches!(
        RuntimeStore::open_with_legacy(&database, Some(&prepared)),
        Err(StoreError::Migration(
            crate::runtime::migrations::MigrationError::LegacyInvariant
        ))
    ));
    let conn = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM legacy_conversation_imports",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM legacy_conversation_aliases",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM legacy_conversation_entries",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE id=?1",
            [native_id.as_str()],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[cfg(unix)]
#[test]
fn legacy_snapshot_rejects_symlinks_and_other_writable_sources() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let root = ScratchDir::new("legacy-unsafe");
    let runtime_dir = root.path().join("daemon");
    private_dir(&runtime_dir);
    let target = root.path().join("target");
    write_private(&target, b"legacy-private\n");
    symlink(&target, root.path().join("chat-id")).unwrap();
    assert!(matches!(
        crate::runtime::legacy::prepare(root.path(), &runtime_dir),
        Err(crate::runtime::legacy::LegacyError::UnsafeSource)
    ));
    std::fs::remove_file(root.path().join("chat-id")).unwrap();
    write_private(&root.path().join("chat-id"), b"legacy-private\n");
    std::fs::set_permissions(
        root.path().join("chat-id"),
        std::fs::Permissions::from_mode(0o622),
    )
    .unwrap();
    assert!(matches!(
        crate::runtime::legacy::prepare(root.path(), &runtime_dir),
        Err(crate::runtime::legacy::LegacyError::UnsafeSource)
    ));
}

include!("tests/run_lifecycle.rs");
