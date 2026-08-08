//! Abbey — hybrid persona/role CLI/TUI (cursor-agent + abi-ai + SQLite memory).

use std::process::ExitCode;

/// Restore default `SIGPIPE` so `abbey … | head` ends quietly.
///
/// Rust's std sets `SIGPIPE` to `SIG_IGN` before `main`, which turns a closed
/// downstream reader into an `EPIPE` error that `println!` panics on. This must
/// run before Clap can write `--help` or `--version` output.
#[cfg(unix)]
#[allow(unsafe_code)]
fn restore_sigpipe() {
    // SAFETY: setting SIG_DFL is async-signal-safe, and this runs as the first
    // statement of `main` before any threads exist or output is written.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Windows has no `SIGPIPE`; a closed pipe surfaces as a normal write error.
#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() -> ExitCode {
    restore_sigpipe();
    abbey::run_cli()
}
