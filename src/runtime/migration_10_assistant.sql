-- Desktop assistant content. These tables are not a replacement for the
-- legacy identity `conversations` table. Deleting a transcript never deletes
-- a legacy identity, a run, an idempotency key, or a migration backup.
-- `assistant_content_events` stores kind and byte counts only. Message bodies
-- and authorized content payloads live in their own tables.

CREATE TABLE assistant_permission_profiles (
    profile_id TEXT PRIMARY KEY CHECK (length(profile_id) BETWEEN 1 AND 128),
    document_json TEXT NOT NULL CHECK (
        length(document_json) BETWEEN 2 AND 65536
        AND json_valid(document_json)
    ),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
);

CREATE TABLE assistant_conversations (
    id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE RESTRICT,
    sequence INTEGER NOT NULL UNIQUE CHECK (sequence >= 1),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 256),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    -- Zero marks a legacy identity whose message bodies were not imported.
    imported_history INTEGER NOT NULL DEFAULT 1 CHECK (imported_history IN (0, 1)),
    deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
);

CREATE TABLE assistant_messages (
    id TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 128),
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE RESTRICT,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE RESTRICT,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    body TEXT NOT NULL CHECK (length(cast(body as blob)) <= 32768),
    text_bytes INTEGER NOT NULL CHECK (text_bytes >= 0 AND text_bytes <= 4194304),
    text_complete INTEGER NOT NULL CHECK (text_complete IN (0, 1)),
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'starting', 'running', 'cancel_requested',
            'succeeded', 'failed', 'cancelled', 'interrupted'
        )
    ),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    UNIQUE (conversation_id, sequence)
);

CREATE TABLE assistant_turns (
    run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE RESTRICT,
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE RESTRICT,
    user_message_id TEXT UNIQUE REFERENCES assistant_messages(id) ON DELETE SET NULL,
    assistant_message_id TEXT UNIQUE REFERENCES assistant_messages(id) ON DELETE SET NULL,
    user_sequence INTEGER NOT NULL CHECK (user_sequence >= 1),
    route_id TEXT NOT NULL CHECK (length(route_id) BETWEEN 1 AND 128),
    permission_profile_id TEXT NOT NULL REFERENCES assistant_permission_profiles(profile_id) ON DELETE RESTRICT,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
);

CREATE TABLE assistant_content_events (
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE RESTRICT,
    sequence INTEGER NOT NULL CHECK (sequence BETWEEN 1 AND 65536),
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (
        kind IN (
            'queued', 'started', 'output_bounded', 'completed',
            'cancelled', 'failed', 'interrupted'
        )
    ),
    byte_count INTEGER NOT NULL CHECK (byte_count >= 0 AND byte_count <= 32768),
    timestamp_ms INTEGER NOT NULL CHECK (timestamp_ms >= 0),
    PRIMARY KEY (conversation_id, sequence)
);

CREATE TABLE assistant_content_payloads (
    conversation_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    payload_json TEXT NOT NULL CHECK (
        length(payload_json) BETWEEN 2 AND 131072
        AND json_valid(payload_json)
    ),
    PRIMARY KEY (conversation_id, sequence),
    FOREIGN KEY (conversation_id, sequence)
        REFERENCES assistant_content_events(conversation_id, sequence)
        ON DELETE CASCADE
);

CREATE TABLE assistant_tasks (
    task_id TEXT PRIMARY KEY CHECK (length(task_id) BETWEEN 1 AND 128),
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE RESTRICT,
    goal TEXT NOT NULL CHECK (length(goal) BETWEEN 1 AND 1024),
    route_id TEXT NOT NULL CHECK (length(route_id) BETWEEN 1 AND 128),
    permission_profile_id TEXT NOT NULL REFERENCES assistant_permission_profiles(profile_id) ON DELETE RESTRICT,
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'running', 'awaiting_approval', 'completed',
            'cancelled', 'failed', 'interrupted'
        )
    ),
    budget_tokens INTEGER NOT NULL CHECK (budget_tokens BETWEEN 1 AND 100000),
    attempt INTEGER NOT NULL CHECK (attempt >= 1),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
);

CREATE TABLE assistant_automations (
    automation_id TEXT PRIMARY KEY CHECK (length(automation_id) BETWEEN 1 AND 128),
    instruction TEXT NOT NULL CHECK (length(instruction) BETWEEN 1 AND 1024),
    schedule TEXT NOT NULL CHECK (schedule IN ('once', 'hourly', 'daily', 'weekly', 'cron')),
    cron TEXT CHECK (cron IS NULL OR length(cron) BETWEEN 1 AND 128),
    timezone TEXT NOT NULL CHECK (length(timezone) BETWEEN 1 AND 64),
    route_id TEXT NOT NULL CHECK (length(route_id) BETWEEN 1 AND 128),
    permission_profile_id TEXT NOT NULL REFERENCES assistant_permission_profiles(profile_id) ON DELETE RESTRICT,
    budget_tokens INTEGER NOT NULL CHECK (budget_tokens BETWEEN 1 AND 100000),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    next_run_at_ms INTEGER CHECK (next_run_at_ms IS NULL OR next_run_at_ms >= 0),
    CHECK (
        (schedule = 'cron' AND cron IS NOT NULL)
        OR (schedule != 'cron' AND cron IS NULL)
    )
);

CREATE TABLE assistant_automation_runs (
    occurrence_id TEXT PRIMARY KEY CHECK (length(occurrence_id) BETWEEN 1 AND 128),
    automation_id TEXT NOT NULL REFERENCES assistant_automations(automation_id) ON DELETE CASCADE,
    scheduled_at_ms INTEGER NOT NULL CHECK (scheduled_at_ms >= 0),
    state TEXT NOT NULL CHECK (
        state IN (
            'admitted', 'missed', 'running', 'completed',
            'failed', 'cancelled', 'interrupted'
        )
    ),
    task_id TEXT REFERENCES assistant_tasks(task_id) ON DELETE RESTRICT,
    UNIQUE (automation_id, scheduled_at_ms)
);

CREATE TABLE assistant_action_decisions (
    action_id TEXT PRIMARY KEY CHECK (length(action_id) BETWEEN 1 AND 128),
    decision_id TEXT NOT NULL UNIQUE CHECK (length(decision_id) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('approve', 'deny')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
);

CREATE INDEX idx_assistant_conversations_list
    ON assistant_conversations(deleted, archived, pinned, updated_at_ms, id);
CREATE INDEX idx_assistant_messages_conversation
    ON assistant_messages(conversation_id, sequence);
CREATE INDEX idx_assistant_messages_run
    ON assistant_messages(run_id);
CREATE INDEX idx_assistant_turns_conversation
    ON assistant_turns(conversation_id);
