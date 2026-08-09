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
