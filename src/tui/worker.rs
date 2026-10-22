//! Background runs for the chat TUI: agent turns stream through the canonical
//! path on a thread; local slash commands run as a captured `abbey` child so
//! their stdout lands in the transcript instead of on the alternate screen.

use super::app::SHUTDOWN_JOIN_GRACE;
use crate::actions::{RunSpec, run_agent};
use crate::agent::AgentConfig;
use crate::runtime::CancellationToken;
use crate::slash::{SLASH_CATALOG, SlashKind};
use crate::state::AbbeyState;
use crate::stream::{StreamEvent, StreamTap};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const INTERACTIVE_SLASH: &[&str] = &["voice", "listen"];

/// What came of waiting for a worker.
///
/// "Finished", "panicked", and "did not stop" are three different
/// facts, and the app has to tell them apart: a worker that was
/// abandoned is not a worker that succeeded, and it is not a worker
/// that crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinOutcome {
    /// The worker finished inside the bound.
    Finished,
    /// The worker panicked.
    Panicked,
    /// The worker did not finish inside the bound and was abandoned.
    Abandoned,
}

/// Wait up to `grace` for `thread`, then give up on it.
///
/// Returns `true` when the thread finished inside the bound. A thread
/// that misses it is left alone: the caller drops the handle, the
/// thread is detached, and it finishes on its own.
///
/// The wait is a poll because `JoinHandle` has no timed join. The only
/// other option is to block for ever, which is the defect this bounds:
/// a worker that never finishes would hold the UI thread — and, at
/// shutdown, the terminal in raw mode — with no way out.
pub(crate) fn join_within(thread: &JoinHandle<()>, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        if thread.is_finished() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(crate) enum RunKind {
    Prompt { text: String, fresh: bool },
    PleaseFix(String),
    AgentSlash(String),
    Local(Vec<String>),
}

pub(crate) struct RunHandle {
    pub events: Receiver<StreamEvent>,
    pub done: Receiver<(i32, Option<String>)>,
    pub cancel: CancellationToken,
    pub started: Instant,
    pub thread: Option<JoinHandle<()>>,
    pub completion: Option<(i32, Option<String>)>,
    pub local: bool,
    /// When the interrupt was asked for, if it was.
    ///
    /// An interrupt asks the worker to stop; it does not promise that the
    /// worker stops. The app needs to know that the interrupt was asked
    /// for, and when, so it can say so and so it can stop waiting.
    pub cancel_requested: Option<Instant>,
    /// Whether the app gave up on this worker.
    ///
    /// An abandoned worker is detached rather than joined: joining it
    /// would block the UI thread on the very worker that never finished.
    pub abandoned: bool,
}

impl RunHandle {
    /// Wait up to `grace` for the worker, then give up on it.
    ///
    /// The wait is bounded: a worker that never finishes must not hold
    /// the UI thread — and, at shutdown, the terminal — for ever. A
    /// worker that misses the bound is *detached*: it keeps running on
    /// its own, and the app stops waiting for an answer that may never
    /// come.
    pub fn join_within(&mut self, grace: Duration) -> JoinOutcome {
        let Some(thread) = self.thread.take() else {
            // Nothing to wait for: the worker was joined already.
            return JoinOutcome::Finished;
        };
        if !join_within(&thread, grace) {
            // Dropping the handle detaches the thread: it finishes on
            // its own, and its answer goes nowhere.
            return JoinOutcome::Abandoned;
        }
        match thread.join() {
            Ok(()) => JoinOutcome::Finished,
            Err(_) => JoinOutcome::Panicked,
        }
    }

    /// Join the worker, waiting at most the shutdown grace.
    ///
    /// The message is for the paths that report an outcome as text: a
    /// worker that panicked, and a worker that missed the bound.
    pub fn join(&mut self) -> Result<(), &'static str> {
        match self.join_within(SHUTDOWN_JOIN_GRACE) {
            JoinOutcome::Finished => Ok(()),
            JoinOutcome::Panicked => Err("run worker panicked"),
            JoinOutcome::Abandoned => {
                Err("the run worker did not finish within the shutdown grace")
            }
        }
    }

    /// Give up on the worker and stop waiting for it.
    ///
    /// The worker is detached: it keeps whatever it was doing, and the
    /// app stops waiting for an answer that may never come.
    pub fn abandon(&mut self) {
        self.abandoned = true;
        self.thread.take();
        self.cancel.cancel();
    }
}

impl Drop for RunHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
        // The wait is bounded: a worker that will not stop must not hold
        // the drop — and the terminal — for ever.
        let _ = self.join_within(SHUTDOWN_JOIN_GRACE);
    }
}

pub(crate) fn spawn_run(
    mut cfg: AgentConfig,
    state: AbbeyState,
    kind: RunKind,
) -> std::io::Result<RunHandle> {
    let (ev_tx, events) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let tap = StreamTap::new(ev_tx);
    let cancel = tap.cancel.clone();
    let local = matches!(&kind, RunKind::Local(_));
    let thread = std::thread::Builder::new()
        .name("abbey-tui-run".into())
        .spawn(move || {
            let outcome_tap = tap.clone();
            let result = match kind {
                RunKind::Prompt { text, fresh } => {
                    let spec = if fresh {
                        RunSpec::fresh()
                    } else {
                        RunSpec::resume()
                    };
                    run_agent(&mut cfg, &state, &[text], spec.streaming(tap))
                }
                RunKind::PleaseFix(input) => {
                    cfg.stream = Some(tap.clone());
                    let text = crate::please_fix::build_prompt_soft_reported(&input, |message| {
                        cfg.notice(message);
                    });
                    run_agent(&mut cfg, &state, &[text], RunSpec::max().streaming(tap))
                }
                RunKind::Local(args) => {
                    if args.len() == 1 && super::local_recipe::owned_slash(&args[0]) {
                        cfg.stream = Some(tap.clone());
                        crate::slash_dispatch::dispatch_slash(&args[0], &state, &mut cfg)
                    } else {
                        let recipe = super::local_recipe::encode(&cfg, &state, &args);
                        recipe
                            .and_then(|recipe| run_abbey_capture(&args, &tap.cancel, &recipe))
                            .map(|out| {
                                let body = format!("{}{}", out.stdout, out.stderr);
                                if !body.trim().is_empty() {
                                    tap.notice(format!("```\n{}\n```", body.trim_end()));
                                }
                                out.code
                            })
                    }
                }
                RunKind::AgentSlash(cmd) => {
                    cfg.stream = Some(tap);
                    crate::slash_dispatch::dispatch_slash(&cmd, &state, &mut cfg)
                }
            };
            let result = if outcome_tap.exceeded() {
                Err(anyhow::anyhow!(
                    "stream exceeded the 4 MiB decoded-payload or 65,536-event limit"
                ))
            } else {
                result
            };
            let _ = done_tx.send(match result {
                Ok(code) => (code, None),
                Err(e) if e.is::<crate::agent::CaptureCancelled>() => (130, None),
                Err(e) => (1, Some(format!("{e:#}"))),
            });
        })?;
    Ok(RunHandle {
        events,
        done,
        cancel,
        started: Instant::now(),
        thread: Some(thread),
        completion: None,
        local,
        cancel_requested: None,
        abandoned: false,
    })
}

pub(crate) enum SlashRoute {
    Agent,
    Local,
    Interactive,
}

pub(crate) fn route_slash(input: &str) -> SlashRoute {
    let Some((name, _)) = crate::slash::parse_slash(input) else {
        return SlashRoute::Local;
    };
    let name = crate::slash_alias::resolve_name(name).unwrap_or(name);
    if INTERACTIVE_SLASH.contains(&name) {
        return SlashRoute::Interactive;
    }
    match SLASH_CATALOG
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.kind)
    {
        Some(SlashKind::Agent) => SlashRoute::Agent,
        _ => SlashRoute::Local,
    }
}

pub(crate) struct ChildOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// `abbey <args…>` as a bounded, captured child (60 s, 1 MiB per stream).
fn run_abbey_capture(
    args: &[String],
    cancel: &CancellationToken,
    recipe: &str,
) -> anyhow::Result<ChildOutput> {
    capture_local(
        std::env::current_exe()?,
        args,
        &std::env::current_dir()?,
        cancel,
        Some(recipe),
    )
}

fn capture_local(
    exe: std::path::PathBuf,
    args: &[String],
    cwd: &std::path::Path,
    cancel: &CancellationToken,
    recipe: Option<&str>,
) -> anyhow::Result<ChildOutput> {
    #[cfg(unix)]
    {
        use crate::runtime::supervisor::{
            ProcessSpec, SupervisorLimits, SupervisorOutcome, run_with_checkpoint,
        };
        use std::time::Duration;
        let mut spec =
            ProcessSpec::inherited(exe, args.iter().map(std::ffi::OsString::from).collect());
        spec.current_dir = Some(cwd.to_path_buf());
        if let Some(recipe) = recipe {
            let mut environment: Vec<_> = std::env::vars_os()
                .filter(|(key, _)| key != super::local_recipe::ENV)
                .collect();
            environment.push((super::local_recipe::ENV.into(), recipe.into()));
            spec.environment =
                crate::runtime::supervisor::ProcessEnvironment::ClearAndSet(environment);
        }
        let limits = SupervisorLimits {
            timeout: Duration::from_secs(60),
            terminate_grace: Duration::from_millis(500),
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 1024 * 1024,
            poll_interval: Duration::from_millis(10),
        };
        match run_with_checkpoint(&spec, &limits, || cancel.is_cancelled()) {
            Ok(SupervisorOutcome::Exited {
                status,
                stdout,
                stderr,
            }) => Ok(ChildOutput {
                code: status.code().unwrap_or(1),
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            }),
            Ok(SupervisorOutcome::Cancelled) => Ok(ChildOutput {
                code: 130,
                stdout: String::new(),
                stderr: String::new(),
            }),
            Ok(other) => anyhow::bail!("abbey child ended as {other:?}"),
            Err(e) => anyhow::bail!("abbey child: {e}"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (exe, args, cwd, cancel, recipe);
        anyhow::bail!("bounded cancellable local TUI commands are unavailable on this platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn local_child_cancels_and_reaps_its_descendant_before_returning() {
        use std::time::{Duration, Instant};
        let root =
            std::env::temp_dir().join(format!("abbey-local-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let script = "sleep 30 & child=$!; printf '%s' \"$child\" > child.pid.tmp; mv child.pid.tmp child.pid; wait";
        let cancel = CancellationToken::new();
        let thread_cancel = cancel.clone();
        let thread_root = root.clone();
        let run = std::thread::spawn(move || {
            capture_local(
                "/bin/sh".into(),
                &["-c".into(), script.into()],
                &thread_root,
                &thread_cancel,
                None,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("child.pid").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid: i32 = std::fs::read_to_string(root.join("child.pid"))
            .expect("child ready")
            .parse()
            .unwrap();
        let start = Instant::now();
        cancel.cancel();
        assert_eq!(run.join().unwrap().unwrap().code, 130);
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn slash_routing_follows_the_catalog_kind_and_aliases() {
        assert!(matches!(route_slash("/review"), SlashRoute::Agent));
        assert!(matches!(route_slash("/plan write it"), SlashRoute::Agent));
        assert!(matches!(route_slash("/doctor"), SlashRoute::Local));
        assert!(matches!(route_slash("/reset"), SlashRoute::Local));
        assert!(matches!(route_slash("/voice"), SlashRoute::Interactive));
        assert!(matches!(route_slash("/listen"), SlashRoute::Interactive));
    }
}
