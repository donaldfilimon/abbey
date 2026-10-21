//! Copying TUI text to the system clipboard.
//!
//! The TUI runs inside a terminal, so it has no browser clipboard and no
//! toolkit to ask. The host's own command is the way: `pbcopy` on macOS,
//! `wl-copy` or `xclip` on Linux, `clip` on Windows.
//!
//! # Boundaries
//!
//! * **Read-only with respect to the transcript.** Copying reads the text
//!   and hands it to the host; it never edits, sends, or persists anything.
//! * **The host command is looked up on `PATH`, never hard-coded.** A
//!   literal `/usr/bin/pbcopy` would be a macOS-only feature in a codebase
//!   that supports three primary hosts, and it would break wherever the
//!   command lives somewhere else. `crate::host::which_bin` already does the
//!   portable lookup, including the Windows `PATHEXT` rules.
//! * **A missing clipboard is reported, never faked.** The action returns
//!   what happened, and the TUI says so. Silently doing nothing would look
//!   like a successful copy that never happened.
//! * **No shell.** The command is spawned with an argument vector, so text
//!   containing shell metacharacters is data, not syntax. This is the same
//!   rule `src/os_control.rs` and the executor path follow.

use std::io::Write;
use std::process::{Command, Stdio};

/// The outcome of a copy attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    /// The text was handed to the host's clipboard command.
    Copied,
    /// No clipboard command is available on this host.
    Unavailable,
    /// The clipboard command was found but refused the copy.
    Refused(String),
}

impl CopyOutcome {
    /// A one-line description for the TUI's status line.
    pub fn describe(&self) -> String {
        match self {
            Self::Copied => "copied to the clipboard".to_owned(),
            Self::Unavailable => {
                "no clipboard command on this host (tried pbcopy, wl-copy, xclip, clip)".to_owned()
            }
            Self::Refused(reason) => format!("the clipboard refused the copy: {reason}"),
        }
    }
}

/// The clipboard commands to try, in order.
///
/// `pbcopy` comes first because macOS is the primary host; the Linux
/// candidates follow because a Wayland session has `wl-copy` and an X11
/// session has `xclip`; `clip` is the Windows one.
const CANDIDATES: &[&str] = &["pbcopy", "wl-copy", "xclip", "clip"];

/// Copy `text` to the host's clipboard.
///
/// Returns what happened. It never fails hard: the TUI is a reading and
/// writing surface, and a machine with no clipboard command is a perfectly
/// ordinary machine.
pub fn copy(text: &str) -> CopyOutcome {
    let Some((command, args)) = first_available() else {
        return CopyOutcome::Unavailable;
    };
    copy_with(&command, &args, text)
}

/// A copy running on its own thread.
///
/// The TUI's event loop must not block: a clipboard command that hangs —
/// a `pbcopy` waiting on a pasteboard server that never answers — would
/// otherwise freeze the whole app behind a keystroke the user cannot
/// cancel. The copy runs on a worker and the result is collected on a
/// later pump.
pub struct PendingCopy {
    result: std::sync::mpsc::Receiver<CopyOutcome>,
    thread: std::thread::JoinHandle<()>,
}

impl PendingCopy {
    /// Start a copy of `text`.
    pub fn start(text: String) -> Self {
        let (tx, result) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("abbey-tui-clipboard".into())
            .spawn(move || {
                let outcome = copy(&text);
                // The receiver may be gone if the TUI quit first; that is
                // not an error worth reporting anywhere.
                let _ = tx.send(outcome);
            })
            .expect("the clipboard worker thread starts");
        Self { result, thread }
    }

    /// The outcome, if the copy has finished.
    ///
    /// `None` means it is still running. The caller keeps its "copying…"
    /// status and asks again on the next pump.
    pub fn poll(&self) -> Option<CopyOutcome> {
        self.result.try_recv().ok()
    }

    /// Wait for the copy to finish and join the worker.
    ///
    /// Used when the TUI is shutting down, so the worker is not left
    /// running against a terminal that is going away.
    pub fn join(self) -> Option<CopyOutcome> {
        let outcome = self.result.recv().ok();
        let _ = self.thread.join();
        outcome
    }
}

/// Copy `text` using one specific command.
///
/// Split out so a test can point the action at a stub without a real
/// clipboard on the machine.
pub fn copy_with(command: &std::path::Path, args: &[&str], text: &str) -> CopyOutcome {
    let mut child = match Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return CopyOutcome::Refused(error.to_string()),
    };
    // The command reads the text on stdin rather than as an argument, so
    // the text never appears in a process listing and its size is not
    // bounded by an argument limit.
    if let Some(stdin) = child.stdin.as_mut()
        && let Err(error) = stdin.write_all(text.as_bytes())
    {
        let _ = child.kill();
        let _ = child.wait();
        return CopyOutcome::Refused(error.to_string());
    }
    // Drop stdin so the command sees EOF and finishes.
    drop(child.stdin.take());
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => return CopyOutcome::Refused(error.to_string()),
    };
    if output.status.success() {
        CopyOutcome::Copied
    } else {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        CopyOutcome::Refused(if reason.is_empty() {
            format!("exited with {}", output.status)
        } else {
            reason
        })
    }
}

/// The first clipboard command available on this host, with its arguments.
///
/// The arguments differ per command: `pbcopy` and `clip` read stdin and
/// need none, `wl-copy` reads stdin, and `xclip` needs to be told which
/// selection to fill.
fn first_available() -> Option<(std::path::PathBuf, Vec<&'static str>)> {
    CANDIDATES.iter().find_map(|name| {
        crate::host::which_bin(name).map(|path| {
            let args: Vec<&'static str> = match *name {
                "xclip" => vec!["-selection", "clipboard"],
                _ => Vec::new(),
            };
            (path, args)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    fn stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("abbey-clipboard-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_successful_command_copies_the_text() {
        let dir = scratch("ok");
        let out = dir.join("copied.txt");
        // A stub that records what it was handed on stdin.
        let command = stub(
            &dir,
            "pbcopy",
            &format!("#!/bin/sh\ncat > {}\n", out.display()),
        );
        let outcome = copy_with(&command, &[], "hello\nworld");
        assert_eq!(outcome, CopyOutcome::Copied);
        assert_eq!(fs::read_to_string(&out).unwrap(), "hello\nworld");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failing_command_is_reported_with_its_own_diagnosis() {
        let dir = scratch("fail");
        let command = stub(
            &dir,
            "pbcopy",
            "#!/bin/sh\nprintf 'no clipboard daemon\\n' >&2\nexit 1\n",
        );
        let outcome = copy_with(&command, &[], "text");
        match outcome {
            CopyOutcome::Refused(reason) => {
                assert!(reason.contains("no clipboard daemon"), "{reason}");
            }
            other => panic!("a failing command must be refused, saw {other:?}"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_that_cannot_start_is_reported_rather_than_silently_ignored() {
        let dir = scratch("missing");
        let outcome = copy_with(&dir.join("not-there"), &[], "text");
        assert!(
            matches!(outcome, CopyOutcome::Refused(_)),
            "saw {outcome:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_with_shell_metacharacters_is_data_not_syntax() {
        // The text goes on stdin, so nothing in it can be interpreted. A
        // file that would be created by a shell expansion proves it.
        let dir = scratch("metachar");
        let canary = dir.join("canary");
        let out = dir.join("copied.txt");
        let command = stub(
            &dir,
            "pbcopy",
            &format!("#!/bin/sh\ncat > {}\n", out.display()),
        );
        let text = format!(
            "$(touch {})`touch {}`; rm -rf /\n",
            canary.display(),
            canary.display()
        );
        let outcome = copy_with(&command, &[], &text);
        assert_eq!(outcome, CopyOutcome::Copied);
        assert_eq!(fs::read_to_string(&out).unwrap(), text);
        assert!(!canary.exists(), "the text must never reach a shell");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pending_copy_reports_its_outcome_when_it_finishes() {
        // The copy runs on a worker so a hanging clipboard command cannot
        // freeze the event loop. The outcome arrives later, through the
        // poll.
        //
        // The outcome depends on whether this machine has a clipboard
        // command, so the test asserts that an outcome *arrives* and that
        // it is one of the three honest ones — not which one.
        let pending = PendingCopy::start("copied text".to_owned());

        // Poll until the outcome arrives; the worker is a real thread, so
        // this is a bounded wait rather than an instant.
        let mut outcome = None;
        for _ in 0..200 {
            if let Some(seen) = pending.poll() {
                outcome = Some(seen);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            matches!(
                outcome,
                Some(CopyOutcome::Copied)
                    | Some(CopyOutcome::Unavailable)
                    | Some(CopyOutcome::Refused(_))
            ),
            "a finished copy must report one of the three outcomes, saw {outcome:?}"
        );
    }

    #[test]
    fn a_pending_copy_can_be_joined() {
        // Shutdown joins the worker, so a copy in flight is not left running
        // against a terminal that is going away.
        let pending = PendingCopy::start("text".to_owned());
        let outcome = pending.join();
        assert!(outcome.is_some(), "joining a copy must yield its outcome");
    }

    #[test]
    fn the_outcome_describes_itself_for_the_status_line() {
        assert!(CopyOutcome::Copied.describe().contains("copied"));
        assert!(
            CopyOutcome::Unavailable
                .describe()
                .contains("no clipboard command")
        );
        assert!(
            CopyOutcome::Refused("boom".into())
                .describe()
                .contains("boom")
        );
    }

    #[test]
    fn the_candidates_start_with_the_macos_command() {
        // macOS is the primary host, so its command is tried first.
        assert_eq!(CANDIDATES.first(), Some(&"pbcopy"));
    }
}
