//! Explicit standalone compatibility entry; owns process globals and lifetime.
//!
//! Abbey must not call this to construct an extension. Host injection belongs to
//! M9.2; this entry retains the independently operated Bot process contract.

use super::*;

/// Run the original environment-driven command line and process lifecycle.
///
/// This may install hooks, mutate the environment before creating threads,
/// create a Tokio runtime, read credentials and terminate the process. Call only
/// as the standalone executable entry.
///
/// # Safety
///
/// The caller must own the process entry, before any other application thread
/// exists. In particular, no other thread may read or modify the environment
/// while managed startup installs its resolved environment.
pub unsafe fn run() -> Result<(), Error> {
    // Argument validation and managed credential loading happen before any
    // runtime/log worker thread exists. Self-tests never enter managed preflight.
    let managed_requested = std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--managed-service");
    let startup = match startup_action() {
        Ok(action) => action,
        Err(error) => {
            if managed_requested {
                eprintln!("invalid managed service arguments");
            } else {
                eprintln!("{error}");
            }
            std::process::exit(2);
        }
    };
    if let StartupAction::FmManifestIdentity(cli) = &startup {
        return provider_identity::run(cli);
    }
    if let StartupAction::LearningQuality(corpus) = &startup {
        std::process::exit(learning_quality_cli::run(corpus));
    }
    let managed = if startup == StartupAction::ManagedDiscord {
        configure_managed_panic_hook(None);
        let Some(home) = std::env::var_os("HOME") else {
            std::process::exit(78);
        };
        let prepared = match managed_service::begin(std::path::Path::new(&home)) {
            Ok(prepared) => prepared,
            Err(_) => std::process::exit(78),
        };
        let fatal = prepared.fatal.clone();
        configure_managed_panic_hook(Some(fatal));
        for (name, value) in prepared.environment.into_values() {
            // SAFETY: the caller owns process entry; this function has not constructed a
            // runtime, logging worker or any
            // other application thread. Every environment reader starts later.
            unsafe {
                std::env::set_var(name, value);
            }
        }
        Some(ManagedStartup {
            publisher: prepared.publisher,
            log: prepared.log,
            privacy_report: prepared.privacy_report,
            fatal: prepared.fatal,
        })
    } else {
        None
    };
    let is_managed = managed.is_some();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let mut terminal = service::shutdown::TerminalBoundary::default();
    let result = runtime.block_on(startup::run(startup, managed, &mut terminal));
    if let Some(initialization) = &mut terminal.initialization {
        let remaining = terminal.budget.map_or(service::SHUTDOWN_BUDGET, |budget| {
            budget.remaining(tokio::time::Instant::now())
        });
        let joined = runtime.block_on(async {
            tokio::time::timeout(remaining, initialization)
                .await
                .is_ok()
        });
        terminal.incomplete |= !joined;
    }
    if !terminal.refresh_joined
        && let Some(refresh) = &mut terminal.refresh
    {
        refresh.stop();
        let remaining = terminal.budget.map_or(service::SHUTDOWN_BUDGET, |budget| {
            budget.remaining(tokio::time::Instant::now())
        });
        let result =
            runtime.block_on(async { tokio::time::timeout(remaining, refresh.joined()).await });
        terminal.refresh_joined = result.is_ok();
        terminal.incomplete |= !result.is_ok_and(|r| r.is_ok());
    }
    if !terminal.telemetry_joined
        && let Some(telemetry) = &mut terminal.telemetry
    {
        telemetry.stop();
        let remaining = terminal.budget.map_or(service::SHUTDOWN_BUDGET, |budget| {
            budget.remaining(tokio::time::Instant::now())
        });
        terminal.telemetry_joined = runtime.block_on(async {
            tokio::time::timeout(remaining, telemetry.joined())
                .await
                .is_ok_and(|r| r.is_ok())
        });
        terminal.incomplete |= !terminal.telemetry_joined;
    }
    let remaining = terminal.budget.map_or(service::SHUTDOWN_BUDGET, |budget| {
        budget.remaining(tokio::time::Instant::now())
    });
    runtime.shutdown_timeout(remaining);
    // Incomplete blocking work is contained by process termination, never
    // reported as joined or allowed into unbounded implicit Runtime::drop.
    if terminal.incomplete {
        std::process::exit(70);
    }
    if is_managed && result.is_err() {
        std::process::exit(if terminal.budget.is_none() { 78 } else { 1 });
    }
    result
}
