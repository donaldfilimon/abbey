//! Provider-neutral execution seam for the durable run manager.
//!
//! This module deliberately owns no model, tool, shell, network, or memory
//! behavior. Concrete providers adapt their request into [`Executor::Request`]
//! and report only completion or a bounded failure to the manager.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::app_core::{RunId, RunRequest};

const MAX_FAILURE_BYTES: usize = 4_096;

/// Monotonic cooperative-cancellation signal shared with one running executor.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Request cancellation. Repeated calls are harmless.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Return whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Bounded, display-safe execution failure retained by the run manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionError {
    kind: ExecutionErrorKind,
    message: String,
}

/// Stable execution-failure class. Values contain no provider output or paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionErrorKind {
    General,
    Unsupported,
    Spawn,
    TimedOut,
    OutputLimit,
    ProviderExit,
    Teardown,
}

impl ExecutionError {
    /// Construct a general execution failure, preserving the Phase 4A API.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_kind(ExecutionErrorKind::General, message)
    }

    #[must_use]
    pub fn with_kind(kind: ExecutionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: bounded_message(message.into()),
        }
    }

    #[must_use]
    pub fn kind(&self) -> ExecutionErrorKind {
        self.kind
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ExecutionError {}

/// Execution implementation injected into a [`RunManager`](super::manager::RunManager).
///
/// Cancellation is cooperative. Implementations should inspect `cancellation`
/// at natural interruption points and return promptly when it becomes set.
pub trait Executor: Send + Sync + 'static {
    fn execute(
        &self,
        run_id: &RunId,
        request: RunRequest,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError>;
}

/// Outcome produced without allowing an executor panic to kill the manager worker.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExecutionAttempt {
    Completed,
    Failed(ExecutionError),
    Panicked(ExecutionError),
}

pub(crate) fn execute_catching_panics<E: Executor>(
    executor: &E,
    run_id: &RunId,
    request: RunRequest,
    cancellation: &CancellationToken,
) -> ExecutionAttempt {
    match catch_unwind(AssertUnwindSafe(|| {
        executor.execute(run_id, request, cancellation)
    })) {
        Ok(Ok(())) => ExecutionAttempt::Completed,
        Ok(Err(error)) => ExecutionAttempt::Failed(error),
        Err(payload) => ExecutionAttempt::Panicked(ExecutionError::new(panic_message(payload))),
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "executor panicked with a non-string payload".to_owned());
    format!("executor panicked: {message}")
}

fn bounded_message(message: String) -> String {
    if message.len() <= MAX_FAILURE_BYTES {
        return message;
    }

    let mut end = MAX_FAILURE_BYTES.saturating_sub(3);
    while !message.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut bounded = message[..end].to_owned();
    bounded.push_str("...");
    bounded
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PanicExecutor;

    impl Executor for PanicExecutor {
        fn execute(
            &self,
            _run_id: &RunId,
            _request: RunRequest,
            _cancellation: &CancellationToken,
        ) -> Result<(), ExecutionError> {
            panic!("fixture panic")
        }
    }

    #[test]
    fn cancellation_is_monotonic_and_shared() {
        let token = CancellationToken::default();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn execution_errors_are_utf8_safely_bounded() {
        let error = ExecutionError::new("🦀".repeat(MAX_FAILURE_BYTES));
        assert_eq!(error.kind(), ExecutionErrorKind::General);
        assert!(error.message().len() <= MAX_FAILURE_BYTES);
        assert!(error.message().ends_with("..."));
    }

    #[test]
    fn execution_error_kind_is_stable_and_contains_no_extra_context() {
        let error =
            ExecutionError::with_kind(ExecutionErrorKind::TimedOut, "delegated process timed out");
        assert_eq!(error.kind(), ExecutionErrorKind::TimedOut);
        assert_eq!(error.message(), "delegated process timed out");
        assert_eq!(error.to_string(), "delegated process timed out");
    }

    #[test]
    fn executor_panics_become_explicit_attempts() {
        let request = RunRequest {
            idempotency_key: "panic-fixture".parse().unwrap(),
            conversation_id: None,
            mode: crate::app_core::RunMode::Background,
            backend: crate::app_core::BackendSelection::Abi,
            input: "panic fixture".into(),
            labels: Vec::new(),
        };
        let attempt = execute_catching_panics(
            &PanicExecutor,
            &RunId::new(),
            request,
            &CancellationToken::default(),
        );
        let ExecutionAttempt::Panicked(error) = attempt else {
            panic!("expected panic outcome")
        };
        assert_eq!(error.message(), "executor panicked: fixture panic");
    }
}
