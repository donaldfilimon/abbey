//! In-process WDBX memory backend (feature `wdbx`).
//!
//! Records are stored as JSON in the WDBX durable KV space under `mem/<id>`.
//! Writes land in the CRC-framed WAL immediately; `checkpoint` folds the WAL
//! into a segment. Nothing is ever deleted — `invalidate` marks `obsolete`,
//! matching the SQLite backend and the no-silent-deletes rule.
//!
//! ## Why there is a lock file here
//!
//! `DurableStore` has no cross-process concurrency control: each process
//! recovers its own in-memory snapshot and appends to the shared WAL. Twenty
//! `abbey` processes writing at once interleave their appends and leave the WAL
//! permanently unreadable ("CRC mismatch at line 2" — every later open fails and
//! the whole store reads as empty). SQLite survives the same load via file
//! locking, so a WDBX backend without a lock is not a safe substitute.
//!
//! An exclusive advisory lock on `<dir>/abbey.lock` (via `fs4`: `flock(2)` on
//! Unix, `LockFileEx` on Windows) serializes whole open→write→drop sessions.
//! The OS drops the lock when the handle closes, including on process death, so
//! a crashed process cannot wedge the store the way a lock *file* would.

use super::{
    EmbeddingStatus, MemoryFilter, MemoryRecord, MemoryStore, ReflectReport, SemanticHit,
    StoredEmbedding,
};
use abi_wdbx::{DurableStore, RecordId, StorePaths, VersionedStore};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// KV key namespace for Abbey memory records inside a shared WDBX store.
const KEY_PREFIX: &str = "mem/";
const EMBEDDING_MAP_PREFIX: &str = "map/";

/// How long to wait for another process to finish before giving up.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

pub struct WdbxMemory {
    // Field order is load-bearing: `store` drops (flushing) before `_lock`
    // releases, so no other process can observe a half-finished session.
    store: Mutex<DurableStore>,
    dir: PathBuf,
    _lock: File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorMapping {
    vector_id: RecordId,
    embedding: StoredEmbedding,
}

impl WdbxMemory {
    /// Store directory for a given Abbey state dir.
    pub fn path_for_state_dir(state_dir: &Path) -> PathBuf {
        state_dir.join("wdbx")
    }

    pub fn open(dir: &Path) -> Result<Self> {
        Self::open_with_timeout(dir, LOCK_TIMEOUT)
    }

    pub fn open_with_timeout(dir: &Path, timeout: Duration) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        // Must be held before recovery: `DurableStore::open` reads the WAL.
        let lock = lock_exclusive(&dir.join("abbey.lock"), timeout)?;
        let store = DurableStore::open_directory(dir)
            .map_err(|e| anyhow!("open wdbx store {}: {e}", dir.display()))?;
        Ok(Self {
            store: Mutex::new(store),
            dir: dir.to_path_buf(),
            _lock: lock,
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
        // Coordinates live in the JSON record; Abbey's `near` recomputes them
        // via `memory::map::nearest_to`. DurableStore can `put_spatial` but has no
        // public nearest query, so a dual write would be dead weight.
        store
            .put(&Self::key(&rec.id), &json)
            .map_err(|e| anyhow!("wdbx put {}: {e}", rec.id))
    }

    fn space_dir(&self, space_id: &str) -> Result<PathBuf> {
        if space_id.is_empty()
            || !space_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            bail!("invalid embedding space id: {space_id:?}");
        }
        // Never open the abandoned legacy semantic sub-stores here. Their v1
        // HNSW width cap rejected real Apple 512-d vectors. The sibling v2
        // namespace is intentionally fresh and old directories remain intact.
        Ok(self.dir.join("embedding-spaces-v2").join(space_id))
    }

    fn open_space(&self, space_id: &str, create: bool) -> Result<Option<VersionedStore>> {
        let path = self.space_dir(space_id)?;
        if !create && !path.exists() {
            return Ok(None);
        }
        std::fs::create_dir_all(&path)?;
        VersionedStore::open(StorePaths::new(&path))
            .map(Some)
            .map_err(|error| anyhow!("open WDBX embedding space {}: {error}", path.display()))
    }

    fn mappings(
        space: &VersionedStore,
        records: &[MemoryRecord],
    ) -> HashMap<String, VectorMapping> {
        // V2 deliberately has no public KV iterator. Authoritative memory IDs
        // bound this scan and orphan mappings are ignored by construction.
        records
            .iter()
            .filter_map(|record| {
                space
                    .get(&format!("{EMBEDDING_MAP_PREFIX}{}", record.id))
                    .and_then(|value| serde_json::from_str::<VectorMapping>(&value).ok())
                    .map(|mapping| (record.id.clone(), mapping))
            })
            .collect()
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

/// Lock a store directory without opening it — used to hold Abbey's lock across
/// an `abi wdbx` subprocess that would otherwise ignore it.
pub fn lock_store_dir(dir: &Path, timeout: Duration) -> Result<File> {
    std::fs::create_dir_all(dir)?;
    lock_exclusive(&dir.join("abbey.lock"), timeout)
}

/// Take an exclusive advisory lock, retrying until `timeout` elapses.
///
/// Uses `fs4` (maintained fork of the abandoned `fs2`) so Linux / macOS /
/// Windows / other Unix share one path.
fn lock_exclusive(path: &Path, timeout: Duration) -> Result<File> {
    use fs4::fs_std::FileExt;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    let deadline = Instant::now() + timeout;
    loop {
        // fs4 reports contention as Ok(false) rather than an errno to match.
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(file),
            Ok(false) => {}
            Err(err) if is_lock_busy(&err) => {}
            Err(err) => return Err(anyhow!("lock {}: {err}", path.display())),
        }
        if Instant::now() >= deadline {
            bail!(
                "wdbx store is locked by another abbey process ({}s). \
                 Retry, or use the sqlite backend for concurrent use.",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn is_lock_busy(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    // EAGAIN/EWOULDBLOCK (Unix) · ERROR_LOCK_VIOLATION (Windows)
    matches!(err.raw_os_error(), Some(11 | 35 | 33))
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

    fn search_keyword_with(
        &self,
        query: &str,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let needle = query.to_ascii_lowercase();
        Ok(self
            .scan()
            .into_iter()
            .filter(|record| !record.obsolete && filter.matches(record))
            .filter(|record| {
                record.summary.to_ascii_lowercase().contains(&needle)
                    || record.payload.to_ascii_lowercase().contains(&needle)
                    || record.provenance.to_ascii_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }

    fn filter_with(&self, filter: &MemoryFilter, limit: usize) -> Result<Vec<MemoryRecord>> {
        Ok(self
            .scan()
            .into_iter()
            .filter(|r| !r.obsolete)
            .filter(|record| filter.matches(record))
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

    fn put_embedding(&self, embedding: StoredEmbedding) -> Result<()> {
        embedding.validate()?;
        let Some(record) = self.get(&embedding.memory_id)? else {
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
        // One WDBX v2 store per exact semantic space gives each model/dimension
        // an independent <=4096-d HNSW graph. Updates append a vector then move
        // the current mapping; old IDs remain history but are never returned.
        let mut space = self
            .open_space(&embedding.space_id, true)?
            .expect("create=true always opens a store");
        let snapshot = space.snapshot();
        if snapshot.vector_count() != 0 && snapshot.vector_dimensions() != Some(embedding.dimension)
        {
            bail!(
                "WDBX semantic space {} has dimension {:?}; embedding requires {}",
                embedding.space_id,
                snapshot.vector_dimensions(),
                embedding.dimension
            );
        }
        let vector_id = space
            .put_vector(&embedding.vector)
            .map_err(|error| anyhow!("put WDBX semantic vector: {error}"))?;
        let mapping = VectorMapping {
            vector_id,
            embedding,
        };
        let value = serde_json::to_string(&mapping)?;
        space
            .put(
                &format!("{EMBEDDING_MAP_PREFIX}{}", mapping.embedding.memory_id),
                &value,
            )
            .map_err(|error| anyhow!("put WDBX semantic vector mapping: {error}"))?;
        Ok(())
    }

    fn embedding_candidates(&self, space_id: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let records = self.scan();
        let mappings = self
            .open_space(space_id, false)?
            .as_ref()
            .map(|space| Self::mappings(space, &records))
            .unwrap_or_default();
        Ok(records
            .into_iter()
            .filter(|record| !record.obsolete)
            .filter(|record| {
                mappings.get(&record.id).is_none_or(|mapping| {
                    mapping.embedding.content_hash != super::semantic::content_hash(record)
                })
            })
            .take(limit)
            .collect())
    }

    fn embedding_status(&self, space_id: &str) -> Result<EmbeddingStatus> {
        let records = self.scan();
        let mappings = self
            .open_space(space_id, false)?
            .as_ref()
            .map(|space| Self::mappings(space, &records))
            .unwrap_or_default();
        let mut status = EmbeddingStatus::default();
        for record in records.into_iter().filter(|record| !record.obsolete) {
            status.total += 1;
            match mappings.get(&record.id) {
                None => status.missing += 1,
                Some(mapping)
                    if mapping.embedding.content_hash == super::semantic::content_hash(&record) =>
                {
                    status.ready += 1;
                }
                Some(_) => status.stale += 1,
            }
        }
        Ok(status)
    }

    fn embedding_is_current(&self, memory_id: &str, space_id: &str) -> Result<bool> {
        let Some(record) = self.get(memory_id)? else {
            bail!("memory id not found: {memory_id}");
        };
        if record.obsolete {
            return Ok(false);
        }
        let mapping = self
            .open_space(space_id, false)?
            .as_ref()
            .and_then(|space| space.get(&format!("{EMBEDDING_MAP_PREFIX}{memory_id}")))
            .and_then(|value| serde_json::from_str::<VectorMapping>(&value).ok());
        Ok(mapping.is_some_and(|mapping| {
            mapping.embedding.content_hash == super::semantic::content_hash(&record)
        }))
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
        let Some(space) = self.open_space(space_id, false)? else {
            return Ok(Vec::new());
        };
        let records = self.scan();
        let mappings = Self::mappings(&space, &records);
        let current_by_vector = mappings
            .values()
            .map(|mapping| (mapping.vector_id, mapping))
            .collect::<HashMap<_, _>>();
        let records = records
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect::<HashMap<_, _>>();
        // Ask for every indexed vector so post-search project/source/staleness
        // filters cannot hide an eligible older candidate behind stale history.
        let indexed = space.stats().vectors;
        let results = space
            .search(query, indexed)
            .map_err(|error| anyhow!("search WDBX semantic space: {error}"))?;
        let mut hits = Vec::new();
        for result in results {
            let Some(mapping) = current_by_vector.get(&result.id) else {
                continue;
            };
            let Some(record) = records.get(&mapping.embedding.memory_id) else {
                continue;
            };
            if record.obsolete
                || !filter.matches(record)
                || mapping.embedding.space_id != space_id
                || mapping.embedding.dimension != query.len()
                || mapping.embedding.content_hash != super::semantic::content_hash(record)
            {
                continue;
            }
            hits.push(SemanticHit {
                score: result.score,
                record: record.clone(),
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
    fn second_handle_is_locked_out_while_the_first_lives() {
        let dir = tmp("lock");
        let first = WdbxMemory::open(&dir).unwrap();

        let err = match WdbxMemory::open_with_timeout(&dir, Duration::from_millis(50)) {
            Ok(_) => panic!("a second concurrent handle must not open the store"),
            Err(e) => e,
        };
        assert!(
            format!("{err:#}").contains("locked by another abbey process"),
            "unexpected error: {err:#}"
        );

        drop(first);
        // Once the lock is released the store opens normally again.
        assert!(
            WdbxMemory::open_with_timeout(&dir, Duration::from_millis(500)).is_ok(),
            "store must reopen after the lock is released"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The TUI redraw calls `open_backend_with_timeout`; if a short timeout were
    /// ignored, a locked store would freeze the render loop for the full default.
    #[test]
    fn a_short_timeout_fails_fast_instead_of_waiting_the_default() {
        let state_dir = tmp("fastfail");
        let held = WdbxMemory::open(&WdbxMemory::path_for_state_dir(&state_dir)).unwrap();

        let start = Instant::now();
        let result = crate::memory::open_backend_with_timeout(
            &state_dir,
            "wdbx",
            Duration::from_millis(100),
        );
        let elapsed = start.elapsed();

        assert!(result.is_err(), "a locked store must not open");
        assert!(
            elapsed < Duration::from_secs(2),
            "short timeout was ignored: waited {elapsed:?}"
        );
        drop(held);
        let _ = std::fs::remove_dir_all(&state_dir);
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

    #[test]
    fn shared_source_project_and_timestamp_filters_apply_after_reopen() {
        let dir = tmp("filters");
        let mut record = MemoryRecord::new_stm("filtered", "body");
        record.provenance = "test".into();
        record.source_type = "import".into();
        record.source_ref = "bundle-1".into();
        record.project = "project-a".into();
        record.timestamp = "2026-08-08T12:00:00Z".into();
        {
            let db = WdbxMemory::open(&dir).unwrap();
            db.store(record).unwrap();
            db.checkpoint().unwrap();
        }
        let db = WdbxMemory::open(&dir).unwrap();
        let filter = MemoryFilter::new(
            None,
            None,
            Some("import".into()),
            Some("bundle-1".into()),
            Some("project-a".into()),
            Some("2026-08-08T12:00:00Z".into()),
            Some("2026-08-08T12:00:00Z".into()),
        )
        .unwrap();
        assert_eq!(db.filter_with(&filter, 10).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apple_sized_512d_semantic_vector_survives_reopen_and_searches() {
        use crate::memory::{embedding::EmbeddingSpace, semantic::StoredEmbedding};

        let dir = tmp("semantic-512-reopen");
        let space = EmbeddingSpace::new("apple", "sentence:en", "r1", 512).unwrap();
        let mut record = MemoryRecord::new_stm("semantic durable", "private payload");
        record.tags.push("storage".into());
        let id = record.id.clone();
        let mut vector = vec![0.0; 512];
        vector[0] = 1.0;
        let legacy_dir = dir.join("embedding-spaces").join(&space.space_id);
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("sentinel"), b"leave legacy untouched").unwrap();
        {
            let db = WdbxMemory::open(&dir).unwrap();
            db.store(record.clone()).unwrap();
            db.put_embedding(StoredEmbedding::new(&record, &space, vector.clone()).unwrap())
                .unwrap();
            db.checkpoint().unwrap();
        }
        let db = WdbxMemory::open(&dir).unwrap();
        assert_eq!(db.embedding_status(&space.space_id).unwrap().ready, 1);
        let hits = db
            .semantic_search(&space.space_id, &vector, &MemoryFilter::default(), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.id, id);
        assert!(
            dir.join("embedding-spaces-v2")
                .join(&space.space_id)
                .is_dir()
        );
        assert!(
            legacy_dir.join("sentinel").is_file(),
            "new semantic writes must not migrate or remove the legacy namespace"
        );
        assert_eq!(
            std::fs::read(legacy_dir.join("sentinel")).unwrap(),
            b"leave legacy untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn semantic_space_isolation_and_staleness_survive_reopen() {
        use crate::memory::{embedding::EmbeddingSpace, semantic::StoredEmbedding};

        let dir = tmp("semantic-isolation");
        let first = EmbeddingSpace::new("test", "model-a", "r1", 2).unwrap();
        let second = EmbeddingSpace::new("test", "model-b", "r1", 2).unwrap();
        let mut record = MemoryRecord::new_stm("original summary", "private payload");
        let id = record.id.clone();
        {
            let db = WdbxMemory::open(&dir).unwrap();
            db.store(record.clone()).unwrap();
            db.put_embedding(StoredEmbedding::new(&record, &first, vec![1.0, 0.0]).unwrap())
                .unwrap();
        }
        record.summary = "changed summary".into();
        {
            let db = WdbxMemory::open(&dir).unwrap();
            db.update(record).unwrap();
        }
        let db = WdbxMemory::open(&dir).unwrap();
        assert_eq!(db.embedding_status(&first.space_id).unwrap().stale, 1);
        assert_eq!(db.embedding_status(&second.space_id).unwrap().missing, 1);
        assert!(
            db.semantic_search(&first.space_id, &[1.0, 0.0], &MemoryFilter::default(), 10,)
                .unwrap()
                .is_empty()
        );
        assert_eq!(db.get(&id).unwrap().unwrap().summary, "changed summary");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mixed_dimensions_fail_before_poisoning_an_existing_space() {
        use crate::memory::{embedding::EmbeddingSpace, semantic::StoredEmbedding};

        let dir = tmp("semantic-mixed-dimensions");
        let space = EmbeddingSpace::new("test", "model-a", "r1", 2).unwrap();
        let record = MemoryRecord::new_stm("dimension guard", "private payload");
        let id = record.id.clone();
        let db = WdbxMemory::open(&dir).unwrap();
        db.store(record.clone()).unwrap();
        db.put_embedding(StoredEmbedding::new(&record, &space, vec![1.0, 0.0]).unwrap())
            .unwrap();

        let mut invalid = StoredEmbedding::new(&record, &space, vec![1.0, 0.0]).unwrap();
        invalid.dimension = 3;
        invalid.vector = vec![1.0, 0.0, 0.0];
        assert!(db.put_embedding(invalid).is_err());

        let hits = db
            .semantic_search(&space.space_id, &[1.0, 0.0], &MemoryFilter::default(), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.id, id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn semantic_score_ties_use_stable_record_id_order() {
        use crate::memory::{embedding::EmbeddingSpace, semantic::StoredEmbedding};

        let dir = tmp("semantic-ties");
        let db = WdbxMemory::open(&dir).unwrap();
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
    fn zero_limit_matches_sqlite() {
        let dir = tmp("zero");
        let db = WdbxMemory::open(&dir).unwrap();
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
        assert!(
            db.embedding_candidates("sem-v1-test", 0)
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
