//! Admission control: a concurrency permit and a fixed-window rate limiter.
//!
//! Both are **instance state**, never `static`. A process-global limiter would
//! make one test's traffic change another test's verdict, and a server that
//! cannot be instantiated twice cannot be tested deterministically at all.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Bounds how many requests are being serviced at once.
///
/// A permit is taken at `accept` and released when the connection's guard
/// drops, so the count tracks live connections exactly — including ones whose
/// worker panics, because `Drop` still runs.
#[derive(Debug)]
pub struct ConcurrencyGate {
    in_flight: AtomicUsize,
    capacity: usize,
}

impl ConcurrencyGate {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Take a permit, or `None` when the cap is already reached.
    ///
    /// Uses compare-exchange rather than `fetch_add` + rollback so the count can
    /// never transiently exceed the cap, which would make an observer (or a
    /// test) see a value the type promises is impossible.
    pub fn try_acquire(self: &Arc<Self>) -> Option<ConcurrencyPermit> {
        let mut current = self.in_flight.load(Ordering::Acquire);
        loop {
            if current >= self.capacity {
                return None;
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(ConcurrencyPermit {
                        gate: Arc::clone(self),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Releases its permit on drop.
pub struct ConcurrencyPermit {
    gate: Arc<ConcurrencyGate>,
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        self.gate.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Fixed-window request budget for the whole listener.
///
/// Fixed windows admit a burst of up to twice the budget across a window
/// boundary. That is accepted deliberately: the limiter exists to stop a local
/// runaway loop from pinning the server, not to shape traffic, and a sliding
/// window would add per-request bookkeeping for no security gain on loopback.
#[derive(Debug)]
pub struct RateLimiter {
    inner: Mutex<Window>,
    budget: u32,
    window: Duration,
}

#[derive(Debug)]
struct Window {
    started: Instant,
    used: u32,
}

impl RateLimiter {
    #[must_use]
    pub fn new(budget: u32, window: Duration) -> Self {
        Self {
            inner: Mutex::new(Window {
                started: Instant::now(),
                used: 0,
            }),
            budget,
            window,
        }
    }

    /// Charge one request against the budget. `false` means "reject with 429".
    pub fn try_admit(&self) -> bool {
        self.try_admit_at(Instant::now())
    }

    /// Deterministic form used by the window-rollover test.
    pub fn try_admit_at(&self, now: Instant) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if now.duration_since(state.started) >= self.window {
            state.started = now;
            state.used = 0;
        }
        if state.used >= self.budget {
            return false;
        }
        state.used += 1;
        true
    }

    #[must_use]
    pub fn budget(&self) -> u32 {
        self.budget
    }

    #[must_use]
    pub fn window(&self) -> Duration {
        self.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_limiter_rejects_past_the_documented_threshold() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.try_admit(), "1st request is inside the budget");
        assert!(limiter.try_admit(), "2nd request is inside the budget");
        assert!(limiter.try_admit(), "3rd request is exactly at the budget");
        assert!(!limiter.try_admit(), "4th request must be rejected");
        assert!(!limiter.try_admit(), "and it stays rejected");
    }

    #[test]
    fn a_window_rollover_restores_the_rate_budget() {
        let window = Duration::from_secs(60);
        let limiter = RateLimiter::new(2, window);
        let start = Instant::now();
        assert!(limiter.try_admit_at(start));
        assert!(limiter.try_admit_at(start + Duration::from_secs(1)));
        assert!(
            !limiter.try_admit_at(start + Duration::from_secs(59)),
            "still inside the window, budget is spent"
        );
        assert!(
            limiter.try_admit_at(start + window),
            "the window rolled over, so the budget is restored"
        );
    }

    #[test]
    fn permits_are_capped_and_returned_on_drop() {
        let gate = Arc::new(ConcurrencyGate::new(2));
        let first = gate.try_acquire().expect("first permit");
        let second = gate.try_acquire().expect("second permit");
        assert_eq!(gate.in_flight(), 2);
        assert!(
            gate.try_acquire().is_none(),
            "a third permit must be refused at the cap"
        );
        drop(second);
        assert_eq!(gate.in_flight(), 1);
        let third = gate.try_acquire().expect("a slot freed up");
        assert_eq!(gate.in_flight(), 2);
        drop(first);
        drop(third);
        assert_eq!(gate.in_flight(), 0);
    }
}
