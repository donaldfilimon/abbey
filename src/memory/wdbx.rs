//! In-process WDBX memory backend (feature `wdbx`).
//!
//! Records are stored as JSON in the WDBX durable KV space under `mem/<id>`.
//! Writes land in the CRC-framed WAL immediately; `checkpoint` folds the WAL
//! into a segment. Nothing is ever deleted — `invalidate` marks `obsolete`,
//! matching the SQLite backend and the no-silent-deletes rule.

use super::{MemoryRecord, MemoryStore, ReflectReport};
use abi_wdbx::DurableStore;
use anyhow::{Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// KV key namespace for Abbey memory records inside a shared WDBX store.
const KEY_PREFIX: &str = "mem/";

/// Scan ceiling, mirroring `SqliteMemory::filter`'s `LIMIT 1000`.
const SCAN_LIMIT: usize = 1000;

pub struct WdbxMemory {
    store: Mutex<DurableStore>,
    dir: PathBuf,
}

impl WdbxMemory {
    /// Store directory for a given Abbey state dir.
    pub fn path_for_state_dir(state_dir: &Path) -> PathBuf {
        state_dir.join("wdbx")
    }

    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let store = DurableStore::open_directory(dir)
            .map_err(|e| anyhow!("open wdbx store {}: {e}", dir.display()))?;
        Ok(Self {
            store: Mutex::new(store),
            dir: dir.to_path_buf(),
        })
    }

    fn key(id: &str) -> String {
        format!("{KEY_PREFIX}{id}")
    }

    fn write(&self, rec: &MemoryRecord) -> Result<()> {
        super::validate_train(rec)?;
        if rec.id.trim().is_empty() {
            bail!("memory record id must not be empty");
        }
        let json = serde_json::to_string(rec)?;
        let mut store = self.store.lock().expect("wdbx lock");
        store
            .put(&Self::key(&rec.id), &json)
            .map_err(|e| anyhow!("wdbx put {}: {e}", rec.id))
    }

    /// All live records, newest first — the shared basis for search/filter/reflect.
    fn scan(&self) -> Vec<MemoryRecord> {
        let store = self.store.lock().expect("wdbx lock");
        let mut out: Vec<MemoryRecord> = store
            .snapshot()
            .kv
            .iter()
            .filter(|(k, _)| k.starts_with(KEY_PREFIX))
            .filter_map(|(_, v)| serde_json::from_str::<MemoryRecord>(v).ok())
            .collect();
        // BTreeMap iterates by key (uuid) — impose the SQLite backend's ordering.
        out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        out.truncate(SCAN_LIMIT);
        out
    }

    /// Fold the WAL into a checkpoint segment. Returns the checkpoint epoch.
    pub fn checkpoint(&self) -> Result<u64> {
        let mut store = self.store.lock().expect("wdbx lock");
        store
            .checkpoint()
            .map_err(|e| anyhow!("wdbx checkpoint: {e}"))
    }

    /// Store path plus `kv_entries / vectors / blocks` counts, for `abbey wdbx stats`.
    pub fn stats_line(&self) -> String {
        let store = self.store.lock().expect("wdbx lock");
        let s = store.stats();
        format!(
            "{} kv={} vectors={} blocks={} epochs={}",
            self.dir.display(),
            s.kv_entries,
            s.vectors,
            s.blocks,
            s.epochs_loaded
        )
    }
}

impl MemoryStore for WdbxMemory {
    fn store(&self, rec: MemoryRecord) -> Result<()> {
        self.write(&rec)
    }

    fn get(&self, id: &str) -> Result<Option<MemoryRecord>> {
        let store = self.store.lock().expect("wdbx lock");
        let Some(raw) = store.get(&Self::key(id)) else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(raw)?))
    }

    fn update(&self, rec: MemoryRecord) -> Result<()> {
        if self.get(&rec.id)?.is_none() {
            bail!("memory id not found: {}", rec.id);
        }
        self.write(&rec)
    }

    fn invalidate(&self, id: &str) -> Result<()> {
        let Some(mut rec) = self.get(id)? else {
            bail!("memory id not found: {id}");
        };
        rec.obsolete = true;
        self.write(&rec)
    }

    fn search_keyword(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        let needle = query.to_ascii_lowercase();
        Ok(self
            .scan()
            .into_iter()
            .filter(|r| !r.obsolete)
            .filter(|r| {
                r.summary.to_ascii_lowercase().contains(&needle)
                    || r.payload.to_ascii_lowercase().contains(&needle)
                    || r.provenance.to_ascii_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }

    fn filter(
        &self,
        retention: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        Ok(self
            .scan()
            .into_iter()
            .filter(|r| !r.obsolete)
            .filter(|r| retention.is_none_or(|ret| r.retention == ret))
            .filter(|r| tag.is_none_or(|t| r.tags.iter().any(|x| x == t)))
            .take(limit)
            .collect())
    }

    fn promote(&self, id: &str, new_retention: &str) -> Result<()> {
        let Some(mut rec) = self.get(id)? else {
            bail!("memory id not found: {id}");
        };
        rec.retention = new_retention.into();
        if !rec.tags.iter().any(|t| t == new_retention) {
            rec.tags.push(new_retention.into());
        }
        self.write(&rec)
    }

    fn supersede(&self, old_id: &str, mut new_rec: MemoryRecord) -> Result<()> {
        new_rec.supersedes = Some(old_id.into());
        self.write(&new_rec)?;
        self.invalidate(old_id)
    }

    fn reflect(&self) -> Result<ReflectReport> {
        Ok(super::reflect_over(&self.filter(None, None, 500)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryRecord;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("abbey-wdbx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn store_get_search_promote_roundtrip() {
        let dir = tmp("rt");
        let db = WdbxMemory::open(&dir).unwrap();
        let mut rec = MemoryRecord::new_stm("hello world summary", "payload body");
        rec.provenance = "test".into();
        let id = rec.id.clone();

        db.store(rec).unwrap();
        assert!(db.get(&id).unwrap().is_some());
        assert_eq!(db.search_keyword("hello", 10).unwrap().len(), 1);
        assert_eq!(db.search_keyword("nonexistent", 10).unwrap().len(), 0);

        db.promote(&id, "ltm").unwrap();
        assert_eq!(db.get(&id).unwrap().unwrap().retention, "ltm");
        assert_eq!(db.filter(Some("ltm"), None, 10).unwrap().len(), 1);
        assert_eq!(db.filter(Some("stm"), None, 10).unwrap().len(), 0);

        db.invalidate(&id).unwrap();
        assert!(db.get(&id).unwrap().unwrap().obsolete, "record is kept");
        assert_eq!(db.search_keyword("hello", 10).unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn survives_reopen_via_wal() {
        let dir = tmp("reopen");
        let mut rec = MemoryRecord::new_stm("durable across reopen", "body");
        rec.provenance = "test".into();
        let id = rec.id.clone();
        {
            let db = WdbxMemory::open(&dir).unwrap();
            db.store(rec).unwrap();
            db.checkpoint().unwrap();
        }
        let db = WdbxMemory::open(&dir).unwrap();
        let got = db.get(&id).unwrap().expect("record recovered after reopen");
        assert_eq!(got.summary, "durable across reopen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn train_requires_provenance() {
        let dir = tmp("train");
        let db = WdbxMemory::open(&dir).unwrap();
        let mut rec = MemoryRecord::new_stm("x", "y");
        rec.retention = "train_candidate".into();
        rec.provenance.clear();
        assert!(db.store(rec).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_requires_existing_record() {
        let dir = tmp("update");
        let db = WdbxMemory::open(&dir).unwrap();
        let mut rec = MemoryRecord::new_stm("ghost", "body");
        rec.provenance = "test".into();
        assert!(db.update(rec).is_err(), "update must not create records");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
