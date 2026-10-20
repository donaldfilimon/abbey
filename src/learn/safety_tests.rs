//! Learning safety regressions using disposable local state only.

use super::*;
use crate::memory::SqliteMemory;
use crate::route_log::RouteRecord;
use std::fs;

struct Scratch(AbbeyState);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("abbey-learn-safe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self(AbbeyState {
            chat_file: dir.join("chat-id"),
            model_file: dir.join("model"),
            history_file: dir.join("history.log"),
            cwd_dir: dir.join("by-cwd"),
            per_cwd: false,
            cwd: dir.clone(),
            state_dir: dir,
        })
    }

    fn sqlite(&self) -> SqliteMemory {
        SqliteMemory::open(&SqliteMemory::path_for_state_dir(&self.0.state_dir)).unwrap()
    }

    fn assert_no_store(&self) {
        assert!(!self.0.state_dir.join("memory.sqlite").exists());
        assert!(!self.0.state_dir.join("wdbx").exists());
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0.state_dir);
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}

#[test]
fn invalid_learning_arguments_never_create_store_or_consume_routes() {
    let scratch = Scratch::new();
    route_log::append_route_record(
        &scratch.0.state_dir,
        &RouteRecord::new(".", "abbey", "max", "local", "hybrid", 0.9),
    )
    .unwrap();
    let log = route_log::route_log_path(&scratch.0.state_dir);
    let before = fs::read(&log).unwrap();
    for args in [
        vec!["improve", "--apply", "typo"],
        vec!["improve", "--apply", "--aply"],
        vec!["improve", "1", "2", "--apply"],
        vec!["improve", "--apply", "--apply"],
        vec!["improve", "-1", "--apply"],
        vec!["routes", "typo"],
        vec!["routes", "2", "extra"],
        vec!["digest", "--typo"],
        vec!["review", "-1"],
        vec!["export", "--typo"],
        vec!["train", "  "],
        vec!["correction", "\t"],
        vec!["preference", "  "],
    ] {
        assert!(
            dispatch(&scratch.0, &strings(&args), None).is_err(),
            "{args:?}"
        );
        scratch.assert_no_store();
        assert_eq!(fs::read(&log).unwrap(), before);
    }
}

#[test]
fn improve_parser_accepts_count_and_apply_in_either_order() {
    for args in [vec!["3", "--apply"], vec!["--apply", "3"]] {
        assert_eq!(args::improve(&strings(&args)).unwrap(), (3, true));
    }
    assert_eq!(args::improve(&[]).unwrap(), (20, false));
    assert_eq!(args::improve(&strings(&["0"])).unwrap(), (0, false));
    assert!(args::improve(&strings(&["184467440737095516160", "--apply"])).is_err());
}

#[test]
fn absent_store_read_surfaces_and_empty_route_apply_do_not_create_it() {
    let scratch = Scratch::new();
    for args in [
        vec![],
        vec!["stats"],
        vec!["review"],
        vec!["export"],
        vec!["improve"],
    ] {
        assert_eq!(dispatch(&scratch.0, &strings(&args), None).unwrap(), 0);
        scratch.assert_no_store();
    }
    assert!(curation_summary(&scratch.0).unwrap().contains("no store"));
    assert!(preference_context(&scratch.0.state_dir, 10).is_empty());
    assert_eq!(learn_from_routes(&scratch.0, 10).unwrap(), 0);
    assert_eq!(apply_routes(&scratch.0, &[]).unwrap(), 0);
    scratch.assert_no_store();
}

fn assert_candidates_unchanged_after_route_apply(mem: &dyn MemoryStore) {
    let mut candidate = MemoryRecord::new_stm("curated candidate", "original pair");
    candidate.retention = "train_candidate".into();
    candidate.provenance = "human-curated fixture".into();
    let id = candidate.id.clone();
    let before = serde_json::to_value(&candidate).unwrap();
    mem.store(candidate).unwrap();
    let routes = [RouteRecord::new(
        ".", "abbey", "max", "local", "hybrid", 0.9,
    )];
    assert_eq!(store_route_activity(mem, &routes).unwrap(), 1);
    assert_eq!(store_route_activity(mem, &routes).unwrap(), 0);
    assert_eq!(
        serde_json::to_value(mem.get(&id).unwrap().unwrap()).unwrap(),
        before
    );
}

#[test]
fn sqlite_route_apply_preserves_candidates_and_provenance() {
    let scratch = Scratch::new();
    assert_candidates_unchanged_after_route_apply(&scratch.sqlite());
}

#[test]
fn route_apply_uses_supplied_snapshot_not_a_newer_log_tail() {
    let scratch = Scratch::new();
    let first = RouteRecord::new(".", "abbey", "max", "first", "hybrid", 0.9);
    route_log::append_route_record(&scratch.0.state_dir, &first).unwrap();
    let snapshot = route_log::recent_routes(&scratch.0.state_dir, 1).unwrap();
    let later = RouteRecord::new(".", "abbey", "max", "later", "hybrid", 0.9);
    route_log::append_route_record(&scratch.0.state_dir, &later).unwrap();
    assert_eq!(apply_routes(&scratch.0, &snapshot).unwrap(), 1);
    let mem = open_mem(&scratch.0).unwrap();
    let rows = mem.filter(Some("activity"), None, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provenance, route_provenance(&first).unwrap());
    assert_ne!(rows[0].provenance, route_provenance(&later).unwrap());
}

#[cfg(feature = "wdbx")]
#[test]
fn wdbx_route_apply_preserves_candidates_and_provenance() {
    let scratch = Scratch::new();
    let mem = memory::open_backend(&scratch.0.state_dir, "wdbx").unwrap();
    assert_candidates_unchanged_after_route_apply(mem.as_ref());
}

#[test]
fn route_partial_write_error_reports_retained_progress_and_retry_is_idempotent() {
    let scratch = Scratch::new();
    let mem = scratch.sqlite();
    let routes = [
        RouteRecord::new(".", "abbey", "max", "first", "hybrid", 0.9),
        RouteRecord::new(".", "abbey", "max", "second", "hybrid", 0.9),
    ];
    let conn =
        rusqlite::Connection::open(SqliteMemory::path_for_state_dir(&scratch.0.state_dir)).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_second BEFORE INSERT ON memory
         WHEN NEW.summary LIKE '%second'
         BEGIN SELECT RAISE(FAIL, 'fixture write failure'); END;",
    )
    .unwrap();
    let error = format!("{:#}", store_route_activity(&mem, &routes).unwrap_err());
    assert!(error.contains("after 1 record(s) stored"), "{error}");
    assert!(error.contains("earlier writes retained"), "{error}");
    assert!(error.contains("fixture write failure"), "{error}");
    assert_eq!(mem.filter(Some("activity"), None, 10).unwrap().len(), 1);
    conn.execute_batch("DROP TRIGGER reject_second;").unwrap();
    assert_eq!(store_route_activity(&mem, &routes).unwrap(), 1);
    assert_eq!(store_route_activity(&mem, &routes).unwrap(), 0);
    assert_eq!(mem.filter(Some("activity"), None, 10).unwrap().len(), 2);
}
