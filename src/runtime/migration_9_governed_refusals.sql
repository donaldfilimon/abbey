-- Stage11 admission remains an atomic intent-linked allow in migration8's table.
-- A refused preparation is a separate append-only event and grants no admission.
CREATE TABLE governed_admission_refusals (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id TEXT NOT NULL CHECK(length(execution_id)=71),
    stage INTEGER NOT NULL CHECK(stage=11),
    record_json TEXT NOT NULL CHECK(length(record_json)<=1024
        AND json_valid(record_json)
        AND json_extract(record_json,'$.version') IS 1
        AND json_extract(record_json,'$.stage') IS 11
        AND json_extract(record_json,'$.decision') IS 'deny'
        AND json_extract(record_json,'$.execution_id') IS execution_id)
);
CREATE TRIGGER governed_refusals_no_update BEFORE UPDATE ON governed_admission_refusals
BEGIN SELECT RAISE(ABORT, 'append only refusal'); END;
CREATE TRIGGER governed_refusals_no_delete BEFORE DELETE ON governed_admission_refusals
BEGIN SELECT RAISE(ABORT, 'append only refusal'); END;
