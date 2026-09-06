ALTER TABLE conversation_identity_commit RENAME TO conversation_identity_commit_v3;

CREATE TABLE conversation_identity_commit (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL CHECK (revision > 0),
    operation TEXT NOT NULL CHECK (
        operation IN ('save', 'clear_scope', 'clear_all')
    ),
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
    alias_sha256 TEXT,
    conversation_id TEXT,
    mutation_sha256 TEXT NOT NULL CHECK (
        length(mutation_sha256) = 64
        AND mutation_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    committed_at TEXT NOT NULL,
    CHECK (
        (operation = 'save' AND alias_sha256 IS NOT NULL AND conversation_id IS NOT NULL)
        OR
        (operation IN ('clear_scope', 'clear_all')
            AND alias_sha256 IS NULL AND conversation_id IS NULL)
    ),
    FOREIGN KEY (alias_sha256)
        REFERENCES conversation_identity_aliases(alias_sha256) ON DELETE RESTRICT,
    FOREIGN KEY (conversation_id)
        REFERENCES conversations(id) ON DELETE RESTRICT
);

INSERT INTO conversation_identity_commit(
    singleton, revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
    alias_sha256, conversation_id, mutation_sha256, committed_at
)
SELECT singleton, revision, operation, edition_sha256, scope_sha256, scope_set_sha256,
       alias_sha256, conversation_id, mutation_sha256, committed_at
FROM conversation_identity_commit_v3;

DROP TABLE conversation_identity_commit_v3;

CREATE TABLE conversation_identity_tombstones (
    edition_sha256 TEXT NOT NULL CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    revision INTEGER NOT NULL CHECK (revision > 0),
    cleared_at TEXT NOT NULL,
    PRIMARY KEY (edition_sha256, scope_sha256)
);

CREATE TABLE conversation_identity_clear_all (
    edition_sha256 TEXT PRIMARY KEY CHECK (
        length(edition_sha256) = 64
        AND edition_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    revision INTEGER NOT NULL CHECK (revision > 0),
    cleared_at TEXT NOT NULL
);

CREATE TABLE conversation_identity_mutations (
    mutation_sha256 TEXT PRIMARY KEY CHECK (
        length(mutation_sha256) = 64
        AND mutation_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    revision INTEGER NOT NULL UNIQUE CHECK (revision > 0),
    operation TEXT NOT NULL CHECK (
        operation IN ('save', 'clear_scope', 'clear_all')
    ),
    edition_sha256 TEXT NOT NULL,
    scope_sha256 TEXT NOT NULL,
    scope_set_sha256 TEXT NOT NULL,
    alias_sha256 TEXT,
    conversation_id TEXT,
    committed_at TEXT NOT NULL,
    CHECK (
        (operation = 'save' AND alias_sha256 IS NOT NULL AND conversation_id IS NOT NULL)
        OR
        (operation IN ('clear_scope', 'clear_all')
            AND alias_sha256 IS NULL AND conversation_id IS NULL)
    )
);

INSERT INTO conversation_identity_mutations(
    mutation_sha256, revision, operation, edition_sha256, scope_sha256,
    scope_set_sha256, alias_sha256, conversation_id, committed_at
)
SELECT mutation_sha256, revision, operation, edition_sha256, scope_sha256,
       scope_set_sha256, alias_sha256, conversation_id, committed_at
FROM conversation_identity_commit;

CREATE TABLE conversation_identity_mutation_scopes (
    mutation_sha256 TEXT NOT NULL,
    scope_sha256 TEXT NOT NULL CHECK (
        length(scope_sha256) = 64
        AND scope_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    PRIMARY KEY (mutation_sha256, scope_sha256),
    FOREIGN KEY (mutation_sha256)
        REFERENCES conversation_identity_mutations(mutation_sha256) ON DELETE RESTRICT
);

INSERT INTO conversation_identity_mutation_scopes(mutation_sha256, scope_sha256)
SELECT c.mutation_sha256, s.scope_sha256
FROM conversation_identity_commit c
JOIN conversation_identity_scopes s
  ON s.edition_sha256 = c.edition_sha256 AND s.revision = c.revision
WHERE c.operation = 'save';

CREATE TABLE conversation_identity_migrated_scopes (
    edition_sha256 TEXT NOT NULL,
    scope_sha256 TEXT NOT NULL,
    alias_sha256 TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (edition_sha256, scope_sha256)
);

INSERT INTO conversation_identity_migrated_scopes(
    edition_sha256, scope_sha256, alias_sha256, conversation_id, revision, updated_at
)
SELECT edition_sha256, scope_sha256, alias_sha256, conversation_id, revision, updated_at
FROM conversation_identity_scopes;
