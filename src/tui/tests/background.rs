//! The work the app does off the UI thread: the panel refresh, and the
//! interrupt that never ends.

use super::*;
use crate::tui::app::INTERRUPT_GRACE;
use crate::tui::refresh::{RefreshPayload, RefreshPoll};
use std::time::{Duration, Instant};

/// The shutdown coverage lives in its own file.
///
/// The module registry (`src/tui/tests.rs`) belongs to another session,
/// so the file is included from here rather than registered there.
#[path = "worker_shutdown.rs"]
mod worker_shutdown;

fn empty_payload() -> RefreshPayload {
    RefreshPayload {
        doctor_lines: Vec::new(),
        persona_lines: Vec::new(),
        memory_lines: Vec::new(),
        skill_lines: Vec::new(),
        route_lines: Vec::new(),
        live_models: Vec::new(),
        history: Vec::new(),
    }
}

#[test]
fn a_slow_refresh_does_not_block_the_ui_thread() {
    // The refresh used to run on the thread that draws: the doctor
    // panel's `--version` probe, the model list, and the memory
    // backend's open all ran there, in the frame the user was reading
    // the answer on.
    let mut app = scratch_app("slow-refresh", AgentBackend::Cursor);
    let started = Instant::now();
    app.request_refresh_with(|| {
        std::thread::sleep(Duration::from_millis(600));
        RefreshPayload {
            doctor_lines: vec!["injected doctor line".into()],
            ..empty_payload()
        }
    });
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "the refresh collection must not run on the UI thread"
    );

    // The result is applied when it lands.
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.doctor_lines.is_empty() && Instant::now() < deadline {
        app.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        app.doctor_lines,
        vec!["injected doctor line".to_string()],
        "the finished refresh must be applied on the UI thread"
    );
}

#[test]
fn a_refresh_worker_that_panics_is_reported_as_failed() {
    // "Still refreshing" and "the worker is gone" are different facts.
    // The poll used to collapse them into one, so a worker that died
    // left the status line spinning on "refreshing…" for the rest of
    // the session.
    let mut app = scratch_app("refresh-panic", AgentBackend::Cursor);
    app.status = "refreshing…".into();
    app.request_refresh_with(|| panic!("the refresh worker died"));

    // The worker thread dies, so its channel is disconnected: the poll
    // has to say so rather than report "still running".
    let mut poll = None;
    for _ in 0..200 {
        poll = app.panel_view.refresh.as_ref().map(|w| w.poll());
        if !matches!(poll, Some(RefreshPoll::Pending) | None) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(poll, Some(RefreshPoll::Failed(_))),
        "a dead refresh worker must be reported, saw {poll:?}"
    );

    // And the app says so, instead of spinning on "refreshing…".
    app.pump();
    assert!(
        app.status.contains("refresh failed"),
        "the status must report the failed refresh, said {:?}",
        app.status
    );
    assert!(
        app.panel_view.refresh.is_none(),
        "a dead worker must not be polled again"
    );
}

#[test]
fn an_interrupt_that_never_ends_is_given_up_on() {
    // An interrupt asks the worker to stop; it does not promise that the
    // worker stops. The app has to stop waiting, and say so.
    let mut app = scratch_app("interrupt-grace", AgentBackend::Cursor);
    let (events_tx, events) = std::sync::mpsc::channel();
    let (_done_tx, done) = std::sync::mpsc::channel();
    let cancel = crate::runtime::CancellationToken::new();
    app.run = Some(super::super::worker::RunHandle {
        events,
        done,
        cancel: cancel.clone(),
        started: Instant::now(),
        thread: Some(std::thread::spawn(move || {
            // A worker that never reports: it holds the event sender and
            // the completion.
            std::thread::sleep(Duration::from_secs(30));
            drop(events_tx);
        })),
        completion: None,
        local: false,
        cancel_requested: None,
        abandoned: false,
    });

    press(&mut app, KeyCode::Esc);
    assert!(cancel.is_cancelled(), "Esc must ask the worker to stop");
    assert!(
        app.run
            .as_ref()
            .is_some_and(|run| run.cancel_requested.is_some()),
        "the app must know that the interrupt was asked for"
    );

    // The grace is spent: the app gives up on the worker.
    app.run.as_mut().expect("the run").cancel_requested =
        Some(Instant::now() - INTERRUPT_GRACE - Duration::from_secs(1));
    app.pump();

    assert!(
        !app.is_running(),
        "the app must stop waiting for the worker"
    );
    assert!(
        app.transcript.cells.iter().any(|cell| matches!(
            cell,
            super::super::transcript::Cell::Error(text) if text.contains("gave up")
        )),
        "the app must say that it gave up on the worker: {:?}",
        app.transcript.cells
    );
    assert!(
        app.status.contains("130"),
        "the interrupted run must report the interrupt code, said {:?}",
        app.status
    );
}
