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

pub(super) struct ChildGuard {
    child: Child,
    process_group: Pid,
    reaped: bool,
    disarmed: bool,
}

impl ChildGuard {
    pub(super) fn new(child: Child) -> Result<Self, SupervisorError> {
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

    fn reap_leader(&mut self) -> Result<(), SupervisorError> {
        if !self.reaped {
            self.child.wait().map_err(SupervisorError::Wait)?;
            self.reaped = true;
        }
        Ok(())
    }

    pub(super) fn terminate(
        &mut self,
        grace: Duration,
        poll_interval: Duration,
    ) -> Result<(), SupervisorError> {
        let deadline = Instant::now() + grace;
        self.signal_group_until(Signal::SIGTERM, deadline, poll_interval)?;
        while group_exists(self.process_group)? && Instant::now() < deadline {
            let _ = self.try_wait()?;
            thread::sleep(poll_interval.min(deadline.saturating_duration_since(Instant::now())));
        }
        if group_exists(self.process_group)? {
            let kill_deadline = Instant::now() + grace;
            self.signal_group_until(Signal::SIGKILL, kill_deadline, poll_interval)?;
        }
        // `killpg(pgid, 0)` reports an unreaped leader zombie as a live
        // group on Darwin and Linux. Reaping is therefore a prerequisite,
        // not a cleanup after the final post-SIGKILL liveness decision.
        self.reap_leader()?;
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

    #[cfg(test)]
    pub(super) fn test_state(&self) -> (Pid, bool, bool) {
        (self.process_group, self.reaped, self.disarmed)
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
    let stderr_reader = spawn_reader(stderr, StreamName::Stderr, limits.stderr_bytes, reader_tx)?;

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
    let status = status
        .ok_or_else(|| SupervisorError::Teardown("leader ended without an exit status".into()))?;
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

fn try_signal_group(process_group: Pid, signal: Signal) -> Result<SignalAttempt, SupervisorError> {
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

pub(super) fn group_exists(process_group: Pid) -> Result<bool, SupervisorError> {
    match killpg(process_group, None) {
        Ok(()) | Err(Errno::EPERM) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(_) => Err(SupervisorError::Teardown(
            "inspect process group failed".into(),
        )),
    }
}
