//! Bounded event delivery shared by every clone of a streaming run.

use super::StreamEvent;
use crate::runtime::CancellationToken;
use std::fmt;
use std::sync::{Arc, Mutex, mpsc::Sender};

pub(crate) const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_EVENTS: usize = 65_536;

#[derive(Default)]
struct Budget {
    bytes: usize,
    raw_bytes: usize,
    events: usize,
    exceeded: bool,
    finished: bool,
    failed: bool,
}

#[derive(Clone)]
pub struct StreamTap {
    events: Sender<StreamEvent>,
    pub cancel: CancellationToken,
    budget: Arc<Mutex<Budget>>,
}

impl StreamTap {
    pub fn new(events: Sender<StreamEvent>) -> Self {
        Self {
            events,
            cancel: CancellationToken::new(),
            budget: Arc::new(Mutex::new(Budget::default())),
        }
    }

    /// Enforce cumulative payload and event ceilings before enqueueing.
    /// A small failure and terminal event have reserved delivery after overflow.
    pub fn emit(&self, event: StreamEvent) -> bool {
        let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(event, StreamEvent::Done { .. }) {
            if budget.finished {
                return false;
            }
            budget.finished = true;
            return self.events.send(event).is_ok();
        }
        if budget.exceeded || budget.finished {
            return false;
        }
        let bytes = event.payload_bytes();
        if bytes > MAX_PAYLOAD_BYTES.saturating_sub(budget.bytes) || budget.events >= MAX_EVENTS - 2
        {
            budget.exceeded = true;
            self.cancel.cancel();
            let _ = self.events.send(StreamEvent::Failed(
                "stream exceeded the 4 MiB decoded-payload or 65,536-event limit".into(),
            ));
            return false;
        }
        if matches!(event, StreamEvent::Failed(_)) {
            budget.failed = true;
            self.cancel.cancel();
        }
        budget.bytes += bytes;
        budget.events += 1;
        if self.events.send(event).is_err() {
            self.cancel.cancel();
            return false;
        }
        true
    }

    pub(crate) fn raw_remaining(&self) -> usize {
        MAX_PAYLOAD_BYTES.saturating_sub(
            self.budget
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .raw_bytes,
        )
    }

    pub(crate) fn record_raw(&self, bytes: usize) -> bool {
        let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        if bytes > MAX_PAYLOAD_BYTES.saturating_sub(budget.raw_bytes) {
            drop(budget);
            self.emit(StreamEvent::Failed(
                "stream exceeded the 4 MiB cumulative raw-output limit".into(),
            ));
            return false;
        }
        budget.raw_bytes += bytes;
        true
    }

    pub(crate) fn failed(&self) -> bool {
        self.budget.lock().unwrap_or_else(|e| e.into_inner()).failed
    }

    pub(crate) fn exceeded(&self) -> bool {
        self.budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exceeded
    }

    /// Run-path diagnostics that would otherwise corrupt the alternate screen.
    pub fn notice(&self, msg: impl Into<String>) {
        self.emit(StreamEvent::Notice(msg.into()));
    }
}

impl fmt::Debug for StreamTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamTap")
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn raw_budget_is_shared_across_attempts_and_clones() {
        let (tx, rx) = mpsc::channel();
        let tap = StreamTap::new(tx);
        assert!(tap.record_raw(MAX_PAYLOAD_BYTES / 2));
        assert!(tap.clone().record_raw(MAX_PAYLOAD_BYTES / 2));
        assert_eq!(tap.raw_remaining(), 0);
        assert!(!tap.record_raw(1));
        assert!(tap.cancel.is_cancelled());
        assert!(matches!(rx.try_recv(), Ok(StreamEvent::Failed(_))));
    }

    #[test]
    fn clones_share_payload_budget_and_overflow_is_reported_once() {
        let (tx, rx) = mpsc::channel();
        let tap = StreamTap::new(tx);
        assert!(tap.emit(StreamEvent::TextDelta("x".repeat(MAX_PAYLOAD_BYTES))));
        assert!(!tap.clone().emit(StreamEvent::ThinkingDelta("🌍".into())));
        assert!(!tap.emit(StreamEvent::Notice("ignored".into())));
        assert!(tap.cancel.is_cancelled());
        assert!(tap.exceeded());
        assert!(tap.emit(StreamEvent::Done { exit: 1 }));
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[1], StreamEvent::Failed(_)));
        assert_eq!(events[2], StreamEvent::Done { exit: 1 });
    }

    #[test]
    fn zero_payload_event_flood_is_bounded() {
        let (tx, rx) = mpsc::channel();
        let tap = StreamTap::new(tx);
        for _ in 0..MAX_EVENTS - 2 {
            assert!(tap.emit(StreamEvent::Usage {
                input_tokens: 0,
                output_tokens: 0
            }));
        }
        assert!(!tap.emit(StreamEvent::Usage {
            input_tokens: 0,
            output_tokens: 0
        }));
        assert!(tap.emit(StreamEvent::Done { exit: 1 }));
        assert!(!tap.emit(StreamEvent::Done { exit: 1 }));
        assert_eq!(rx.try_iter().count(), MAX_EVENTS);
    }
}
