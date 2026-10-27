fn v12_refusal(conn: &Connection, sql: &str) -> String {
    conn.execute_batch(sql)
        .expect_err("statement must be refused")
        .to_string()
}

fn v12_task_sql(task: u32, state: &str, finished: &str) -> String {
    format!(
        "INSERT INTO v4_tasks (task_id, sequence, revision, conversation_id, goal, workspace_id,
             route_id, permission_profile_id, profile_revision, budget_tokens, execution_kind,
             state, current_attempt_id, created_at_ms, updated_at_ms)
         VALUES ('task-{task}', {task}, 1, 'chat-12', 'goal', 'ws', 'route', 'desk.default', 1,
             10, 'read_only', '{state}', 'attempt-{task}', 5, 5);
         INSERT INTO v4_task_attempts (attempt_id, task_id, ordinal, run_id, state,
             admitted_at_ms, finished_at_ms)
         VALUES ('attempt-{task}', 'task-{task}', 1,
             '00000000-0000-4000-8000-0000000000{task:02}', '{state}', 5, {finished});"
    )
}

#[test]
fn migration_12_adds_v4_admin_constraints_without_touching_applied_rows() {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_set(&mut conn, "2026-10-27T00:00:00Z", &MIGRATIONS[..11], 11).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn.execute_batch(
        "INSERT INTO conversations (id, created_at, updated_at) VALUES ('chat-12', 't', 't');
         INSERT INTO assistant_conversations (id, sequence, title, created_at_ms, updated_at_ms)
             VALUES ('chat-12', 1, 'Before 12', 1, 1);
         INSERT INTO assistant_permission_profiles (profile_id, document_json, updated_at_ms)
             VALUES ('desk.default', '{}', 1);
         INSERT INTO runs (id, conversation_id, idempotency_key, request_digest, status,
             next_event_sequence, created_at, updated_at)
             VALUES ('12121212-1212-4121-8121-121212121212', 'chat-12', 'req-12',
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'queued', 2, 't', 't');",
    )
    .unwrap();
    let counts = |conn: &Connection| -> Vec<i64> {
        ["conversations", "assistant_conversations", "runs"]
            .iter()
            .map(|t| {
                conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                    .unwrap()
            })
            .collect()
    };
    let before = counts(&conn);

    apply(&mut conn, "2026-10-27T00:00:01Z").unwrap();
    assert_eq!(current_version(&conn).unwrap(), 12);
    assert_eq!(before, counts(&conn), "applied rows survive migration 12");
    let title: String = conn
        .query_row(
            "SELECT title FROM assistant_conversations WHERE id='chat-12'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "Before 12");

    // A task snapshot is immutable once written; state changes with a new revision.
    conn.execute_batch(&v12_task_sql(1, "queued", "NULL"))
        .unwrap();
    let refused = v12_refusal(
        &conn,
        "UPDATE v4_tasks SET budget_tokens=99, revision=2 WHERE task_id='task-1'",
    );
    assert!(refused.contains("immutable task snapshot"), "{refused}");
    conn.execute_batch("UPDATE v4_tasks SET state='cancelled', revision=2 WHERE task_id='task-1'")
        .unwrap();
    // A terminal attempt carries its finish time, and a live one does not.
    let refused = v12_refusal(&conn, &v12_task_sql(9, "succeeded", "NULL"));
    assert!(refused.contains("CHECK constraint failed"), "{refused}");

    // Occurrence identity: one scheduled occurrence per (automation, revision, instant).
    conn.execute_batch(&format!(
        "INSERT INTO v4_automations (automation_id, sequence, revision, spec_json, enabled,
             cursor_ms, next_run_at_ms, created_at_ms, updated_at_ms)
             VALUES ('auto-1', 1, 1, '{{\"automation_id\":\"auto-1\",\"enabled\":true}}', 1,
             0, NULL, 1, 1);
         {}{}
         INSERT INTO v4_automation_occurrences (occurrence_id, automation_id, revision,
             source_kind, scheduled_at_ms, manual_operation_id, task_id, attempt_id, run_id,
             state, created_at_ms)
             VALUES ('occ-1', 'auto-1', 1, 'scheduled', 1000, NULL, 'task-2', 'attempt-2',
             '00000000-0000-4000-8000-000000000002', 'reserved', 1);",
        v12_task_sql(2, "queued", "NULL"),
        v12_task_sql(3, "queued", "NULL"),
    ))
    .unwrap();
    let refused = v12_refusal(
        &conn,
        "UPDATE v4_automations SET spec_json='{\"automation_id\":\"auto-1\",\"enabled\":false}'
             WHERE automation_id='auto-1'",
    );
    assert!(
        refused.contains("CHECK constraint failed"),
        "spec and column agree: {refused}"
    );
    let occurrence = |id: &str, revision: u32, task: u32| {
        format!(
            "INSERT INTO v4_automation_occurrences (occurrence_id, automation_id, revision,
                 source_kind, scheduled_at_ms, manual_operation_id, task_id, attempt_id, run_id,
                 state, created_at_ms)
                 VALUES ('{id}', 'auto-1', {revision}, 'scheduled', 1000, NULL, 'task-{task}',
                 'attempt-{task}', '00000000-0000-4000-8000-0000000000{task:02}', 'reserved', 1)"
        )
    };
    let refused = v12_refusal(&conn, &occurrence("occ-2", 1, 3));
    assert!(
        refused.contains("v4_automation_occurrences.automation_id"),
        "a second occurrence for the same revision and instant is refused: {refused}"
    );
    let refused = v12_refusal(&conn, &occurrence("occ-3", 2, 2));
    assert!(
        refused.contains("v4_automation_occurrences.task_id"),
        "one task per occurrence: {refused}"
    );
    let refused = v12_refusal(
        &conn,
        "UPDATE v4_automation_occurrences SET attempt_id='attempt-3',
             run_id='00000000-0000-4000-8000-000000000003' WHERE occurrence_id='occ-1'",
    );
    assert!(
        refused.contains("FOREIGN KEY"),
        "attempt belongs to the occurrence task: {refused}"
    );
    conn.execute_batch(
        "UPDATE v4_automation_occurrences SET state='running' WHERE occurrence_id='occ-1'",
    )
    .unwrap();
    let refused = v12_refusal(
        &conn,
        "UPDATE v4_automation_occurrences SET scheduled_at_ms=2000 WHERE occurrence_id='occ-1'",
    );
    assert!(
        refused.contains("immutable automation occurrence"),
        "{refused}"
    );

    // A decision binds action id + decision id + kind to the action's exact binding.
    let digest = "b".repeat(64);
    conn.execute_batch(&format!(
        "INSERT INTO v4_actions (action_id, task_id, attempt_id, run_id, revision, operation_id,
             resource_label, preview_digest, input_digest, target_digest, binding_digest,
             expires_at_ms, state)
             VALUES ('action-1', 'task-2', 'attempt-2', '00000000-0000-4000-8000-000000000002',
             1, 'op-1', 'label', '{digest}', '{digest}', '{digest}', '{digest}', 9,
             'awaiting_approval');"
    ))
    .unwrap();
    let decision = |id: &str, preview: &str| {
        format!(
            "INSERT INTO v4_action_decisions (decision_id, action_id, task_id, attempt_id,
                 expected_revision, preview_digest, kind, applied_revision, created_at_ms)
                 VALUES ('{id}', 'action-1', 'task-2', 'attempt-2', 1, '{preview}', 'approve',
                 2, 3)"
        )
    };
    let refused = v12_refusal(&conn, &decision("decision-0", &"c".repeat(64)));
    assert!(
        refused.contains("FOREIGN KEY"),
        "decision binds the preview: {refused}"
    );
    conn.execute_batch(&decision("decision-1", &digest))
        .unwrap();
    let refused = v12_refusal(&conn, &decision("decision-2", &digest));
    assert!(
        refused.contains("v4_action_decisions.action_id"),
        "one decision per action: {refused}"
    );
    let refused = v12_refusal(
        &conn,
        "UPDATE v4_action_decisions SET kind='deny' WHERE decision_id='decision-1'",
    );
    assert!(refused.contains("immutable action decision"), "{refused}");
    let refused = v12_refusal(&conn, "DELETE FROM v4_actions WHERE action_id='action-1'");
    assert!(refused.contains("durable action"), "{refused}");
}
