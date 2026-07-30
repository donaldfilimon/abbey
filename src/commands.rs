//! Clap subcommand dispatch.

use crate::actions::{RunSpec, fork_prompt, run_agent, run_commit, run_pr, run_review};
use crate::agent::{AgentConfig, run_resilient};
use crate::build_info;
use crate::cli::{Cli, Commands, MemoryCmd, Shell};
use crate::config;
use crate::doctor::{
    cmd_debug, cmd_doctor, cmd_init, cmd_memory_store, cmd_persona, cmd_role, print_config,
    print_permissions,
};
use crate::gitops;
use crate::inventory;
use crate::learn;
use crate::models::{alias_table, resolve_model};
use crate::os_control;
use crate::parallel;
use crate::please_fix;
use crate::route_log;
use crate::session::compact_history;
use crate::slash;
use crate::state::AbbeyState;
use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{Shell as ClapShell, generate};
use std::io;

pub fn run_cli(cli: Cli, state: AbbeyState, mut cfg: AgentConfig) -> Result<i32> {
    match cli.command {
        None | Some(Commands::Tui) => {
            if cli.prompt.is_empty() {
                return cmd_doctor(&state, &cfg);
            }
            run_agent(&mut cfg, &state, &cli.prompt, RunSpec::resume())
        }
        Some(Commands::New { prompt }) => run_agent(&mut cfg, &state, &prompt, RunSpec::fresh()),
        Some(Commands::Continue { prompt }) => {
            run_agent(&mut cfg, &state, &prompt, RunSpec::resume())
        }
        Some(Commands::Fork { prompt }) => {
            run_agent(&mut cfg, &state, &fork_prompt(&prompt), RunSpec::fresh())
        }
        Some(Commands::PleaseFix { text }) => {
            let prompt = please_fix::build_prompt(&text)?;
            run_agent(&mut cfg, &state, &[prompt], RunSpec::max())
        }
        Some(Commands::Ask { prompt }) => run_agent(&mut cfg, &state, &prompt, RunSpec::ask()),
        Some(Commands::Plan { prompt }) => run_agent(&mut cfg, &state, &prompt, RunSpec::plan()),
        Some(Commands::Print { prompt }) => {
            cfg.print = true;
            let chat = state.read_chat();
            let (st, out, err) = cfg.run_capture(chat.as_deref(), &prompt)?;
            print!("{out}");
            eprint!("{err}");
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Diff { staged }) => {
            println!("{}", gitops::diff_text(staged)?);
            Ok(0)
        }
        Some(Commands::Review { staged, note }) => {
            run_review(&mut cfg, &state, staged, &note, false)
        }
        Some(Commands::SecurityReview { staged }) => {
            run_review(&mut cfg, &state, staged, &[], true)
        }
        Some(Commands::Commit) => run_commit(&mut cfg, &state),
        Some(Commands::Pr) => run_pr(&mut cfg, &state),
        Some(Commands::Init {
            force,
            print,
            agent,
        }) => cmd_init(&cfg, &state, force, print, agent),
        Some(Commands::Branch { name }) => {
            println!("{}", gitops::create_branch(&name)?);
            Ok(0)
        }
        Some(Commands::Explain { target, question }) => {
            if cfg.mode.is_none() {
                cfg.mode = Some("ask".into());
            }
            let mut prompt = if std::path::Path::new(&target).exists() {
                format!(
                    "Explain {target} clearly for a skilled engineer. Cover purpose, key flow, gotchas, and how to change it safely."
                )
            } else {
                format!("Explain: {target}")
            };
            if !question.is_empty() {
                prompt.push_str("\n\nSpecific question: ");
                prompt.push_str(&question.join(" "));
            }
            run_agent(&mut cfg, &state, &[prompt], RunSpec::resume())
        }
        Some(Commands::Model { name }) => match name.as_deref() {
            None => {
                println!("{}", state.read_model());
                Ok(0)
            }
            Some("clear" | "default" | "auto") => {
                state.clear_model()?;
                eprintln!("abbey: model reset to auto");
                Ok(0)
            }
            Some(m) => {
                let resolved = resolve_model(m);
                state.save_model(&resolved)?;
                eprintln!("abbey: default model set to {resolved}");
                println!("{resolved}");
                Ok(0)
            }
        },
        Some(Commands::Aliases) => {
            println!("Model aliases (abbey -m NAME):");
            for (a, full) in alias_table() {
                println!("  {a:<16} {full}");
            }
            println!("\nFull ids always pass through. See: abbey models");
            Ok(0)
        }
        Some(Commands::Doctor) => cmd_doctor(&state, &cfg),
        Some(Commands::Debug) => cmd_debug(&state, &cfg),
        Some(Commands::ChatId) => match state.read_chat() {
            Some(id) => {
                println!("{id}");
                Ok(0)
            }
            None => {
                eprintln!("abbey: no chat id yet (run abbey / abbey new)");
                Ok(1)
            }
        },
        Some(Commands::Clear { all }) => {
            state.clear_chat(all)?;
            eprintln!(
                "abbey: cleared {}",
                if all { "all chats" } else { "cwd chat" }
            );
            Ok(0)
        }
        Some(Commands::Compact { keep }) => {
            let n = compact_history(&state, keep)?;
            println!("abbey: history compacted to last {n} entries");
            Ok(0)
        }
        Some(Commands::History { n }) => {
            for e in state.history(n) {
                println!("{}\t{}\t{}", e.timestamp, e.chat_id, e.cwd);
            }
            Ok(0)
        }
        Some(Commands::Memory { cmd }) => match cmd {
            None | Some(MemoryCmd::Chat) => {
                println!(
                    "chat: {}",
                    state.read_chat().unwrap_or_else(|| "(none)".into())
                );
                println!("chat file: {}", state.active_chat_file().display());
                println!("history:");
                for e in state.history(10) {
                    println!("  {}\t{}\t{}", e.timestamp, e.chat_id, e.cwd);
                }
                Ok(0)
            }
            Some(other) => cmd_memory_store(&state, other),
        },
        Some(Commands::Persona { name }) => cmd_persona(name.as_deref()),
        Some(Commands::Role { name }) => cmd_role(name.as_deref()),
        Some(Commands::Routes { n }) => {
            for r in route_log::recent_routes(&state.state_dir, n)? {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    r.ts, r.persona, r.role, r.model, r.reason
                );
            }
            Ok(0)
        }
        Some(Commands::Os { args }) => os_control::run_os(&args, true),
        Some(Commands::Learn { args }) => learn::dispatch(&state, &args),
        Some(Commands::Parallel { prompt }) => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            parallel::run_parallel_cli(&cfg, &state, &prompt, &ac.roles.max, &ac.roles.gemma)
        }
        Some(Commands::Agents) => {
            inventory::print_agent_tools();
            for line in build_info::lines() {
                println!("{line}");
            }
            Ok(0)
        }
        Some(Commands::Skills) => {
            inventory::print_skills()?;
            Ok(0)
        }
        Some(Commands::Plugins { args }) => inventory::run_plugin_passthrough(&args),
        Some(Commands::Permissions) => {
            print_permissions(&cfg);
            Ok(0)
        }
        Some(Commands::Config) => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            print_config(&state, &cfg);
            for line in ac.status_lines() {
                println!("{line}");
            }
            Ok(0)
        }
        Some(Commands::Cost) => {
            println!(
                "abbey: cost/token accounting is not exposed by cursor-agent.\n\
                 Use your Cursor account dashboard for usage. (Claude Code /cost parity: N/A)"
            );
            Ok(0)
        }
        Some(Commands::SlashHelp) => {
            print!("{}", slash::help_text());
            Ok(0)
        }
        Some(Commands::CreateChat) => {
            let id = cfg.create_chat()?;
            state.save_chat(&id)?;
            println!("{id}");
            Ok(0)
        }
        Some(Commands::Status) => {
            let st = cfg.passthrough(&["status".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Models) => {
            let st = cfg.passthrough(&["models".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Ls) => {
            let st = cfg.passthrough(&["ls".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Login) => {
            let st = cfg.passthrough(&["login".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Logout) => {
            let st = cfg.passthrough(&["logout".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Mcp { args }) => {
            let mut a = vec!["mcp".into()];
            a.extend(args);
            let st = cfg.passthrough(&a)?;
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Plugin { args }) => inventory::run_plugin_passthrough(&args),
        Some(Commands::Completion { shell }) => {
            let mut cmd = Cli::command();
            let sh = match shell {
                Shell::Bash => ClapShell::Bash,
                Shell::Zsh => ClapShell::Zsh,
                Shell::Fish => ClapShell::Fish,
                Shell::Pwsh => ClapShell::PowerShell,
                Shell::Elvish => ClapShell::Elvish,
            };
            generate(sh, &mut cmd, "abbey", &mut io::stdout());
            Ok(0)
        }
        Some(Commands::External(args)) => {
            let passthrough = [
                "login",
                "logout",
                "update",
                "mcp",
                "plugin",
                "worker",
                "about",
                "list-models",
                "install-shell-integration",
                "uninstall-shell-integration",
                "generate-rule",
                "rule",
                "record",
                "bedrock",
            ];
            if args
                .first()
                .is_some_and(|a| passthrough.contains(&a.as_str()))
            {
                let st = cfg.passthrough(&args)?;
                return Ok(st.code().unwrap_or(1));
            }
            run_resilient(&cfg, &state, false, &args)
        }
    }
}
