CREATE TABLE tool_approvals (
    call_id TEXT PRIMARY KEY,
    tool_id TEXT NOT NULL,
    call_digest TEXT NOT NULL CHECK (
        length(call_digest) = 64
        AND call_digest NOT GLOB '*[^0-9a-f]*'
    ),
    state TEXT NOT NULL CHECK (
        state IN ('pending', 'approved', 'denied', 'cancelled', 'expired', 'consumed')
    ),
    decision_id TEXT UNIQUE,
    cancellation_id TEXT UNIQUE,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > created_at_ms),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    CHECK (
        (state = 'pending' AND decision_id IS NULL AND cancellation_id IS NULL)
        OR (state IN ('approved', 'denied', 'consumed')
            AND decision_id IS NOT NULL AND cancellation_id IS NULL)
        OR (state = 'cancelled' AND cancellation_id IS NOT NULL)
        OR (state = 'expired' AND cancellation_id IS NULL)
    )
);

CREATE TABLE tool_approval_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    call_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('pending', 'approved', 'denied', 'cancelled', 'expired', 'consumed')
    ),
    decision_id TEXT,
    cancellation_id TEXT,
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    FOREIGN KEY (call_id) REFERENCES tool_approvals(call_id) ON DELETE RESTRICT
);

CREATE INDEX idx_tool_approval_events_call
    ON tool_approval_events(call_id, sequence);
