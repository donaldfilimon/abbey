//! Read-only previews plus separately consented project memory commands.
//! Live memory is explicit and never injected into the global agent path.

#[cfg(unix)]
mod folder;
mod live;

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::cli::KnowledgeCmd;

pub(super) const MAX_QUERY_BYTES: usize = 256;
pub(super) const MAX_EXCERPT_BYTES: usize = 512;
pub(super) const MAX_FILES: usize = 64;
pub(super) const MAX_FILE_BYTES: usize = 64 * 1024;
pub(super) const MAX_TOTAL_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeConfig {
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub folder: Option<PathBuf>,
}

fn default_source() -> String {
    "folder".into()
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            source: default_source(),
            folder: None,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct SearchHit {
    /// A content-addressed opaque ID; never a source filename or path.
    id: String,
    sha256: String,
    excerpt: String,
}

/// Configuration-only status. Deliberately never probes a path or opens memory.
fn status(cfg: &KnowledgeConfig) -> Result<serde_json::Value> {
    let availability = match cfg.source.as_str() {
        "folder" if cfg.folder.is_none() => "unavailable: no folder configured",
        "folder" if cfg!(unix) => "configured; not inspected",
        "folder" => "unavailable: secure folder traversal unsupported on this platform",
        "synthetic" => "available: built-in synthetic fixtures",
        "memory" => "unavailable: explicit approved memory record selection is not implemented",
        _ => bail!("invalid knowledge source; expected folder, synthetic, or memory (no fallback)"),
    };
    Ok(serde_json::json!({
        "source": cfg.source,
        "availability": availability,
        "mode": "explicit local CLI preview only; not chat-RAG",
        "private_contents_in_training_weights": false,
        "automatic_injection": false,
        "folder_configured": cfg.folder.is_some(),
        "limits": {
            "depth": 0, "entries": 128, "files": MAX_FILES,
            "per_file_bytes": MAX_FILE_BYTES, "total_bytes": MAX_TOTAL_BYTES,
            "query_bytes": MAX_QUERY_BYTES, "results": 10,
            "excerpt_bytes": MAX_EXCERPT_BYTES
        }
    }))
}

fn search(cfg: &KnowledgeConfig, query: &str, limit: u8) -> Result<Vec<SearchHit>> {
    ensure!(
        !query.trim().is_empty()
            && query.len() <= MAX_QUERY_BYTES
            && !query.chars().any(char::is_control),
        "knowledge query must be nonempty, at most 256 UTF-8 bytes, without controls"
    );
    ensure!(
        (1..=10).contains(&limit),
        "knowledge result limit must be 1..=10"
    );
    let documents = match cfg.source.as_str() {
        "synthetic" => vec![
            "Synthetic fixture: the imaginary Juniper project meets on Tuesday.".to_string(),
            "Synthetic fixture: the imaginary Indigo handbook recommends local retrieval, not weight training.".to_string(),
        ],
        "folder" => {
            let path = cfg.folder.as_deref().ok_or_else(|| anyhow::anyhow!("knowledge folder is not configured; no path is inferred"))?;
            #[cfg(unix)]
            { folder::read_documents(path)? }
            #[cfg(not(unix))]
            {
                let _ = path;
                bail!("knowledge folder unavailable: secure traversal unsupported on this platform");
            }
        }
        "memory" => bail!("knowledge memory unavailable: explicit approved record selection is not implemented; no store opened"),
        _ => bail!("invalid knowledge source; expected folder, synthetic, or memory (no fallback)"),
    };
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    for document in documents {
        let folded = document.to_lowercase();
        let Some(offset) = folded.find(&needle) else {
            continue;
        };
        let sha256 = format!("{:x}", Sha256::digest(document.as_bytes()));
        // Map the folded byte offset back to a UTF-8 boundary in one pass,
        // rather than repeatedly folding every suffix of a private document.
        let mut folded_bytes = 0;
        let start = document
            .char_indices()
            .find_map(|(index, ch)| {
                folded_bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                (offset < folded_bytes).then_some(index)
            })
            .unwrap_or(0);
        let excerpt = bounded_excerpt(&document[start..]);
        hits.push(SearchHit {
            id: format!("doc-{sha256}"),
            sha256,
            excerpt,
        });
    }
    hits.sort_by(|a, b| a.id.cmp(&b.id));
    hits.dedup_by(|a, b| a.id == b.id);
    hits.truncate(usize::from(limit));
    Ok(hits)
}

fn bounded_excerpt(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if result.len() + ch.len_utf8() > MAX_EXCERPT_BYTES {
            break;
        }
        result.push(ch);
    }
    result
}

pub(crate) fn dispatch(cmd: KnowledgeCmd) -> Result<i32> {
    // Unlike the generation display helpers, never substitute defaults after
    // an invalid explicit configuration.
    let cfg = crate::config::AbbeyConfig::load()?;
    let value = match cmd {
        KnowledgeCmd::Status => status(&cfg.knowledge)?,
        KnowledgeCmd::Search { query, limit } => serde_json::json!({
            "source": cfg.knowledge.source,
            "mode": "explicit local CLI preview only; not chat-RAG",
            "results": search(&cfg.knowledge, &query, limit)?
        }),
        other => live::dispatch(other, &cfg)?,
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_unselected_folder_and_never_falls_back() {
        let cfg = KnowledgeConfig::default();
        assert_eq!(cfg.source, "folder");
        assert!(
            status(&cfg).unwrap()["availability"]
                .as_str()
                .unwrap()
                .contains("no folder")
        );
        assert!(search(&cfg, "synthetic", 5).is_err());
        for source in ["", "unknown", "memory"] {
            let cfg = KnowledgeConfig {
                source: source.into(),
                folder: None,
            };
            assert!(search(&cfg, "fixture", 5).is_err());
        }
    }

    #[test]
    fn synthetic_search_is_deterministic_bounded_and_explicit() {
        let cfg = KnowledgeConfig {
            source: "synthetic".into(),
            folder: None,
        };
        let hits = search(&cfg, "SYNTHETIC", 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits, search(&cfg, "synthetic", 1).unwrap());
        assert_eq!(hits[0].sha256.len(), 64);
        assert!(hits[0].excerpt.contains("Synthetic fixture"));
        assert!(search(&cfg, "not present", 5).unwrap().is_empty());
        for query in ["".to_string(), " ".into(), "a".repeat(257), "a\nb".into()] {
            assert!(search(&cfg, &query, 5).is_err());
        }
        for limit in [0, 11, 255] {
            assert!(search(&cfg, "fixture", limit).is_err());
        }
        assert!(bounded_excerpt(&"é".repeat(1000)).len() <= MAX_EXCERPT_BYTES);
    }

    #[test]
    fn status_never_checks_even_an_invalid_folder() {
        let cfg = KnowledgeConfig {
            folder: Some(PathBuf::from("/missing/.secret/link")),
            ..Default::default()
        };
        let text = status(&cfg).unwrap().to_string();
        assert!(!text.contains("/missing"));
        assert!(text.contains("not inspected") || text.contains("unsupported"));
    }

    #[test]
    fn memory_selection_is_not_approval_and_never_falls_back_to_folder_or_synthetic() {
        let cfg = KnowledgeConfig {
            source: "memory".into(),
            folder: Some(PathBuf::from("/nonexistent/selected")),
        };
        let status = status(&cfg).unwrap();
        assert!(
            status["availability"]
                .as_str()
                .unwrap()
                .starts_with("unavailable:")
        );
        let error = search(&cfg, "Synthetic fixture", 5)
            .unwrap_err()
            .to_string();
        assert!(error.contains("approved record selection"));
        assert!(error.contains("no store opened"));
    }
}
