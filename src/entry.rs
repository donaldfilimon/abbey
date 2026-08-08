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

/// Local inventory/index/proof commands do not execute the selected model
/// backend and therefore must remain usable when that backend is unavailable.
fn command_needs_executor(cli: &Cli) -> bool {
    !matches!(
        &cli.command,
        Some(
            Commands::Memory { .. }
                | Commands::Mesh { .. }
                | Commands::Daemon { .. }
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
            &["abbey", "daemon", "status"][..],
            &["abbey", "daemon", "claims", "--status", "current"][..],
            &["abbey", "daemon", "claims", "--status", "partial"][..],
            &["abbey", "daemon", "claims", "--status", "proposed"][..],
            &["abbey", "daemon", "claims", "--status", "blocked"][..],
            &["abbey", "daemon", "claims", "--status", "oos"][..],
            &["abbey", "daemon", "claims", "--status", "out-of-scope"][..],
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
