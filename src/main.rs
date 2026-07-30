//! Abbey — hybrid persona/role CLI/TUI (cursor-agent + abi-ai + SQLite memory).

mod actions;
mod agent;
mod build_info;
mod claims;
mod cli;
mod commands;
mod config;
mod deferred;
mod doctor;
mod generate;
mod gitops;
mod highlight;
mod hybrid_loop;
mod init;
mod inventory;
mod learn;
mod media;
mod memory;
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

fn main() -> ExitCode {
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
    let mut cfg = AgentConfig::default().with_resolved_agent()?;

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
