-- Same RuntimeStore authority. A0 never manufactures a human approval.
CREATE TABLE governed_tool_intents (
    execution_id TEXT PRIMARY KEY CHECK(length(execution_id)=71),
    call_id TEXT NOT NULL UNIQUE CHECK(length(call_id) BETWEEN 1 AND 128),
    tool_id TEXT NOT NULL CHECK(length(tool_id) BETWEEN 1 AND 128),
    approval_call_id TEXT UNIQUE REFERENCES tool_approvals(call_id) ON DELETE RESTRICT,
    legacy_execution_call_id TEXT UNIQUE REFERENCES tool_executions(call_id) ON DELETE RESTRICT,
    binding_json TEXT NOT NULL CHECK(length(binding_json) <= 2048),
    approval_json TEXT CHECK(length(approval_json) <= 8192),
    outcome_json TEXT NOT NULL CHECK(length(outcome_json) <= 1024),
    state TEXT NOT NULL CHECK(state IN ('prepared','interrupted','succeeded','failed')),
    started_at_ms INTEGER NOT NULL CHECK(started_at_ms >= 0),
    CHECK ((approval_call_id IS NULL AND legacy_execution_call_id IS NULL AND approval_json IS NULL)
      OR (approval_call_id=call_id AND legacy_execution_call_id=call_id AND approval_json IS NOT NULL))
);
CREATE TABLE governed_stage_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id TEXT NOT NULL CHECK(length(execution_id)=71),
    intent_id TEXT REFERENCES governed_tool_intents(execution_id) ON DELETE RESTRICT,
    stage INTEGER NOT NULL CHECK(stage BETWEEN 0 AND 16),
    record_json TEXT NOT NULL CHECK(length(record_json) <= 1024),
    CHECK(stage != 11 OR (intent_id IS NOT NULL AND intent_id=execution_id))
);
CREATE TRIGGER governed_events_no_update BEFORE UPDATE ON governed_stage_events
BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER governed_events_no_delete BEFORE DELETE ON governed_stage_events
BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TABLE governed_outcome_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id TEXT NOT NULL REFERENCES governed_tool_intents(execution_id) ON DELETE RESTRICT,
    outcome_json TEXT NOT NULL CHECK(length(outcome_json)<=1024)
);
CREATE TRIGGER governed_outcomes_no_update BEFORE UPDATE ON governed_outcome_events
BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER governed_outcomes_no_delete BEFORE DELETE ON governed_outcome_events
BEGIN SELECT RAISE(ABORT, 'append only'); END;
CREATE TRIGGER governed_intents_no_delete BEFORE DELETE ON governed_tool_intents
BEGIN SELECT RAISE(ABORT, 'permanent replay reservation'); END;
CREATE TABLE governed_approval_previews (
    call_id TEXT PRIMARY KEY REFERENCES tool_approvals(call_id) ON DELETE RESTRICT,
    binding_json TEXT NOT NULL CHECK(length(binding_json)<=2048)
);
CREATE TRIGGER governed_previews_no_update BEFORE UPDATE ON governed_approval_previews
BEGIN SELECT RAISE(ABORT, 'immutable preview'); END;
CREATE TRIGGER governed_previews_no_delete BEFORE DELETE ON governed_approval_previews
BEGIN SELECT RAISE(ABORT, 'immutable preview'); END;
-- Identity remains a permanent replay reservation. Only one prepared-to-terminal
-- state/outcome update is permitted; recovery uses the same transition boundary.
CREATE TRIGGER governed_intents_update_guard BEFORE UPDATE ON governed_tool_intents
WHEN OLD.execution_id IS NOT NEW.execution_id
  OR OLD.call_id IS NOT NEW.call_id
  OR OLD.tool_id IS NOT NEW.tool_id
  OR OLD.approval_call_id IS NOT NEW.approval_call_id
  OR OLD.legacy_execution_call_id IS NOT NEW.legacy_execution_call_id
  OR OLD.binding_json IS NOT NEW.binding_json
  OR OLD.approval_json IS NOT NEW.approval_json
  OR OLD.started_at_ms IS NOT NEW.started_at_ms
  OR OLD.state != 'prepared'
  OR NEW.state NOT IN ('interrupted', 'succeeded', 'failed')
BEGIN SELECT RAISE(ABORT, 'immutable intent or terminal transition'); END;
