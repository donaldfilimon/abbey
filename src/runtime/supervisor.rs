//! Crate-private, bounded Unix child-process supervision.
//! This Abbey adapter primitive is neither a public command runner nor a shell.

use super::CancellationToken;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 32 * 1024;
const MAX_TOTAL_ARG_BYTES: usize = 64 * 1024;
const MAX_ENVIRONMENT: usize = 128;
const MAX_ENV_BYTES: usize = 64 * 1024;
const MAX_STREAM_BYTES: usize = 4 * 1024 * 1024;
const MAX_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_TERMINATE_GRACE: Duration = Duration::from_secs(5);
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ProcessEnvironment {
    Inherit,
    ClearAndSet(Vec<(OsString, OsString)>),
}

impl fmt::Debug for ProcessEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inherit => formatter.write_str("Inherit"),
            Self::ClearAndSet(environment) => formatter
                .debug_struct("ClearAndSet")
                .field("entries", &environment.len())
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub environment: ProcessEnvironment,
}

impl fmt::Debug for ProcessSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessSpec")
            .field("program", &"<redacted>")
            .field("argument_count", &self.args.len())
            .field(
                "argument_bytes",
                &self
                    .args
                    .iter()
                    .map(|argument| os_len(argument))
                    .fold(0_usize, usize::saturating_add),
            )
            .field("has_current_dir", &self.current_dir.is_some())
            .field("environment", &self.environment)
            .finish()
    }
}

impl ProcessSpec {
    #[must_use]
    pub(crate) fn inherited(program: PathBuf, args: Vec<OsString>) -> Self {
        Self {
            program,
            args,
            current_dir: None,
            environment: ProcessEnvironment::Inherit,
        }
    }

    fn validate(&self) -> Result<(PathBuf, Option<PathBuf>), SupervisorError> {
        if self.program.as_os_str().is_empty() || has_control(self.program.as_os_str()) {
            return Err(SupervisorError::Invalid("program cannot be empty"));
        }
        let canonical_program = std::fs::canonicalize(&self.program)
            .map_err(|_| SupervisorError::Invalid("program must resolve to a regular file"))?;
        if !canonical_program.is_file() {
            return Err(SupervisorError::Invalid(
                "program must resolve to a regular file",
            ));
        }
        let canonical_current_dir = self
            .current_dir
            .as_ref()
            .map(|current_dir| {
                let canonical_dir = std::fs::canonicalize(current_dir).map_err(|_| {
                    SupervisorError::Invalid("current directory must resolve to a directory")
                })?;
                if !canonical_dir.is_dir() {
                    return Err(SupervisorError::Invalid(
                        "current directory must resolve to a directory",
                    ));
                }
                Ok(canonical_dir)
            })
            .transpose()?;
        if self.args.len() > MAX_ARGS {
            return Err(SupervisorError::Invalid("argument count exceeds 128"));
        }
        let mut total_arg_bytes = 0_usize;
        for argument in &self.args {
            if os_len(argument) > MAX_ARG_BYTES || has_arg_control(argument) {
                return Err(SupervisorError::Invalid(
                    "an argument is invalid or exceeds 32768 bytes",
                ));
            }
            total_arg_bytes =
                total_arg_bytes
                    .checked_add(os_len(argument))
                    .ok_or(SupervisorError::Invalid(
                        "arguments exceed 65536 total bytes",
                    ))?;
        }
        if total_arg_bytes > MAX_TOTAL_ARG_BYTES {
            return Err(SupervisorError::Invalid(
                "arguments exceed 65536 total bytes",
            ));
        }
        if let ProcessEnvironment::ClearAndSet(environment) = &self.environment {
            validate_environment(environment)?;
        }
        Ok((canonical_program, canonical_current_dir))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SupervisorLimits {
    pub timeout: Duration,
    pub terminate_grace: Duration,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub poll_interval: Duration,
}

impl SupervisorLimits {
    fn validate(self) -> Result<(), SupervisorError> {
        if self.timeout.is_zero() || self.timeout > MAX_TIMEOUT {
            return Err(SupervisorError::Invalid(
                "timeout must be within 1ns..=1800s",
            ));
        }
        if self.terminate_grace.is_zero() || self.terminate_grace > MAX_TERMINATE_GRACE {
            return Err(SupervisorError::Invalid(
                "termination grace must be within 1ns..=5s",
            ));
        }
        if self.poll_interval.is_zero() || self.poll_interval > MAX_POLL_INTERVAL {
            return Err(SupervisorError::Invalid(
                "poll interval must be within 1ns..=1s",
            ));
        }
        if !(1..=MAX_STREAM_BYTES).contains(&self.stdout_bytes)
            || !(1..=MAX_STREAM_BYTES).contains(&self.stderr_bytes)
        {
            return Err(SupervisorError::Invalid(
                "stream limits must be within 1..=4194304 bytes",
            ));
        }
        Ok(())
    }
}

pub(crate) enum SupervisorOutcome {
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Cancelled,
    TimedOut,
    StdoutLimit,
    StderrLimit,
}

impl fmt::Debug for SupervisorOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited {
                status,
                stdout,
                stderr,
            } => formatter
                .debug_struct("Exited")
                .field("status", status)
                .field("stdout_bytes", &stdout.len())
                .field("stderr_bytes", &stderr.len())
                .finish(),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::TimedOut => formatter.write_str("TimedOut"),
            Self::StdoutLimit => formatter.write_str("StdoutLimit"),
            Self::StderrLimit => formatter.write_str("StderrLimit"),
        }
    }
}

pub(crate) enum SupervisorError {
    Invalid(&'static str),
    #[cfg(not(unix))]
    Unsupported,
    Spawn(std::io::Error),
    Pipe(&'static str),
    Wait(std::io::Error),
    Reader(StreamName, std::io::Error),
    ReaderThread(StreamName),
    Teardown(String),
}

impl fmt::Debug for SupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.debug_tuple("Invalid").field(message).finish(),
            #[cfg(not(unix))]
            Self::Unsupported => formatter.write_str("Unsupported"),
            Self::Spawn(error) => formatter.debug_tuple("Spawn").field(&error.kind()).finish(),
            Self::Pipe(stream) => formatter.debug_tuple("Pipe").field(stream).finish(),
            Self::Wait(error) => formatter.debug_tuple("Wait").field(&error.kind()).finish(),
            Self::Reader(stream, error) => formatter
                .debug_tuple("Reader")
                .field(stream)
                .field(&error.kind())
                .finish(),
            Self::ReaderThread(stream) => {
                formatter.debug_tuple("ReaderThread").field(stream).finish()
            }
            Self::Teardown(message) => formatter.debug_tuple("Teardown").field(message).finish(),
        }
    }
}

impl SupervisorError {
    #[must_use]
    pub(crate) fn is_teardown(&self) -> bool {
        matches!(self, Self::Teardown(_))
    }
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid process specification: {message}"),
            #[cfg(not(unix))]
            Self::Unsupported => formatter.write_str(
                "process supervision is supported only on Unix hosts with process groups",
            ),
            Self::Spawn(error) => write!(
                formatter,
                "spawn supervised process failed ({:?})",
                error.kind()
            ),
            Self::Pipe(stream) => write!(formatter, "capture supervised process {stream}"),
            Self::Wait(error) => write!(
                formatter,
                "wait for supervised process failed ({:?})",
                error.kind()
            ),
            Self::Reader(stream, error) => {
                write!(
                    formatter,
                    "read supervised process {stream} failed ({:?})",
                    error.kind()
                )
            }
            Self::ReaderThread(stream) => {
                write!(formatter, "supervised process {stream} reader panicked")
            }
            Self::Teardown(message) => write!(formatter, "supervised process teardown: {message}"),
        }
    }
}

impl std::error::Error for SupervisorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) | Self::Wait(error) | Self::Reader(_, error) => Some(error),
            Self::Invalid(_) | Self::Pipe(_) | Self::ReaderThread(_) | Self::Teardown(_) => None,
            #[cfg(not(unix))]
            Self::Unsupported => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamName {
    Stdout,
    Stderr,
}

impl fmt::Display for StreamName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdout => formatter.write_str("stdout"),
            Self::Stderr => formatter.write_str("stderr"),
        }
    }
}

#[cfg(unix)]
pub(crate) fn run(
    spec: &ProcessSpec,
    limits: &SupervisorLimits,
    cancellation: &CancellationToken,
) -> Result<SupervisorOutcome, SupervisorError> {
    run_with_checkpoint(spec, limits, || cancellation.is_cancelled())
}

#[cfg(unix)]
pub(crate) fn run_with_checkpoint(
    spec: &ProcessSpec,
    limits: &SupervisorLimits,
    checkpoint: impl FnMut() -> bool,
) -> Result<SupervisorOutcome, SupervisorError> {
    unix::run_with_checkpoint(spec, *limits, checkpoint)
}

#[cfg(not(unix))]
pub(crate) fn run(
    spec: &ProcessSpec,
    limits: &SupervisorLimits,
    _cancellation: &CancellationToken,
) -> Result<SupervisorOutcome, SupervisorError> {
    let _ = spec.validate()?;
    limits.validate()?;
    Err(SupervisorError::Unsupported)
}

#[cfg(not(unix))]
pub(crate) fn run_with_checkpoint(
    spec: &ProcessSpec,
    limits: &SupervisorLimits,
    _checkpoint: impl FnMut() -> bool,
) -> Result<SupervisorOutcome, SupervisorError> {
    let _ = spec.validate()?;
    limits.validate()?;
    Err(SupervisorError::Unsupported)
}

fn validate_environment(environment: &[(OsString, OsString)]) -> Result<(), SupervisorError> {
    if environment.len() > MAX_ENVIRONMENT {
        return Err(SupervisorError::Invalid("environment count exceeds 128"));
    }
    let mut total = 0_usize;
    for (key, value) in environment {
        let key_bytes = key.as_os_str().as_encoded_bytes();
        if key_bytes.is_empty()
            || key_bytes.contains(&b'=')
            || key_bytes.iter().any(|byte| byte.is_ascii_control())
        {
            return Err(SupervisorError::Invalid("environment key is invalid"));
        }
        let value_bytes = value.as_os_str().as_encoded_bytes();
        if value_bytes.iter().any(|byte| byte.is_ascii_control()) {
            return Err(SupervisorError::Invalid("environment value is invalid"));
        }
        total = total
            .checked_add(key_bytes.len())
            .and_then(|bytes| bytes.checked_add(value_bytes.len()))
            .ok_or(SupervisorError::Invalid("environment exceeds 65536 bytes"))?;
    }
    if total > MAX_ENV_BYTES {
        return Err(SupervisorError::Invalid("environment exceeds 65536 bytes"));
    }
    Ok(())
}

fn os_len(value: &OsStr) -> usize {
    value.as_encoded_bytes().len()
}

fn has_control(value: &OsStr) -> bool {
    value
        .as_encoded_bytes()
        .iter()
        .any(|byte| byte.is_ascii_control())
}

fn has_arg_control(value: &OsStr) -> bool {
    value
        .as_encoded_bytes()
        .iter()
        .any(|byte| byte.is_ascii_control() && !matches!(*byte, b'\n' | b'\r' | b'\t'))
}

#[cfg(unix)]
#[path = "supervisor/unix.rs"]
mod unix;

#[cfg(test)]
mod tests;
