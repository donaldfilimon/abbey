use super::*;
use crate::memory::{MemoryRecord, MemoryStore, SqliteMemory};

fn scratch(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "abbey-migrate-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn record(id: &str, summary: &str) -> MemoryRecord {
    let mut rec = MemoryRecord::new_stm(summary, "payload");
    rec.id = id.into();
    rec.timestamp = "2026-08-08T12:00:00Z".into();
    rec.provenance = "original provenance".into();
    rec
}

#[test]
fn a_migrated_record_keeps_its_id_timestamp_and_provenance() {
    let src_dir = scratch("src-identity");
    let dst_dir = scratch("dst-identity");
    let src = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&src_dir)).unwrap();
    let dst = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dst_dir)).unwrap();
    src.store(record("keep-me", "the only record")).unwrap();

    let report = migrate(&src, &dst).unwrap();

    assert_eq!(report.migrated, 1);
    let moved = dst
        .get("keep-me")
        .unwrap()
        .expect("the record must exist in the destination under its original id");
    assert_eq!(moved.timestamp, "2026-08-08T12:00:00Z");
    assert_eq!(moved.provenance, "original provenance");
    assert_eq!(moved.summary, "the only record");
}

/// abbey never deletes: `invalidate` marks a record obsolete and keeps it for
/// provenance. A migration that silently drops obsolete records would convert
/// that guarantee into data loss at exactly the moment nobody is watching.
#[test]
fn obsolete_records_survive_the_migration_still_marked_obsolete() {
    let src_dir = scratch("src-obsolete");
    let dst_dir = scratch("dst-obsolete");
    let src = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&src_dir)).unwrap();
    let dst = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dst_dir)).unwrap();
    src.store(record("live", "still current")).unwrap();
    src.store(record("dead", "superseded long ago")).unwrap();
    src.invalidate("dead").unwrap();

    let report = migrate(&src, &dst).unwrap();

    assert_eq!(
        report.migrated, 2,
        "both records must cross, not just the live one"
    );
    let carried = dst
        .get("dead")
        .unwrap()
        .expect("an obsolete record must still be migrated");
    assert!(
        carried.obsolete,
        "the obsolete flag must survive, or invalidate silently becomes undo"
    );
}

/// Running a migration twice, or pointing it at a store that already holds
/// data, must fail loudly. Both backends `INSERT` rather than upsert, so the
/// second run would either error mid-way leaving a half-copy, or silently merge
/// two histories. Refusing up front is the only outcome that leaves the
/// destination in a state the operator predicted.
#[test]
fn a_destination_that_already_holds_records_is_refused_before_anything_is_written() {
    let src_dir = scratch("src-guard");
    let dst_dir = scratch("dst-guard");
    let src = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&src_dir)).unwrap();
    let dst = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dst_dir)).unwrap();
    src.store(record("incoming", "from the source")).unwrap();
    dst.store(record("resident", "already here")).unwrap();

    let error = migrate(&src, &dst).expect_err("a non-empty destination must be refused");

    assert!(
        error.to_string().contains("not empty"),
        "the error must say why, got: {error}"
    );
    assert!(
        dst.get("incoming").unwrap().is_none(),
        "nothing may be written when the migration is refused"
    );
}

/// A store that accepted a write is not the same as a store that can return it.
/// The evidence ladder in the Abbey System Constitution separates "execution
/// succeeded" (L3) from "postconditions verified" (L6); a migration that
/// reports only what it wrote is claiming L6 on L3 evidence. So every record is
/// read back and compared before the migration reports success.
#[test]
fn every_migrated_record_is_read_back_and_compared_not_just_counted() {
    let src_dir = scratch("src-verify");
    let dst_dir = scratch("dst-verify");
    let src = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&src_dir)).unwrap();
    let dst = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dst_dir)).unwrap();
    src.store(record("one", "first")).unwrap();
    src.store(record("two", "second")).unwrap();
    src.invalidate("two").unwrap();

    let report = migrate(&src, &dst).unwrap();

    assert_eq!(report.migrated, 2);
    assert_eq!(
        report.verified, 2,
        "every written record must be read back, obsolete ones included"
    );
}

/// The migration this command exists for: SQLite (the default backend, and the
/// one the installed binary has been writing to) into WDBX. The two backends
/// share no storage code, so a record surviving this crossing is the only real
/// evidence that switching `memory_backend` is non-destructive.
#[cfg(feature = "wdbx")]
#[test]
fn records_cross_from_sqlite_to_wdbx_intact_including_obsolete_ones() {
    use crate::memory::WdbxMemory;

    let src_dir = scratch("src-cross");
    let dst_dir = scratch("dst-cross");
    let src = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&src_dir)).unwrap();
    src.store(record("live-one", "kept and current")).unwrap();
    src.store(record("dead-one", "kept but obsolete")).unwrap();
    src.invalidate("dead-one").unwrap();

    let dst = WdbxMemory::open(&WdbxMemory::path_for_state_dir(&dst_dir)).unwrap();
    let report = migrate(&src, &dst).unwrap();

    assert_eq!((report.migrated, report.verified), (2, 2));
    let live = dst.get("live-one").unwrap().expect("live record crossed");
    assert_eq!(live.timestamp, "2026-08-08T12:00:00Z");
    assert_eq!(live.provenance, "original provenance");
    assert!(!live.obsolete);
    let dead = dst
        .get("dead-one")
        .unwrap()
        .expect("obsolete record crossed");
    assert!(dead.obsolete, "obsolete must survive a real backend change");
}

/// An operator about to change where 171 records live should be able to see the
/// count and the destination's emptiness check without committing to anything.
#[test]
fn a_dry_run_reports_what_would_move_and_writes_nothing() {
    let src_dir = scratch("src-dry");
    let dst_dir = scratch("dst-dry");
    let src = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&src_dir)).unwrap();
    let dst = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dst_dir)).unwrap();
    src.store(record("a", "first")).unwrap();
    src.store(record("b", "second")).unwrap();

    let report = plan(&src, &dst).unwrap();

    assert_eq!(report.migrated, 2, "the plan reports what would move");
    assert_eq!(
        report.verified, 0,
        "a plan verifies nothing because it wrote nothing"
    );
    assert!(dst.get("a").unwrap().is_none(), "a dry run must not write");
    assert!(dst.get("b").unwrap().is_none(), "a dry run must not write");
}
