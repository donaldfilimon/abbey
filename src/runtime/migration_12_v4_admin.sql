-- Protocol-v4 task, automation, action and permission-revision authority.
--
-- Additive only. Migration 10's assistant_tasks/automations/automation_runs/
-- action_decisions tables are left untouched: their state vocabularies and
-- uniqueness rules predate the v4 contract (no revision, attempt record,
-- schedule shape, occurrence source, or action binding) and SQLite cannot
-- alter CHECK constraints in place.
--
-- Column vocabularies mirror src/app_core/v4/{task,automation,action}.rs.
-- Digests are lowercase 64-character hex, as `digest()` requires.

CREATE TABLE v4_profile_revisions (
    profile_id TEXT PRIMARY KEY REFERENCES assistant_permission_profiles(profile_id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    digest TEXT NOT NULL CHECK (length(digest) = 64 AND digest NOT GLOB '*[^0-9a-f]*'),
    revocation_epoch INTEGER NOT NULL CHECK (revocation_epoch >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
);

-- One row per admin operation id; replay of an identical request returns the
-- recorded resource, a different request under the same id is refused.
CREATE TABLE v4_admin_operations (
    operation_id TEXT PRIMARY KEY CHECK (length(operation_id) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 32),
    request_digest TEXT NOT NULL CHECK (
        length(request_digest) = 64 AND request_digest NOT GLOB '*[^0-9a-f]*'
    ),
    resource_id TEXT NOT NULL CHECK (length(resource_id) BETWEEN 1 AND 128),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
);
CREATE TRIGGER v4_admin_operations_append_only_update BEFORE UPDATE ON v4_admin_operations
BEGIN SELECT RAISE(ABORT, 'append-only admin operation'); END;
CREATE TRIGGER v4_admin_operations_append_only_delete BEFORE DELETE ON v4_admin_operations
BEGIN SELECT RAISE(ABORT, 'append-only admin operation'); END;

-- V4Task. `current_attempt_id` names V4Task.current_attempt; it carries no
-- foreign key because attempts reference their task (the store keeps the pair
-- consistent and admits both in one transaction).
CREATE TABLE v4_tasks (
    task_id TEXT PRIMARY KEY CHECK (length(task_id) BETWEEN 1 AND 128),
    sequence INTEGER NOT NULL UNIQUE CHECK (sequence >= 1),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE RESTRICT,
    goal TEXT NOT NULL CHECK (length(goal) BETWEEN 1 AND 1024),
    workspace_id TEXT NOT NULL CHECK (length(workspace_id) BETWEEN 1 AND 128),
    route_id TEXT NOT NULL CHECK (length(route_id) BETWEEN 1 AND 128),
    permission_profile_id TEXT NOT NULL REFERENCES assistant_permission_profiles(profile_id) ON DELETE RESTRICT,
    profile_revision INTEGER NOT NULL CHECK (profile_revision >= 1),
    budget_tokens INTEGER NOT NULL CHECK (budget_tokens BETWEEN 1 AND 100000),
    execution_kind TEXT NOT NULL CHECK (execution_kind IN ('read_only', 'governed')),
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'starting', 'running', 'awaiting_approval', 'paused',
            'succeeded', 'failed', 'cancelled', 'cancel_requested', 'interrupted'
        )
    ),
    current_attempt_id TEXT NOT NULL CHECK (length(current_attempt_id) BETWEEN 1 AND 128),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
);
-- The route, profile, budget and goal snapshot taken at creation is immutable.
CREATE TRIGGER v4_tasks_snapshot_immutable BEFORE UPDATE ON v4_tasks
WHEN OLD.task_id IS NOT NEW.task_id OR OLD.sequence IS NOT NEW.sequence
 OR OLD.conversation_id IS NOT NEW.conversation_id OR OLD.goal IS NOT NEW.goal
 OR OLD.workspace_id IS NOT NEW.workspace_id OR OLD.route_id IS NOT NEW.route_id
 OR OLD.permission_profile_id IS NOT NEW.permission_profile_id
 OR OLD.profile_revision IS NOT NEW.profile_revision
 OR OLD.budget_tokens IS NOT NEW.budget_tokens
 OR OLD.execution_kind IS NOT NEW.execution_kind
 OR OLD.created_at_ms IS NOT NEW.created_at_ms
 OR NEW.revision <= OLD.revision
BEGIN SELECT RAISE(ABORT, 'immutable task snapshot'); END;
CREATE TRIGGER v4_tasks_no_delete BEFORE DELETE ON v4_tasks
BEGIN SELECT RAISE(ABORT, 'durable task'); END;

-- V4TaskAttempt. `state` is RunState; a terminal state and `finished_at_ms`
-- appear together, as V4TaskAttempt::validate requires.
CREATE TABLE v4_task_attempts (
    attempt_id TEXT PRIMARY KEY CHECK (length(attempt_id) BETWEEN 1 AND 128),
    task_id TEXT NOT NULL REFERENCES v4_tasks(task_id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    run_id TEXT NOT NULL UNIQUE CHECK (length(run_id) = 36),
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'starting', 'running', 'cancel_requested',
            'succeeded', 'failed', 'cancelled', 'interrupted'
        )
    ),
    admitted_at_ms INTEGER NOT NULL CHECK (admitted_at_ms >= 0),
    finished_at_ms INTEGER CHECK (finished_at_ms IS NULL OR finished_at_ms >= admitted_at_ms),
    CHECK (
        (state IN ('succeeded', 'failed', 'cancelled', 'interrupted'))
        = (finished_at_ms IS NOT NULL)
    ),
    UNIQUE (task_id, ordinal),
    -- Parent key for records that bind a task, attempt and run together.
    UNIQUE (task_id, attempt_id, run_id)
);
CREATE TRIGGER v4_task_attempts_identity_immutable BEFORE UPDATE ON v4_task_attempts
WHEN OLD.attempt_id IS NOT NEW.attempt_id OR OLD.task_id IS NOT NEW.task_id
 OR OLD.ordinal IS NOT NEW.ordinal OR OLD.run_id IS NOT NEW.run_id
 OR OLD.admitted_at_ms IS NOT NEW.admitted_at_ms
 OR OLD.finished_at_ms IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'immutable task attempt'); END;
CREATE TRIGGER v4_task_attempts_no_delete BEFORE DELETE ON v4_task_attempts
BEGIN SELECT RAISE(ABORT, 'durable task attempt'); END;

-- V4TaskEvent: the attempt and run must belong to the event's task.
CREATE TABLE v4_task_events (
    task_id TEXT NOT NULL REFERENCES v4_tasks(task_id) ON DELETE RESTRICT,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    attempt_id TEXT NOT NULL CHECK (length(attempt_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL CHECK (length(run_id) = 36),
    timestamp_ms INTEGER NOT NULL CHECK (timestamp_ms >= 0),
    kind_json TEXT NOT NULL CHECK (length(kind_json) BETWEEN 2 AND 4096 AND json_valid(kind_json)),
    PRIMARY KEY (task_id, sequence),
    FOREIGN KEY (task_id, attempt_id, run_id)
        REFERENCES v4_task_attempts(task_id, attempt_id, run_id) ON DELETE RESTRICT
);
CREATE TRIGGER v4_task_events_append_only_update BEFORE UPDATE ON v4_task_events
BEGIN SELECT RAISE(ABORT, 'append-only task event'); END;
CREATE TRIGGER v4_task_events_append_only_delete BEFORE DELETE ON v4_task_events
BEGIN SELECT RAISE(ABORT, 'append-only task event'); END;

-- V4Automation. `spec_json` is the V4AutomationUpsert; the identity and enabled
-- flag it carries must agree with the indexed columns. Profile existence and
-- schedule validity are enforced by the store (Task 4) before a spec is written,
-- and again by v4_tasks' foreign keys when an occurrence admits its task.
CREATE TABLE v4_automations (
    automation_id TEXT PRIMARY KEY CHECK (length(automation_id) BETWEEN 1 AND 128),
    sequence INTEGER NOT NULL UNIQUE CHECK (sequence >= 1),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    spec_json TEXT NOT NULL CHECK (
        length(spec_json) BETWEEN 2 AND 16384
        AND json_valid(spec_json)
        AND json_type(spec_json) = 'object'
        AND json_extract(spec_json, '$.automation_id') IS automation_id
        AND json_extract(spec_json, '$.enabled') IS enabled
    ),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    -- Exclusive lower bound for the next catch-up; advances only on reservation.
    cursor_ms INTEGER NOT NULL CHECK (cursor_ms >= 0),
    next_run_at_ms INTEGER CHECK (next_run_at_ms IS NULL OR next_run_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
);
CREATE TRIGGER v4_automations_identity_immutable BEFORE UPDATE ON v4_automations
WHEN OLD.automation_id IS NOT NEW.automation_id OR OLD.sequence IS NOT NEW.sequence
 OR OLD.created_at_ms IS NOT NEW.created_at_ms OR NEW.revision < OLD.revision
BEGIN SELECT RAISE(ABORT, 'immutable automation identity'); END;
CREATE TRIGGER v4_automations_no_delete BEFORE DELETE ON v4_automations
BEGIN SELECT RAISE(ABORT, 'durable automation'); END;

-- V4AutomationOccurrence. Identity is (automation, revision, scheduled UTC
-- instant) or (automation, manual operation id); each occurrence owns exactly
-- one task. Identity is immutable; state and the current attempt/run (a retry
-- creates a new attempt of the same task) may change.
CREATE TABLE v4_automation_occurrences (
    occurrence_id TEXT PRIMARY KEY CHECK (length(occurrence_id) BETWEEN 1 AND 128),
    automation_id TEXT NOT NULL REFERENCES v4_automations(automation_id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('scheduled', 'manual')),
    scheduled_at_ms INTEGER CHECK (scheduled_at_ms IS NULL OR scheduled_at_ms >= 0),
    manual_operation_id TEXT CHECK (manual_operation_id IS NULL OR length(manual_operation_id) BETWEEN 1 AND 128),
    task_id TEXT NOT NULL UNIQUE REFERENCES v4_tasks(task_id) ON DELETE RESTRICT,
    attempt_id TEXT NOT NULL CHECK (length(attempt_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL CHECK (length(run_id) = 36),
    state TEXT NOT NULL CHECK (
        state IN (
            'reserved', 'running', 'succeeded', 'failed',
            'cancelled', 'interrupted', 'reconciliation_required'
        )
    ),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CHECK (
        (source_kind = 'scheduled' AND scheduled_at_ms IS NOT NULL AND manual_operation_id IS NULL)
        OR (source_kind = 'manual' AND scheduled_at_ms IS NULL AND manual_operation_id IS NOT NULL)
    ),
    UNIQUE (automation_id, revision, scheduled_at_ms),
    UNIQUE (automation_id, manual_operation_id),
    FOREIGN KEY (task_id, attempt_id, run_id)
        REFERENCES v4_task_attempts(task_id, attempt_id, run_id) ON DELETE RESTRICT
);
CREATE TRIGGER v4_automation_occurrences_identity_immutable BEFORE UPDATE ON v4_automation_occurrences
WHEN OLD.occurrence_id IS NOT NEW.occurrence_id OR OLD.automation_id IS NOT NEW.automation_id
 OR OLD.revision IS NOT NEW.revision OR OLD.source_kind IS NOT NEW.source_kind
 OR OLD.scheduled_at_ms IS NOT NEW.scheduled_at_ms
 OR OLD.manual_operation_id IS NOT NEW.manual_operation_id
 OR OLD.task_id IS NOT NEW.task_id OR OLD.created_at_ms IS NOT NEW.created_at_ms
BEGIN SELECT RAISE(ABORT, 'immutable automation occurrence'); END;
CREATE TRIGGER v4_automation_occurrences_no_delete BEFORE DELETE ON v4_automation_occurrences
BEGIN SELECT RAISE(ABORT, 'durable automation occurrence'); END;

-- V4Action: bound to one task attempt and its run.
CREATE TABLE v4_actions (
    action_id TEXT PRIMARY KEY CHECK (length(action_id) BETWEEN 1 AND 128),
    task_id TEXT NOT NULL REFERENCES v4_tasks(task_id) ON DELETE RESTRICT,
    attempt_id TEXT NOT NULL CHECK (length(attempt_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL CHECK (length(run_id) = 36),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    operation_id TEXT NOT NULL CHECK (length(operation_id) BETWEEN 1 AND 128),
    resource_label TEXT NOT NULL CHECK (length(resource_label) BETWEEN 1 AND 256),
    preview_digest TEXT NOT NULL CHECK (
        length(preview_digest) = 64 AND preview_digest NOT GLOB '*[^0-9a-f]*'
    ),
    input_digest TEXT NOT NULL CHECK (
        length(input_digest) = 64 AND input_digest NOT GLOB '*[^0-9a-f]*'
    ),
    target_digest TEXT NOT NULL CHECK (
        length(target_digest) = 64 AND target_digest NOT GLOB '*[^0-9a-f]*'
    ),
    binding_digest TEXT NOT NULL CHECK (
        length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'
    ),
    expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0),
    state TEXT NOT NULL CHECK (
        state IN (
            'awaiting_approval', 'approved', 'denied', 'running',
            'succeeded', 'failed', 'interrupted', 'uncertain'
        )
    ),
    FOREIGN KEY (task_id, attempt_id, run_id)
        REFERENCES v4_task_attempts(task_id, attempt_id, run_id) ON DELETE RESTRICT,
    -- Parent key for the decision binding below.
    UNIQUE (action_id, task_id, attempt_id, preview_digest)
);
CREATE TRIGGER v4_actions_binding_immutable BEFORE UPDATE ON v4_actions
WHEN OLD.action_id IS NOT NEW.action_id OR OLD.task_id IS NOT NEW.task_id
 OR OLD.attempt_id IS NOT NEW.attempt_id OR OLD.run_id IS NOT NEW.run_id
 OR OLD.operation_id IS NOT NEW.operation_id
 OR OLD.resource_label IS NOT NEW.resource_label
 OR OLD.preview_digest IS NOT NEW.preview_digest OR OLD.input_digest IS NOT NEW.input_digest
 OR OLD.target_digest IS NOT NEW.target_digest OR OLD.binding_digest IS NOT NEW.binding_digest
 OR OLD.expires_at_ms IS NOT NEW.expires_at_ms OR NEW.revision <= OLD.revision
BEGIN SELECT RAISE(ABORT, 'immutable action binding'); END;
CREATE TRIGGER v4_actions_no_delete BEFORE DELETE ON v4_actions
BEGIN SELECT RAISE(ABORT, 'durable action'); END;

-- V4ActionDecisionReceipt: one decision per action, bound to action id +
-- decision id + kind, and to the action's task, attempt and preview digest.
CREATE TABLE v4_action_decisions (
    decision_id TEXT PRIMARY KEY CHECK (length(decision_id) BETWEEN 1 AND 128),
    action_id TEXT NOT NULL UNIQUE,
    task_id TEXT NOT NULL CHECK (length(task_id) BETWEEN 1 AND 128),
    attempt_id TEXT NOT NULL CHECK (length(attempt_id) BETWEEN 1 AND 128),
    expected_revision INTEGER NOT NULL CHECK (expected_revision >= 1),
    preview_digest TEXT NOT NULL CHECK (
        length(preview_digest) = 64 AND preview_digest NOT GLOB '*[^0-9a-f]*'
    ),
    kind TEXT NOT NULL CHECK (kind IN ('approve', 'deny')),
    applied_revision INTEGER NOT NULL CHECK (applied_revision > expected_revision),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    revoked_at_ms INTEGER CHECK (revoked_at_ms IS NULL OR revoked_at_ms >= created_at_ms),
    FOREIGN KEY (action_id, task_id, attempt_id, preview_digest)
        REFERENCES v4_actions(action_id, task_id, attempt_id, preview_digest) ON DELETE RESTRICT
);
CREATE TRIGGER v4_action_decisions_binding_immutable BEFORE UPDATE ON v4_action_decisions
WHEN OLD.decision_id IS NOT NEW.decision_id OR OLD.action_id IS NOT NEW.action_id
 OR OLD.task_id IS NOT NEW.task_id OR OLD.attempt_id IS NOT NEW.attempt_id
 OR OLD.kind IS NOT NEW.kind OR OLD.applied_revision IS NOT NEW.applied_revision
 OR OLD.expected_revision IS NOT NEW.expected_revision
 OR OLD.preview_digest IS NOT NEW.preview_digest
 OR OLD.created_at_ms IS NOT NEW.created_at_ms OR OLD.revoked_at_ms IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'immutable action decision'); END;
CREATE TRIGGER v4_action_decisions_no_delete BEFORE DELETE ON v4_action_decisions
BEGIN SELECT RAISE(ABORT, 'durable action decision'); END;

CREATE INDEX idx_v4_task_attempts_state ON v4_task_attempts(state);
CREATE INDEX idx_v4_actions_task ON v4_actions(task_id, attempt_id);
CREATE INDEX idx_v4_automation_occurrences_state ON v4_automation_occurrences(state);
