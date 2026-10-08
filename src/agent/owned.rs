//! Captured executor work retains its TUI cancellation owner and joins.

use super::AgentConfig;
use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt as _;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::process::ExitStatus;

#[derive(Debug)]
pub(crate) struct CaptureCancelled;

impl std::fmt::Display for CaptureCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("agent capture was cancelled after observed cleanup")
    }
}

impl std::error::Error for CaptureCancelled {}

impl AgentConfig {
    pub(super) fn lock_local_turn(&self, chat_id: &str) -> Result<Option<File>> {
        let Some(path) = self.transcript_path(chat_id) else {
            return Ok(None);
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let lock_path = path.with_extension("transcript.lock");
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("open conversation turn lock {}", lock_path.display()))?;
        if self.stream.is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
            loop {
                self.check_cancelled()?;
                match lock.try_lock_exclusive() {
                    Ok(true) => {
                        self.check_cancelled()?;
                        break;
                    }
                    Ok(false) => {
                        if std::time::Instant::now() >= deadline {
                            bail!("conversation turn lock deadline exceeded");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            bail!("conversation turn lock deadline exceeded");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(error) => return Err(error).context("lock conversation turn"),
                }
            }
        } else {
            lock.lock_exclusive()
                .with_context(|| format!("lock conversation turn {}", lock_path.display()))?;
        }
        Ok(Some(lock))
    }

    pub(crate) fn check_cancelled(&self) -> Result<()> {
        if self
            .stream
            .as_ref()
            .is_some_and(|tap| tap.exceeded() || tap.failed())
        {
            bail!("stream failed or exceeded its shared output budget");
        }
        if self
            .stream
            .as_ref()
            .is_some_and(|tap| tap.cancel.is_cancelled())
        {
            return Err(CaptureCancelled.into());
        }
        Ok(())
    }

    pub(crate) fn output_line(&self, text: impl Into<String>) {
        let text = text.into();
        if let Some(tap) = &self.stream {
            tap.notice(text);
        } else {
            println!("{text}");
        }
    }

    pub(crate) fn emit_captured(&self, stdout: &str, stderr: &str) {
        if let Some(tap) = &self.stream {
            if !stderr.is_empty() {
                tap.notice(stderr);
            }
            if !stdout.is_empty() {
                tap.emit(crate::stream::StreamEvent::TextDelta(stdout.into()));
            }
        } else {
            eprint!("{stderr}");
            crate::highlight::emit_agent_stdout(stdout);
        }
    }

    pub(crate) fn capture_command(
        &self,
        program: &Path,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<(ExitStatus, String, String)> {
        use std::time::Duration;
        self.capture_command_limited(
            program,
            args,
            cwd,
            crate::runtime::supervisor::SupervisorLimits {
                timeout: Duration::from_secs(30 * 60),
                terminate_grace: Duration::from_secs(1),
                stdout_bytes: super::MAX_CAPTURE_BYTES,
                stderr_bytes: super::MAX_CAPTURE_BYTES,
                poll_interval: Duration::from_millis(20),
            },
        )
    }

    pub(crate) fn capture_command_limited(
        &self,
        program: &Path,
        args: &[String],
        cwd: Option<&Path>,
        limits: crate::runtime::supervisor::SupervisorLimits,
    ) -> Result<(ExitStatus, String, String)> {
        self.check_cancelled()?;
        #[cfg(unix)]
        {
            use crate::runtime::supervisor::{ProcessSpec, SupervisorOutcome, run_with_checkpoint};
            let mut spec = ProcessSpec::inherited(
                program.to_path_buf(),
                args.iter().map(std::ffi::OsString::from).collect(),
            );
            spec.current_dir = cwd.map(Path::to_path_buf);
            let remaining = self.stream.as_ref().map_or(
                super::MAX_CAPTURE_BYTES,
                crate::stream::StreamTap::raw_remaining,
            );
            if remaining == 0 {
                if let Some(tap) = &self.stream {
                    tap.emit(crate::stream::StreamEvent::Failed(
                        "stream exceeded the 4 MiB cumulative raw-output limit".into(),
                    ));
                }
                bail!("stream exceeded the 4 MiB cumulative raw-output limit");
            }
            let limits = crate::runtime::supervisor::SupervisorLimits {
                stdout_bytes: limits.stdout_bytes.min(remaining),
                ..limits
            };
            let outcome = run_with_checkpoint(&spec, &limits, || {
                self.stream
                    .as_ref()
                    .is_some_and(|tap| tap.cancel.is_cancelled())
            })
            .context("supervise captured executor")?;
            match outcome {
                SupervisorOutcome::Exited {
                    status,
                    stdout,
                    stderr,
                } => {
                    self.check_cancelled()?;
                    if let Some(tap) = &self.stream {
                        anyhow::ensure!(
                            tap.record_raw(stdout.len()),
                            "stream exceeded the 4 MiB cumulative raw-output limit"
                        );
                    }
                    Ok((
                        status,
                        String::from_utf8_lossy(&stdout).into_owned(),
                        String::from_utf8_lossy(&stderr).into_owned(),
                    ))
                }
                SupervisorOutcome::Cancelled => Err(CaptureCancelled.into()),
                SupervisorOutcome::TimedOut => {
                    bail!("agent capture exceeded its execution deadline")
                }
                SupervisorOutcome::StdoutLimit => {
                    let message = format!(
                        "agent stdout exceeded the {}-byte {} limit",
                        limits.stdout_bytes,
                        if limits.stdout_bytes == remaining {
                            "remaining cumulative raw-output"
                        } else {
                            "per-command stdout"
                        }
                    );
                    if let Some(tap) = &self.stream {
                        tap.emit(crate::stream::StreamEvent::Failed(message.clone()));
                    }
                    bail!("{message}")
                }
                SupervisorOutcome::StderrLimit => bail!(
                    "agent stderr exceeded the {}-byte limit",
                    limits.stderr_bytes
                ),
            }
        }
        #[cfg(not(unix))]
        {
            if self.stream.is_some() {
                bail!("owned captured TUI execution is unavailable on this platform");
            }
            let _ = (cwd, limits);
            super::capture::bounded_capture_output(program, args)
        }
    }
}

impl AgentConfig {
    pub(crate) fn metadata_owned(&self, args: &[String]) -> Result<(ExitStatus, String, String)> {
        use std::time::Duration;
        self.capture_command_limited(
            &self.agent_path,
            args,
            None,
            crate::runtime::supervisor::SupervisorLimits {
                timeout: Duration::from_millis(500),
                terminate_grace: Duration::from_millis(100),
                stdout_bytes: 4096,
                stderr_bytes: 4096,
                poll_interval: Duration::from_millis(5),
            },
        )
    }

    pub(crate) fn version_owned(&self) -> Result<String> {
        if self.agent_path.as_os_str().is_empty() {
            return Ok("unknown".into());
        }
        let (status, stdout, _) = self.metadata_owned(&["--version".into()])?;
        Ok(if status.success() && !stdout.trim().is_empty() {
            stdout.trim().into()
        } else {
            "unknown".into()
        })
    }

    pub(crate) fn fm_availability_owned(&self) -> Result<String> {
        let (_, stdout, stderr) = self.metadata_owned(&["models".into()])?;
        let models =
            super::backend::parse_fm_models(&super::strip_ansi(&format!("{stdout}\n{stderr}")));
        Ok(format!(
            "on-device system model: {} · private cloud compute: {}",
            if models.system {
                "available"
            } else {
                "unavailable"
            },
            if models.pcc {
                "available"
            } else {
                "unavailable"
            }
        ))
    }
}
