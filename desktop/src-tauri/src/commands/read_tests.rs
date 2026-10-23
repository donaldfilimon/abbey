//! Daemon reads must yield to the UI and execute blocking work elsewhere.
use super::*;
use std::future::Future;
use std::sync::mpsc::sync_channel;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

#[test]
fn daemon_read_bridges_return_futures_without_performing_io_at_invocation() {
    fn zero<T, F: Future<Output = Result<T, IpcError>>>(_: fn() -> F) {}
    fn one<A, T, F: Future<Output = Result<T, IpcError>>>(_: fn(A) -> F) {}
    fn three<A, B, C, T, F: Future<Output = Result<T, IpcError>>>(_: fn(A, B, C) -> F) {}
    // Function items only: this test never contacts a daemon or reads a bearer.
    zero(app_status);
    zero(app_v3_grants);
    zero(app_connection);
    three(app_effective_model);
    one(app_claims);
    one(app_routes);
    one(app_run_status);
    one(app_run_events);
    one(app_memory_search);
    one(app_memory_metadata);
    one(app_claim_by_id);
}

#[test]
fn a_stalled_read_yields_and_runs_on_a_different_thread() {
    let caller = std::thread::current().id();
    let (started_tx, started_rx) = sync_channel(1);
    let (release_tx, release_rx) = sync_channel(1);
    let mut future = Box::pin(read_task(move || {
        started_tx.send(std::thread::current().id()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(17_u8)
    }));
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    let worker = started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(worker, caller, "blocking read ran on the polling/UI thread");
    release_tx.send(()).unwrap();
    assert_eq!(tauri::async_runtime::block_on(future).unwrap(), 17);
}

#[test]
fn read_worker_panic_is_a_sanitized_transport_failure() {
    let error = tauri::async_runtime::block_on(read_task(|| -> Result<(), IpcError> {
        panic!("worker-fixture-payload");
    }))
    .unwrap_err();
    assert_eq!(error.kind, IpcErrorKind::Transport);
    assert!(!error.message.contains("worker-fixture-payload"));
}
