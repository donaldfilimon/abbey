//! Backend-neutral semantic-memory orchestration.
//!
//! Memory records are authoritative. Embeddings are a rebuildable index keyed
//! by provider space and the hash of only `summary + subject tags`. A failed
//! provider call therefore cannot lose a write; the record simply remains
//! pending until a later explicit embed/backfill.

use super::embedding::{Embedder, EmbeddingSpace, MAX_EMBEDDING_BATCH, normalize, stable_digest};
use super::{MemoryFilter, MemoryRecord, MemoryStore};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

const NON_SUBJECT_TAGS: [&str; 5] = ["activity", "stm", "ltm", "train_candidate", "self-learn"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEmbedding {
    pub memory_id: String,
    pub space_id: String,
    pub content_hash: String,
    pub dimension: usize,
    pub vector: Vec<f32>,
    pub updated_at: String,
}

impl StoredEmbedding {
    pub fn new(record: &MemoryRecord, space: &EmbeddingSpace, vector: Vec<f32>) -> Result<Self> {
        if vector.len() != space.dimension {
            bail!(
                "embedding has dimension {}; space {} requires {}",
                vector.len(),
                space.space_id,
                space.dimension
            );
        }
        Ok(Self {
            memory_id: record.id.clone(),
            space_id: space.space_id.clone(),
            content_hash: content_hash(record),
            dimension: vector.len(),
            vector,
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmbeddingStatus {
    pub total: usize,
    pub ready: usize,
    pub missing: usize,
    pub stale: usize,
}

impl EmbeddingStatus {
    #[must_use]
    pub fn pending(&self) -> usize {
        self.missing + self.stale
    }
}

#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub score: f32,
    pub record: MemoryRecord,
}

#[derive(Debug, Clone, Default)]
pub struct BackfillReport {
    pub attempted: usize,
    pub embedded: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StoreEmbeddingOutcome {
    pub memory_id: String,
    #[allow(dead_code)] // useful to non-CLI callers; CLI reports the optional error
    pub embedded: bool,
    pub embedding_error: Option<String>,
}

/// Text sent to learned providers: summary plus subject tags, never payload,
/// provenance, source references, or other potentially sensitive metadata.
#[must_use]
pub fn embedding_text(record: &MemoryRecord) -> String {
    let tags = subject_tags(record);
    if tags.is_empty() {
        record.summary.clone()
    } else {
        format!("{}\nsubject-tags: {}", record.summary, tags.join(", "))
    }
}

#[must_use]
pub fn content_hash(record: &MemoryRecord) -> String {
    stable_digest(embedding_text(record).as_bytes())
}

fn subject_tags(record: &MemoryRecord) -> Vec<&str> {
    let mut tags = record
        .tags
        .iter()
        .map(String::as_str)
        .filter(|tag| !NON_SUBJECT_TAGS.contains(tag))
        .collect::<Vec<_>>();
    tags.sort_unstable();
    tags.dedup();
    tags
}

/// Store the authoritative memory first, then make one best-effort embedding.
/// Provider failure is returned as data, not as a failed/lost memory write.
pub fn store_with_embedding(
    store: &dyn MemoryStore,
    record: MemoryRecord,
    embedder: &dyn Embedder,
) -> Result<StoreEmbeddingOutcome> {
    let id = record.id.clone();
    store.store(record)?;
    match embed_one(store, &id, embedder) {
        Ok(()) => Ok(StoreEmbeddingOutcome {
            memory_id: id,
            embedded: true,
            embedding_error: None,
        }),
        Err(error) => Ok(StoreEmbeddingOutcome {
            memory_id: id,
            embedded: false,
            embedding_error: Some(format!("{error:#}")),
        }),
    }
}

/// Embed or refresh one existing memory in the configured provider space.
pub fn embed_one(store: &dyn MemoryStore, id: &str, embedder: &dyn Embedder) -> Result<()> {
    let record = store
        .get(id)?
        .ok_or_else(|| anyhow::anyhow!("memory id not found: {id}"))?;
    if record.obsolete {
        bail!("cannot embed obsolete memory: {id}");
    }
    let mut vectors = embedder.embed(&[embedding_text(&record)])?;
    if vectors.len() != 1 {
        bail!(
            "embedding provider returned {} vectors for one memory",
            vectors.len()
        );
    }
    let vector = normalize(vectors.remove(0))?;
    store.put_embedding(StoredEmbedding::new(&record, embedder.space(), vector)?)
}

/// Embed one memory only when missing/stale, unless `force` is set. Returns
/// `true` when the provider was called and a vector was written.
pub fn embed_one_if_needed(
    store: &dyn MemoryStore,
    id: &str,
    embedder: &dyn Embedder,
    force: bool,
) -> Result<bool> {
    let record = store
        .get(id)?
        .ok_or_else(|| anyhow::anyhow!("memory id not found: {id}"))?;
    if record.obsolete {
        bail!("cannot embed obsolete memory: {id}");
    }
    if !force && store.embedding_is_current(id, &embedder.space().space_id)? {
        return Ok(false);
    }
    embed_one(store, id, embedder)?;
    Ok(true)
}

/// Backfill missing/stale vectors, or deliberately recompute every live record
/// in the current space when `force` is true.
pub fn backfill_with_force(
    store: &dyn MemoryStore,
    embedder: &dyn Embedder,
    limit: usize,
    force: bool,
) -> Result<BackfillReport> {
    let mut report = BackfillReport::default();
    if limit == 0 {
        return Ok(report);
    }
    if embedder.space().provider == "none" {
        bail!("semantic embeddings are disabled (embedding provider is `none`)");
    }
    let records = if force {
        store.filter_with(&MemoryFilter::default(), limit)?
    } else {
        store.embedding_candidates(&embedder.space().space_id, limit)?
    };
    for chunk in records.chunks(MAX_EMBEDDING_BATCH) {
        report.attempted += chunk.len();
        let inputs = chunk.iter().map(embedding_text).collect::<Vec<_>>();
        let vectors = match embedder.embed(&inputs) {
            Ok(vectors) if vectors.len() == chunk.len() => vectors,
            Ok(vectors) => {
                report.failed += chunk.len();
                report.errors.push(format!(
                    "provider returned {} vectors for {} records",
                    vectors.len(),
                    chunk.len()
                ));
                continue;
            }
            Err(error) => {
                report.failed += chunk.len();
                report.errors.push(format!("{error:#}"));
                continue;
            }
        };
        for (record, vector) in chunk.iter().zip(vectors) {
            let result = normalize(vector).and_then(|vector| {
                StoredEmbedding::new(record, embedder.space(), vector)
                    .and_then(|embedding| store.put_embedding(embedding))
            });
            match result {
                Ok(()) => report.embedded += 1,
                Err(error) => {
                    report.failed += 1;
                    report.errors.push(format!("{}: {error:#}", record.id));
                }
            }
        }
    }
    Ok(report)
}

pub fn status(store: &dyn MemoryStore, embedder: &dyn Embedder) -> Result<EmbeddingStatus> {
    store.embedding_status(&embedder.space().space_id)
}

/// Query only the configured space. Stale or differently configured vectors
/// are excluded by the backend before ranking.
pub fn search(
    store: &dyn MemoryStore,
    embedder: &dyn Embedder,
    query: &str,
    filter: &MemoryFilter,
    limit: usize,
) -> Result<Vec<SemanticHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut vectors = embedder.embed(&[query.to_string()])?;
    if vectors.len() != 1 {
        bail!(
            "embedding provider returned {} vectors for one query",
            vectors.len()
        );
    }
    let query = normalize(vectors.remove(0))?;
    if query.len() != embedder.space().dimension {
        bail!(
            "query embedding has dimension {}; configured dimension is {}",
            query.len(),
            embedder.space().dimension
        );
    }
    store.semantic_search(&embedder.space().space_id, &query, filter, limit)
}

pub(crate) fn cosine(left: &[f32], right: &[f32]) -> Result<f32> {
    if left.len() != right.len() || left.is_empty() {
        bail!(
            "cannot compare embedding dimensions {} and {}",
            left.len(),
            right.len()
        );
    }
    Ok(left.iter().zip(right).map(|(a, b)| a * b).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::SqliteMemory;
    use std::sync::Mutex;

    struct TestEmbedder {
        space: EmbeddingSpace,
        fail: bool,
        calls: Mutex<usize>,
    }

    impl TestEmbedder {
        fn new(model: &str, fail: bool) -> Self {
            Self {
                space: EmbeddingSpace::new("test", model, "r1", 2).unwrap(),
                fail,
                calls: Mutex::new(0),
            }
        }
    }

    impl Embedder for TestEmbedder {
        fn space(&self) -> &EmbeddingSpace {
            &self.space
        }

        fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
            *self.calls.lock().unwrap() += 1;
            if self.fail {
                bail!("mock provider unavailable");
            }
            Ok(inputs
                .iter()
                .map(|input| {
                    if input.contains("alpha") {
                        vec![1.0, 0.0]
                    } else {
                        vec![0.0, 1.0]
                    }
                })
                .collect())
        }
    }

    fn db(tag: &str) -> (SqliteMemory, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "abbey-semantic-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (
            SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap(),
            dir,
        )
    }

    fn record(summary: &str) -> MemoryRecord {
        let mut record = MemoryRecord::new_stm(summary, "SECRET PAYLOAD");
        record.tags = vec!["stm".into(), "subject".into()];
        record
    }

    #[test]
    fn embedding_input_excludes_payload_and_storage_tags() {
        let record = record("safe summary");
        assert_eq!(
            embedding_text(&record),
            "safe summary\nsubject-tags: subject"
        );
        assert!(!embedding_text(&record).contains("SECRET"));
        let mut changed_payload = record.clone();
        changed_payload.payload = "different secret".into();
        assert_eq!(content_hash(&record), content_hash(&changed_payload));
    }

    #[test]
    fn failed_embedding_keeps_memory_pending_then_backfill_recovers() {
        let (db, dir) = db("pending");
        let record = record("alpha memory");
        let id = record.id.clone();
        let failing = TestEmbedder::new("same-space", true);
        let outcome = store_with_embedding(&db, record, &failing).unwrap();
        assert!(!outcome.embedded);
        assert!(
            db.get(&id).unwrap().is_some(),
            "write must survive provider failure"
        );
        assert_eq!(status(&db, &failing).unwrap().pending(), 1);

        let working = TestEmbedder::new("same-space", false);
        let report = backfill_with_force(&db, &working, 10, false).unwrap();
        assert_eq!((report.embedded, report.failed), (1, 0));
        assert_eq!(status(&db, &working).unwrap().ready, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_makes_old_vector_stale_and_search_ignores_it() {
        let (db, dir) = db("stale");
        let provider = TestEmbedder::new("model", false);
        let mut record = record("alpha memory");
        let id = record.id.clone();
        db.store(record.clone()).unwrap();
        embed_one(&db, &id, &provider).unwrap();
        assert_eq!(status(&db, &provider).unwrap().ready, 1);

        record.summary = "beta memory".into();
        db.update(record).unwrap();
        let state = status(&db, &provider).unwrap();
        assert_eq!((state.ready, state.stale), (0, 1));
        let hits = search(&db, &provider, "alpha", &MemoryFilter::default(), 10).unwrap();
        assert!(hits.is_empty(), "stale vector must not leak into results");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn spaces_are_isolated_in_search_and_status() {
        let (db, dir) = db("spaces");
        let one = TestEmbedder::new("one", false);
        let two = TestEmbedder::new("two", false);
        let record = record("alpha memory");
        let id = record.id.clone();
        db.store(record).unwrap();
        embed_one(&db, &id, &one).unwrap();
        assert_eq!(status(&db, &one).unwrap().ready, 1);
        assert_eq!(status(&db, &two).unwrap().missing, 1);
        assert!(
            search(&db, &two, "alpha", &MemoryFilter::default(), 10)
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sqlite_vectors_survive_backend_reopen() {
        let (db, dir) = db("sqlite-reopen");
        let provider = TestEmbedder::new("model", false);
        let record = record("alpha memory");
        let id = record.id.clone();
        db.store(record).unwrap();
        embed_one(&db, &id, &provider).unwrap();
        drop(db);

        let reopened = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();
        assert_eq!(status(&reopened, &provider).unwrap().ready, 1);
        let hits = search(&reopened, &provider, "alpha", &MemoryFilter::default(), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.id, id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn embed_if_needed_skips_current_unless_forced() {
        let (db, dir) = db("force");
        let provider = TestEmbedder::new("model", false);
        let record = record("alpha memory");
        let id = record.id.clone();
        db.store(record).unwrap();
        assert!(embed_one_if_needed(&db, &id, &provider, false).unwrap());
        assert!(!embed_one_if_needed(&db, &id, &provider, false).unwrap());
        assert!(embed_one_if_needed(&db, &id, &provider, true).unwrap());
        assert_eq!(*provider.calls.lock().unwrap(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn forced_backfill_recomputes_all_current_records() {
        let (db, dir) = db("force-all");
        let provider = TestEmbedder::new("model", false);
        let record = record("alpha memory");
        db.store(record).unwrap();
        assert_eq!(
            backfill_with_force(&db, &provider, 10, false)
                .unwrap()
                .embedded,
            1
        );
        assert_eq!(
            backfill_with_force(&db, &provider, 10, false)
                .unwrap()
                .attempted,
            0
        );
        let forced = backfill_with_force(&db, &provider, 10, true).unwrap();
        assert_eq!((forced.attempted, forced.embedded), (1, 1));
        assert_eq!(*provider.calls.lock().unwrap(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
