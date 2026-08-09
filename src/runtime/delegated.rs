//! Fixed-recipe delegated execution over the bounded Unix supervisor.
//!
//! This adapter deliberately supports only non-tool-capable ABI-local and
//! Foundation Models recipes. A [`RunRequest`] cannot provide a program, argv
//! prefix, environment, workspace, model, or trust flag.

use super::executor::{CancellationToken, ExecutionError, ExecutionErrorKind, Executor};
use super::supervisor::{
    ProcessEnvironment, ProcessSpec, SupervisorError, SupervisorLimits, SupervisorOutcome, run,
};
use crate::app_core::{BackendSelection, RunId, RunMode, RunRequest};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_TERMINATE_GRACE: Duration = Duration::from_secs(5);
const MAX_OUTPUT_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_POLL_INTERVAL: Duration = Duration::from_millis(100);
const BENIGN_ENVIRONMENT: [&str; 8] = [
    "HOME", "LANG", "LC_ALL", "LC_CTYPE", "TEMP", "TMP", "TMPDIR", "TZ",
];

/// Bounded operating limits for one delegated child process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelegatedLimits {
    pub timeout: Duration,
    pub terminate_grace: Duration,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub poll_interval: Duration,
}

impl Default for DelegatedLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            terminate_grace: Duration::from_millis(500),
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 1024 * 1024,
            poll_interval: Duration::from_millis(10),
        }
    }
}

impl DelegatedLimits {
    pub fn validate(self) -> Result<(), DelegatedConfigError> {
        if self.timeout.is_zero() || self.timeout > MAX_TIMEOUT {
            return Err(DelegatedConfigError::InvalidLimits);
        }
        if self.terminate_grace.is_zero() || self.terminate_grace > MAX_TERMINATE_GRACE {
            return Err(DelegatedConfigError::InvalidLimits);
        }
        if self.stdout_bytes == 0
            || self.stdout_bytes > MAX_OUTPUT_BYTES
            || self.stderr_bytes == 0
            || self.stderr_bytes > MAX_OUTPUT_BYTES
        {
            return Err(DelegatedConfigError::InvalidLimits);
        }
        if self.poll_interval.is_zero() || self.poll_interval > MAX_POLL_INTERVAL {
            return Err(DelegatedConfigError::InvalidLimits);
        }
        Ok(())
    }

    fn supervisor(self) -> SupervisorLimits {
        SupervisorLimits {
            timeout: self.timeout,
            terminate_grace: self.terminate_grace,
            stdout_bytes: self.stdout_bytes,
            stderr_bytes: self.stderr_bytes,
            poll_interval: self.poll_interval,
        }
    }
}

/// Configuration failure that never renders a configured filesystem path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegatedConfigError {
    InvalidWorkspace,
    InvalidExecutable(BackendSelection),
    InvalidLimits,
}

impl fmt::Display for DelegatedConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWorkspace => formatter.write_str("delegated workspace is invalid"),
            Self::InvalidExecutable(backend) => {
                write!(
                    formatter,
                    "delegated {} executable is invalid",
                    backend_label(*backend)
                )
            }
            Self::InvalidLimits => formatter.write_str("delegated process limits are invalid"),
        }
    }
}

impl std::error::Error for DelegatedConfigError {}

/// Startup-owned delegated executor configuration.
///
/// Fields are private so request data cannot reshape process authority.
pub struct DelegatedExecutorConfig {
    workspace: PathBuf,
    abi_local: Option<PathBuf>,
    foundation_models: Option<PathBuf>,
    limits: DelegatedLimits,
    environment: Vec<(OsString, OsString)>,
}

impl DelegatedExecutorConfig {
    pub fn new(workspace: impl AsRef<Path>) -> Result<Self, DelegatedConfigError> {
        let workspace = canonical_directory(workspace.as_ref())?;
        Ok(Self {
            workspace,
            abi_local: None,
            foundation_models: None,
            limits: DelegatedLimits::default(),
            environment: selected_environment(std::env::vars_os()),
        })
    }

    pub fn bind_abi_local(
        mut self,
        executable: impl AsRef<Path>,
    ) -> Result<Self, DelegatedConfigError> {
        self.abi_local = Some(canonical_executable(
            executable.as_ref(),
            BackendSelection::Abi,
        )?);
        Ok(self)
    }

    pub fn bind_foundation_models(
        mut self,
        executable: impl AsRef<Path>,
    ) -> Result<Self, DelegatedConfigError> {
        self.foundation_models = Some(canonical_executable(
            executable.as_ref(),
            BackendSelection::FoundationModels,
        )?);
        Ok(self)
    }

    pub fn with_limits(mut self, limits: DelegatedLimits) -> Result<Self, DelegatedConfigError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }

    #[cfg(test)]
    fn with_test_environment<I, K, V>(mut self, environment: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.environment = selected_environment(
            environment
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }
}

/// Delegates a validated request through one of two fixed, non-tool recipes.
pub struct DelegatedExecutor {
    config: DelegatedExecutorConfig,
}

impl DelegatedExecutor {
    #[must_use]
    pub fn new(config: DelegatedExecutorConfig) -> Self {
        Self { config }
    }

    fn spec(&self, request: &RunRequest) -> Result<ProcessSpec, ExecutionError> {
        request.validate().map_err(|_| {
            ExecutionError::with_kind(
                ExecutionErrorKind::Unsupported,
                "delegated request is invalid",
            )
        })?;
        if matches!(request.mode, RunMode::Interactive | RunMode::Automation) {
            return Err(unsupported());
        }

        let (program, args) = match request.backend {
            BackendSelection::Abi => (
                self.config.abi_local.as_ref().ok_or_else(unsupported)?,
                vec![
                    OsString::from("complete"),
                    OsString::from("--model"),
                    OsString::from("local"),
                    OsString::from("--"),
                    OsString::from(request.input.as_str()),
                ],
            ),
            BackendSelection::FoundationModels => (
                self.config
                    .foundation_models
                    .as_ref()
                    .ok_or_else(unsupported)?,
                vec![
                    OsString::from("respond"),
                    OsString::from("--model"),
                    OsString::from("system"),
                    OsString::from("--no-stream"),
                    OsString::from(request.input.as_str()),
                ],
            ),
            BackendSelection::Cursor | BackendSelection::Grok => return Err(unsupported()),
        };

        Ok(ProcessSpec {
            program: program.clone(),
            args,
            current_dir: Some(self.config.workspace.clone()),
            environment: ProcessEnvironment::ClearAndSet(self.config.environment.clone()),
        })
    }
}

impl Executor for DelegatedExecutor {
    fn execute(
        &self,
        _run_id: &RunId,
        request: RunRequest,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        let spec = self.spec(&request)?;
        let outcome = run(&spec, &self.config.limits.supervisor(), cancellation)
            .map_err(map_supervisor_error)?;
        match outcome {
            SupervisorOutcome::Exited { status, .. } if status.success() => Ok(()),
            SupervisorOutcome::Exited { .. } => Err(ExecutionError::with_kind(
                ExecutionErrorKind::ProviderExit,
                generic_message(ExecutionErrorKind::ProviderExit),
            )),
            SupervisorOutcome::Cancelled if cancellation.is_cancelled() => Ok(()),
            SupervisorOutcome::Cancelled => Err(ExecutionError::with_kind(
                ExecutionErrorKind::Teardown,
                generic_message(ExecutionErrorKind::Teardown),
            )),
            SupervisorOutcome::TimedOut => Err(ExecutionError::with_kind(
                ExecutionErrorKind::TimedOut,
                generic_message(ExecutionErrorKind::TimedOut),
            )),
            SupervisorOutcome::StdoutLimit | SupervisorOutcome::StderrLimit => {
                Err(ExecutionError::with_kind(
                    ExecutionErrorKind::OutputLimit,
                    generic_message(ExecutionErrorKind::OutputLimit),
                ))
            }
        }
    }
}

fn map_supervisor_error(error: SupervisorError) -> ExecutionError {
    let kind = if error.is_teardown() {
        ExecutionErrorKind::Teardown
    } else {
        match &error {
            SupervisorError::Invalid(_) => ExecutionErrorKind::Unsupported,
            #[cfg(not(unix))]
            SupervisorError::Unsupported => ExecutionErrorKind::Unsupported,
            SupervisorError::Spawn(_) | SupervisorError::Pipe(_) => ExecutionErrorKind::Spawn,
            SupervisorError::Wait(_)
            | SupervisorError::Reader(_, _)
            | SupervisorError::ReaderThread(_) => ExecutionErrorKind::Teardown,
            SupervisorError::Teardown(_) => unreachable!("handled by is_teardown"),
        }
    };
    ExecutionError::with_kind(kind, generic_message(kind))
}

fn unsupported() -> ExecutionError {
    ExecutionError::with_kind(
        ExecutionErrorKind::Unsupported,
        generic_message(ExecutionErrorKind::Unsupported),
    )
}

fn generic_message(kind: ExecutionErrorKind) -> &'static str {
    match kind {
        ExecutionErrorKind::General => "delegated execution failed",
        ExecutionErrorKind::Unsupported => "delegated execution is unsupported",
        ExecutionErrorKind::Spawn => "delegated process could not start",
        ExecutionErrorKind::TimedOut => "delegated process timed out",
        ExecutionErrorKind::OutputLimit => "delegated process exceeded an output limit",
        ExecutionErrorKind::ProviderExit => "delegated provider exited unsuccessfully",
        ExecutionErrorKind::Teardown => "delegated process teardown failed",
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, DelegatedConfigError> {
    let canonical = fs::canonicalize(path).map_err(|_| DelegatedConfigError::InvalidWorkspace)?;
    if !canonical.is_dir() {
        return Err(DelegatedConfigError::InvalidWorkspace);
    }
    Ok(canonical)
}

fn canonical_executable(
    path: &Path,
    backend: BackendSelection,
) -> Result<PathBuf, DelegatedConfigError> {
    let invalid = || DelegatedConfigError::InvalidExecutable(backend);
    let canonical = fs::canonicalize(path).map_err(|_| invalid())?;
    let metadata = fs::metadata(&canonical).map_err(|_| invalid())?;
    if !metadata.is_file() || !is_executable(&metadata) {
        return Err(invalid());
    }
    Ok(canonical)
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

fn selected_environment<I>(environment: I) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let mut selected = environment
        .into_iter()
        .filter(|(key, _)| {
            BENIGN_ENVIRONMENT
                .iter()
                .any(|allowed| key == OsStr::new(allowed))
        })
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| left.0.cmp(&right.0));
    selected.dedup_by(|left, right| left.0 == right.0);
    selected
}

fn backend_label(backend: BackendSelection) -> &'static str {
    match backend {
        BackendSelection::Cursor => "cursor",
        BackendSelection::Abi => "abi-local",
        BackendSelection::FoundationModels => "foundation-models",
        BackendSelection::Grok => "grok",
    }
}

#[cfg(test)]
mod manager_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::{IdempotencyKey, RunMode};
    use crate::runtime::supervisor::StreamName;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::Instant;

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "abbey-delegated-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        #[cfg(unix)]
        fn script(&self, name: &str, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt as _;
            let path = self.0.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(backend: BackendSelection, mode: RunMode, input: &str) -> RunRequest {
        RunRequest {
            idempotency_key: "delegated-fixture".parse::<IdempotencyKey>().unwrap(),
            conversation_id: None,
            mode,
            backend,
            input: input.into(),
            labels: vec!["ignored-authority-label".into()],
        }
    }

    fn execute(executor: &DelegatedExecutor, request: RunRequest) -> Result<(), ExecutionError> {
        executor.execute(&RunId::new(), request, &CancellationToken::default())
    }

    #[test]
    fn limits_reject_zero_and_unbounded_values() {
        for limits in [
            DelegatedLimits {
                timeout: Duration::ZERO,
                ..DelegatedLimits::default()
            },
            DelegatedLimits {
                terminate_grace: Duration::from_secs(6),
                ..DelegatedLimits::default()
            },
            DelegatedLimits {
                stdout_bytes: MAX_OUTPUT_BYTES + 1,
                ..DelegatedLimits::default()
            },
            DelegatedLimits {
                poll_interval: Duration::from_millis(101),
                ..DelegatedLimits::default()
            },
        ] {
            assert_eq!(limits.validate(), Err(DelegatedConfigError::InvalidLimits));
        }
    }

    #[test]
    fn environment_selection_is_fixed_and_excludes_daemon_secrets() {
        let selected = selected_environment([
            (OsString::from("HOME"), OsString::from("/safe/home")),
            (OsString::from("LANG"), OsString::from("C")),
            (
                OsString::from("ABBEYD_BEARER_TOKEN"),
                OsString::from("do-not-forward"),
            ),
            (
                OsString::from("ABBEYD_BEARER_TOKEN_FILE"),
                OsString::from("/secret"),
            ),
            (
                OsString::from("OPENAI_API_KEY"),
                OsString::from("do-not-forward"),
            ),
        ]);
        assert_eq!(
            selected,
            [
                (OsString::from("HOME"), OsString::from("/safe/home")),
                (OsString::from("LANG"), OsString::from("C")),
            ]
        );
    }

    #[test]
    fn supervisor_errors_map_to_stable_redacted_execution_kinds() {
        let cases = vec![
            #[cfg(not(unix))]
            (
                SupervisorError::Unsupported,
                ExecutionErrorKind::Unsupported,
            ),
            (
                SupervisorError::Invalid("private prompt in invalid specification"),
                ExecutionErrorKind::Unsupported,
            ),
            (
                SupervisorError::Spawn(std::io::Error::other(
                    "/private/provider-path contained bearer-secret",
                )),
                ExecutionErrorKind::Spawn,
            ),
            (
                SupervisorError::Pipe("private-output-stream"),
                ExecutionErrorKind::Spawn,
            ),
            (
                SupervisorError::Wait(std::io::Error::other("/private/provider-path wait failed")),
                ExecutionErrorKind::Teardown,
            ),
            (
                SupervisorError::Reader(
                    StreamName::Stdout,
                    std::io::Error::other("private provider output read failed"),
                ),
                ExecutionErrorKind::Teardown,
            ),
            (
                SupervisorError::ReaderThread(StreamName::Stderr),
                ExecutionErrorKind::Teardown,
            ),
            (
                SupervisorError::Teardown("/private/provider-path retained private output".into()),
                ExecutionErrorKind::Teardown,
            ),
        ];
        for (source, expected) in cases {
            let error = map_supervisor_error(source);
            assert_eq!(error.kind(), expected);
            let rendered = error.to_string();
            assert!(!rendered.contains("private"));
            assert!(!rendered.contains("bearer-secret"));
            assert!(!rendered.contains("provider-path"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn abi_recipe_is_exact_literal_and_environment_is_scrubbed() {
        let scratch = ScratchDir::new("abi-exact");
        let marker = scratch.0.join("injection-marker");
        let input = format!("$(/usr/bin/touch {}) ; --live", marker.display());
        let script = scratch.script(
            "abi",
            &format!(
                "[ \"$#\" -eq 5 ] || exit 11\n\
                 [ \"$1\" = complete ] || exit 12\n\
                 [ \"$2\" = --model ] || exit 13\n\
                 [ \"$3\" = local ] || exit 14\n\
                 [ \"$4\" = -- ] || exit 15\n\
                 [ \"$5\" = '{}' ] || exit 16\n\
                 [ \"$HOME\" = /safe/home ] || exit 17\n\
                 [ -z \"${{ABBEYD_BEARER_TOKEN+x}}\" ] || exit 18\n\
                 [ -z \"${{OPENAI_API_KEY+x}}\" ] || exit 19",
                input
            ),
        );
        let config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&script)
            .unwrap()
            .with_test_environment([
                ("ABBEYD_BEARER_TOKEN", "do-not-forward"),
                ("OPENAI_API_KEY", "do-not-forward"),
                ("HOME", "/safe/home"),
            ]);
        let executor = DelegatedExecutor::new(config);
        execute(
            &executor,
            request(BackendSelection::Abi, RunMode::OneShot, &input),
        )
        .unwrap();
        assert!(
            !marker.exists(),
            "metacharacter input was executed by a shell"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fixed_abi_and_fm_recipes_succeed_without_authority_from_labels() {
        let scratch = ScratchDir::new("recipes");
        let abi = scratch.script(
            "abi",
            "[ \"$*\" = \"complete --model local -- literal-input\" ] || exit 21",
        );
        let fm = scratch.script(
            "fm",
            "[ \"$*\" = \"respond --model system --no-stream literal-input\" ] || exit 22",
        );
        let config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&abi)
            .unwrap()
            .bind_foundation_models(&fm)
            .unwrap();
        let executor = DelegatedExecutor::new(config);
        execute(
            &executor,
            request(BackendSelection::Abi, RunMode::OneShot, "literal-input"),
        )
        .unwrap();
        execute(
            &executor,
            request(
                BackendSelection::FoundationModels,
                RunMode::Background,
                "literal-input",
            ),
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_backends_and_modes_fail_before_spawn() {
        let scratch = ScratchDir::new("unsupported");
        let marker = scratch.0.join("spawned");
        let script = scratch.script(
            "abi",
            &format!("/usr/bin/touch '{}'\nexit 0", marker.display()),
        );
        let config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&script)
            .unwrap();
        let executor = DelegatedExecutor::new(config);
        for request in [
            request(BackendSelection::Cursor, RunMode::OneShot, "input"),
            request(BackendSelection::Grok, RunMode::Background, "input"),
            request(BackendSelection::Abi, RunMode::Interactive, "input"),
            request(BackendSelection::Abi, RunMode::Automation, "input"),
        ] {
            let error = execute(&executor, request).unwrap_err();
            assert_eq!(error.kind(), ExecutionErrorKind::Unsupported);
        }
        assert!(
            !marker.exists(),
            "unsupported request reached process spawn"
        );
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_does_not_disclose_stderr_prompt_or_path() {
        let scratch = ScratchDir::new("nonzero");
        let secret = "provider-secret-output";
        let prompt = "private prompt";
        let script = scratch.script("abi", &format!("printf '{secret}' >&2\nexit 23"));
        let config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&script)
            .unwrap();
        let error = execute(
            &DelegatedExecutor::new(config),
            request(BackendSelection::Abi, RunMode::OneShot, prompt),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::ProviderExit);
        let rendered = error.to_string();
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains(prompt));
        assert!(!rendered.contains(script.to_string_lossy().as_ref()));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_output_limit_map_to_stable_generic_kinds() {
        let scratch = ScratchDir::new("limits");
        let sleeper = scratch.script("sleep", "exec /bin/sleep 30");
        let noisy = scratch.script("noisy", "printf 12345678901234567\nexec /bin/sleep 30");
        let timeout_limits = DelegatedLimits {
            timeout: Duration::from_millis(80),
            terminate_grace: Duration::from_millis(500),
            stdout_bytes: 16,
            stderr_bytes: 16,
            poll_interval: Duration::from_millis(5),
        };

        let timeout_config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&sleeper)
            .unwrap()
            .with_limits(timeout_limits)
            .unwrap();
        let started = Instant::now();
        let error = execute(
            &DelegatedExecutor::new(timeout_config),
            request(BackendSelection::Abi, RunMode::OneShot, "private timeout"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));

        let output_config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&noisy)
            .unwrap()
            .with_limits(DelegatedLimits {
                timeout: Duration::from_secs(2),
                ..timeout_limits
            })
            .unwrap();
        let error = execute(
            &DelegatedExecutor::new(output_config),
            request(BackendSelection::Abi, RunMode::OneShot, "private output"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ExecutionErrorKind::OutputLimit);
        assert!(!error.to_string().contains("private output"));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_with_successful_teardown_returns_ok() {
        let scratch = ScratchDir::new("cancel");
        let script = scratch.script("abi", "exec /bin/sleep 30");
        let limits = DelegatedLimits {
            timeout: Duration::from_secs(5),
            terminate_grace: Duration::from_millis(40),
            stdout_bytes: 1024,
            stderr_bytes: 1024,
            poll_interval: Duration::from_millis(5),
        };
        let config = DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&script)
            .unwrap()
            .with_limits(limits)
            .unwrap();
        let executor = Arc::new(DelegatedExecutor::new(config));
        let token = CancellationToken::default();
        let child_token = token.clone();
        let handle = thread::spawn(move || {
            executor.execute(
                &RunId::new(),
                request(BackendSelection::Abi, RunMode::Background, "cancel"),
                &child_token,
            )
        });
        thread::sleep(Duration::from_millis(50));
        token.cancel();
        assert!(handle.join().unwrap().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn paths_are_canonical_regular_executables_and_errors_are_redacted() {
        let scratch = ScratchDir::new("paths");
        let missing = scratch.0.join("missing-secret-name");
        let error = match DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&missing)
        {
            Ok(_) => panic!("missing executable unexpectedly accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            DelegatedConfigError::InvalidExecutable(BackendSelection::Abi)
        );
        assert!(!error.to_string().contains("missing-secret-name"));

        let directory_error = match DelegatedExecutorConfig::new(&scratch.0)
            .unwrap()
            .bind_abi_local(&scratch.0)
        {
            Ok(_) => panic!("directory unexpectedly accepted as executable"),
            Err(error) => error,
        };
        assert_eq!(
            directory_error,
            DelegatedConfigError::InvalidExecutable(BackendSelection::Abi)
        );
    }
}
