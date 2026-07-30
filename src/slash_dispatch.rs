//! Slash command dispatch shared by CLI and TUI.

use crate::actions::{RunSpec, fork_prompt, run_agent, run_commit, run_pr, run_review};
use crate::agent::AgentConfig;
use crate::config;
use crate::doctor::{
    cmd_debug, cmd_doctor, cmd_init, cmd_persona, cmd_role, print_config, print_permissions,
};
use crate::gitops;
use crate::init;
use crate::inventory;
use crate::learn;
use crate::models::resolve_model;
use crate::os_control;
use crate::parallel;
use crate::please_fix;
use crate::route_log;
use crate::session::{compact_history, open_memory};
use crate::slash;
use crate::state::AbbeyState;
use anyhow::{Result, bail};

fn rest_prompt(rest: &str) -> Vec<String> {
    if rest.is_empty() {
        Vec::new()
    } else {
        vec![rest.to_string()]
    }
}

pub fn dispatch_slash(input: &str, state: &AbbeyState, cfg: &mut AgentConfig) -> Result<i32> {
    let Some((cmd, rest)) = slash::parse_slash(input) else {
        bail!("not a slash command");
    };
    match cmd {
        "help" | "h" | "?" => {
            print!("{}", slash::help_text());
            Ok(0)
        }
        "clear" | "rewind" => {
            let all = rest == "--all" || rest == "all";
            state.clear_chat(all)?;
            println!("abbey: chat cleared");
            Ok(0)
        }
        "compact" => {
            let keep = rest.parse().unwrap_or(50);
            let n = compact_history(state, keep)?;
            println!("abbey: history compacted to {n}");
            Ok(0)
        }
        "doctor" | "debug" => {
            if cmd == "debug" {
                cmd_debug(state, cfg)
            } else {
                cmd_doctor(state, cfg)
            }
        }
        "model" => {
            if rest.is_empty() {
                println!("{}", state.read_model());
            } else if rest == "clear" {
                state.clear_model()?;
                println!("model reset to auto");
            } else {
                let m = resolve_model(rest);
                state.save_model(&m)?;
                cfg.model = m.clone();
                println!("{m}");
            }
            Ok(0)
        }
        "diff" => {
            let staged = rest.contains("staged") || rest.contains("--staged");
            println!("{}", gitops::diff_text(staged)?);
            Ok(0)
        }
        "init" => {
            let opts = init::parse_init_args(rest);
            cmd_init(cfg, state, opts.force, opts.print_only, opts.agent)
        }
        "branch" => {
            println!("{}", gitops::create_branch(rest)?);
            Ok(0)
        }
        "memory" | "tasks" | "history" => {
            if rest.starts_with("search ") {
                let q = rest.trim_start_matches("search ").trim();
                let mem = open_memory(state)?;
                for r in mem.search_keyword(q, 20)? {
                    println!("{}\t{}\t{}", r.id, r.retention, r.summary);
                }
                return Ok(0);
            }
            let n: usize = rest.parse().unwrap_or(15);
            println!(
                "chat: {}",
                state.read_chat().unwrap_or_else(|| "(none)".into())
            );
            for e in state.history(n) {
                println!("{}\t{}\t{}", e.timestamp, e.chat_id, e.cwd);
            }
            Ok(0)
        }
        "persona" => cmd_persona(if rest.is_empty() { None } else { Some(rest) }),
        "role" => cmd_role(if rest.is_empty() { None } else { Some(rest) }),
        "routes" => {
            let n: usize = rest.parse().unwrap_or(10);
            for r in route_log::recent_routes(&state.state_dir, n)? {
                println!("{}", route_log::format_route_line(&r));
            }
            Ok(0)
        }
        "max" => {
            if rest.is_empty() {
                bail!("/max needs a prompt");
            }
            run_agent(cfg, state, &rest_prompt(rest), RunSpec::max())
        }
        "gemma" => {
            if rest.is_empty() {
                bail!("/gemma needs a prompt");
            }
            run_agent(cfg, state, &rest_prompt(rest), RunSpec::gemma())
        }
        "skills" => {
            inventory::print_skills()?;
            Ok(0)
        }
        "plugins" | "plugin" => {
            let args: Vec<String> = if rest.is_empty() {
                Vec::new()
            } else {
                rest.split_whitespace().map(String::from).collect()
            };
            inventory::run_plugin_passthrough(&args)
        }
        "agents" | "tools" => {
            inventory::print_agent_tools();
            Ok(0)
        }
        "os" => {
            let args: Vec<String> = rest.split_whitespace().map(String::from).collect();
            os_control::run_os(&args, true)
        }
        "learn" => {
            let args: Vec<String> = rest.split_whitespace().map(String::from).collect();
            learn::dispatch(state, &args)
        }
        "parallel" => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            if rest.is_empty() {
                bail!("/parallel needs a prompt");
            }
            parallel::run_parallel_cli(
                cfg,
                state,
                &rest_prompt(rest),
                &ac.roles.max,
                &ac.roles.gemma,
            )
        }
        "permissions" => {
            print_permissions(cfg);
            Ok(0)
        }
        "config" => {
            print_config(state, cfg);
            Ok(0)
        }
        "cost" => {
            println!("abbey: cost N/A for cursor-agent — use Cursor account dashboard");
            Ok(0)
        }
        "status" => {
            let st = cfg.passthrough(&["status".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        "models" => {
            let st = cfg.passthrough(&["models".into()])?;
            Ok(st.code().unwrap_or(1))
        }
        "mcp" => {
            let mut a = vec!["mcp".into()];
            if !rest.is_empty() {
                a.extend(rest.split_whitespace().map(String::from));
            }
            let st = cfg.passthrough(&a)?;
            Ok(st.code().unwrap_or(1))
        }
        "plan" => {
            let prompt = if rest.is_empty() {
                vec!["Create a step-by-step plan for the current repository goals.".into()]
            } else {
                rest_prompt(rest)
            };
            run_agent(cfg, state, &prompt, RunSpec::plan())
        }
        "ask" => {
            if rest.is_empty() {
                bail!("/ask needs a question");
            }
            run_agent(cfg, state, &rest_prompt(rest), RunSpec::ask())
        }
        "new" => run_agent(cfg, state, &rest_prompt(rest), RunSpec::fresh()),
        "continue" | "resume" => run_agent(cfg, state, &rest_prompt(rest), RunSpec::resume()),
        "fork" => {
            let extra = rest_prompt(rest);
            run_agent(cfg, state, &fork_prompt(&extra), RunSpec::fresh())
        }
        "review" => {
            let staged = rest.contains("staged");
            run_review(cfg, state, staged, &[], false)
        }
        "security-review" | "sec" => run_review(cfg, state, false, &[], true),
        "commit" => run_commit(cfg, state),
        "pr" => run_pr(cfg, state),
        "please-fix" | "fix" => {
            let text = if rest.is_empty() {
                please_fix::build_prompt(&[])?
            } else {
                rest.to_string()
            };
            run_agent(cfg, state, &[text], RunSpec::max())
        }
        other => {
            if let Some(meta) = slash::lookup(other) {
                eprintln!(
                    "abbey: /{other} is registered ({}) but not wired yet",
                    meta.help
                );
            } else {
                eprintln!("abbey: unknown slash /{other} — try /help");
            }
            Ok(2)
        }
    }
}
