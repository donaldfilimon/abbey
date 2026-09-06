CREATE TABLE legacy_conversation_imports (
    snapshot_sha256 TEXT PRIMARY KEY CHECK (
        length(snapshot_sha256) = 64
        AND snapshot_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    backup_sha256 TEXT NOT NULL CHECK (backup_sha256 = snapshot_sha256),
    source_count INTEGER NOT NULL CHECK (source_count > 0),
    entry_count INTEGER NOT NULL CHECK (entry_count >= 0),
    skipped_count INTEGER NOT NULL CHECK (skipped_count >= 0),
    captured_at TEXT NOT NULL,
    imported_at TEXT NOT NULL
);

CREATE TABLE legacy_conversation_aliases (
    alias_sha256 TEXT PRIMARY KEY CHECK (
        length(alias_sha256) = 64
        AND alias_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    conversation_id TEXT NOT NULL UNIQUE,
    first_import_sha256 TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT,
    FOREIGN KEY (first_import_sha256)
        REFERENCES legacy_conversation_imports(snapshot_sha256) ON DELETE RESTRICT
);

CREATE TABLE legacy_conversation_entries (
    snapshot_sha256 TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (
        source_kind IN ('history', 'chat_id', 'by_cwd')
    ),
    observed_at TEXT,
    PRIMARY KEY (snapshot_sha256, ordinal),
    FOREIGN KEY (snapshot_sha256)
        REFERENCES legacy_conversation_imports(snapshot_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (alias_sha256)
        REFERENCES legacy_conversation_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE INDEX idx_legacy_entries_conversation
    ON legacy_conversation_entries(conversation_id, snapshot_sha256, ordinal);
