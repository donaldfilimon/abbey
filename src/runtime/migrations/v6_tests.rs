#[test]
fn schema_v6_adds_crash_recoverable_tool_execution_ledgers() {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_set(&mut conn, "2026-08-08T00:00:00Z", &MIGRATIONS[..5], 5).unwrap();
    apply(&mut conn, "2026-08-08T00:00:01Z").unwrap();
    for table in ["tool_executions", "tool_execution_events"] {
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    assert!(
        conn.execute(
            "INSERT INTO tool_executions(
                call_id, execution_id, state, result_digest,
                started_at_ms, finished_at_ms
             ) VALUES ('missing-call', 'execution', 'prepared', NULL, 1, NULL)",
            []
        )
        .is_err(),
        "an execution must reference a durable approval"
    );
}
