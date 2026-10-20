//! Local-only route identity and digest regression tests.

use super::*;
use crate::memory::SqliteMemory;
use crate::route_log::RouteRecord;
use std::fs;
use std::path::PathBuf;

struct Scratch {
    dir: PathBuf,
    mem: SqliteMemory,
}

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("abbey-learn-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let mem = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
        Self { dir, mem }
    }

    fn rows(&self) -> Vec<MemoryRecord> {
        self.mem.all_records_including_obsolete().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn route() -> RouteRecord {
    let mut route = RouteRecord::new("/project", "abbey", "max", "local-model", "hybrid", 0.9)
        .with_routing(Some("gemma".into()), Some("review only".into()));
    route.ts = "2026-10-20T12:00:00Z".into();
    route
}

#[test]
fn same_second_distinct_routes_and_repeated_tail_are_lossless_and_idempotent() {
    let scratch = Scratch::new();
    let first = route();
    let mut second = first.clone();
    second.reason = "different decision".into();
    let mut third = first.clone();
    third.tools = vec!["read".into()];
    let fourth = first.clone().in_stage("run-1", "interpret");
    for r in [&first, &second, &third, &fourth, &first] {
        route_log::append_route_record(&scratch.dir, r).unwrap();
    }
    let path = route_log::route_log_path(&scratch.dir);
    let evidence = fs::read(&path).unwrap();
    let tail = route_log::recent_routes(&scratch.dir, 10).unwrap();
    assert_eq!(store_route_activity(&scratch.mem, &tail).unwrap(), 4);
    assert_eq!(store_route_activity(&scratch.mem, &tail).unwrap(), 0);
    assert_eq!(
        fs::read(&path).unwrap(),
        evidence,
        "source log is untouched"
    );
    let rows = scratch.rows();
    assert_eq!(rows.len(), 4);
    for r in [&first, &second, &third, &fourth] {
        let provenance = route_provenance(r).unwrap();
        assert!(provenance.starts_with(&format!("route.jsonl @ {} sha256:", r.ts)));
        let row = rows
            .iter()
            .find(|row| row.provenance == provenance)
            .unwrap();
        assert_eq!(row.source_type, "route");
        assert_eq!(row.retention, "activity");
        assert_eq!(row.payload, route_activity_payload(r));
        assert_eq!(row.confidence, r.confidence);
        assert!(row.tags.contains(&"self-learn".into()));
    }
}

#[test]
fn legacy_exact_evidence_is_reused_without_suppressing_same_second_routes() {
    let scratch = Scratch::new();
    let first = route();
    let mut legacy = MemoryRecord::new_stm(
        format!("route {}/{} → {}", first.persona, first.role, first.model),
        route_activity_payload(&first),
    );
    legacy.source_type = "route".into();
    legacy.retention = "ltm".into(); // A prior digest must not break dedupe.
    legacy.confidence = first.confidence;
    legacy.provenance = format!("route.jsonl @ {}", first.ts);
    let original = serde_json::to_value(&legacy).unwrap();
    let id = legacy.id.clone();
    scratch.mem.store(legacy).unwrap();
    let mut second = first.clone();
    second.reason = "distinct payload".into();
    // These fields were absent from the legacy projection. One legacy row
    // must not suppress two distinct full-record identities in the same tail.
    let third = first.clone().in_stage("another-run", "implement");
    // Put the metadata-bearing route first: it must not consume the legacy
    // match that belongs to the plain route, despite an identical projection.
    let routes = [third, first.clone(), first, second];
    assert_eq!(store_route_activity(&scratch.mem, &routes).unwrap(), 2);
    assert_eq!(store_route_activity(&scratch.mem, &routes).unwrap(), 0);
    assert_eq!(scratch.rows().len(), 3);
    assert_eq!(
        serde_json::to_value(scratch.mem.get(&id).unwrap().unwrap()).unwrap(),
        original,
        "legacy provenance and user evidence remain unchanged"
    );
}

#[test]
fn legacy_timestamp_does_not_match_different_summary_or_exact_confidence() {
    let scratch = Scratch::new();
    let first = route();
    let mut legacy = MemoryRecord::new_stm("different summary", route_activity_payload(&first));
    legacy.source_type = "route".into();
    legacy.provenance = format!("route.jsonl @ {}", first.ts);
    legacy.confidence = first.confidence;
    scratch.mem.store(legacy).unwrap();
    assert_eq!(
        store_route_activity(&scratch.mem, std::slice::from_ref(&first)).unwrap(),
        1
    );
    let mut exact_projection = MemoryRecord::new_stm(
        format!("route {}/{} → {}", first.persona, first.role, first.model),
        route_activity_payload(&first),
    );
    exact_projection.source_type = "route".into();
    exact_projection.provenance = format!("route.jsonl @ {}", first.ts);
    exact_projection.confidence = first.confidence;
    scratch.mem.store(exact_projection).unwrap();
    let mut close = first;
    close.confidence = 0.901; // Same two-decimal payload, different exact identity.
    assert_eq!(store_route_activity(&scratch.mem, &[close]).unwrap(), 1);
    assert_eq!(scratch.rows().len(), 4);
}

#[test]
fn digest_propagates_promotion_failure_and_leaves_candidates_untouched() {
    let scratch = Scratch::new();
    let mut candidate = MemoryRecord::new_stm("curated pair", "do not change");
    candidate.retention = "train_candidate".into();
    candidate.provenance = "user review".into();
    let candidate_id = candidate.id.clone();
    let candidate_before = serde_json::to_value(&candidate).unwrap();
    scratch.mem.store(candidate).unwrap();
    store_route_activity(&scratch.mem, &[route()]).unwrap();
    let conn = rusqlite::Connection::open(SqliteMemory::path_for_state_dir(&scratch.dir)).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_promotion BEFORE UPDATE ON memory
         BEGIN SELECT RAISE(FAIL, 'promotion refused by fixture'); END;",
    )
    .unwrap();
    let error = digest_memory(&scratch.mem).unwrap_err();
    assert!(format!("{error:#}").contains("promotion refused by fixture"));
    assert_eq!(
        scratch
            .mem
            .filter(Some("activity"), None, 10)
            .unwrap()
            .len(),
        1
    );
    conn.execute_batch("DROP TRIGGER reject_promotion;")
        .unwrap();
    assert!(digest_memory(&scratch.mem).unwrap().contains("promoted=1"));
    assert!(digest_memory(&scratch.mem).unwrap().contains("promoted=0"));
    assert_eq!(
        serde_json::to_value(scratch.mem.get(&candidate_id).unwrap().unwrap()).unwrap(),
        candidate_before
    );
}
