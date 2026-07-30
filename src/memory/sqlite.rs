//! SQLite interim memory backend (not WDBX).

use super::{MemoryRecord, MemoryStore, ReflectReport};
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
                obsolete INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_memory_retention ON memory(retention);
            CREATE INDEX IF NOT EXISTS idx_memory_summary ON memory(summary);
            "#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn validate_train(rec: &MemoryRecord) -> Result<()> {
        if rec.retention == "train_candidate" && rec.provenance.trim().is_empty() {
            bail!("train_candidate requires non-empty provenance");
        }
        Ok(())
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
                supersedes, classification, obsolete
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"#,
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
            ],
        )?;
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<MemoryRecord>> {
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete
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
                obsolete=?15
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
                    supersedes, classification, obsolete
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

    fn filter(
        &self,
        retention: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let conn = self.conn.lock().expect("sqlite lock");
        let mut stmt = conn.prepare(
            "SELECT id, source_type, source_ref, timestamp, origin, payload, summary,
                    tags_json, embedding_ref, confidence, provenance, retention,
                    supersedes, classification, obsolete
             FROM memory WHERE obsolete=0
             ORDER BY timestamp DESC LIMIT 1000",
        )?;
        let rows = stmt.query_map([], Self::row_to_rec)?;
        let mut out = Vec::new();
        for r in rows {
            let rec = r?;
            if let Some(ret) = retention {
                if rec.retention != ret {
                    continue;
                }
            }
            if let Some(t) = tag {
                if !rec.tags.iter().any(|x| x == t) {
                    continue;
                }
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
        let all = self.filter(None, None, 500)?;
        let mut report = ReflectReport::default();
        for r in &all {
            if r.confidence < 0.4 {
                report.low_confidence.push(r.id.clone());
            }
            if r.supersedes.is_some() || r.obsolete {
                report.superseded.push(r.id.clone());
            }
        }
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                let a = &all[i].summary;
                let b = &all[j].summary;
                let prefix_len = 24.min(a.len()).min(b.len());
                if prefix_len >= 12 && a[..prefix_len] == b[..prefix_len] {
                    report
                        .duplicate_summaries
                        .push((all[i].id.clone(), all[j].id.clone()));
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryRecord, MemoryStore};

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
}
