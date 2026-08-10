//! Fixed process recipes implementing ABI's provider-neutral model contract.
//!
//! These adapters reuse Abbey's existing backend argv builders and bounded
//! process-group supervisor. Executables, models, workspace, environment, and
//! process limits are canonicalized once at startup. A [`ModelRequest`] may
//! supply only one user message; it cannot reshape execution authority.
//! Vendor CLIs do not expose trustworthy tokenizer usage here, so the adapters
//! report no invented token count and reject per-request token hints. Output is
//! instead hard-bounded by the startup-owned byte limits and ABI capture cap.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use abi_agent_runtime::{
    Flow, ModelEvent, ModelProvider, ModelRequest, Role, RunContext, RuntimeError,
};

use super::valid_identifier;
use crate::agent::{AgentBackend, AgentConfig, looks_like_flags};
use crate::app_core::BackendSelection;
use crate::runtime::delegated::{
    DelegatedLimits, canonical_directory, canonical_executable, selected_environment,
};
use crate::runtime::supervisor::{
    ProcessEnvironment, ProcessSpec, SupervisorOutcome, run_with_checkpoint,
};

const MAX_MODEL_BYTES: usize = 256;
const MAX_INPUT_BYTES: usize = 32 * 1_024;

/// One existing vendor/runtime grammar wrapped as an ABI [`ModelProvider`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedProviderKind {
    /// Existing `cursor-agent` one-shot print grammar.
    Cursor,
    /// Existing `grok` one-shot print grammar.
    Grok,
    /// Existing offline `abi complete --model local` grammar.
    AbiLocal,
    /// Existing Apple `fm respond` grammar with an exact `system` or `pcc` model.
    FoundationModels,
}

impl FixedProviderKind {
    /// Closed Abbey backend selected by this adapter.
    #[must_use]
    pub const fn backend(self) -> BackendSelection {
        match self {
            Self::Cursor => BackendSelection::Cursor,
            Self::Grok => BackendSelection::Grok,
            Self::AbiLocal => BackendSelection::Abi,
            Self::FoundationModels => BackendSelection::FoundationModels,
        }
    }

    const fn agent_backend(self) -> AgentBackend {
        match self {
            Self::Cursor => AgentBackend::Cursor,
            Self::Grok => AgentBackend::Grok,
            Self::AbiLocal => AgentBackend::Abi,
            Self::FoundationModels => AgentBackend::Fm,
        }
    }

    const fn provider_id(self) -> &'static str {
        match self {
            Self::Cursor => "abbey.cursor.fixed-recipe",
            Self::Grok => "abbey.grok.fixed-recipe",
            Self::AbiLocal => "abbey.abi-local.fixed-recipe",
            Self::FoundationModels => "abbey.foundation-models.fixed-recipe",
        }
    }
}

/// Invalid startup-owned fixed provider configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedRecipeProviderError {
    /// The startup-bound program did not resolve to an executable file.
    InvalidExecutable,
    /// The startup-bound logical workspace did not resolve to a directory.
    InvalidWorkspace,
    /// The model was malformed or would silently change the selected route.
    InvalidModel,
    /// A process timeout, stream, teardown, or polling bound was invalid.
    InvalidLimits,
}

impl fmt::Display for FixedRecipeProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidExecutable => "fixed provider executable is invalid",
            Self::InvalidWorkspace => "fixed provider workspace is invalid",
            Self::InvalidModel => "fixed provider model is invalid",
            Self::InvalidLimits => "fixed provider limits are invalid",
        })
    }
}

impl std::error::Error for FixedRecipeProviderError {}

/// Startup-owned adapter over one existing Abbey backend executable.
pub struct FixedRecipeProvider {
    kind: FixedProviderKind,
    executable: PathBuf,
    workspace: PathBuf,
    model: String,
    limits: DelegatedLimits,
    environment: Vec<(OsString, OsString)>,
}

impl FixedRecipeProvider {
    /// Validate and freeze one executable, model, workspace, and limit set.
    pub fn new(
        kind: FixedProviderKind,
        executable: impl AsRef<Path>,
        workspace: impl AsRef<Path>,
        model: impl Into<String>,
        limits: DelegatedLimits,
    ) -> Result<Self, FixedRecipeProviderError> {
        let model = model.into();
        if !valid_model(kind, &model) {
            return Err(FixedRecipeProviderError::InvalidModel);
        }
        limits
            .validate()
            .map_err(|_| FixedRecipeProviderError::InvalidLimits)?;
        let executable = canonical_executable(executable.as_ref(), kind.backend())
            .map_err(|_| FixedRecipeProviderError::InvalidExecutable)?;
        let workspace = canonical_directory(workspace.as_ref())
            .map_err(|_| FixedRecipeProviderError::InvalidWorkspace)?;
        Ok(Self {
            kind,
            executable,
            workspace,
            model,
            limits,
            environment: selected_environment(std::env::vars_os()),
        })
    }

    /// Adapter grammar selected at startup.
    #[must_use]
    pub const fn kind(&self) -> FixedProviderKind {
        self.kind
    }

    /// Exact startup-selected model.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    fn request_input<'a>(&self, request: &'a ModelRequest) -> Result<&'a str, RuntimeError> {
        let valid_shape = request.model() == self.model
            && request.system().is_none()
            && request.tools().is_empty()
            && request.max_output_tokens().is_none()
            && request.messages().len() == 1
            && request.messages()[0].role == Role::User;
        if !valid_shape {
            return Err(self.failure("request shape is unsupported"));
        }
        let input = request.messages()[0].content.as_str();
        if input.is_empty() || input.len() > MAX_INPUT_BYTES || input.contains('\0') {
            return Err(self.failure("request input is invalid"));
        }
        if self.kind != FixedProviderKind::AbiLocal && looks_like_flags(&[input.to_owned()]) {
            return Err(self.failure("flag-shaped input is unsupported"));
        }
        Ok(input)
    }

    fn process_spec(&self, input: &str) -> ProcessSpec {
        let config = AgentConfig::fixed_provider_recipe(
            self.executable.clone(),
            self.kind.agent_backend(),
            self.model.clone(),
        );
        ProcessSpec {
            program: self.executable.clone(),
            args: config
                .build_args(None, &[input.to_owned()])
                .into_iter()
                .map(OsString::from)
                .collect(),
            current_dir: Some(self.workspace.clone()),
            environment: ProcessEnvironment::ClearAndSet(self.environment.clone()),
        }
    }

    fn failure(&self, message: &'static str) -> RuntimeError {
        RuntimeError::Provider {
            provider: self.id().to_owned(),
            message: message.to_owned(),
        }
    }
}

impl fmt::Debug for FixedRecipeProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FixedRecipeProvider")
            .field("kind", &self.kind)
            .field("executable", &"[BOUND]")
            .field("workspace", &"[BOUND]")
            .field("model", &self.model)
            .field("limits", &self.limits)
            .field("environment_entries", &self.environment.len())
            .finish()
    }
}

impl ModelProvider for FixedRecipeProvider {
    fn id(&self) -> &str {
        self.kind.provider_id()
    }

    fn run(&self, request: &ModelRequest, run: &mut RunContext<'_>) -> Result<(), RuntimeError> {
        let input = self.request_input(request)?;
        if run.emit(&ModelEvent::started(&self.model)) == Flow::Stop {
            return Ok(());
        }
        let outcome =
            run_with_checkpoint(&self.process_spec(input), &self.limits.supervisor(), || {
                run.checkpoint() == Flow::Stop
            })
            .map_err(|_| self.failure("process supervision failed"))?;

        match outcome {
            SupervisorOutcome::Exited { status, stdout, .. } if status.success() => {
                if stdout.is_empty() {
                    return Err(self.failure("provider returned no output"));
                }
                let output = String::from_utf8(stdout)
                    .map_err(|_| self.failure("provider output was not UTF-8"))?;
                let _ = run.emit(&ModelEvent::text(output));
                Ok(())
            }
            SupervisorOutcome::Cancelled => Ok(()),
            SupervisorOutcome::TimedOut => Err(self.failure("provider process timed out")),
            SupervisorOutcome::StdoutLimit | SupervisorOutcome::StderrLimit => {
                Err(self.failure("provider exceeded an output limit"))
            }
            SupervisorOutcome::Exited { .. } => {
                Err(self.failure("provider process exited unsuccessfully"))
            }
        }
    }
}

fn valid_model(kind: FixedProviderKind, model: &str) -> bool {
    if !valid_identifier(model, MAX_MODEL_BYTES) {
        return false;
    }
    match kind {
        FixedProviderKind::AbiLocal => model == "local",
        FixedProviderKind::FoundationModels => matches!(model, "system" | "pcc"),
        FixedProviderKind::Cursor | FixedProviderKind::Grok => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi_agent_runtime::{
        CancellationToken, CollectingSink, ModelRequest, RunBudget, StopReason, ToolSpec,
        run_provider,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new(label: &str, body: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "abbey-fixed-provider-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let executable = root.join("provider");
            std::fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
                permissions.set_mode(0o700);
                std::fs::set_permissions(&executable, permissions).unwrap();
            }
            Self { root }
        }

        fn executable(&self) -> PathBuf {
            self.root.join("provider")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn limits() -> DelegatedLimits {
        DelegatedLimits {
            timeout: Duration::from_secs(2),
            terminate_grace: Duration::from_secs(1),
            stdout_bytes: 1024,
            stderr_bytes: 1024,
            poll_interval: Duration::from_millis(2),
        }
    }

    fn provider(scratch: &Scratch, kind: FixedProviderKind, model: &str) -> FixedRecipeProvider {
        FixedRecipeProvider::new(kind, scratch.executable(), &scratch.root, model, limits())
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn every_adapter_uses_its_existing_least_authority_argv_recipe() {
        let scratch = Scratch::new("argv", "printf unused");
        for (kind, model, expected) in [
            (
                FixedProviderKind::Cursor,
                "cursor-model",
                vec!["--model", "cursor-model", "--print", "hello"],
            ),
            (
                FixedProviderKind::Grok,
                "grok-model",
                vec!["--model", "grok-model", "--print", "hello"],
            ),
            (
                FixedProviderKind::AbiLocal,
                "local",
                vec!["complete", "--model", "local", "--", "hello"],
            ),
            (
                FixedProviderKind::FoundationModels,
                "system",
                vec!["respond", "--model", "system", "--no-stream", "hello"],
            ),
        ] {
            let provider = provider(&scratch, kind, model);
            let spec = provider.process_spec("hello");
            let args = spec
                .args
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert_eq!(args, expected, "{kind:?}");
            assert!(
                !args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "--trust" | "--force"))
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn every_adapter_runs_through_the_abi_event_contract() {
        let scratch = Scratch::new(
            "events",
            "for last do :; done; printf '%s:%s' \"$PWD\" \"$last\"",
        );
        for (kind, model) in [
            (FixedProviderKind::Cursor, "cursor-model"),
            (FixedProviderKind::Grok, "grok-model"),
            (FixedProviderKind::AbiLocal, "local"),
            (FixedProviderKind::FoundationModels, "system"),
        ] {
            let provider = provider(&scratch, kind, model);
            let mut sink = CollectingSink::new();
            let report = run_provider(
                &provider,
                &ModelRequest::new(model).with_user("hello"),
                &mut sink,
                &CancellationToken::new(),
                RunBudget::unlimited()
                    .with_max_events(8)
                    .with_max_output_tokens(4096)
                    .with_max_duration(Duration::from_secs(2)),
            )
            .unwrap();
            assert_eq!(report.stop, StopReason::Completed);
            assert_eq!(sink.kinds(), vec!["started", "text_delta", "finished"]);
            assert_eq!(
                report.text(),
                format!(
                    "{}:hello",
                    std::fs::canonicalize(&scratch.root).unwrap().display()
                )
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn request_data_cannot_change_model_tools_or_process_options() {
        let scratch = Scratch::new("request-refusal", "printf should-not-run");
        let provider = provider(&scratch, FixedProviderKind::Cursor, "fixed-model");
        for request in [
            ModelRequest::new("different").with_user("hello"),
            ModelRequest::new("fixed-model")
                .with_user("hello")
                .with_tool(ToolSpec::new("shell.exec")),
            ModelRequest::new("fixed-model")
                .with_system("override")
                .with_user("hello"),
            ModelRequest::new("fixed-model").with_user("--force"),
            ModelRequest::new("fixed-model")
                .with_user("hello")
                .with_max_output_tokens(10),
        ] {
            let mut sink = CollectingSink::new();
            let error = run_provider(
                &provider,
                &request,
                &mut sink,
                &CancellationToken::new(),
                RunBudget::unlimited(),
            )
            .unwrap_err();
            assert_eq!(sink.kinds(), vec!["finished"]);
            assert!(!error.to_string().contains("--force"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_checkpoint_tears_down_a_running_process_group() {
        let scratch = Scratch::new("cancel", "touch ready; trap '' TERM; sleep 30");
        let provider = Arc::new(provider(&scratch, FixedProviderKind::AbiLocal, "local"));
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker = Arc::clone(&provider);
        let handle = std::thread::spawn(move || {
            let mut sink = CollectingSink::new();
            run_provider(
                worker.as_ref(),
                &ModelRequest::new("local").with_user("hello"),
                &mut sink,
                &worker_token,
                RunBudget::unlimited()
                    .with_max_events(8)
                    .with_max_output_tokens(4096)
                    .with_max_duration(Duration::from_secs(10)),
            )
            .unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !scratch.root.join("ready").is_file() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(scratch.root.join("ready").is_file());
        let cancelled_at = Instant::now();
        token.cancel();
        let report = handle.join().unwrap();
        assert_eq!(report.stop, StopReason::Cancelled);
        assert!(cancelled_at.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn provider_failure_is_bounded_and_redacted() {
        let scratch = Scratch::new("redaction", "printf 'secret stderr' >&2; exit 7");
        let provider = provider(&scratch, FixedProviderKind::Grok, "grok-model");
        let mut sink = CollectingSink::new();
        let error = run_provider(
            &provider,
            &ModelRequest::new("grok-model").with_user("secret prompt"),
            &mut sink,
            &CancellationToken::new(),
            RunBudget::unlimited().with_max_duration(Duration::from_secs(2)),
        )
        .unwrap_err();
        let rendered = error.to_string();
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains(scratch.root.to_string_lossy().as_ref()));
        assert_eq!(sink.kinds(), vec!["started", "finished"]);
    }

    #[cfg(unix)]
    #[test]
    fn local_and_foundation_models_forbid_silent_route_substitution() {
        let scratch = Scratch::new("models", "printf unused");
        assert!(matches!(
            FixedRecipeProvider::new(
                FixedProviderKind::AbiLocal,
                scratch.executable(),
                &scratch.root,
                "live",
                limits(),
            ),
            Err(FixedRecipeProviderError::InvalidModel)
        ));
        assert!(matches!(
            FixedRecipeProvider::new(
                FixedProviderKind::FoundationModels,
                scratch.executable(),
                &scratch.root,
                "private-cloud-compute",
                limits(),
            ),
            Err(FixedRecipeProviderError::InvalidModel)
        ));
    }
}
