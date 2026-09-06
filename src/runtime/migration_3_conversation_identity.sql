CREATE TABLE conversation_identity_aliases (
    alias_sha256 TEXT PRIMARY KEY CHECK (
        length(alias_sha256) = 64
        AND alias_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    conversation_id TEXT NOT NULL UNIQUE,
    origin TEXT NOT NULL CHECK (origin IN ('legacy_v2', 'runtime_v3')),
    created_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE TABLE conversation_identity_scopes (
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (edition_sha256, scope_sha256),
    FOREIGN KEY (alias_sha256)
        REFERENCES conversation_identity_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

CREATE TABLE conversation_identity_commit (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL CHECK (revision > 0),
    operation TEXT NOT NULL CHECK (operation = 'save'),
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_set_sha256 TEXT NOT NULL CHECK (
        length(scope_set_sha256) = 64
        AND scope_set_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    mutation_sha256 TEXT NOT NULL CHECK (
        length(mutation_sha256) = 64
        AND mutation_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    committed_at TEXT NOT NULL,
    FOREIGN KEY (alias_sha256)
        REFERENCES conversation_identity_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

INSERT INTO conversation_identity_aliases(
    alias_sha256, conversation_id, origin, created_at
)
SELECT alias_sha256, conversation_id, 'legacy_v2', imported_at
FROM legacy_conversation_aliases;

CREATE INDEX idx_conversation_identity_scope_alias
    ON conversation_identity_scopes(alias_sha256, conversation_id);
