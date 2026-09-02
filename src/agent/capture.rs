//! Bounded non-Unix agent capture. Unix uses the supervisor instead.

use super::MAX_CAPTURE_BYTES;
use super::argv::map_exec_err;
use anyhow::{Context, Result, bail};
use std::process::{Command, ExitStatus, Stdio};

pub(super) fn bounded_capture_output(
    agent: &std::path::Path,
    args: &[String],
) -> Result<(ExitStatus, String, String)> {
    let mut child = Command::new(agent)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| map_exec_err(e, agent))?;
    let mut stdout_pipe = child.stdout.take().context("agent stdout")?;
    let mut stderr_pipe = child.stderr.take().context("agent stderr")?;
    let stdout_thread =
        std::thread::spawn(move || read_limited(&mut stdout_pipe, MAX_CAPTURE_BYTES));
    let stderr_thread =
        std::thread::spawn(move || read_limited(&mut stderr_pipe, MAX_CAPTURE_BYTES));
    let status = child.wait().map_err(|e| map_exec_err(e, agent))?;
    let (stdout, stdout_exceeded) = stdout_thread
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("stdout reader")))?;
    let (stderr, stderr_exceeded) = stderr_thread
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("stderr reader")))?;
    if stdout_exceeded || stderr_exceeded {
        bail!("agent output exceeded the {MAX_CAPTURE_BYTES}-byte limit");
    }
    Ok((
        status,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

fn read_limited<R: std::io::Read>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    use std::io::Read as _;
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
            continue;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}
