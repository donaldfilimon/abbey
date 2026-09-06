CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE conversation_backends (
    conversation_id TEXT NOT NULL,
    backend TEXT NOT NULL,
    backend_conversation_id TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (conversation_id, backend),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    request_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'queued', 'starting', 'running', 'cancel_requested',
            'succeeded', 'failed', 'cancelled', 'interrupted'
        )
    ),
    next_event_sequence INTEGER NOT NULL DEFAULT 1 CHECK (next_event_sequence >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE TABLE run_events (
    run_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (run_id, sequence),
    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
);

CREATE TABLE audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE SET NULL
);

CREATE INDEX idx_runs_conversation ON runs(conversation_id, created_at);
CREATE INDEX idx_runs_status ON runs(status, created_at);
CREATE INDEX idx_run_events_created ON run_events(created_at);
CREATE INDEX idx_audit_events_run ON audit_events(run_id, created_at);
