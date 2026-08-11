//! Bounded, presentation-only memory reads for the authenticated v3 daemon.
//!
//! This authority deliberately exposes one summary projection rather than the
//! underlying store. Payload, provenance, source metadata, paths, and internal
//! record identifiers never enter a protocol event.

use std::collections::VecDeque;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::app_core::{
    V3EntityPage, V3EntityRecord, V3Event, V3OperationState, V3PageQuery, V3ResourceQuery,
    V3SearchRequest,
};
use crate::memory::{MemoryFilter, MemoryRecord};

use super::{HandlerFailure, internal_failure, invalid_command_failure, not_found_failure};

const SUMMARY_SPACE_ID: &str = "memory-v1-summary";
const SUMMARY_SPACE_LABEL: &str = "Sanitized memory summaries";
const MAX_MEMORY_RECORDS: usize = 1_024;
const MAX_SNAPSHOTS: usize = 16;
const MAX_LABEL_BYTES: usize = 256;
const SNAPSHOT_TOKEN_START: u64 = 1_u64 << 63;
const RECORD_ID_DOMAIN: &[u8] = b"abbey:v3-memory-record:v1\0";
const QUERY_DOMAIN: &[u8] = b"abbey:v3-memory-query:v1\0";

/// Startup-owned route for this edition's exact memory backend.
pub(crate) struct MemoryEffectRoute {
    state_root: PathBuf,
    backend: String,
}

impl MemoryEffectRoute {
    pub(crate) fn new(state_root: PathBuf, backend: String) -> Self {
        Self {
            state_root,
            backend,
        }
    }
}

/// Exact backend access plus a bounded cache of query-bound fixed snapshots.
pub(super) struct MemoryAuthority {
    route: MemoryEffectRoute,
    readable: bool,
    snapshots: Mutex<SnapshotCache>,
}

impl MemoryAuthority {
    pub(super) fn new(route: MemoryEffectRoute) -> Self {
        let readable = crate::memory::open_backend_exact(&route.state_root, &route.backend).is_ok();
        Self {
            route,
            readable,
            snapshots: Mutex::new(SnapshotCache::new()),
        }
    }

    pub(super) const fn readable(&self) -> bool {
        self.readable
    }

    pub(super) fn invalidate(&self, record_id: &str) -> anyhow::Result<()> {
        crate::memory::open_backend_exact(&self.route.state_root, &self.route.backend)?
            .invalidate(record_id)
    }

    pub(super) fn list_spaces(&self, page: V3PageQuery) -> Result<V3Event, HandlerFailure> {
        if page.after > 1 || page.through.is_some_and(|through| through != 1) {
            return Err(invalid_command_failure());
        }
        let records = (page.after == 0)
            .then(|| V3EntityRecord {
                id: SUMMARY_SPACE_ID.to_owned(),
                label: SUMMARY_SPACE_LABEL.to_owned(),
                state: V3OperationState::Available,
            })
            .into_iter()
            .collect();
        Ok(V3Event::MemorySpaces(V3EntityPage {
            after: page.after,
            through: 1,
            records,
        }))
    }

    pub(super) fn search(&self, request: V3SearchRequest) -> Result<V3Event, HandlerFailure> {
        if request.space_id != SUMMARY_SPACE_ID {
            return Err(not_found_failure());
        }
        let query_digest = domain_digest(QUERY_DOMAIN, request.query.as_bytes());
        let (through, records) = if let Some(through) = request.page.through {
            let cache = self.snapshots.lock().map_err(|_| internal_failure())?;
            let snapshot = cache
                .find(through, &query_digest)
                .ok_or_else(invalid_command_failure)?;
            (through, page(snapshot, request.page)?)
        } else {
            let records = self.search_snapshot(&request.query)?;
            let mut cache = self.snapshots.lock().map_err(|_| internal_failure())?;
            let through = cache.insert(query_digest, records);
            let snapshot = cache
                .find(through, &query_digest)
                .ok_or_else(internal_failure)?;
            (through, page(snapshot, request.page)?)
        };
        Ok(V3Event::MemorySearchResults(V3EntityPage {
            after: request.page.after,
            through,
            records,
        }))
    }

    pub(super) fn metadata(&self, query: V3ResourceQuery) -> Result<V3Event, HandlerFailure> {
        let memory = self.open()?;
        let record = memory
            .filter_with(&MemoryFilter::default(), MAX_MEMORY_RECORDS)
            .map_err(|_| internal_failure())?
            .into_iter()
            .find(|record| opaque_record_id(&record.id) == query.resource_id)
            .ok_or_else(not_found_failure)?;
        Ok(V3Event::MemoryMetadata(project_record(&record)))
    }

    fn search_snapshot(&self, query: &str) -> Result<Vec<V3EntityRecord>, HandlerFailure> {
        let needle = query.to_lowercase();
        let mut records = self
            .open()?
            .filter_with(&MemoryFilter::default(), MAX_MEMORY_RECORDS)
            .map_err(|_| internal_failure())?
            .into_iter()
            .filter_map(|record| {
                let label = sanitize_summary(&record.summary);
                label.to_lowercase().contains(&needle).then(|| {
                    let mut projected = project_record(&record);
                    projected.label = label;
                    projected
                })
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(records)
    }

    fn open(&self) -> Result<Box<dyn crate::memory::MemoryStore>, HandlerFailure> {
        if !self.readable {
            return Err(internal_failure());
        }
        crate::memory::open_backend_exact(&self.route.state_root, &self.route.backend)
            .map_err(|_| internal_failure())
    }
}

struct Snapshot {
    token: u64,
    query_digest: [u8; 32],
    records: Vec<V3EntityRecord>,
}

struct SnapshotCache {
    next_token: u64,
    entries: VecDeque<Snapshot>,
}

impl SnapshotCache {
    const fn new() -> Self {
        Self {
            next_token: SNAPSHOT_TOKEN_START,
            entries: VecDeque::new(),
        }
    }

    fn insert(&mut self, query_digest: [u8; 32], records: Vec<V3EntityRecord>) -> u64 {
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .unwrap_or(SNAPSHOT_TOKEN_START);
        if self.entries.len() == MAX_SNAPSHOTS {
            self.entries.pop_front();
        }
        self.entries.push_back(Snapshot {
            token,
            query_digest,
            records,
        });
        token
    }

    fn find(&self, token: u64, query_digest: &[u8; 32]) -> Option<&[V3EntityRecord]> {
        self.entries
            .iter()
            .find(|snapshot| snapshot.token == token && &snapshot.query_digest == query_digest)
            .map(|snapshot| snapshot.records.as_slice())
    }
}

fn page(
    snapshot: &[V3EntityRecord],
    query: V3PageQuery,
) -> Result<Vec<V3EntityRecord>, HandlerFailure> {
    let after = usize::try_from(query.after).map_err(|_| invalid_command_failure())?;
    if after > snapshot.len() {
        return Err(invalid_command_failure());
    }
    Ok(snapshot
        .iter()
        .skip(after)
        .take(usize::from(query.limit))
        .cloned()
        .collect())
}

fn project_record(record: &MemoryRecord) -> V3EntityRecord {
    V3EntityRecord {
        id: opaque_record_id(&record.id),
        label: sanitize_summary(&record.summary),
        state: V3OperationState::Available,
    }
}

fn opaque_record_id(record_id: &str) -> String {
    let digest = domain_digest(RECORD_ID_DOMAIN, record_id.as_bytes());
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("memory-");
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn domain_digest(domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(value);
    hasher.finalize().into()
}

fn sanitize_summary(raw: &str) -> String {
    let mut output = String::new();
    for token in raw.split_whitespace() {
        let cleaned = if is_path(token) {
            "[path]".to_owned()
        } else {
            token
                .chars()
                .filter(|character| !character.is_control())
                .collect()
        };
        if cleaned.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(&cleaned);
    }
    if output.is_empty() {
        output.push_str("Memory record");
    }
    truncate(output, MAX_LABEL_BYTES)
}

fn is_path(token: &str) -> bool {
    path_segment(token) || token.split('=').any(path_segment) || token.split(':').any(path_segment)
}

fn path_segment(part: &str) -> bool {
    part.starts_with('/')
        || part.starts_with('~')
        || part.starts_with("\\\\")
        || (part.len() >= 3
            && part.as_bytes()[0].is_ascii_alphabetic()
            && part.as_bytes()[1] == b':'
            && matches!(part.as_bytes()[2], b'/' | b'\\'))
}

fn truncate(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

#[cfg(test)]
mod tests;
