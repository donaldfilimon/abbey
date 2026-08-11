CREATE TABLE tool_executions (
    call_id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (
        state IN ('prepared', 'interrupted', 'succeeded', 'failed')
    ),
    result_digest TEXT CHECK (
        result_digest IS NULL OR (
            length(result_digest) = 64
            AND result_digest NOT GLOB '*[^0-9a-f]*'
        )
    ),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    finished_at_ms INTEGER CHECK (
        finished_at_ms IS NULL OR finished_at_ms >= started_at_ms
    ),
    CHECK (
        (state = 'prepared' AND result_digest IS NULL AND finished_at_ms IS NULL)
        OR (state = 'interrupted' AND result_digest IS NULL AND finished_at_ms IS NOT NULL)
        OR (state IN ('succeeded', 'failed')
            AND result_digest IS NOT NULL AND finished_at_ms IS NOT NULL)
    ),
    FOREIGN KEY (call_id) REFERENCES tool_approvals(call_id) ON DELETE RESTRICT
);

CREATE TABLE tool_execution_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    call_id TEXT NOT NULL,
    execution_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('prepared', 'interrupted', 'succeeded', 'failed')
    ),
    result_digest TEXT CHECK (
        result_digest IS NULL OR (
            length(result_digest) = 64
            AND result_digest NOT GLOB '*[^0-9a-f]*'
        )
    ),
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    FOREIGN KEY (call_id) REFERENCES tool_executions(call_id) ON DELETE RESTRICT
);

CREATE INDEX idx_tool_execution_events_call
    ON tool_execution_events(call_id, sequence);
CREATE INDEX idx_tool_executions_state
    ON tool_executions(state, started_at_ms, call_id);
