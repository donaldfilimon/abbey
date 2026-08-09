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
            2
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

#[test]
fn idempotency_reuses_only_an_identical_digest() {
    let (_dir, store) = scratch_store("idempotency");
    let first = store.create_or_get_run(new_run("same-key")).unwrap();
    let second = store.create_or_get_run(new_run("same-key")).unwrap();
    assert_eq!(first.id, second.id);

    let mut mismatch = new_run("same-key");
    mismatch.request_digest = "b".repeat(64);
    assert!(matches!(
        store.create_or_get_run(mismatch),
        Err(StoreError::IdempotencyConflict)
    ));
}

#[test]
fn transition_and_event_are_atomic_and_sequences_are_monotonic() {
    let (_dir, store) = scratch_store("transition");
    let run = store.create_or_get_run(new_run("transition-key")).unwrap();
    let started = store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Starting,
            event("run_starting"),
        )
        .unwrap();
    let note = store
        .append_run_event(&run.id, event("worker_selected"))
        .unwrap();
    let running = store
        .transition_run(
            &run.id,
            RunState::Starting,
            RunState::Running,
            event("run_running"),
        )
        .unwrap();
    assert_eq!(
        (started.sequence, note.sequence, running.sequence),
        (2, 3, 4)
    );
    assert_eq!(
        store.get_run(&run.id).unwrap().unwrap().status,
        RunState::Running
    );
    let events = store.run_events(&run.id).unwrap();
    assert_eq!(
        events
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );

    assert!(matches!(
        store.transition_run(
            &run.id,
            RunState::Starting,
            RunState::Failed,
            event("stale")
        ),
        Err(StoreError::UnexpectedStatus { .. })
    ));
    assert_eq!(store.run_events(&run.id).unwrap().len(), 4);
}

#[test]
fn event_pages_keep_a_fixed_watermark_across_concurrent_appends() {
    let dir = ScratchDir::new("event-page-concurrent-append");
    let path = RuntimeStore::path_for_state_dir(dir.path());
    let store = RuntimeStore::open(&path).unwrap();
    let run = store
        .create_or_get_run(new_run("event-page-concurrent-key"))
        .unwrap();
    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Starting,
            event("run_starting"),
        )
        .unwrap();
    store
        .transition_run(
            &run.id,
            RunState::Starting,
            RunState::Running,
            event("run_started"),
        )
        .unwrap();

    let first = store.run_events_page(&run.id, 0, None, 2).unwrap();
    assert_eq!(first.through_sequence, 3);
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(first.next_after_sequence, 2);
    assert!(first.has_more);

    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                store
                    .transition_run(
                        &run.id,
                        RunState::Running,
                        RunState::CancelRequested,
                        event("run_cancel_requested"),
                    )
                    .unwrap();
            })
            .join()
            .unwrap();
    });

    let second = store
        .run_events_page(
            &run.id,
            first.next_after_sequence,
            Some(first.through_sequence),
            2,
        )
        .unwrap();
    assert_eq!(second.through_sequence, 3);
    assert_eq!(
        second
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert!(!second.has_more);

    let latest = store.run_events_page(&run.id, 0, None, 16).unwrap();
    assert_eq!(latest.through_sequence, 4);
    assert_eq!(latest.events.last().unwrap().sequence, 4);
}

#[test]
fn event_page_cursor_survives_reopen_and_excludes_recovery_append() {
    let dir = ScratchDir::new("event-page-reopen");
    let path = RuntimeStore::path_for_state_dir(dir.path());
    let (run_id, first) = {
        let store = RuntimeStore::open(&path).unwrap();
        let run = store
            .create_or_get_run(new_run("event-page-reopen-key"))
            .unwrap();
        store
            .transition_run(
                &run.id,
                RunState::Queued,
                RunState::Starting,
                event("run_starting"),
            )
            .unwrap();
        let page = store.run_events_page(&run.id, 0, None, 1).unwrap();
        (run.id, page)
    };
    assert_eq!(first.through_sequence, 2);
    assert_eq!(first.next_after_sequence, 1);

    let reopened = RuntimeStore::open(&path).unwrap();
    assert_eq!(reopened.recovered_runs(), 1);
    let continuation = reopened
        .run_events_page(
            &run_id,
            first.next_after_sequence,
            Some(first.through_sequence),
            16,
        )
        .unwrap();
    assert_eq!(continuation.through_sequence, 2);
    assert_eq!(continuation.events.len(), 1);
    assert_eq!(continuation.events[0].sequence, 2);
    assert_eq!(continuation.events[0].event, RunLifecycleEvent::Starting);
    assert!(!continuation.has_more);
    assert_eq!(reopened.run_events(&run_id).unwrap().len(), 3);
}

#[test]
fn event_page_rejects_missing_future_and_invalid_boundaries() {
    let (_dir, store) = scratch_store("event-page-invalid");
    let run = store
        .create_or_get_run(new_run("event-page-invalid-key"))
        .unwrap();
    let first = store.run_events_page(&run.id, 0, None, 1).unwrap();

    for result in [
        store.run_events_page(&run.id, 1, None, 1),
        store.run_events_page(&run.id, 2, Some(first.through_sequence), 1),
        store.run_events_page(&run.id, 0, Some(first.through_sequence + 1), 1),
        store.run_events_page(&run.id, 0, None, 0),
        store.run_events_page(&run.id, 0, None, 17),
    ] {
        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
    }
    assert!(matches!(
        store.run_events_page(&RunId::new(), 0, None, 1),
        Err(StoreError::RunNotFound(_))
    ));
}

#[test]
fn event_page_detects_sequence_gaps_and_a_missing_watermark_event() {
    for (label, deleted_sequence) in [("gap", 2_i64), ("watermark", 3_i64)] {
        let (_dir, store) = scratch_store(&format!("event-page-{label}"));
        let run = store
            .create_or_get_run(new_run(&format!("event-page-{label}-key")))
            .unwrap();
        store
            .transition_run(
                &run.id,
                RunState::Queued,
                RunState::Starting,
                event("run_starting"),
            )
            .unwrap();
        store
            .transition_run(
                &run.id,
                RunState::Starting,
                RunState::Running,
                event("run_started"),
            )
            .unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM run_events WHERE run_id=?1 AND sequence=?2",
                params![run.id.as_str(), deleted_sequence],
            )
            .unwrap();
        }
        assert!(matches!(
            store.run_events_page(&run.id, 0, None, 16),
            Err(StoreError::CorruptData(
                "run event snapshot contains a sequence gap"
            ))
        ));
    }
}

#[test]
fn event_page_after_terminal_watermark_is_empty_and_complete() {
    let (_dir, store) = scratch_store("event-page-terminal-empty");
    let run = store
        .create_or_get_run(new_run("event-page-terminal-empty-key"))
        .unwrap();
    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Failed,
            failed_event("executor_failed", "executor returned a failure"),
        )
        .unwrap();
    let complete = store.run_events_page(&run.id, 0, None, 16).unwrap();
    assert_eq!(complete.through_sequence, 2);
    assert!(!complete.has_more);

    let empty = store.run_events_page(&run.id, 2, Some(2), 16).unwrap();
    assert!(empty.events.is_empty());
    assert_eq!(empty.through_sequence, 2);
    assert!(!empty.has_more);
}

#[test]
fn run_snapshot_projects_only_closed_failure_evidence() {
    let (_dir, store) = scratch_store("run-snapshot-failure");
    let run = store
        .create_or_get_run(new_run("run-snapshot-failure-key"))
        .unwrap();
    let queued = store.run_snapshot(&run.id).unwrap().unwrap();
    assert_eq!(queued.state, RunState::Queued);
    assert_eq!(queued.event_count, 1);
    assert!(queued.failure.is_none());

    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Failed,
            failed_event("executor_timed_out", "executor exceeded its deadline"),
        )
        .unwrap();
    let failed = store.run_snapshot(&run.id).unwrap().unwrap();
    let failure = failed.failure.unwrap();
    assert_eq!(failure.code, "executor_timed_out");
    assert_eq!(failure.message, "executor exceeded its deadline");
    assert!(failure.retryable);

    let invalid = store
        .create_or_get_run(new_run("run-snapshot-invalid-failure-key"))
        .unwrap();
    store
        .transition_run(
            &invalid.id,
            RunState::Queued,
            RunState::Failed,
            failed_event("secret_provider_code", "private provider output"),
        )
        .unwrap();
    assert!(matches!(
        store.run_snapshot(&invalid.id),
        Err(StoreError::CorruptData("run failure code is invalid"))
    ));

    let mismatched = store
        .create_or_get_run(new_run("run-snapshot-mismatched-event-key"))
        .unwrap();
    store
        .transition_run(
            &mismatched.id,
            RunState::Queued,
            RunState::Starting,
            event("run_queued"),
        )
        .unwrap();
    assert!(matches!(
        store.run_snapshot(&mismatched.id),
        Err(StoreError::CorruptData(
            "latest run event does not match the run state"
        ))
    ));
}

#[test]
fn manager_shutdown_projection_uses_the_durable_terminal_state() {
    let (_dir, store) = scratch_store("manager-shutdown-projection");
    let requested = store
        .create_or_get_run(new_run("requested-cancellation-key"))
        .unwrap();
    store
        .transition_run(
            &requested.id,
            RunState::Queued,
            RunState::Cancelled,
            NewRunEvent {
                kind: "run_cancelled".into(),
                payload: serde_json::json!({"reason": "requested"}),
            },
        )
        .unwrap();
    let requested_page = store.run_events_page(&requested.id, 0, None, 16).unwrap();
    assert_eq!(
        requested_page.events.last().unwrap().event,
        RunLifecycleEvent::Cancelled {
            reason: RunCancellationReason::Requested,
        }
    );

    let queued = store
        .create_or_get_run(new_run("manager-shutdown-queued-key"))
        .unwrap();
    store
        .transition_run(
            &queued.id,
            RunState::Queued,
            RunState::Cancelled,
            event("run_manager_stopped"),
        )
        .unwrap();
    let queued_page = store.run_events_page(&queued.id, 0, None, 16).unwrap();
    assert_eq!(
        queued_page.events.last().unwrap().event,
        RunLifecycleEvent::Cancelled {
            reason: RunCancellationReason::ManagerShutdown,
        }
    );

    let active = store
        .create_or_get_run(new_run("manager-shutdown-active-key"))
        .unwrap();
    store
        .transition_run(
            &active.id,
            RunState::Queued,
            RunState::Starting,
            event("run_starting"),
        )
        .unwrap();
    store
        .transition_run(
            &active.id,
            RunState::Starting,
            RunState::Interrupted,
            event("run_manager_stopped"),
        )
        .unwrap();
    let active_page = store.run_events_page(&active.id, 0, None, 16).unwrap();
    assert_eq!(
        active_page.events.last().unwrap().event,
        RunLifecycleEvent::Interrupted {
            reason: RunInterruptionReason::ManagerShutdown,
        }
    );
}

#[test]
fn terminal_runs_are_immutable() {
    let (_dir, store) = scratch_store("terminal");
    let run = store.create_or_get_run(new_run("terminal-key")).unwrap();
    store
        .transition_run(
            &run.id,
            RunState::Queued,
            RunState::Failed,
            event("run_failed"),
        )
        .unwrap();
    assert!(matches!(
        store.append_run_event(&run.id, event("too_late")),
        Err(StoreError::TerminalRun { .. })
    ));
    assert!(matches!(
        store.transition_run(
            &run.id,
            RunState::Failed,
            RunState::Starting,
            event("restart")
        ),
        Err(StoreError::TerminalRun { .. })
    ));
}

#[test]
fn reopen_interrupts_active_runs_but_preserves_queued_runs() {
    let dir = ScratchDir::new("recovery");
    let path = RuntimeStore::path_for_state_dir(dir.path());
    let (queued_id, starting_id, running_id, cancelling_id) = {
        let store = RuntimeStore::open(&path).unwrap();
        let queued = store.create_or_get_run(new_run("queued-key")).unwrap();
        let starting = store.create_or_get_run(new_run("starting-key")).unwrap();
        let running = store.create_or_get_run(new_run("running-key")).unwrap();
        let cancelling = store.create_or_get_run(new_run("cancelling-key")).unwrap();
        store
            .transition_run(
                &starting.id,
                RunState::Queued,
                RunState::Starting,
                event("starting"),
            )
            .unwrap();
        store
            .transition_run(
                &running.id,
                RunState::Queued,
                RunState::Starting,
                event("starting"),
            )
            .unwrap();
        store
            .transition_run(
                &running.id,
                RunState::Starting,
                RunState::Running,
                event("running"),
            )
            .unwrap();
        store
            .transition_run(
                &cancelling.id,
                RunState::Queued,
                RunState::Starting,
                event("starting"),
            )
            .unwrap();
        store
            .transition_run(
                &cancelling.id,
                RunState::Starting,
                RunState::CancelRequested,
                event("cancel_requested"),
            )
            .unwrap();
        (queued.id, starting.id, running.id, cancelling.id)
    };

    let reopened = RuntimeStore::open(&path).unwrap();
    assert_eq!(reopened.recovered_runs(), 3);
    assert_eq!(
        reopened.get_run(&queued_id).unwrap().unwrap().status,
        RunState::Queued
    );
    assert_eq!(
        reopened.get_run(&running_id).unwrap().unwrap().status,
        RunState::Interrupted
    );
    assert_eq!(
        reopened.get_run(&starting_id).unwrap().unwrap().status,
        RunState::Interrupted
    );
    assert_eq!(
        reopened.get_run(&cancelling_id).unwrap().unwrap().status,
        RunState::Interrupted
    );
    assert_eq!(
        reopened
            .run_events(&running_id)
            .unwrap()
            .last()
            .unwrap()
            .kind,
        "run_recovered_interrupted"
    );
}

#[test]
fn conversation_foreign_keys_and_backend_binding_are_enforced() {
    let (_dir, store) = scratch_store("conversation");
    let conversation = ConversationId::new();
    assert!(matches!(
        store.set_conversation_backend(&conversation, BackendSelection::Cursor, Some("remote")),
        Err(StoreError::ConversationNotFound(_))
    ));
    store.create_conversation(&conversation).unwrap();
    let binding = store
        .set_conversation_backend(&conversation, BackendSelection::Cursor, Some("remote"))
        .unwrap();
    assert_eq!(binding.backend, BackendSelection::Cursor);

    let mut run = new_run("conversation-key");
    run.conversation_id = Some(conversation.clone());
    assert_eq!(
        store
            .create_or_get_run(run)
            .unwrap()
            .conversation_id
            .as_ref(),
        Some(&conversation)
    );
}

#[test]
fn audit_metadata_is_bounded_and_redacts_prompts_and_credentials() {
    let (dir, store) = scratch_store("audit");
    let metadata = AuditMetadata::new(serde_json::json!({
        "operation": "status",
        "prompt": "private user prompt",
        "nested": {"api_key": "sk-private", "safe": "visible"},
        "auth_header": "Bearer abcdef"
    }))
    .unwrap();
    let saved = store
        .record_audit(NewAuditEvent {
            run_id: None,
            action: "daemon_request".into(),
            outcome: "allowed".into(),
            metadata,
        })
        .unwrap();
    let encoded = serde_json::to_string(&saved.metadata).unwrap();
    assert!(!encoded.contains("private user prompt"));
    assert!(!encoded.contains("sk-private"));
    assert!(!encoded.contains("Bearer abcdef"));
    assert!(encoded.contains("visible"));
    drop(store);
    let reopened = RuntimeStore::open(&RuntimeStore::path_for_state_dir(dir.path())).unwrap();
    let persisted = reopened.audit_events_for_run(None).unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].metadata, saved.metadata);

    assert!(
        AuditMetadata::new(serde_json::json!({
            "large": "x".repeat(513)
        }))
        .is_err()
    );
}
