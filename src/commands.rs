//! Clap subcommand dispatch.

use crate::actions::{RunSpec, fork_prompt, run_agent, run_commit, run_pr, run_review};
use crate::agent::{AgentConfig, run_resilient};
use crate::build_info;
use crate::claims;
use crate::cli::{Cli, Commands, GenerateCmd, MemoryCmd, Shell};
use crate::config;
use crate::deferred;
use crate::doctor::{
    cmd_debug, cmd_doctor, cmd_init, cmd_memory_store, cmd_persona, cmd_role, print_config,
    print_permissions,
};
use crate::generate::{self, GenKind};
use crate::gitops;
use crate::highlight;
use crate::improve;
use crate::inventory;
use crate::learn;
use crate::models::{alias_table, resolve_model};
use crate::os_control;
use crate::output;
use crate::parallel;
use crate::platform;
use crate::please_fix;
use crate::protocols;
use crate::route_log;
use crate::session::compact_history;
use crate::slash;
use crate::state::AbbeyState;
use crate::subagents;
use crate::surfaces;
use crate::voice;
use crate::wdbx_bridge;
use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{Shell as ClapShell, generate};
use std::io;

/// Refuse account/session/MCP verbs under backends that have no such surface.
fn passthrough_or_refuse(cfg: &AgentConfig, verb: &str, args: &[String]) -> Result<i32> {
    if !cfg.backend.supports_account_surface() {
        eprintln!(
            "abbey: `{verb}` is not applicable under the on-device backend (ABBEY_BACKEND=fm).\n\
             The Apple Foundation Models CLI has no account, session list, or MCP surface.\n\
             Unset ABBEY_BACKEND to use cursor-agent."
        );
        return Ok(2);
    }
    let st = cfg.passthrough(args)?;
    Ok(st.code().unwrap_or(1))
}

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
            eprint!("{err}");
            highlight::emit_agent_stdout(&out);
            Ok(st.code().unwrap_or(1))
        }
        Some(Commands::Diff { staged }) => {
            let text = gitops::diff_text(staged)?;
            if highlight::enabled() {
                let _ = output::print(highlight::colorize_code(&text, Some("diff"), None));
            } else {
                print!("{text}");
            }
            Ok(0)
        }
        Some(Commands::Highlight { args }) => highlight::dispatch(&args),
        Some(Commands::Claims { args }) => claims::dispatch(&args),
        Some(Commands::Platform { args }) => platform::dispatch(&args),
        Some(Commands::Compute { args }) => {
            let mut a = vec!["compute".into()];
            a.extend(args);
            platform::dispatch(&a)
        }
        Some(Commands::Vision { args }) => surfaces::dispatch_vision(&args),
        Some(Commands::Cot { args }) => {
            if args.first().map(String::as_str) == Some("run") {
                let prompt: Vec<String> = args.iter().skip(1).cloned().collect();
                generate::run_reason(&mut cfg, &state, &prompt, None)
            } else {
                surfaces::dispatch_cot(&args, &state)
            }
        }
        Some(Commands::Runtime { args }) => surfaces::dispatch_runtime(&args),
        Some(Commands::Oos { args }) => deferred::dispatch_oos(&args),
        Some(Commands::Lora { args }) => deferred::dispatch_topic("lora", &args),
        Some(Commands::Weights { args }) => deferred::dispatch_topic("weights", &args),
        Some(Commands::Accel { args }) => deferred::dispatch_topic("accel", &args),
        Some(Commands::Shell { args }) => deferred::dispatch_topic("shell", &args),
        Some(Commands::Host { args }) => deferred::dispatch_topic("host", &args),
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
        Some(Commands::Routes { n, correlation }) => {
            let records = match &correlation {
                Some(id) => route_log::correlated_routes(&state.state_dir, id)?,
                None => route_log::recent_routes(&state.state_dir, n)?,
            };
            if correlation.is_some() && records.is_empty() {
                eprintln!("abbey: no routes for that correlation id");
                return Ok(1);
            }
            for r in records {
                println!("{}", route_log::format_route_line(&r));
            }
            Ok(0)
        }
        Some(Commands::Wdbx { args }) => wdbx_bridge::run(&state, &args),
        Some(Commands::HybridLoop { prompt }) => {
            run_agent(&mut cfg, &state, &prompt, RunSpec::hybrid_loop())
        }
        Some(Commands::Imagine {
            out,
            aspect,
            edit,
            prompt,
        }) => generate::run_generate(
            &mut cfg,
            &state,
            GenKind::Image,
            &prompt,
            out,
            aspect.as_deref(),
            edit,
        ),
        Some(Commands::Generate { cmd }) => match cmd {
            GenerateCmd::Image {
                out,
                aspect,
                edit,
                prompt,
            } => generate::run_generate(
                &mut cfg,
                &state,
                GenKind::Image,
                &prompt,
                out,
                aspect.as_deref(),
                edit,
            ),
            GenerateCmd::Video { out, prompt } => {
                generate::run_generate(&mut cfg, &state, GenKind::Video, &prompt, out, None, None)
            }
        },
        Some(Commands::Reason { thinking, prompt }) => {
            generate::run_reason(&mut cfg, &state, &prompt, thinking.as_deref())
        }
        Some(Commands::Voice { args }) => voice::dispatch(&state, &mut cfg, &args),
        Some(Commands::Speak {
            voice: v,
            rate,
            out,
            text,
        }) => {
            let mut args = vec!["speak".into()];
            if let Some(name) = v {
                args.push("-v".into());
                args.push(name);
            }
            if let Some(r) = rate {
                args.push("-r".into());
                args.push(r.to_string());
            }
            if let Some(path) = out {
                args.push("-o".into());
                args.push(path.display().to_string());
            }
            args.extend(text);
            voice::dispatch(&state, &mut cfg, &args)
        }
        Some(Commands::Os { args }) => os_control::run_os(&args, true),
        Some(Commands::Allowlist) => os_control::print_policy(false),
        Some(Commands::Learn { args }) => learn::dispatch(&state, &args),
        Some(Commands::LearnReview { n }) => {
            learn::review_train(&state, n)?;
            Ok(0)
        }
        Some(Commands::LearnStats) => {
            learn::train_stats(&state)?;
            Ok(0)
        }
        Some(Commands::Parallel { prompt }) => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            parallel::run_parallel_cli(&cfg, &state, &prompt, &ac.roles.max, &ac.roles.gemma)
        }
        Some(Commands::Subagents { args }) => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            subagents::dispatch(&cfg, &state, &args, &ac.roles.max, &ac.roles.gemma)
        }
        Some(Commands::Improve { args }) => improve::dispatch(&cfg, &state, &args),
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
        Some(Commands::Status) => passthrough_or_refuse(&cfg, "status", &["status".into()]),
        Some(Commands::Models) => {
            if !cfg.backend.supports_account_surface() {
                print!("{}", cfg.list_models_text()?);
                return Ok(0);
            }
            passthrough_or_refuse(&cfg, "models", &["models".into()])
        }
        Some(Commands::Ls) => passthrough_or_refuse(&cfg, "ls", &["ls".into()]),
        Some(Commands::Login) => passthrough_or_refuse(&cfg, "login", &["login".into()]),
        Some(Commands::Logout) => passthrough_or_refuse(&cfg, "logout", &["logout".into()]),
        Some(Commands::Mcp { args }) => protocols::dispatch_mcp(&cfg, &state.cwd, &args),
        Some(Commands::Acp { args }) => protocols::dispatch_acp(&args),
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
