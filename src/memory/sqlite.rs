//! SQLite interim memory backend (not WDBX).

use super::{
    EmbeddingStatus, MemoryFilter, MemoryRecord, MemoryStore, ReflectReport, SemanticHit,
    StoredEmbedding,
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod migrations;

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
        let mut conn =
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
        migrations::ensure_embedding_schema(&mut conn)?;
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

    fn all_records_including_obsolete(&self) -> Result<Vec<MemoryRecord>> {
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete, project
             FROM memory
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_rec)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
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
        embedding.validate()?;
        let vector_blob = encode_vector(&embedding.vector);
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
                    record_id, space_id, provider, model, revision, normalization,
                    content_hash, dimension, vector_blob, updated_at
               ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
               ON CONFLICT(record_id, space_id) DO UPDATE SET
                    provider=excluded.provider,
                    model=excluded.model,
                    revision=excluded.revision,
                    normalization=excluded.normalization,
                    content_hash=excluded.content_hash,
                    dimension=excluded.dimension,
                    vector_blob=excluded.vector_blob,
                    updated_at=excluded.updated_at"#,
            params![
                embedding.memory_id,
                embedding.space_id,
                embedding.provider,
                embedding.model,
                embedding.revision,
                embedding.normalization,
                embedding.content_hash,
                embedding.dimension as i64,
                vector_blob,
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
                     WHERE e.record_id=memory.id AND e.space_id=?1)
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
                     WHERE e.record_id=memory.id AND e.space_id=?1)
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
                     WHERE e.record_id=memory.id AND e.space_id=?2)
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
                    e.content_hash, e.dimension, e.vector_blob
             FROM memory m JOIN memory_embeddings e ON e.record_id=m.id
             WHERE m.obsolete=0 AND e.space_id=?1",
        )?;
        let rows = statement.query_map(params![space_id], |row| {
            Ok((
                Self::row_to_rec(row)?,
                row.get::<_, String>(16)?,
                row.get::<_, i64>(17)?,
                row.get::<_, Vec<u8>>(18)?,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (record, stored_hash, dimension, vector_blob) = row?;
            if !filter.matches(&record)
                || stored_hash != super::semantic::content_hash(&record)
                || dimension < 0
                || dimension as usize != query.len()
            {
                continue;
            }
            let vector = decode_vector(&vector_blob, dimension as usize)?;
            hits.push(SemanticHit {
                score: super::semantic::cosine(&vector, query)?,
                record,
            });
        }
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.record.id.cmp(&b.record.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_vector(bytes: &[u8], dimension: usize) -> Result<Vec<f32>> {
    if bytes.len() != dimension.saturating_mul(size_of::<f32>()) {
        bail!("stored embedding BLOB length does not match its dimension");
    }
    let vector = bytes
        .as_chunks::<{ size_of::<f32>() }>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect::<Vec<_>>();
    if !vector.iter().all(|value| value.is_finite()) {
        bail!("stored embedding BLOB contains a non-finite value");
    }
    Ok(vector)
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod tests;
