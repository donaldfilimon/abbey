//! Interpretable 3-D memory map: topic × recency × consolidation.
//!
//! Axes are deterministic — Abbey has no embedder, so distances answer
//! "same subject / around this time / this consolidation depth", not semantic
//! similarity in a learned space.

use super::MemoryRecord;

/// A memory's position in Abbey's 3-D map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl MemoryPoint {
    pub fn distance_to(self, other: Self) -> f32 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
    }
}

/// Retention layers, innermost last — the `z` axis's integer part.
const CONSOLIDATION: [&str; 4] = ["activity", "stm", "ltm", "train_candidate"];

/// Tags that describe storage, not subject — skipped when picking a topic.
const NON_TOPIC_TAGS: [&str; 5] = ["activity", "stm", "ltm", "train_candidate", "self-learn"];

/// Shown for records carrying no subject tag.
pub const UNTAGGED_TOPIC: &str = "(untagged)";

/// The tag Abbey treats as a record's subject.
///
/// Untagged records share one column rather than a fake topic spread from
/// hashing wording. Tag with `abbey memory put --tag <subject>`.
pub fn primary_topic(rec: &MemoryRecord) -> &str {
    rec.tags
        .iter()
        .map(String::as_str)
        .find(|t| !NON_TOPIC_TAGS.contains(t))
        .unwrap_or(UNTAGGED_TOPIC)
}

/// FNV-1a — stable across runs/platforms (unlike `DefaultHasher`).
pub(crate) fn stable_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Hours between `timestamp` (`%Y-%m-%dT%H:%M:%SZ`) and now; 0 if unparseable.
fn hours_since(timestamp: &str) -> f64 {
    let Ok(then) = chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%SZ") else {
        return 0.0;
    };
    let now = chrono::Utc::now().naive_utc();
    (now - then).num_seconds().max(0) as f64 / 3600.0
}

/// Place a record in the 3-D map.
pub fn coordinates(rec: &MemoryRecord) -> MemoryPoint {
    let topic = primary_topic(rec);
    // 64 columns: rare subject collisions, still legible.
    let x = (stable_hash(topic) % 64) as f32;
    let y = (hours_since(&rec.timestamp) + 1.0).log2() as f32;
    let depth = CONSOLIDATION
        .iter()
        .position(|l| *l == rec.retention)
        .unwrap_or(0) as f32;
    let z = depth + rec.confidence.clamp(0.0, 1.0);
    MemoryPoint { x, y, z }
}

/// Records nearest `target`, closest first, as `(distance, record)`.
pub fn nearest(
    records: &[MemoryRecord],
    target: MemoryPoint,
    limit: usize,
) -> Vec<(f32, &MemoryRecord)> {
    let mut scored: Vec<(f32, &MemoryRecord)> = records
        .iter()
        .map(|r| (coordinates(r).distance_to(target), r))
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    scored.truncate(limit);
    scored
}

/// Neighbours of `anchor_id` in the 3-D map (anchor itself excluded).
pub fn nearest_to(
    store: &dyn super::MemoryStore,
    anchor_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<(f32, MemoryRecord)>> {
    let Some(anchor) = store.get(anchor_id)? else {
        anyhow::bail!("memory id not found: {anchor_id}");
    };
    let target = coordinates(&anchor);
    let all = store.filter(None, None, 1000)?;
    Ok(nearest(&all, target, limit + 1)
        .into_iter()
        .filter(|(_, r)| r.id != anchor.id)
        .take(limit)
        .map(|(d, r)| (d, r.clone()))
        .collect())
}

#[cfg(test)]
mod map_tests {
    use super::*;

    fn rec(summary: &str, topic: &str, retention: &str, confidence: f32) -> MemoryRecord {
        let mut r = MemoryRecord::new_stm(summary, "body");
        r.tags = vec![retention.into(), topic.into()];
        r.retention = retention.into();
        r.confidence = confidence;
        r
    }

    #[test]
    fn same_topic_shares_a_column_different_topics_usually_do_not() {
        let a = coordinates(&rec("a", "guitar", "ltm", 0.8));
        let b = coordinates(&rec("b", "guitar", "stm", 0.5));
        let c = coordinates(&rec("c", "woodworking", "ltm", 0.8));
        assert_eq!(a.x, b.x, "one subject is one column");
        assert_ne!(a.x, c.x, "different subjects separate");
    }

    #[test]
    fn consolidation_lifts_z_and_confidence_fine_tunes_it() {
        let activity = coordinates(&rec("a", "t", "activity", 0.0));
        let stm = coordinates(&rec("a", "t", "stm", 0.0));
        let ltm = coordinates(&rec("a", "t", "ltm", 0.0));
        let train = coordinates(&rec("a", "t", "train_candidate", 0.0));
        assert!(activity.z < stm.z && stm.z < ltm.z && ltm.z < train.z);

        let unsure = coordinates(&rec("a", "t", "ltm", 0.1));
        let certain = coordinates(&rec("a", "t", "ltm", 0.9));
        assert!(certain.z > unsure.z, "confidence lifts within a layer");
    }

    #[test]
    fn recency_axis_increases_with_age() {
        let stamp = |ago_hours: i64| {
            (chrono::Utc::now() - chrono::Duration::hours(ago_hours))
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        };
        let mut now = rec("now", "t", "ltm", 0.5);
        now.timestamp = stamp(0);
        let mut yesterday = rec("yesterday", "t", "ltm", 0.5);
        yesterday.timestamp = stamp(24);
        let mut last_year = rec("last year", "t", "ltm", 0.5);
        last_year.timestamp = stamp(24 * 365);

        let (y0, y1, y2) = (
            coordinates(&now).y,
            coordinates(&yesterday).y,
            coordinates(&last_year).y,
        );
        assert!(
            y0 < y1 && y1 < y2,
            "older memories sit further out: {y0} {y1} {y2}"
        );
        assert!(
            y2 - y1 < y1 - y0 + 10.0,
            "log compression keeps old memories from running away"
        );
    }

    #[test]
    fn unparseable_timestamp_does_not_panic() {
        let mut r = rec("broken", "t", "ltm", 0.5);
        r.timestamp = "not-a-timestamp".into();
        assert_eq!(coordinates(&r).y, 0.0, "drift degrades to 0, not a crash");
    }

    #[test]
    fn placement_is_deterministic_across_calls() {
        let r = rec("stable", "guitar", "ltm", 0.8);
        assert_eq!(coordinates(&r), coordinates(&r));
    }

    #[test]
    fn retention_and_storage_tags_are_not_mistaken_for_a_subject() {
        let mut r = MemoryRecord::new_stm("s", "b");
        r.tags = vec!["stm".into(), "self-learn".into(), "guitar".into()];
        assert_eq!(primary_topic(&r), "guitar");
    }

    #[test]
    fn nearest_to_skips_anchor_and_ranks_same_topic_first() {
        use crate::memory::{MemoryStore, SqliteMemory};

        let dir = std::env::temp_dir().join(format!("abbey-near-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = SqliteMemory::open(&SqliteMemory::path_for_state_dir(&dir)).unwrap();

        let mut anchor = MemoryRecord::new_stm("anchor", "body");
        anchor.tags = vec!["ltm".into(), "guitar".into()];
        anchor.retention = "ltm".into();
        let aid = anchor.id.clone();
        db.store(anchor).unwrap();

        let mut same = MemoryRecord::new_stm("same topic", "body");
        same.tags = vec!["ltm".into(), "guitar".into()];
        same.retention = "ltm".into();
        db.store(same).unwrap();

        let mut other = MemoryRecord::new_stm("other topic", "body");
        other.tags = vec!["activity".into(), "woodworking".into()];
        other.retention = "activity".into();
        other.confidence = 0.1;
        db.store(other).unwrap();

        let got = nearest_to(&db, &aid, 2).unwrap();
        assert_eq!(got.len(), 2);
        assert_ne!(got[0].1.id, aid);
        assert_eq!(got[0].1.summary, "same topic");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod topic_tests {
    use super::*;

    #[test]
    fn untagged_records_are_labelled_not_scattered() {
        let mut a = MemoryRecord::new_stm("plays guitar", "b");
        a.tags = vec!["stm".into()];
        a.retention = "ltm".into();
        let mut b = MemoryRecord::new_stm("builds furniture", "b");
        b.tags = vec!["stm".into()];
        b.retention = "ltm".into();

        assert_eq!(primary_topic(&a), UNTAGGED_TOPIC);
        assert_eq!(
            coordinates(&a).x,
            coordinates(&b).x,
            "untagged memories share one visible column rather than a fake topic spread"
        );

        a.tags.push("guitar".into());
        assert_eq!(primary_topic(&a), "guitar");
        assert_ne!(
            coordinates(&a).x,
            coordinates(&b).x,
            "tagging moves it into its own subject column"
        );
    }
}
