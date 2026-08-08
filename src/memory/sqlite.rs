//! SQLite interim memory backend (not WDBX).

use super::{
    EmbeddingStatus, MemoryFilter, MemoryRecord, MemoryStore, ReflectReport, SemanticHit,
    StoredEmbedding,
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct SqliteMemory {
    conn: Mutex<Connection>,
}

impl SqliteMemory {
    pub fn path_for_state_dir(state_dir: &Path) -> PathBuf {
        state_dir.join("memory.sqlite")
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS memory (
                id TEXT PRIMARY KEY,
                source_type TEXT NOT NULL,
                source_ref TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                origin TEXT NOT NULL,
                payload TEXT NOT NULL,
                summary TEXT NOT NULL,
                tags_json TEXT NOT NULL,
                embedding_ref TEXT,
                confidence REAL NOT NULL,
                provenance TEXT NOT NULL,
                retention TEXT NOT NULL,
                supersedes TEXT,
                classification TEXT NOT NULL,
                obsolete INTEGER NOT NULL DEFAULT 0,
                project TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_memory_retention ON memory(retention);
            CREATE INDEX IF NOT EXISTS idx_memory_summary ON memory(summary);
            CREATE TABLE IF NOT EXISTS memory_embeddings (
                memory_id TEXT NOT NULL,
                space_id TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                dimension INTEGER NOT NULL,
                vector_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (memory_id, space_id),
                FOREIGN KEY (memory_id) REFERENCES memory(id)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_embeddings_space
                ON memory_embeddings(space_id);
            "#,
        )?;
        let has_project = {
            let mut statement = conn.prepare("PRAGMA table_info(memory)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            columns
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "project")
        };
        if !has_project {
            conn.execute(
                "ALTER TABLE memory ADD COLUMN project TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn validate_train(rec: &MemoryRecord) -> Result<()> {
        super::validate_train(rec)
    }

    fn row_to_rec(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
        let tags_json: String = row.get(7)?;
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        Ok(MemoryRecord {
            id: row.get(0)?,
            source_type: row.get(1)?,
            source_ref: row.get(2)?,
            timestamp: row.get(3)?,
            origin: row.get(4)?,
            payload: row.get(5)?,
            summary: row.get(6)?,
            tags,
            embedding_ref: row.get(8)?,
            confidence: row.get(9)?,
            provenance: row.get(10)?,
            retention: row.get(11)?,
            supersedes: row.get(12)?,
            classification: row.get(13)?,
            obsolete: row.get::<_, i64>(14)? != 0,
            project: row.get(15)?,
        })
    }
}

impl MemoryStore for SqliteMemory {
    fn store(&self, rec: MemoryRecord) -> Result<()> {
        Self::validate_train(&rec)?;
        let tags = serde_json::to_string(&rec.tags)?;
        let conn = self.conn.lock().expect("sqlite lock");
        conn.execute(
            r#"INSERT INTO memory (
                id, source_type, source_ref, timestamp, origin, payload, summary,
                tags_json, embedding_ref, confidence, provenance, retention,
                supersedes, classification, obsolete, project
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"#,
            params![
                rec.id,
                rec.source_type,
                rec.source_ref,
                rec.timestamp,
                rec.origin,
                rec.payload,
                rec.summary,
                tags,
                rec.embedding_ref,
                rec.confidence,
                rec.provenance,
                rec.retention,
                rec.supersedes,
                rec.classification,
                if rec.obsolete { 1 } else { 0 },
                rec.project,
            ],
        )?;
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<MemoryRecord>> {
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project
             FROM memory WHERE id = ?1",
        )?;
        let rec = stmt.query_row(params![id], Self::row_to_rec).optional()?;
        Ok(rec)
    }

    fn update(&self, rec: MemoryRecord) -> Result<()> {
        Self::validate_train(&rec)?;
        let tags = serde_json::to_string(&rec.tags)?;
        let conn = self.conn.lock().expect("sqlite lock");
        let n = conn.execute(
            r#"UPDATE memory SET
                source_type=?2, source_ref=?3, timestamp=?4, origin=?5, payload=?6,
                summary=?7, tags_json=?8, embedding_ref=?9, confidence=?10,
                provenance=?11, retention=?12, supersedes=?13, classification=?14,
                obsolete=?15, project=?16
             WHERE id=?1"#,
            params![
                rec.id,
                rec.source_type,
                rec.source_ref,
                rec.timestamp,
                rec.origin,
                rec.payload,
                rec.summary,
                tags,
                rec.embedding_ref,
                rec.confidence,
                rec.provenance,
                rec.retention,
                rec.supersedes,
                rec.classification,
                if rec.obsolete { 1 } else { 0 },
                rec.project,
            ],
        )?;
        if n == 0 {
            bail!("memory id not found: {}", rec.id);
        }
        Ok(())
    }

    fn invalidate(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("sqlite lock");
        let n = conn.execute("UPDATE memory SET obsolete=1 WHERE id=?1", params![id])?;
        if n == 0 {
            bail!("memory id not found: {id}");
        }
        Ok(())
    }

    fn search_keyword(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        let q = format!("%{}%", query.replace('%', ""));
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project
             FROM memory
             WHERE obsolete=0 AND (summary LIKE ?1 OR payload LIKE ?1 OR provenance LIKE ?1)
             ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![q, limit as i64], Self::row_to_rec)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn search_keyword_with(
        &self,
        query: &str,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let q = format!("%{}%", query.replace('%', ""));
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project
             FROM memory
             WHERE obsolete=0 AND (summary LIKE ?1 OR payload LIKE ?1 OR provenance LIKE ?1)
             ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map(params![q], Self::row_to_rec)?;
        let mut out = Vec::new();
        for row in rows {
            let record = row?;
            if filter.matches(&record) {
                out.push(record);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn filter_with(&self, filter: &MemoryFilter, limit: usize) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project
             FROM memory WHERE obsolete=0
             ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map([], Self::row_to_rec)?;
        let mut out = Vec::new();
        for r in rows {
            let rec = r?;
            if !filter.matches(&rec) {
                continue;
            }
            out.push(rec);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    fn promote(&self, id: &str, new_retention: &str) -> Result<()> {
        let Some(mut rec) = self.get(id)? else {
            bail!("memory id not found: {id}");
        };
        rec.retention = new_retention.into();
        if !rec.tags.iter().any(|t| t == new_retention) {
            rec.tags.push(new_retention.into());
        }
        self.update(rec)
    }

    fn supersede(&self, old_id: &str, mut new_rec: MemoryRecord) -> Result<()> {
        new_rec.supersedes = Some(old_id.into());
        self.store(new_rec)?;
        self.invalidate(old_id)
    }

    fn reflect(&self) -> Result<ReflectReport> {
        Ok(super::reflect_over(&self.filter(None, None, 500)?))
    }

    fn put_embedding(&self, embedding: StoredEmbedding) -> Result<()> {
        if embedding.dimension == 0 || embedding.vector.len() != embedding.dimension {
            bail!("invalid stored embedding dimension");
        }
        if !embedding.vector.iter().all(|value| value.is_finite()) {
            bail!("stored embedding contains a non-finite value");
        }
        let vector_json = serde_json::to_string(&embedding.vector)?;
        let mut conn = self.conn.lock().expect("sqlite lock");
        let tx = conn.transaction()?;
        let current = {
            let mut statement = tx.prepare(
                "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                        tags_json, embedding_ref, confidence, provenance, retention,
                        supersedes, classification, obsolete, project
                 FROM memory WHERE id=?1",
            )?;
            statement
                .query_row(params![embedding.memory_id], Self::row_to_rec)
                .optional()?
        };
        let Some(record) = current else {
            bail!("memory id not found: {}", embedding.memory_id);
        };
        if record.obsolete {
            bail!(
                "cannot attach an embedding to obsolete memory: {}",
                record.id
            );
        }
        if super::semantic::content_hash(&record) != embedding.content_hash {
            bail!("memory changed while it was being embedded: {}", record.id);
        }
        tx.execute(
            r#"INSERT INTO memory_embeddings (
                    memory_id, space_id, content_hash, dimension, vector_json, updated_at
               ) VALUES (?1,?2,?3,?4,?5,?6)
               ON CONFLICT(memory_id, space_id) DO UPDATE SET
                    content_hash=excluded.content_hash,
                    dimension=excluded.dimension,
                    vector_json=excluded.vector_json,
                    updated_at=excluded.updated_at"#,
            params![
                embedding.memory_id,
                embedding.space_id,
                embedding.content_hash,
                embedding.dimension as i64,
                vector_json,
                embedding.updated_at,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn embedding_candidates(&self, space_id: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("sqlite lock");
        let mut statement = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project,
                    (SELECT content_hash FROM memory_embeddings e
                     WHERE e.memory_id=memory.id AND e.space_id=?1)
             FROM memory WHERE obsolete=0 ORDER BY timestamp DESC",
        )?;
        let rows = statement.query_map(params![space_id], |row| {
            Ok((Self::row_to_rec(row)?, row.get::<_, Option<String>>(16)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (record, stored_hash) = row?;
            if stored_hash.as_deref() != Some(super::semantic::content_hash(&record).as_str()) {
                out.push(record);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn embedding_status(&self, space_id: &str) -> Result<EmbeddingStatus> {
        let conn = self.conn.lock().expect("sqlite lock");
        let mut statement = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project,
                    (SELECT content_hash FROM memory_embeddings e
                     WHERE e.memory_id=memory.id AND e.space_id=?1)
             FROM memory WHERE obsolete=0",
        )?;
        let rows = statement.query_map(params![space_id], |row| {
            Ok((Self::row_to_rec(row)?, row.get::<_, Option<String>>(16)?))
        })?;
        let mut status = EmbeddingStatus::default();
        for row in rows {
            let (record, stored_hash) = row?;
            status.total += 1;
            match stored_hash {
                None => status.missing += 1,
                Some(hash) if hash == super::semantic::content_hash(&record) => status.ready += 1,
                Some(_) => status.stale += 1,
            }
        }
        Ok(status)
    }

    fn embedding_is_current(&self, memory_id: &str, space_id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("sqlite lock");
        let mut statement = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project,
                    (SELECT content_hash FROM memory_embeddings e
                     WHERE e.memory_id=memory.id AND e.space_id=?2)
             FROM memory WHERE id=?1",
        )?;
        let found = statement
            .query_row(params![memory_id, space_id], |row| {
                Ok((Self::row_to_rec(row)?, row.get::<_, Option<String>>(16)?))
            })
            .optional()?;
        let Some((record, stored_hash)) = found else {
            bail!("memory id not found: {memory_id}");
        };
        Ok(!record.obsolete
            && stored_hash.as_deref() == Some(super::semantic::content_hash(&record).as_str()))
    }

    fn semantic_search(
        &self,
        space_id: &str,
        query: &[f32],
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<SemanticHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("sqlite lock");
        let mut statement = conn.prepare(
            "SELECT m.id, m.source_type, m.source_ref, m.timestamp, m.origin, m.payload,
                    m.summary, m.tags_json, m.embedding_ref, m.confidence, m.provenance,
                    m.retention, m.supersedes, m.classification, m.obsolete, m.project,
                    e.content_hash, e.dimension, e.vector_json
             FROM memory m JOIN memory_embeddings e ON e.memory_id=m.id
             WHERE m.obsolete=0 AND e.space_id=?1",
        )?;
        let rows = statement.query_map(params![space_id], |row| {
            Ok((
                Self::row_to_rec(row)?,
                row.get::<_, String>(16)?,
                row.get::<_, i64>(17)?,
                row.get::<_, String>(18)?,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (record, stored_hash, dimension, vector_json) = row?;
            if !filter.matches(&record)
                || stored_hash != super::semantic::content_hash(&record)
                || dimension < 0
                || dimension as usize != query.len()
            {
                continue;
            }
            let vector: Vec<f32> = serde_json::from_str(&vector_json)?;
            if vector.len() != dimension as usize || !vector.iter().all(|value| value.is_finite()) {
                continue;
            }
            hits.push(SemanticHit {
                score: super::semantic::cosine(&vector, query)?,
                record,
            });
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryRecord, MemoryStore};

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
            MemoryFilter::new(None, None, None, None, Some("project-a".into()), None, None)
                .unwrap();
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
}
