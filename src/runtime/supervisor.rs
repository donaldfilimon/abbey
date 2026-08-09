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
    unix::run(spec, *limits, cancellation)
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
mod unix {
    use super::*;
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use std::io::Read;
    use std::process::{Child, Command, Stdio};
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
            let raw = i32::try_from(child.id())
                .map_err(|_| SupervisorError::Teardown("child PID does not fit pid_t".into()))?;
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
            let deadline = Instant::now() + grace;
            self.signal_group_until(Signal::SIGTERM, deadline, poll_interval)?;
            while group_exists(self.process_group)? && Instant::now() < deadline {
                let _ = self.try_wait()?;
                thread::sleep(
                    poll_interval.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            if group_exists(self.process_group)? {
                let kill_deadline = Instant::now() + grace;
                self.signal_group_until(Signal::SIGKILL, kill_deadline, poll_interval)?;
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

        fn signal_group_until(
            &mut self,
            signal: Signal,
            deadline: Instant,
            poll_interval: Duration,
        ) -> Result<(), SupervisorError> {
            loop {
                // Darwin can report EPERM for an unreaped zombie group.
                let _ = self.try_wait()?;
                if !group_exists(self.process_group)? {
                    return Ok(());
                }
                match try_signal_group(self.process_group, signal)? {
                    SignalAttempt::DeliveredOrGone => return Ok(()),
                    SignalAttempt::PermissionDenied => {
                        // EPERM is retried, never accepted while the group lives.
                        let _ = self.try_wait()?;
                        if !group_exists(self.process_group)? {
                            return Ok(());
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(SupervisorError::Teardown(format!(
                                "send {signal:?} to process group was not permitted"
                            )));
                        }
                        thread::sleep(poll_interval.min(remaining));
                    }
                }
            }
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
        let (canonical_program, canonical_current_dir) = spec.validate()?;
        limits.validate()?;

        let mut command = Command::new(canonical_program);
        command
            .args(&spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(current_dir) = canonical_current_dir {
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

        let child = command.spawn().map_err(SupervisorError::Spawn)?;
        let mut guard = ChildGuard::new(child)?;
        let stdout = guard
            .child
            .stdout
            .take()
            .ok_or(SupervisorError::Pipe("stdout"))?;
        let stderr = guard
            .child
            .stderr
            .take()
            .ok_or(SupervisorError::Pipe("stderr"))?;
        let (reader_tx, reader_rx) = mpsc::channel();
        let stdout_reader = spawn_reader(
            stdout,
            StreamName::Stdout,
            limits.stdout_bytes,
            reader_tx.clone(),
        )?;
        let stderr_reader =
            spawn_reader(stderr, StreamName::Stderr, limits.stderr_bytes, reader_tx)?;

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
                // Terminate descendants that inherited pipes from an exited leader.
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
        collect_readers(reader_rx, &mut captures, readers, limits.terminate_grace)?;
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
            let chunk = remaining.min(buffer.len());
            let read = reader.read(&mut buffer[..chunk])?;
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
        for (reader, name) in readers
            .into_iter()
            .zip([StreamName::Stdout, StreamName::Stderr])
        {
            reader
                .join()
                .map_err(|_| SupervisorError::ReaderThread(name))?;
        }
        Ok(())
    }

    enum SignalAttempt {
        DeliveredOrGone,
        PermissionDenied,
    }

    fn try_signal_group(
        process_group: Pid,
        signal: Signal,
    ) -> Result<SignalAttempt, SupervisorError> {
        match killpg(process_group, signal) {
            Ok(()) | Err(Errno::ESRCH) => Ok(SignalAttempt::DeliveredOrGone),
            Err(Errno::EPERM) => Ok(SignalAttempt::PermissionDenied),
            Err(Errno::EINVAL) => Err(SupervisorError::Teardown(format!(
                "send {signal:?} to process group was rejected"
            ))),
            Err(_) => Err(SupervisorError::Teardown(format!(
                "send {signal:?} to process group failed"
            ))),
        }
    }

    fn group_exists(process_group: Pid) -> Result<bool, SupervisorError> {
        match killpg(process_group, None) {
            Ok(()) | Err(Errno::EPERM) => Ok(true),
            Err(Errno::ESRCH) => Ok(false),
            Err(_) => Err(SupervisorError::Teardown(
                "inspect process group failed".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests;
