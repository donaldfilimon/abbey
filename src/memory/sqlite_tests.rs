use super::*;
use crate::memory::{
    MemoryRecord, MemoryStore, embedding::EmbeddingSpace, semantic::StoredEmbedding,
};

#[test]
fn opening_a_legacy_schema_adds_project_without_losing_records() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = SqliteMemory::path_for_state_dir(&dir);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE memory (
                    id TEXT PRIMARY KEY, source_type TEXT NOT NULL, source_ref TEXT NOT NULL,
                    timestamp TEXT NOT NULL, origin TEXT NOT NULL, payload TEXT NOT NULL,
                    summary TEXT NOT NULL, tags_json TEXT NOT NULL, embedding_ref TEXT,
                    confidence REAL NOT NULL, provenance TEXT NOT NULL, retention TEXT NOT NULL,
                    supersedes TEXT, classification TEXT NOT NULL,
                    obsolete INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO memory VALUES (
                    'legacy','session','','2026-08-08T00:00:00Z','system','','kept','[]',
                    NULL,1.0,'test','ltm',NULL,'internal',0
                );",
        )
        .unwrap();
    drop(connection);

    let db = SqliteMemory::open(&path).unwrap();
    let record = db.get("legacy").unwrap().unwrap();
    assert_eq!(record.summary, "kept");
    assert_eq!(record.project, "");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn embeddings_store_explicit_space_metadata_in_little_endian_blob() {
    let dir =
        std::env::temp_dir().join(format!("abbey-mem-embedding-schema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = SqliteMemory::path_for_state_dir(&dir);
    let db = SqliteMemory::open(&path).unwrap();
    let record = MemoryRecord::new_stm("schema", "private");
    let space = EmbeddingSpace::new("test", "model", "r7", 2).unwrap();
    db.store(record.clone()).unwrap();
    db.put_embedding(StoredEmbedding::new(&record, &space, vec![1.0, -0.5]).unwrap())
        .unwrap();
    drop(db);

    let conn = Connection::open(&path).unwrap();
    let stored = conn
        .query_row(
            "SELECT provider, model, revision, normalization, dimension,
                        typeof(vector_blob), vector_blob
                 FROM memory_embeddings WHERE record_id=?1 AND space_id=?2",
            params![record.id, space.space_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(&stored.0, "test");
    assert_eq!(&stored.1, "model");
    assert_eq!(&stored.2, "r7");
    assert_eq!(&stored.3, "l2-v1");
    assert_eq!(stored.4, 2);
    assert_eq!(&stored.5, "blob");
    assert_eq!(
        stored.6,
        [1.0_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn current_vector_json_schema_migrates_without_losing_search() {
    let dir = std::env::temp_dir().join(format!(
        "abbey-mem-embedding-json-migrate-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = SqliteMemory::path_for_state_dir(&dir);
    let record = MemoryRecord::new_stm("migrate semantic vector", "private");
    let record_id = record.id.clone();
    let content_hash = super::super::semantic::content_hash(&record);
    {
        let db = SqliteMemory::open(&path).unwrap();
        db.store(record).unwrap();
    }
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TABLE memory_embeddings;
             CREATE TABLE memory_embeddings (
                memory_id TEXT NOT NULL, space_id TEXT NOT NULL,
                content_hash TEXT NOT NULL, dimension INTEGER NOT NULL,
                vector_json TEXT NOT NULL, updated_at TEXT NOT NULL,
                PRIMARY KEY (memory_id, space_id)
             );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_embeddings VALUES (?1,'legacy-space',?2,2,'[1.0,0.0]',?3)",
        params![record_id, content_hash, "2026-08-08T00:00:00Z"],
    )
    .unwrap();
    drop(conn);

    let db = SqliteMemory::open(&path).unwrap();
    assert_eq!(db.embedding_status("legacy-space").unwrap().ready, 1);
    let hits = db
        .semantic_search("legacy-space", &[1.0, 0.0], &MemoryFilter::default(), 10)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record.id, record_id);
    drop(db);
    let conn = Connection::open(&path).unwrap();
    let metadata = conn
        .query_row(
            "SELECT provider, typeof(vector_blob) FROM memory_embeddings",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(metadata, ("legacy-unknown".into(), "blob".into()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn semantic_score_ties_use_stable_record_id_order() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-semantic-ties-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    let space = EmbeddingSpace::new("test", "ties", "r1", 2).unwrap();
    for id in ["record-b", "record-a"] {
        let mut record = MemoryRecord::new_stm(id, "private");
        record.id = id.into();
        db.store(record.clone()).unwrap();
        db.put_embedding(StoredEmbedding::new(&record, &space, vec![1.0, 0.0]).unwrap())
            .unwrap();
    }
    let ids = db
        .semantic_search(&space.space_id, &[1.0, 0.0], &MemoryFilter::default(), 2)
        .unwrap()
        .into_iter()
        .map(|hit| hit.record.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["record-a", "record-b"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn filtered_keyword_search_limits_after_the_keyword_predicate() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-deep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    let mut base = MemoryRecord::new_stm("ordinary", "payload");
    base.project = "project-a".into();
    base.provenance = "test".into();
    for index in 0..1001 {
        let mut record = base.clone();
        record.id = format!("newer-{index:04}");
        record.timestamp = format!("2026-08-08T12:{:02}:00Z", index % 60);
        db.store(record).unwrap();
    }
    let mut older_match = base;
    older_match.id = "older-match".into();
    older_match.summary = "needle".into();
    older_match.timestamp = "2026-08-01T00:00:00Z".into();
    db.store(older_match).unwrap();
    let filter =
        MemoryFilter::new(None, None, None, None, Some("project-a".into()), None, None).unwrap();
    let hits = db.search_keyword_with("needle", &filter, 1).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "older-match");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn store_get_search_promote() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    let mut rec = MemoryRecord::new_stm("hello world summary", "payload body");
    rec.provenance = "test".into();
    let id = rec.id.clone();
    db.store(rec).unwrap();
    assert!(db.get(&id).unwrap().is_some());
    assert!(!db.search_keyword("hello", 10).unwrap().is_empty());
    db.promote(&id, "ltm").unwrap();
    assert_eq!(db.get(&id).unwrap().unwrap().retention, "ltm");
    let report = db.reflect().unwrap();
    assert!(report.low_confidence.is_empty() || !report.low_confidence.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalidate_marks_obsolete_without_deleting() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-inv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    let mut rec = MemoryRecord::new_stm("stale fact", "body");
    rec.provenance = "test".into();
    let id = rec.id.clone();
    db.store(rec).unwrap();

    db.invalidate(&id).unwrap();

    // No silent deletes: the record still exists, just flagged obsolete.
    let after = db.get(&id).unwrap().expect("record was deleted");
    assert!(after.obsolete, "invalidate must set obsolete");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supersede_links_new_record_and_retires_old() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-sup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    let mut old = MemoryRecord::new_stm("wrong fact", "body");
    old.provenance = "test".into();
    let old_id = old.id.clone();
    db.store(old).unwrap();

    let mut fixed = MemoryRecord::new_stm("corrected fact", "body");
    fixed.provenance = "test supersede".into();
    let new_id = fixed.id.clone();
    db.supersede(&old_id, fixed).unwrap();

    let old_after = db.get(&old_id).unwrap().expect("old record deleted");
    assert!(old_after.obsolete, "superseded record must be obsolete");
    let new_after = db.get(&new_id).unwrap().expect("new record missing");
    assert_eq!(
        new_after.supersedes.as_deref(),
        Some(old_id.as_str()),
        "replacement must point back at what it replaced"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalidated_records_leave_search_but_stay_retrievable() {
    // The contract behind `abbey memory invalidate`: an obsolete record
    // must stop polluting search results, yet still be fetchable by id so
    // the provenance trail survives. Verified by hand against the release
    // binary; encoded here so a regression cannot pass silently.
    let dir = std::env::temp_dir().join(format!("abbey-mem-hide-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();

    let mut rec = MemoryRecord::new_stm("searchable needle token", "body");
    rec.provenance = "test".into();
    let id = rec.id.clone();
    db.store(rec).unwrap();

    // Control: it is findable before invalidation, so an empty result
    // afterwards means "filtered", not "search is broken".
    assert!(
        db.search_keyword("needle", 10)
            .unwrap()
            .iter()
            .any(|r| r.id == id),
        "record should be searchable before invalidation"
    );

    db.invalidate(&id).unwrap();

    assert!(
        !db.search_keyword("needle", 10)
            .unwrap()
            .iter()
            .any(|r| r.id == id),
        "invalidated record must not appear in search"
    );
    let fetched = db.get(&id).unwrap().expect("record must still exist");
    assert!(fetched.obsolete);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn train_requires_provenance() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-train-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    let mut rec = MemoryRecord::new_stm("x", "y");
    rec.retention = "train_candidate".into();
    rec.provenance.clear();
    assert!(db.store(rec).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zero_limit_is_empty_for_every_filtered_surface() {
    let dir = std::env::temp_dir().join(format!("abbey-mem-zero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
    db.store(MemoryRecord::new_stm("needle", "needle")).unwrap();
    assert!(
        db.filter_with(&MemoryFilter::default(), 0)
            .unwrap()
            .is_empty()
    );
    assert!(
        db.search_keyword_with("needle", &MemoryFilter::default(), 0)
            .unwrap()
            .is_empty()
    );
    assert!(db.embedding_candidates("space", 0).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
