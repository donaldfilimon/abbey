CREATE TABLE model_operations (
    operation_id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('download', 'load', 'unload')),
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')
    ),
    progress_basis_points INTEGER NOT NULL CHECK (
        progress_basis_points >= 0 AND progress_basis_points <= 10000
    ),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
);

CREATE INDEX idx_model_operations_model
    ON model_operations(model_id, revision, created_at_ms);
