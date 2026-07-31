//! Lexical similarity search over memory — feature-hash embeddings + cosine.
//!
//! ## Honest scope
//!
//! The vectors come from [`abi_ai::text_embedding`], a **deterministic signed
//! feature hash over character n-grams** — the same classical embedding the Zig
//! side wrote. It carries real lexical signal (shared rare trigrams score high),
//! which beats `search_keyword`'s substring match for typos, word order, and
//! morphological variants. It is **not** a learned/neural embedding: no trained
//! semantics, no subword vocabulary, no notion of meaning beyond surface form.
//! Two records about the same idea in entirely different words will *not* match.
//!
//! Learned/semantic embedding space therefore stays **Proposed** — see
//! `claims.rs` and `abbey claims refuse embeddings`, which still exits 2.
//!
//! ## Why vectors are not persisted
//!
//! Embedding happens at query time over `store.filter(..)`, exactly as
//! [`super::map::nearest_to`] recomputes coordinates. Nothing is written, so
//! `abi-wdbx`'s `put_vector` storage stays genuinely unwired (still Proposed)
//! and there is no dual-write to drift. It also avoids persisting vectors next
//! to Zig-written ones, where any hash change would silently corrupt recall.

use super::{MemoryRecord, MemoryStore};
use abi_ai::{EMBED_DIM, text_embedding};

/// Ceiling on records scanned per query — matches `nearest_to`'s scan budget.
const SCAN_LIMIT: usize = 1000;

/// Embed arbitrary query text.
pub fn embed_text(text: &str) -> [f32; EMBED_DIM] {
    text_embedding(text)
}

/// Embed a record: summary carries the signal, tags pull same-subject records
/// together the way the 3-D map's topic axis does.
pub fn embed_record(rec: &MemoryRecord) -> [f32; EMBED_DIM] {
    if rec.tags.is_empty() {
        return text_embedding(&rec.summary);
    }
    text_embedding(&format!("{} {}", rec.summary, rec.tags.join(" ")))
}

/// Cosine similarity in `[-1, 1]`, higher is closer.
///
/// `text_embedding` returns unit vectors (upstream asserts this), so cosine
/// reduces to the dot product — no renormalization needed here.
pub fn cosine(a: &[f32; EMBED_DIM], b: &[f32; EMBED_DIM]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Rank `records` against an already-embedded query, most similar first.
pub fn rank<'a>(
    records: &'a [MemoryRecord],
    query: &[f32; EMBED_DIM],
    limit: usize,
) -> Vec<(f32, &'a MemoryRecord)> {
    let mut scored: Vec<(f32, &MemoryRecord)> = records
        .iter()
        .map(|r| (cosine(&embed_record(r), query), r))
        .collect();
    // Descending: cosine is a similarity, not a distance.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    scored
}

/// Records most similar to free-text `query`, most similar first.
pub fn similar_to_text(
    store: &dyn MemoryStore,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<(f32, MemoryRecord)>> {
    let all = store.filter(None, None, SCAN_LIMIT)?;
    let q = embed_text(query);
    Ok(rank(&all, &q, limit)
        .into_iter()
        .map(|(s, r)| (s, r.clone()))
        .collect())
}

/// Records most similar to an existing record (the anchor itself excluded).
pub fn similar_to_id(
    store: &dyn MemoryStore,
    anchor_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<(f32, MemoryRecord)>> {
    let Some(anchor) = store.get(anchor_id)? else {
        anyhow::bail!("memory id not found: {anchor_id}");
    };
    let all = store.filter(None, None, SCAN_LIMIT)?;
    let q = embed_record(&anchor);
    Ok(rank(&all, &q, limit + 1)
        .into_iter()
        .filter(|(_, r)| r.id != anchor.id)
        .take(limit)
        .map(|(s, r)| (s, r.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(summary: &str, tags: &[&str]) -> MemoryRecord {
        let mut r = MemoryRecord::new_stm(summary, "body");
        r.tags = tags.iter().map(|t| (*t).to_string()).collect();
        r
    }

    #[test]
    fn cosine_of_identical_text_is_one() {
        let a = embed_text("the wdbx store holds vectors");
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn shared_ngrams_outrank_unrelated_text() {
        let records = vec![
            rec("wdbx durable store checkpoint", &["wdbx"]),
            rec("premium voice synthesis on macos", &["voice"]),
        ];
        let q = embed_text("wdbx durable store");
        let ranked = rank(&records, &q, 2);
        assert_eq!(ranked[0].1.summary, "wdbx durable store checkpoint");
        assert!(
            ranked[0].0 > ranked[1].0,
            "expected the wdbx record to outrank the voice one: {ranked:?}"
        );
    }

    #[test]
    fn ranking_is_descending_by_similarity() {
        let records = vec![
            rec("alpha beta gamma", &[]),
            rec("alpha beta", &[]),
            rec("zzz qqq", &[]),
        ];
        let q = embed_text("alpha beta gamma");
        let ranked = rank(&records, &q, 3);
        for w in ranked.windows(2) {
            assert!(w[0].0 >= w[1].0, "not descending: {ranked:?}");
        }
    }

    #[test]
    fn a_typo_still_matches_where_substring_search_would_miss() {
        // `search_keyword` is a substring match, so "chekpoint" finds nothing.
        // Shared trigrams still place the intended record first.
        let records = vec![
            rec("wdbx checkpoint epoch", &["wdbx"]),
            rec("install premium voices", &["voice"]),
        ];
        let ranked = rank(&records, &embed_text("chekpoint"), 2);
        assert_eq!(ranked[0].1.summary, "wdbx checkpoint epoch");
    }

    #[test]
    fn untagged_records_embed_from_summary_alone() {
        let bare = rec("lonely record", &[]);
        assert_eq!(embed_record(&bare), embed_text("lonely record"));
    }

    #[test]
    fn embedding_is_deterministic_across_calls() {
        let r = rec("stable input", &["tag"]);
        assert_eq!(embed_record(&r), embed_record(&r));
    }
}
