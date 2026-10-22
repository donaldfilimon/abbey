//! The shutdown path: a worker that will not stop must not hold the
//! terminal, and the app must say that it gave up on it.
//!
//! The module registry (`src/tui/tests.rs`) belongs to another session,
//! so this file is included from `background.rs`, which owns the rest
//! of the off-UI-thread coverage.

use crate::agent::AgentBackend;
use crate::runtime::CancellationToken;
use crate::tui::app::SHUTDOWN_JOIN_GRACE;
use crate::tui::tests::scratch_app;
use crate::tui::transcript::Cell;
use crate::tui::worker::{JoinOutcome, RunHandle};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A run handle whose worker is `thread`.
///
/// The handle holds the event sender and the completion, exactly as a
/// real run does, so the app's own paths see the same shape.
fn run_with(thread: JoinHandle<()>, cancel: CancellationToken) -> RunHandle {
    let (_events_tx, events) = std::sync::mpsc::channel();
    let (_done_tx, done) = std::sync::mpsc::channel();
    RunHandle {
        events,
        done,
        cancel,
        started: Instant::now(),
        thread: Some(thread),
        completion: None,
        local: false,
        cancel_requested: None,
        abandoned: false,
    }
}

/// A worker that never finishes, and ignores the cancellation.
fn never_finishes() -> JoinHandle<()> {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
    })
}

#[test]
fn a_worker_that_never_finishes_does_not_block_the_shutdown() {
    // The worker is the user's turn. A shutdown that waited for it for
    // ever would keep the terminal in raw mode with no way out, so the
    // wait is bounded — and the app says what it did.
    let mut app = scratch_app("stuck-shutdown", AgentBackend::Cursor);
    let cancel = CancellationToken::new();
    app.run = Some(run_with(never_finishes(), cancel.clone()));

    let started = Instant::now();
    let outcome = app.shutdown();
    let elapsed = started.elapsed();

    assert!(
        elapsed < SHUTDOWN_JOIN_GRACE + Duration::from_secs(1),
        "the shutdown waited {elapsed:?} for a worker that never finishes"
    );
    assert!(cancel.is_cancelled(), "the worker must be asked to stop");
    assert!(
        !app.is_running(),
        "the app must stop waiting for the worker"
    );

    // The abandonment is reported, not swallowed: it is neither a quiet
    // success nor a panic.
    assert!(
        outcome.is_err(),
        "the shutdown must not report a quiet success: {outcome:?}"
    );
    assert!(
        app.status.contains("gave up"),
        "the status must say the app gave up, said {:?}",
        app.status
    );
    assert!(
        app.transcript.cells.iter().any(|cell| matches!(
            cell,
            Cell::Error(text) if text.contains("gave up")
        )),
        "the transcript must carry the abandonment: {:?}",
        app.transcript.cells
    );
}

#[test]
fn a_worker_that_finishes_is_joined_and_reported_as_finished() {
    // The bound is a bound, not a deadline: a worker that stops in time
    // is joined, and the shutdown is quiet.
    let mut app = scratch_app("finished-shutdown", AgentBackend::Cursor);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    app.run = Some(run_with(
        std::thread::spawn(move || {
            // The worker stops as soon as it is asked to.
            while !worker_cancel.is_cancelled() {
                std::thread::yield_now();
            }
        }),
        cancel,
    ));

    let outcome = app.shutdown();
    assert!(
        outcome.is_ok(),
        "a worker that stopped in time must not fail the shutdown: {outcome:?}"
    );
    assert!(!app.is_running());
    assert!(
        !app.status.contains("gave up"),
        "a joined worker must not be reported as abandoned, said {:?}",
        app.status
    );
}

#[test]
fn a_run_handle_reports_the_three_join_outcomes_apart() {
    // "Finished", "panicked", and "did not stop" are three different
    // facts, and the app has to tell them apart.
    let mut finished = run_with(std::thread::spawn(|| {}), CancellationToken::new());
    assert_eq!(
        finished.join_within(Duration::from_secs(5)),
        JoinOutcome::Finished,
        "a worker that ended must be reported as finished"
    );

    let mut panicked = run_with(
        std::thread::spawn(|| panic!("the worker died")),
        CancellationToken::new(),
    );
    assert_eq!(
        panicked.join_within(Duration::from_secs(5)),
        JoinOutcome::Panicked,
        "a worker that died must be reported as panicked"
    );

    let mut stuck = run_with(never_finishes(), CancellationToken::new());
    assert_eq!(
        stuck.join_within(Duration::from_millis(50)),
        JoinOutcome::Abandoned,
        "a worker that never ends must be reported as abandoned"
    );
}
