//! Bounded capture of a child process, with a hard stop when it overflows.
//!
//! The Unix execution path uses the supervisor instead, which owns process
//! groups, deadlines, and cooperative cancellation. This is the portable
//! path, and it is compiled — and tested — on every host, so the bound is
//! exercised wherever the gate runs rather than only on the one platform
//! that needs it.
//!
//! The overflow rule: a stream that crosses the limit is a failure, and a
//! failure must *stop* the child. A child that keeps writing after its limit
//! is exceeded would otherwise be waited on forever, so the run would hang
//! instead of returning the limit error.

use super::MAX_CAPTURE_BYTES;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// How often the parent checks whether the child exited or a stream overflowed.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Run `agent` with `args`, capture both streams under the capture limit.
///
/// `#[allow(dead_code)]`: the Unix execution path uses the supervisor, so
/// outside a test this is reached only on the platforms that need the
/// portable path.
#[allow(dead_code)]
pub(super) fn bounded_capture_output(
    agent: &Path,
    args: &[String],
) -> Result<(ExitStatus, String, String)> {
    let mut child = Command::new(agent)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| super::argv::map_exec_err(e, agent))?;
    let mut stdout_pipe = child.stdout.take().context("agent stdout")?;
    let mut stderr_pipe = child.stderr.take().context("agent stderr")?;
    // The readers report an overflow to the parent, which then stops the
    // child instead of waiting for a process that will not stop writing.
    let (overflow_tx, overflow_rx) = channel::<()>();
    let stdout_thread = std::thread::spawn({
        let overflow = overflow_tx.clone();
        move || read_limited(&mut stdout_pipe, MAX_CAPTURE_BYTES, &overflow)
    });
    let stderr_thread = std::thread::spawn({
        let overflow = overflow_tx;
        move || read_limited(&mut stderr_pipe, MAX_CAPTURE_BYTES, &overflow)
    });
    let (status, killed_for_overflow) = wait_for_exit_or_overflow(&mut child, &overflow_rx);
    if killed_for_overflow {
        // The child was stopped mid-write, which is the failure this bound
        // exists to report. Its readers are not waited for: a pipe that a
        // surviving grandchild still holds open would keep them blocked, and
        // the run must report the limit rather than wait for a process tree
        // this path cannot stop (the Unix path stops the whole group; the
        // portable path has no equivalent).
        bail!("agent output exceeded the {MAX_CAPTURE_BYTES}-byte limit");
    }
    let (stdout, stdout_exceeded) = stdout_thread
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("stdout reader")))?;
    let (stderr, stderr_exceeded) = stderr_thread
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("stderr reader")))?;
    if stdout_exceeded || stderr_exceeded || killed_for_overflow {
        bail!("agent output exceeded the {MAX_CAPTURE_BYTES}-byte limit");
    }
    let status = status.context("agent exit status")?;
    Ok((
        status,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

/// Wait for the child to exit, or stop it as soon as a stream overflows.
///
/// Returns the exit status when the child finished on its own, and `None`
/// when the child was killed because a stream crossed the limit.
fn wait_for_exit_or_overflow(
    child: &mut Child,
    overflow: &Receiver<()>,
) -> (Option<ExitStatus>, bool) {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (Some(status), false),
            Ok(None) => {}
            Err(_) => return (None, false),
        }
        if overflow.try_recv().is_ok() {
            // The child is still writing past the bound: stop it, then let
            // the caller report the limit.
            let _ = child.kill();
            let _ = child.wait();
            return (None, true);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Read up to `limit` bytes, then report an overflow to the parent.
///
/// The reader keeps draining after the limit so the child never blocks on a
/// full pipe, but it stops growing the buffer: the bytes past the limit are
/// dropped, not kept.
fn read_limited<R: std::io::Read>(
    reader: &mut R,
    limit: usize,
    overflow: &Sender<()>,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let n = reader.read(&mut tmp)?;
        if n == 0 {
            return Ok((buf, exceeded));
        }
        if exceeded {
            continue;
        }
        if buf.len().saturating_add(n) > limit {
            buf.extend_from_slice(&tmp[..limit.saturating_sub(buf.len())]);
            exceeded = true;
            // Tell the parent: the child has to be stopped, not waited on.
            let _ = overflow.send(());
            continue;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A child that writes without end: without a hard stop the capture
    /// waits for it forever.
    fn runaway() -> (String, Vec<String>) {
        (
            "/bin/sh".to_string(),
            vec![
                "-c".to_string(),
                "while :; do cat /dev/zero; done".to_string(),
            ],
        )
    }

    #[test]
    fn an_overflowing_child_is_stopped_instead_of_waited_on() {
        let (program, args) = runaway();
        let (sender, receiver) = channel::<Result<(ExitStatus, String, String)>>();
        let started = Instant::now();
        std::thread::spawn(move || {
            let _ = sender.send(bounded_capture_output(Path::new(&program), &args));
        });
        let outcome = receiver.recv_timeout(Duration::from_secs(20));
        assert!(
            outcome.is_ok(),
            "the capture never returned: an overflowing child was waited on"
        );
        let error = outcome.expect("result").expect_err("must fail");
        assert!(
            error.to_string().contains("exceeded"),
            "unexpected error: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the capture took too long to stop the child"
        );
    }

    #[test]
    fn a_quiet_child_reports_its_output_and_status() {
        let program = "/bin/sh".to_string();
        let args = vec![
            "-c".to_string(),
            "printf 'out'; printf 'err' 1>&2; exit 3".to_string(),
        ];
        let (status, stdout, stderr) =
            bounded_capture_output(Path::new(&program), &args).expect("capture");
        assert_eq!(stdout, "out");
        assert_eq!(stderr, "err");
        assert_eq!(status.code(), Some(3));
    }
}
