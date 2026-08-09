//! `Mcp-Session-Id` sessions for the loopback HTTP transport.
//!
//! ## Why sessions exist at all
//!
//! [`Server`] carries the `initialize` handshake state, and this transport
//! serves one request per connection. Handing every request a fresh `Server`
//! would make `tools/list` answer "not initialized" forever; sharing one
//! `Server` across all requests would serialize the whole listener and quietly
//! defeat the concurrency cap. Neither is acceptable, and skipping the
//! handshake entirely *is* MCP's stateless lifecycle revision — which this
//! server explicitly does not claim.
//!
//! So `initialize` mints a session id, returns it in the `Mcp-Session-Id`
//! response header, and every later request presents it. Requests on different
//! sessions run in parallel; requests on the same session serialize on that
//! session's own lock, which is what MCP ordering requires anyway.
//!
//! ## Bounded, not unbounded
//!
//! A session id is **not** an authentication token — it is unguessable only
//! incidentally, and this transport is unauthenticated by design (see the module
//! docs). The table is bounded so that repeated `initialize` calls cannot pin
//! memory: idle sessions are swept first, and only then is a new session
//! refused.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::mcp_host::Server;

/// One live session: an MCP server plus the last time it was touched.
struct Entry {
    server: Arc<Mutex<Server>>,
    last_seen: Instant,
}

/// A bounded, idle-expiring table of MCP sessions.
pub struct SessionTable {
    entries: Mutex<HashMap<String, Entry>>,
    capacity: usize,
    idle_timeout: Duration,
}

impl SessionTable {
    #[must_use]
    pub fn new(capacity: usize, idle_timeout: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            capacity,
            idle_timeout,
        }
    }

    /// Mint a session, or `None` when the table is full even after a sweep.
    pub fn create(&self) -> Option<(String, Arc<Mutex<Server>>)> {
        self.create_at(Instant::now())
    }

    /// Deterministic form used by the capacity/expiry test.
    pub fn create_at(&self, now: Instant) -> Option<(String, Arc<Mutex<Server>>)> {
        let mut entries = self.lock();
        Self::sweep(&mut entries, now, self.idle_timeout);
        if entries.len() >= self.capacity {
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let server = Arc::new(Mutex::new(Server::new()));
        entries.insert(
            id.clone(),
            Entry {
                server: Arc::clone(&server),
                last_seen: now,
            },
        );
        Some((id, server))
    }

    /// Look a session up and mark it as used.
    pub fn get(&self, id: &str) -> Option<Arc<Mutex<Server>>> {
        self.get_at(id, Instant::now())
    }

    /// Deterministic form used by the capacity/expiry test.
    pub fn get_at(&self, id: &str, now: Instant) -> Option<Arc<Mutex<Server>>> {
        let mut entries = self.lock();
        Self::sweep(&mut entries, now, self.idle_timeout);
        let entry = entries.get_mut(id)?;
        entry.last_seen = now;
        Some(Arc::clone(&entry.server))
    }

    /// Drop a session (`DELETE`). `true` when one was actually removed.
    pub fn remove(&self, id: &str) -> bool {
        self.lock().remove(id).is_some()
    }

    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Drop every entry untouched for longer than `idle_timeout`.
    ///
    /// `checked_duration_since` rather than `duration_since`: a session created
    /// with a synthetic future instant in a test must not panic the sweeper.
    fn sweep(entries: &mut HashMap<String, Entry>, now: Instant, idle_timeout: Duration) {
        entries.retain(|_, entry| {
            now.checked_duration_since(entry.last_seen)
                .is_none_or(|idle| idle < idle_timeout)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_capped_and_idle_entries_are_reclaimed() {
        let idle = Duration::from_secs(300);
        let table = SessionTable::new(2, idle);
        let start = Instant::now();

        let (first, _first_server) = table.create_at(start).expect("first session");
        let (second, _second_server) = table.create_at(start).expect("second session");
        assert_eq!(table.len(), 2);
        assert!(
            table.create_at(start).is_none(),
            "a third session must be refused at MAX_HTTP_SESSIONS"
        );

        // Both sessions resolve, and a lookup counts as activity.
        assert!(table.get_at(&first, start).is_some());
        assert!(table.get_at(&second, start).is_some());
        assert!(table.get_at("no-such-session", start).is_none());

        // Keep `first` warm past the idle timeout; `second` goes cold.
        let warm = start + idle - Duration::from_secs(1);
        assert!(table.get_at(&first, warm).is_some());
        let later = start + idle + Duration::from_secs(1);
        let (third, _third_server) = table
            .create_at(later)
            .expect("the idle session is reclaimed, freeing a slot");
        assert!(
            table.get_at(&second, later).is_none(),
            "the idle session is gone"
        );
        assert!(table.get_at(&third, later).is_some());

        assert!(table.remove(&third));
        assert!(!table.remove(&third), "removing twice is not an error");
    }
}
