//! A bounded, cross-platform child-process runner.
//!
//! The trainer and the sampler are external processes, so they need exactly
//! three guarantees: a hard timeout, a bounded captured output, and a
//! deterministic report of what happened. This module provides them without
//! a shell, without `unsafe`, and without a platform-specific branch in the
//! caller.
//!
//! On Unix the child is placed in its own process group, so a timeout kills
//! the trainer and anything it started. On other hosts the direct child is
//! killed.

use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// How often the runner asks the child whether it has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// What a bounded run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The child's exit code, when it exited normally.
    pub code: Option<i32>,
    /// The captured standard output, truncated to the cap.
    pub stdout: String,
    /// The captured standard error, truncated to the cap.
    pub stderr: String,
    /// Whether either stream was longer than its cap.
    pub truncated: bool,
}

/// Why a bounded run did not produce output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The child could not be started.
    Spawn(String),
    /// The child outlived its deadline and was killed.
    TimedOut,
    /// The child produced output that was not UTF-8.
    NotUtf8,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(message) => {
                write!(formatter, "could not start the child process: {message}")
            }
            Self::TimedOut => write!(formatter, "the child process exceeded its time limit"),
            Self::NotUtf8 => write!(formatter, "the child process produced non-UTF-8 output"),
        }
    }
}

impl std::error::Error for RunError {}

/// Run `program` with `args`, bounded by `timeout` and by output caps.
pub fn run_bounded(
    program: &Path,
    args: &[OsString],
    timeout: Duration,
    stdout_cap: usize,
    stderr_cap: usize,
) -> Result<Captured, RunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A dedicated process group lets a timeout take the whole trainer
        // down, not just the shell that launched it.
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| RunError::Spawn(error.to_string()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    #[cfg(unix)]
    for pipe in [
        stdout.as_ref().map(std::os::fd::AsFd::as_fd),
        stderr.as_ref().map(std::os::fd::AsFd::as_fd),
    ]
    .into_iter()
    .flatten()
    {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        let configured = fcntl(pipe, FcntlArg::F_GETFL).and_then(|flags| {
            fcntl(
                pipe,
                FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
            )
        });
        if let Err(error) = configured {
            terminate(&mut child);
            let _ = child.wait();
            return Err(RunError::Spawn(error.to_string()));
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let out_stop = stop.clone();
    let err_stop = stop.clone();
    let out_handle = std::thread::spawn(move || read_capped(stdout, stdout_cap, &out_stop));
    let err_handle = std::thread::spawn(move || read_capped(stderr, stderr_cap, &err_stop));

    let deadline = Instant::now() + timeout;
    let status = loop {
        // Do not reap the group leader until both pipes have closed. Its PID
        // reserves the process-group identity until any timeout teardown.
        if out_handle.is_finished() && err_handle.is_finished() {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    stop.store(true, Ordering::Relaxed);
                    return Err(RunError::Spawn(error.to_string()));
                }
            }
        }
        // EOF is part of the operation: a descendant retaining a pipe must
        // not escape the same deadline merely because its leader has exited.
        if Instant::now() >= deadline {
            terminate(&mut child);
            let _ = child.wait();
            stop.store(true, Ordering::Relaxed);
            return Err(RunError::TimedOut);
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    let (out_bytes, out_truncated) = out_handle.join().unwrap_or_default();
    let (err_bytes, err_truncated) = err_handle.join().unwrap_or_default();
    let stdout = String::from_utf8(out_bytes).map_err(|_| RunError::NotUtf8)?;
    let stderr = String::from_utf8(err_bytes).map_err(|_| RunError::NotUtf8)?;
    Ok(Captured {
        code: status.code(),
        stdout,
        stderr,
        truncated: out_truncated || err_truncated,
    })
}

/// Kill the child (and, on Unix, its process group).
fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;
        let pid = i32::try_from(child.id()).unwrap_or(0);
        if pid > 1 {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        }
    }
    let _ = child.kill();
}

/// Read a stream to the end, keeping at most `cap` bytes.
fn read_capped<R: Read>(source: Option<R>, cap: usize, stop: &AtomicBool) -> (Vec<u8>, bool) {
    let Some(mut source) = source else {
        return (Vec::new(), false);
    };
    let mut kept: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    let mut truncated = false;
    while !stop.load(Ordering::Relaxed) {
        match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if kept.len() < cap {
                    let room = cap - kept.len();
                    let take = room.min(read);
                    kept.extend_from_slice(&buffer[..take]);
                    if take < read {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (kept, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let error = run_bounded(
            Path::new("/nonexistent/abbey-train-fixture"),
            &[],
            Duration::from_secs(1),
            1024,
            1024,
        )
        .unwrap_err();
        assert!(matches!(error, RunError::Spawn(_)));
    }
}
