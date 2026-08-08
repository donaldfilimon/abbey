//! Abbey — hybrid persona/role CLI/TUI (cursor-agent + abi-ai + SQLite memory).

mod actions;
mod agent;
mod build_info;
mod capture;
mod claims;
mod cli;
mod commands;
mod config;
mod deferred;
mod doctor;
mod generate;
mod gitops;
mod highlight;
mod host;
mod hybrid_loop;
mod improve;
mod init;
mod inventory;
mod learn;
mod media;
mod memory;
mod mesh;
mod models;
mod os_control;
mod output;
mod parallel;
mod persona;
mod platform;
mod please_fix;
mod prompts;
mod protocols;
mod roles;
mod route_log;
mod session;
mod slash;
mod slash_dispatch;
mod state;
mod subagents;
mod surfaces;
mod tui;
mod voice;
mod wdbx_bridge;

pub use session::hybrid_run;
pub use slash_dispatch::dispatch_slash;

use crate::actions::{RunSpec, run_agent};
use agent::AgentConfig;
use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use session::apply_global_flags;
use state::AbbeyState;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

/// Restore default `SIGPIPE` so `abbey … | head` ends quietly.
///
/// Rust's std sets `SIGPIPE` to `SIG_IGN` before `main`, which turns a closed
/// downstream reader into an `EPIPE` error that `println!` panics on — so
/// `abbey memory map | head -2` printed a Rust panic instead of exiting like
/// `cat`/`ls` do. Only fires once output exceeds the pipe buffer, which is why
/// short commands looked fine.
#[cfg(unix)]
// The crate denies `unsafe_code` (Cargo.toml `[lints.rust]`). This is the only
// production exception: restoring the default SIGPIPE disposition needs libc,
// and there is no safe std API for it.
#[allow(unsafe_code)]
fn restore_sigpipe() {
    // SAFETY: setting SIG_DFL is async-signal-safe, and this runs as the first
    // statement of `main` — before any threads exist or any output is written.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Windows has no `SIGPIPE`; a closed pipe surfaces as a normal write error.
#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() -> ExitCode {
    // Before `Cli::parse()`, which itself prints for `--help` / `--version`.
    restore_sigpipe();
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
    if command_needs_executor(&cli) {
        cfg = cfg.with_resolved_agent()?;
    }

    apply_global_flags(&cli, &state, &mut cfg)?;

    if let Some(ws) = &cli.workspace {
        std::env::set_current_dir(ws)?;
    }

    if cli.continue_session && cli.command.is_none() {
        return run_agent(&mut cfg, &state, &cli.prompt, RunSpec::resume());
    }

    if cli.command.is_none() && cli.prompt.len() == 1 && cli.prompt[0].starts_with('/') {
        return dispatch_slash(&cli.prompt[0], &state, &mut cfg);
    }
    if cli.command.is_none() && !cli.prompt.is_empty() && cli.prompt[0].starts_with('/') {
        let joined = cli.prompt.join(" ");
        return dispatch_slash(&joined, &state, &mut cfg);
    }

    let open_tui = cli.tui
        || matches!(cli.command, Some(Commands::Tui))
        || (cli.command.is_none()
            && cli.prompt.is_empty()
            && !cli.no_tui
            && io::stdin().is_terminal());

    if open_tui && !cli.no_tui {
        return tui::run_tui(state, cfg);
    }

    commands::run_cli(cli, state, cfg)
}

/// Local inventory/index/proof commands do not execute the selected model
/// backend and therefore must remain usable when that backend is unavailable.
fn command_needs_executor(cli: &Cli) -> bool {
    !matches!(
        &cli.command,
        Some(
            Commands::Memory { .. }
                | Commands::Mesh { .. }
                | Commands::Agents
                | Commands::Skills { .. }
                | Commands::Plugins { .. }
                | Commands::Mcp { .. }
                | Commands::Acp { .. }
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_control_plane_commands_do_not_resolve_the_model_executor() {
        for args in [
            &["abbey", "mcp", "status"][..],
            &["abbey", "plugins"][..],
            &["abbey", "memory", "embed", "status"][..],
            &["abbey", "mesh", "status"][..],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(!command_needs_executor(&cli), "args={args:?}");
        }
    }

    #[test]
    fn generation_still_requires_the_selected_executor() {
        let cli = Cli::try_parse_from(["abbey", "print", "hello"]).unwrap();
        assert!(command_needs_executor(&cli));
    }
}
