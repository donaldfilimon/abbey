-- Complete private previews live only in the user's local runtime database.
CREATE TABLE local_governed_approvals (
 call_id TEXT PRIMARY KEY REFERENCES governed_approval_previews(call_id) ON DELETE RESTRICT,
 preview_json TEXT NOT NULL CHECK(length(preview_json)<=65536),
 owner_json TEXT NOT NULL CHECK(length(owner_json)<=4096),
 exact_json TEXT CHECK(length(exact_json)<=8192)
);
CREATE TRIGGER local_governed_approvals_no_delete BEFORE DELETE ON local_governed_approvals
BEGIN SELECT RAISE(ABORT, 'permanent local approval'); END;
CREATE TRIGGER local_governed_approvals_update_guard BEFORE UPDATE ON local_governed_approvals
WHEN OLD.call_id IS NOT NEW.call_id OR OLD.preview_json IS NOT NEW.preview_json
 OR OLD.owner_json IS NOT NEW.owner_json OR OLD.exact_json IS NOT NULL OR NEW.exact_json IS NULL
BEGIN SELECT RAISE(ABORT, 'immutable local approval'); END;
