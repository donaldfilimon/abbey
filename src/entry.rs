//! Process-level CLI entry kept behind the reusable library boundary.

use crate::actions::{RunSpec, run_agent};
use crate::agent::AgentConfig;
use crate::cli::{Cli, Commands};
use crate::session::apply_global_flags;
use crate::state::AbbeyState;
use anyhow::Result;
use clap::Parser;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

/// Parse process arguments and run Abbey's CLI/TUI surface.
///
/// Process setup that must happen before Clap writes output, such as restoring
/// Unix `SIGPIPE`, remains in the thin binary target.
#[must_use]
pub fn run_cli() -> ExitCode {
    match real_main() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("abbey: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn real_main() -> Result<i32> {
    let cli = Cli::parse();
    let state = AbbeyState::load()?;
    let mut cfg = AgentConfig::default();
    // Best-effort only: a resolved path improves doctor/TUI display, but no
    // command fails here. Verbs that actually execute a backend resolve at
    // spawn time (`AgentConfig::exec_path`), so every local verb — claims,
    // memory, os, routes, the installer probes — works on a machine with no
    // executor installed. That is exactly when installs and audits happen.
    if let Ok(resolved) = cfg.clone().with_resolved_agent() {
        cfg = resolved;
    }

    apply_global_flags(&cli, &state, &mut cfg)?;

    if let Some(ws) = &cli.workspace {
        std::env::set_current_dir(ws)?;
    }

    if cli.continue_session && cli.command.is_none() {
        return run_agent(&mut cfg, &state, &cli.prompt, RunSpec::resume());
    }

    if cli.command.is_none() && cli.prompt.len() == 1 && cli.prompt[0].starts_with('/') {
        return crate::slash_dispatch::dispatch_slash(&cli.prompt[0], &state, &mut cfg);
    }
    if cli.command.is_none() && !cli.prompt.is_empty() && cli.prompt[0].starts_with('/') {
        let joined = cli.prompt.join(" ");
        return crate::slash_dispatch::dispatch_slash(&joined, &state, &mut cfg);
    }

    let open_tui = cli.tui
        || matches!(cli.command, Some(Commands::Tui))
        || (cli.command.is_none()
            && cli.prompt.is_empty()
            && !cli.no_tui
            && io::stdin().is_terminal());

    if open_tui && !cli.no_tui {
        return crate::tui::run_tui(state, cfg);
    }

    crate::commands::run_cli(cli, state, cfg)
}

// The old `command_needs_executor` allowlist lived here. It is gone on
// purpose: startup resolution is best-effort for *every* command and the hard
// requirement moved to spawn time, so the classification (and the drift risk
// of forgetting to exempt a new local verb) no longer exists. The guarantees
// are pinned at process level in `tests/cli_surface.rs`:
// `local_verbs_need_no_executor_backend` and
// `generation_without_any_backend_fails_with_guidance`.
