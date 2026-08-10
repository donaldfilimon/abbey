//! ABI provider-contract adoption at Abbey's durable execution boundary.
//!
//! Routes are constructed only from startup-owned provider objects, model IDs,
//! and finite budgets. A [`RunRequest`] selects one closed backend; it cannot
//! select a provider executable, model, environment, or workspace. This bridge
//! deliberately exposes no tools. Provider text is bounded by ABI's capture
//! contract and is not persisted by Abbey's lifecycle manager.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use abi_agent_runtime::{
    BudgetLimit, EventSink, ModelEvent, ModelProvider, ModelRequest, RunBudget, StopReason,
    run_provider,
};

use super::{CancellationToken, ExecutionError, ExecutionErrorKind, Executor};
use crate::app_core::{BackendSelection, RunId, RunMode, RunRequest};

mod process;

pub use process::{FixedProviderKind, FixedRecipeProvider, FixedRecipeProviderError};

const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_PROVIDER_ID_BYTES: usize = 128;

/// Invalid startup-owned provider authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderConfigError {
    EmptyRoutes,
    DuplicateBackend,
    InvalidModel,
    InvalidProvider,
    UnboundedBudget,
}

impl fmt::Display for ProviderConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyRoutes => "provider routes cannot be empty",
            Self::DuplicateBackend => "provider route backends must be unique",
            Self::InvalidModel => "provider route model identity is invalid",
            Self::InvalidProvider => "provider route identity is invalid",
            Self::UnboundedBudget => "provider route requires finite nonzero budgets",
        })
    }
}

impl std::error::Error for ProviderConfigError {}

/// One startup-bound provider route.
pub struct ProviderRoute {
    backend: BackendSelection,
    model: String,
    provider: Arc<dyn ModelProvider>,
    budget: RunBudget,
}

impl ProviderRoute {
    /// Bind one closed Abbey backend to an exact ABI provider and model.
    pub fn new(
        backend: BackendSelection,
        model: impl Into<String>,
        provider: Arc<dyn ModelProvider>,
        budget: RunBudget,
    ) -> Result<Self, ProviderConfigError> {
        let model = model.into();
        if !valid_identifier(&model, MAX_MODEL_ID_BYTES) {
            return Err(ProviderConfigError::InvalidModel);
        }
        if !valid_identifier(provider.id(), MAX_PROVIDER_ID_BYTES) {
            return Err(ProviderConfigError::InvalidProvider);
        }
        if !finite_budget(budget) {
            return Err(ProviderConfigError::UnboundedBudget);
        }
        Ok(Self {
            backend,
            model,
            provider,
            budget,
        })
    }

    /// Closed backend visible to callers.
    #[must_use]
    pub const fn backend(&self) -> BackendSelection {
        self.backend
    }

    /// Startup-selected opaque model identity.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Startup-selected provider identity.
    #[must_use]
    pub fn provider_id(&self) -> &str {
        self.provider.id()
    }
}

impl fmt::Debug for ProviderRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderRoute")
            .field("backend", &self.backend)
            .field("model", &self.model)
            .field("provider", &self.provider.id())
            .field("budget", &self.budget)
            .finish()
    }
}

/// Abbey executor backed by ABI's provider-neutral model contract.
pub struct ModelProviderExecutor {
    routes: BTreeMap<BackendSelection, ProviderRoute>,
}

impl ModelProviderExecutor {
    /// Validate and freeze the startup-owned provider route table.
    pub fn new(
        routes: impl IntoIterator<Item = ProviderRoute>,
    ) -> Result<Self, ProviderConfigError> {
        let mut table = BTreeMap::new();
        for route in routes {
            if table.insert(route.backend, route).is_some() {
                return Err(ProviderConfigError::DuplicateBackend);
            }
        }
        if table.is_empty() {
            return Err(ProviderConfigError::EmptyRoutes);
        }
        Ok(Self { routes: table })
    }

    /// Stable startup-bound routes in backend order.
    pub fn routes(&self) -> impl ExactSizeIterator<Item = &ProviderRoute> {
        self.routes.values()
    }
}

impl Executor for ModelProviderExecutor {
    fn execute(
        &self,
        _run_id: &RunId,
        request: RunRequest,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        request.validate().map_err(|_| unsupported())?;
        if matches!(request.mode, RunMode::Interactive | RunMode::Automation) {
            return Err(unsupported());
        }
        let route = self.routes.get(&request.backend).ok_or_else(unsupported)?;
        let model_request = ModelRequest::new(&route.model).with_user(request.input);
        let mut sink = ProviderBoundarySink::new(&route.model);
        let report = run_provider(
            route.provider.as_ref(),
            &model_request,
            &mut sink,
            cancellation,
            route.budget,
        )
        .map_err(|_| provider_failed())?;

        match report.stop {
            StopReason::Cancelled if cancellation.is_cancelled() => return Ok(()),
            StopReason::BudgetExhausted(limit) => return Err(budget_error(limit)),
            StopReason::Failed => return Err(provider_failed()),
            StopReason::Completed => {}
            StopReason::Cancelled => return Err(provider_failed()),
        }
        if report.output.truncated() {
            return Err(ExecutionError::with_kind(
                ExecutionErrorKind::OutputLimit,
                "provider exceeded bounded capture",
            ));
        }
        sink.validate()?;
        Ok(())
    }
}

struct ProviderBoundarySink<'a> {
    expected_model: &'a str,
    started: bool,
    terminal: bool,
    invalid: bool,
}

impl<'a> ProviderBoundarySink<'a> {
    const fn new(expected_model: &'a str) -> Self {
        Self {
            expected_model,
            started: false,
            terminal: false,
            invalid: false,
        }
    }

    fn validate(&self) -> Result<(), ExecutionError> {
        if self.started && self.terminal && !self.invalid {
            Ok(())
        } else {
            Err(unsupported())
        }
    }
}

impl EventSink for ProviderBoundarySink<'_> {
    fn emit(&mut self, event: &ModelEvent) {
        match event {
            ModelEvent::Started { model }
                if !self.started && !self.terminal && model == self.expected_model =>
            {
                self.started = true;
            }
            ModelEvent::TextDelta { .. } if self.started && !self.terminal => {}
            ModelEvent::Finished { .. } if self.started && !self.terminal => {
                self.terminal = true;
            }
            ModelEvent::ToolCall(_) | ModelEvent::ToolResult(_) => self.invalid = true,
            _ => self.invalid = true,
        }
    }
}

fn finite_budget(budget: RunBudget) -> bool {
    budget.max_events.is_some_and(|value| value > 0)
        && budget.max_output_tokens.is_some_and(|value| value > 0)
        && budget
            .max_duration
            .is_some_and(|value| value > Duration::ZERO)
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
}

fn unsupported() -> ExecutionError {
    ExecutionError::with_kind(
        ExecutionErrorKind::Unsupported,
        "provider route is unsupported",
    )
}

fn provider_failed() -> ExecutionError {
    ExecutionError::with_kind(
        ExecutionErrorKind::ProviderExit,
        "provider execution failed",
    )
}

fn budget_error(limit: BudgetLimit) -> ExecutionError {
    let kind = match limit {
        BudgetLimit::Duration => ExecutionErrorKind::TimedOut,
        BudgetLimit::Events | BudgetLimit::OutputTokens => ExecutionErrorKind::OutputLimit,
    };
    ExecutionError::with_kind(kind, "provider execution exhausted its budget")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::IdempotencyKey;
    use abi_agent_runtime::{EchoProvider, ModelEvent, RuntimeError, ScriptedProvider, ToolCall};

    fn budget() -> RunBudget {
        RunBudget::unlimited()
            .with_max_events(16)
            .with_max_output_tokens(32)
            .with_max_duration(Duration::from_secs(1))
    }

    fn request(backend: BackendSelection) -> RunRequest {
        RunRequest {
            idempotency_key: IdempotencyKey::new(),
            conversation_id: None,
            mode: RunMode::OneShot,
            backend,
            input: "hello there".into(),
            labels: Vec::new(),
        }
    }

    #[test]
    fn abi_echo_provider_executes_through_the_abbey_boundary() {
        let route = ProviderRoute::new(
            BackendSelection::Abi,
            "startup-model",
            Arc::new(EchoProvider::new()),
            budget(),
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        assert_eq!(executor.routes().len(), 1);
        assert_eq!(executor.routes().next().unwrap().model(), "startup-model");
        executor
            .execute(
                &RunId::new(),
                request(BackendSelection::Abi),
                &CancellationToken::new(),
            )
            .unwrap();
    }

    #[test]
    fn pre_cancelled_provider_run_stops_without_exposing_partial_state() {
        let route = ProviderRoute::new(
            BackendSelection::Abi,
            "startup-model",
            Arc::new(EchoProvider::new()),
            budget(),
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        executor
            .execute(&RunId::new(), request(BackendSelection::Abi), &cancellation)
            .unwrap();
    }

    #[test]
    fn event_budget_exhaustion_maps_to_a_stable_output_limit() {
        let provider = ScriptedProvider::new("scripted").with_text("blocked by event budget");
        let bounded = RunBudget::unlimited()
            .with_max_events(1)
            .with_max_output_tokens(32)
            .with_max_duration(Duration::from_secs(1));
        let route = ProviderRoute::new(
            BackendSelection::Abi,
            "startup-model",
            Arc::new(provider),
            bounded,
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        let error = executor
            .execute(
                &RunId::new(),
                request(BackendSelection::Abi),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::OutputLimit);
        assert_eq!(error.message(), "provider execution exhausted its budget");
    }

    #[test]
    fn caller_cannot_manufacture_an_unbound_route() {
        let route = ProviderRoute::new(
            BackendSelection::Abi,
            "startup-model",
            Arc::new(EchoProvider::new()),
            budget(),
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        let error = executor
            .execute(
                &RunId::new(),
                request(BackendSelection::Cursor),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::Unsupported);
    }

    #[test]
    fn provider_tool_events_are_rejected_without_dispatch() {
        let provider = ScriptedProvider::new("scripted").with_tool_call(ToolCall::new(
            "call-1",
            "shell.exec",
            "{}",
        ));
        let route = ProviderRoute::new(
            BackendSelection::Abi,
            "startup-model",
            Arc::new(provider),
            budget(),
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        let error = executor
            .execute(
                &RunId::new(),
                request(BackendSelection::Abi),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::Unsupported);
    }

    #[test]
    fn provider_failures_do_not_disclose_provider_messages() {
        let provider = ScriptedProvider::new("scripted")
            .with_text("partial")
            .with_failure(RuntimeError::Provider {
                provider: "scripted".into(),
                message: "secret provider detail".into(),
            });
        let route = ProviderRoute::new(
            BackendSelection::Grok,
            "startup-model",
            Arc::new(provider),
            budget(),
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        let error = executor
            .execute(
                &RunId::new(),
                request(BackendSelection::Grok),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::ProviderExit);
        assert!(!error.message().contains("secret"));
    }

    #[test]
    fn route_configuration_requires_unique_finite_startup_authority() {
        assert!(matches!(
            ProviderRoute::new(
                BackendSelection::Abi,
                "model",
                Arc::new(EchoProvider::new()),
                RunBudget::unlimited(),
            ),
            Err(ProviderConfigError::UnboundedBudget)
        ));
        let left = ProviderRoute::new(
            BackendSelection::Abi,
            "model-a",
            Arc::new(EchoProvider::new()),
            budget(),
        )
        .unwrap();
        let right = ProviderRoute::new(
            BackendSelection::Abi,
            "model-b",
            Arc::new(ScriptedProvider::new("scripted")),
            budget(),
        )
        .unwrap();
        assert!(matches!(
            ModelProviderExecutor::new([left, right]),
            Err(ProviderConfigError::DuplicateBackend)
        ));
    }

    #[test]
    fn provider_event_model_substitution_fails_closed() {
        let provider =
            ScriptedProvider::new("scripted").with_event(ModelEvent::started("different-model"));
        let route = ProviderRoute::new(
            BackendSelection::FoundationModels,
            "startup-model",
            Arc::new(provider),
            budget(),
        )
        .unwrap();
        let executor = ModelProviderExecutor::new([route]).unwrap();
        let error = executor
            .execute(
                &RunId::new(),
                request(BackendSelection::FoundationModels),
                &CancellationToken::new(),
            )
            .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::Unsupported);
    }
}
