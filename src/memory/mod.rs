//! Replaceable memory backends (SQLite interim; WDBX later).

mod sqlite;

use serde::{Deserialize, Serialize};

pub use sqlite::SqliteMemory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub source_type: String,
    pub source_ref: String,
    pub timestamp: String,
    pub origin: String,
    pub payload: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub embedding_ref: Option<String>,
    pub confidence: f32,
    pub provenance: String,
    /// stm | ltm | activity | train_candidate
    pub retention: String,
    pub supersedes: Option<String>,
    #[serde(default = "default_classification")]
    pub classification: String,
    #[serde(default)]
    pub obsolete: bool,
}

fn default_classification() -> String {
    "internal".into()
}

impl MemoryRecord {
    pub fn new_stm(summary: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source_type: "session".into(),
            source_ref: String::new(),
            timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            origin: "system".into(),
            payload: payload.into(),
            summary: summary.into(),
            tags: vec!["stm".into()],
            embedding_ref: None,
            confidence: 0.7,
            provenance: "abbey session".into(),
            retention: "stm".into(),
            supersedes: None,
            classification: "internal".into(),
            obsolete: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReflectReport {
    pub duplicate_summaries: Vec<(String, String)>,
    pub low_confidence: Vec<String>,
    pub superseded: Vec<String>,
}

pub trait MemoryStore {
    fn store(&self, rec: MemoryRecord) -> anyhow::Result<()>;
    fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>>;
    fn update(&self, rec: MemoryRecord) -> anyhow::Result<()>;
    #[allow(dead_code)]
    fn invalidate(&self, id: &str) -> anyhow::Result<()>;
    fn search_keyword(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>>;
    fn filter(
        &self,
        retention: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    fn promote(&self, id: &str, new_retention: &str) -> anyhow::Result<()>;
    #[allow(dead_code)]
    fn supersede(&self, old_id: &str, new_rec: MemoryRecord) -> anyhow::Result<()>;
    fn reflect(&self) -> anyhow::Result<ReflectReport>;
}
