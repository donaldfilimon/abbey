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
//! `flock(2)` on `<dir>/abbey.lock` serializes whole open→write→drop sessions.
//! The kernel drops the lock when the fd closes, including on SIGKILL, so a
//! crashed process cannot wedge the store the way a lock *file* would.

use super::{MemoryRecord, MemoryStore, ReflectReport};
use abi_wdbx::DurableStore;
use anyhow::{Result, anyhow, bail};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// WDBX spatial records are keyed by `u64`; Abbey's ids are uuids.
fn spatial_id(memory_id: &str) -> u64 {
    super::stable_hash(memory_id)
}

/// KV key namespace for Abbey memory records inside a shared WDBX store.
const KEY_PREFIX: &str = "mem/";

/// Scan ceiling, mirroring `SqliteMemory::filter`'s `LIMIT 1000`.
const SCAN_LIMIT: usize = 1000;

/// How long to wait for another process to finish before giving up.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

pub struct WdbxMemory {
    // Field order is load-bearing: `store` drops (flushing) before `_lock`
    // releases, so no other process can observe a half-finished session.
    store: Mutex<DurableStore>,
    dir: PathBuf,
    _lock: File,
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
        let point = super::coordinates(rec);
        let mut store = self.store.lock().expect("wdbx lock");
        store
            .put(&Self::key(&rec.id), &json)
            .map_err(|e| anyhow!("wdbx put {}: {e}", rec.id))?;
        // Mirror the 3-D position into WDBX's spatial space so the map is
        // visible to `abi wdbx` too, not just to Abbey.
        store
            .put_spatial(abi_wdbx::SpatialRecord {
                id: spatial_id(&rec.id),
                x: point.x,
                y: point.y,
                z: point.z,
                payload: rec.id.clone(),
            })
            .map_err(|e| anyhow!("wdbx put_spatial {}: {e}", rec.id))
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

/// Lock a store directory without opening it — used to hold Abbey's lock across
/// an `abi wdbx` subprocess that would otherwise ignore it.
pub fn lock_store_dir(dir: &Path, timeout: Duration) -> Result<File> {
    std::fs::create_dir_all(dir)?;
    lock_exclusive(&dir.join("abbey.lock"), timeout)
}

/// Take an exclusive advisory lock, retrying until `timeout` elapses.
#[cfg(unix)]
fn lock_exclusive(path: &Path, timeout: Duration) -> Result<File> {
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    let fd = file.as_raw_fd();
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: `fd` is owned by `file` and stays open for this call.
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::WouldBlock {
            return Err(anyhow!("lock {}: {err}", path.display()));
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

/// No advisory lock available — see the module docs for the consequence.
#[cfg(not(unix))]
fn lock_exclusive(path: &Path, _timeout: Duration) -> Result<File> {
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?)
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
    #[cfg(unix)]
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
    #[cfg(unix)]
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
}
