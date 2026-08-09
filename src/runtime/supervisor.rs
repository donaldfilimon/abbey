//! Crate-private, bounded Unix child-process supervision.
//!
//! This is a lifecycle primitive for Abbey-owned adapters. It is deliberately
//! not a public generic command runner and never invokes a shell.

use super::CancellationToken;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 16 * 1024;
const MAX_ENVIRONMENT: usize = 128;
const MAX_ENV_BYTES: usize = 64 * 1024;
const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;
const MAX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_TERMINATE_GRACE: Duration = Duration::from_secs(10);
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProcessEnvironment {
    /// Preserve the current process environment exactly as `Command` normally does.
    Inherit,
    /// Clear the environment and install only these explicit key/value pairs.
    ClearAndSet(Vec<(OsString, OsString)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub environment: ProcessEnvironment,
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

    fn validate(&self) -> Result<(), SupervisorError> {
        if self.program.as_os_str().is_empty() {
            return Err(SupervisorError::Invalid("program cannot be empty"));
        }
        if self.args.len() > MAX_ARGS {
            return Err(SupervisorError::Invalid("argument count exceeds 128"));
        }
        if self
            .args
            .iter()
            .any(|argument| os_len(argument) > MAX_ARG_BYTES)
        {
            return Err(SupervisorError::Invalid(
                "an argument exceeds 16384 bytes",
            ));
        }
        if let ProcessEnvironment::ClearAndSet(environment) = &self.environment {
            validate_environment(environment)?;
        }
        Ok(())
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
                "timeout must be within 1ns..=600s",
            ));
        }
        if self.terminate_grace.is_zero() || self.terminate_grace > MAX_TERMINATE_GRACE {
            return Err(SupervisorError::Invalid(
                "termination grace must be within 1ns..=10s",
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
                "stream limits must be within 1..=16777216 bytes",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
pub(crate) enum SupervisorError {
    Invalid(&'static str),
    Unsupported,
    Spawn(std::io::Error),
    Pipe(&'static str),
    Wait(std::io::Error),
    Reader(StreamName, std::io::Error),
    ReaderThread(StreamName),
    Teardown(String),
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
            Self::Unsupported => formatter.write_str(
                "process supervision is supported only on Unix hosts with process groups",
            ),
            Self::Spawn(error) => write!(formatter, "spawn supervised process: {error}"),
            Self::Pipe(stream) => write!(formatter, "capture supervised process {stream}"),
            Self::Wait(error) => write!(formatter, "wait for supervised process: {error}"),
            Self::Reader(stream, error) => {
                write!(formatter, "read supervised process {stream}: {error}")
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
            Self::Invalid(_)
            | Self::Unsupported
            | Self::Pipe(_)
            | Self::ReaderThread(_)
            | Self::Teardown(_) => None,
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
    unix::run(spec, *limits, cancellation)
}

#[cfg(not(unix))]
pub(crate) fn run(
    spec: &ProcessSpec,
    limits: &SupervisorLimits,
    _cancellation: &CancellationToken,
) -> Result<SupervisorOutcome, SupervisorError> {
    spec.validate()?;
    limits.validate()?;
    Err(SupervisorError::Unsupported)
}

fn validate_environment(environment: &[(OsString, OsString)]) -> Result<(), SupervisorError> {
    if environment.len() > MAX_ENVIRONMENT {
        return Err(SupervisorError::Invalid(
            "environment count exceeds 128",
        ));
    }
    let mut total = 0_usize;
    for (key, value) in environment {
        let key_bytes = key.as_os_str().as_encoded_bytes();
        if key_bytes.is_empty() || key_bytes.contains(&b'=') || key_bytes.contains(&0) {
            return Err(SupervisorError::Invalid("environment key is invalid"));
        }
        let value_bytes = value.as_os_str().as_encoded_bytes();
        if value_bytes.contains(&0) {
            return Err(SupervisorError::Invalid("environment value is invalid"));
        }
        total = total
            .checked_add(key_bytes.len())
            .and_then(|bytes| bytes.checked_add(value_bytes.len()))
            .ok_or(SupervisorError::Invalid("environment exceeds 65536 bytes"))?;
    }
    if total > MAX_ENV_BYTES {
        return Err(SupervisorError::Invalid(
            "environment exceeds 65536 bytes",
        ));
    }
    Ok(())
}

fn os_len(value: &OsStr) -> usize {
    value.as_encoded_bytes().len()
}

#[cfg(unix)]
mod unix {
    use super::*;
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use std::io::Read;
    use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread::{self, JoinHandle};
    use std::time::Instant;

    struct CapturedStream {
        name: StreamName,
        bytes: Vec<u8>,
        overflowed: bool,
    }

    struct ReaderMessage {
        name: StreamName,
        result: std::io::Result<CapturedStream>,
    }

    #[derive(Default)]
    struct Captures {
        stdout: Option<CapturedStream>,
        stderr: Option<CapturedStream>,
    }

    impl Captures {
        fn insert(&mut self, captured: CapturedStream) {
            match captured.name {
                StreamName::Stdout => self.stdout = Some(captured),
                StreamName::Stderr => self.stderr = Some(captured),
            }
        }

        fn complete(&self) -> bool {
            self.stdout.is_some() && self.stderr.is_some()
        }

        fn overflow(&self) -> Option<StreamName> {
            self.stdout
                .as_ref()
                .and_then(|stream| stream.overflowed.then_some(stream.name))
                .or_else(|| {
                    self.stderr
                        .as_ref()
                        .and_then(|stream| stream.overflowed.then_some(stream.name))
                })
        }
    }

    struct ChildGuard {
        child: Child,
        process_group: Pid,
        reaped: bool,
        disarmed: bool,
    }

    impl ChildGuard {
        fn new(child: Child) -> Result<Self, SupervisorError> {
            let raw = i32::try_from(child.id()).map_err(|_| {
                SupervisorError::Teardown("child PID does not fit pid_t".into())
            })?;
            Ok(Self {
                child,
                process_group: Pid::from_raw(raw),
                reaped: false,
                disarmed: false,
            })
        }

        fn try_wait(&mut self) -> Result<Option<ExitStatus>, SupervisorError> {
            if self.reaped {
                return Ok(None);
            }
            let status = self.child.try_wait().map_err(SupervisorError::Wait)?;
            if status.is_some() {
                self.reaped = true;
            }
            Ok(status)
        }

        fn terminate(
            &mut self,
            grace: Duration,
            poll_interval: Duration,
        ) -> Result<(), SupervisorError> {
            signal_group(self.process_group, Signal::SIGTERM)?;
            let deadline = Instant::now() + grace;
            while group_exists(self.process_group)? && Instant::now() < deadline {
                let _ = self.try_wait()?;
                thread::sleep(poll_interval.min(deadline.saturating_duration_since(Instant::now())));
            }
            if group_exists(self.process_group)? {
                signal_group(self.process_group, Signal::SIGKILL)?;
            }
            if !self.reaped {
                self.child.wait().map_err(SupervisorError::Wait)?;
                self.reaped = true;
            }
            let gone_deadline = Instant::now() + grace;
            while group_exists(self.process_group)? && Instant::now() < gone_deadline {
                thread::sleep(
                    poll_interval.min(gone_deadline.saturating_duration_since(Instant::now())),
                );
            }
            if group_exists(self.process_group)? {
                return Err(SupervisorError::Teardown(
                    "process group survived SIGKILL grace".into(),
                ));
            }
            self.disarmed = true;
            Ok(())
        }

        fn finish_after_exit(
            &mut self,
            grace: Duration,
            poll_interval: Duration,
        ) -> Result<(), SupervisorError> {
            if group_exists(self.process_group)? {
                self.terminate(grace, poll_interval)?;
            } else {
                self.disarmed = true;
            }
            Ok(())
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.disarmed {
                return;
            }
            let _ = killpg(self.process_group, Signal::SIGKILL);
            let _ = self.child.kill();
            if !self.reaped {
                let _ = self.child.wait();
                self.reaped = true;
            }
        }
    }

    pub(super) fn run(
        spec: &ProcessSpec,
        limits: SupervisorLimits,
        cancellation: &CancellationToken,
    ) -> Result<SupervisorOutcome, SupervisorError> {
        spec.validate()?;
        limits.validate()?;

        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(current_dir) = &spec.current_dir {
            command.current_dir(current_dir);
        }
        match &spec.environment {
            ProcessEnvironment::Inherit => {}
            ProcessEnvironment::ClearAndSet(environment) => {
                command.env_clear().envs(environment.iter().cloned());
            }
        }
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);

        let mut child = command.spawn().map_err(SupervisorError::Spawn)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(SupervisorError::Pipe("stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(SupervisorError::Pipe("stderr"))?;
        let mut guard = ChildGuard::new(child)?;
        let (reader_tx, reader_rx) = mpsc::channel();
        let stdout_reader = spawn_reader(
            stdout,
            StreamName::Stdout,
            limits.stdout_bytes,
            reader_tx.clone(),
        )?;
        let stderr_reader = spawn_reader(
            stderr,
            StreamName::Stderr,
            limits.stderr_bytes,
            reader_tx,
        )?;

        supervise(
            &mut guard,
            &reader_rx,
            [stdout_reader, stderr_reader],
            limits,
            cancellation,
        )
    }

    fn supervise(
        guard: &mut ChildGuard,
        reader_rx: &Receiver<ReaderMessage>,
        readers: [JoinHandle<()>; 2],
        limits: SupervisorLimits,
        cancellation: &CancellationToken,
    ) -> Result<SupervisorOutcome, SupervisorError> {
        let deadline = Instant::now() + limits.timeout;
        let mut status = None;
        let mut captures = Captures::default();
        let mut control = None;

        loop {
            drain_reader_messages(reader_rx, &mut captures)?;
            if let Some(stream) = captures.overflow() {
                control = Some(match stream {
                    StreamName::Stdout => SupervisorOutcome::StdoutLimit,
                    StreamName::Stderr => SupervisorOutcome::StderrLimit,
                });
                break;
            }
            if cancellation.is_cancelled() {
                control = Some(SupervisorOutcome::Cancelled);
                break;
            }
            if Instant::now() >= deadline {
                control = Some(SupervisorOutcome::TimedOut);
                break;
            }
            if status.is_none() {
                status = guard.try_wait()?;
            }
            if status.is_some() && captures.complete() {
                break;
            }
            if status.is_some() {
                // A descendant inherited at least one pipe after the leader
                // exited. Terminate the process group before joining readers.
                guard.terminate(limits.terminate_grace, limits.poll_interval)?;
                break;
            }
            thread::sleep(limits.poll_interval);
        }

        if control.is_some() {
            guard.terminate(limits.terminate_grace, limits.poll_interval)?;
        } else if status.is_some() {
            guard.finish_after_exit(limits.terminate_grace, limits.poll_interval)?;
        }
        collect_readers(
            reader_rx,
            &mut captures,
            readers,
            limits.terminate_grace,
        )?;
        if let Some(stream) = captures.overflow() {
            return Ok(match stream {
                StreamName::Stdout => SupervisorOutcome::StdoutLimit,
                StreamName::Stderr => SupervisorOutcome::StderrLimit,
            });
        }
        if let Some(outcome) = control {
            return Ok(outcome);
        }
        let status = status.ok_or_else(|| {
            SupervisorError::Teardown("leader ended without an exit status".into())
        })?;
        Ok(SupervisorOutcome::Exited {
            status,
            stdout: captures.stdout.expect("stdout capture is complete").bytes,
            stderr: captures.stderr.expect("stderr capture is complete").bytes,
        })
    }

    fn spawn_reader<R: Read + Send + 'static>(
        reader: R,
        name: StreamName,
        cap: usize,
        sender: Sender<ReaderMessage>,
    ) -> Result<JoinHandle<()>, SupervisorError> {
        thread::Builder::new()
            .name(format!("abbey-supervisor-{name}"))
            .spawn(move || {
                let result = read_bounded(reader, name, cap);
                let _ = sender.send(ReaderMessage { name, result });
            })
            .map_err(SupervisorError::Spawn)
    }

    fn read_bounded<R: Read>(
        mut reader: R,
        name: StreamName,
        cap: usize,
    ) -> std::io::Result<CapturedStream> {
        let mut bytes = Vec::with_capacity(cap.saturating_add(1));
        let mut buffer = [0_u8; 8 * 1024];
        while bytes.len() <= cap {
            let remaining = cap.saturating_add(1).saturating_sub(bytes.len());
            if remaining == 0 {
                break;
            }
            let read = reader.read(&mut buffer[..remaining.min(buffer.len())])?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        let overflowed = bytes.len() > cap;
        if overflowed {
            bytes.truncate(cap);
        }
        Ok(CapturedStream {
            name,
            bytes,
            overflowed,
        })
    }

    fn drain_reader_messages(
        receiver: &Receiver<ReaderMessage>,
        captures: &mut Captures,
    ) -> Result<(), SupervisorError> {
        while let Ok(message) = receiver.try_recv() {
            let captured = message
                .result
                .map_err(|error| SupervisorError::Reader(message.name, error))?;
            captures.insert(captured);
        }
        Ok(())
    }

    fn collect_readers(
        receiver: &Receiver<ReaderMessage>,
        captures: &mut Captures,
        readers: [JoinHandle<()>; 2],
        grace: Duration,
    ) -> Result<(), SupervisorError> {
        let deadline = Instant::now() + grace;
        while !captures.complete() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SupervisorError::Teardown(
                    "capture pipes remained open after process-group teardown".into(),
                ));
            }
            let message = receiver.recv_timeout(remaining).map_err(|_| {
                SupervisorError::Teardown(
                    "capture pipes remained open after process-group teardown".into(),
                )
            })?;
            let captured = message
                .result
                .map_err(|error| SupervisorError::Reader(message.name, error))?;
            captures.insert(captured);
        }
        for (reader, name) in readers.into_iter().zip([StreamName::Stdout, StreamName::Stderr]) {
            reader
                .join()
                .map_err(|_| SupervisorError::ReaderThread(name))?;
        }
        Ok(())
    }

    fn signal_group(process_group: Pid, signal: Signal) -> Result<(), SupervisorError> {
        match killpg(process_group, signal) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(error) => Err(SupervisorError::Teardown(format!(
                "send {signal:?} to process group: {error}"
            ))),
        }
    }

    fn group_exists(process_group: Pid) -> Result<bool, SupervisorError> {
        match killpg(process_group, None) {
            Ok(()) | Err(Errno::EPERM) => Ok(true),
            Err(Errno::ESRCH) => Ok(false),
            Err(error) => Err(SupervisorError::Teardown(format!(
                "inspect process group: {error}"
            ))),
        }
    }

    #[allow(dead_code)]
    fn _pipe_types(_: ChildStdout, _: ChildStderr) {}
}

#[cfg(test)]
mod tests;
