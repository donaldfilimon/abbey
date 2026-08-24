//! Clap subcommand dispatch.

use crate::actions::{RunSpec, fork_prompt, run_agent, run_commit, run_pr, run_review};
use crate::agent::{AgentConfig, run_resilient};
use crate::app_core::{
    AppCapability, AppCommand, AppEvent, ClaimStatus, ClaimsQuery, Edition, RouteAuditQuery,
    RuntimeState, V3Capability, V3CapabilitySet, V3EntityPage, V3Event, V3GrantNegotiation,
    V3OperationState, V3PageQuery,
};
use crate::build_info;
use crate::claims;
use crate::cli::{
    Cli, Commands, DaemonClaimStatus, DaemonCmd, GenerateCmd, MemoryCmd, MeshCmd, Shell,
};
use crate::config;
#[cfg(not(unix))]
use crate::daemon::ClientError;
#[cfg(unix)]
use crate::daemon::{DaemonClient, DaemonConfig, V3DaemonSession};
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
use crate::mesh;
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
use anyhow::{Result, anyhow};
use clap::CommandFactory;
use clap_complete::{Shell as ClapShell, generate};
use std::io;

/// Refuse account/session/MCP verbs under backends that have no such surface.
fn passthrough_or_refuse(cfg: &AgentConfig, verb: &str, args: &[String]) -> Result<i32> {
    if !cfg.backend.supports_account_surface() {
        eprintln!(
            "abbey: `{verb}` is not applicable under the `{}` backend (ABBEY_BACKEND={}).\n\
             It has no account, session list, or MCP surface.\n\
             Unset ABBEY_BACKEND to use cursor-agent.",
            cfg.backend.label(),
            cfg.backend.label(),
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
        Some(Commands::Print { prompt }) => crate::capture::run_print(&mut cfg, &state, &prompt),
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
                // Under abi, do not persist cursor-expanded `claude-*-thinking-*`
                // ids — they would look like live requests / get collapsed wrongly.
                if cfg.backend == crate::agent::AgentBackend::Abi {
                    let resolved = crate::agent::abi_normalize_model(m);
                    state.save_model_literal(&resolved)?;
                    eprintln!("abbey: default model set to {resolved}");
                    println!("{resolved}");
                } else {
                    let resolved = resolve_model(m);
                    state.save_model(&resolved)?;
                    eprintln!("abbey: default model set to {resolved}");
                    println!("{resolved}");
                }
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
        Some(Commands::Edition { name, daemon_name }) => Ok(crate::edition::cmd_edition(
            &state.state_dir,
            name,
            daemon_name,
        )),
        Some(Commands::ChatId) => match state.read_chat_for(cfg.backend) {
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
                    state
                        .read_chat_for(cfg.backend)
                        .unwrap_or_else(|| "(none)".into())
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
        Some(Commands::Mesh { cmd }) => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            match cmd {
                MeshCmd::Status => mesh::dispatch(&ac, &["status".into()], false),
                MeshCmd::Nodes => mesh::dispatch(&ac, &["nodes".into()], false),
                MeshCmd::LocalDemo { nodes, json } => {
                    mesh::dispatch(&ac, &["local-demo".into(), nodes.to_string()], json)
                }
            }
        }
        Some(Commands::Daemon { cmd }) => run_daemon_command(cmd),
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
        Some(Commands::Skills { all }) => {
            if all {
                inventory::print_skills_all()?;
            } else {
                inventory::print_skills()?;
            }
            Ok(0)
        }
        Some(Commands::Plugins { args }) => inventory::run_plugin_passthrough(&args),
        Some(Commands::Permissions) => {
            print_permissions(&cfg);
            Ok(0)
        }
        Some(Commands::Config { init }) => {
            let ac = config::AbbeyConfig::load().unwrap_or_default();
            if init {
                let existed = config::AbbeyConfig::config_path().is_file();
                let path = ac.ensure_default_file()?;
                if existed {
                    eprintln!(
                        "abbey: config already exists, left untouched: {}",
                        path.display()
                    );
                } else {
                    eprintln!("abbey: wrote default config {}", path.display());
                }
            }
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
            state.ensure_conversation_ready(cfg.backend)?;
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

fn run_daemon_command(command: DaemonCmd) -> Result<i32> {
    let (command, json) = match command {
        DaemonCmd::Run { cmd } => return crate::run_control::dispatch(cmd),
        DaemonCmd::Negotiate { json } => {
            let session = negotiate_model_reads()?;
            let event = V3Event::Negotiated(session.negotiation().clone());
            return print_daemon_v3(event, json);
        }
        DaemonCmd::Models {
            after,
            through,
            limit,
            json,
        } => {
            let session = negotiate_model_reads()?;
            let page = session.list_models(V3PageQuery {
                after,
                through,
                limit,
            })?;
            return print_daemon_v3(V3Event::Models(page), json);
        }
        DaemonCmd::Status { json } => (AppCommand::Status, json),
        DaemonCmd::Claims {
            status,
            contains,
            json,
        } => (
            AppCommand::Claims(ClaimsQuery {
                status: status.map(daemon_claim_status),
                contains,
            }),
            json,
        ),
        DaemonCmd::Routes { limit, json } => {
            (AppCommand::ReadRoutes(RouteAuditQuery { limit }), json)
        }
    };
    let event = request_daemon(command)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&event)?);
    } else {
        print!("{}", format_daemon_event(&event)?);
    }
    Ok(0)
}

fn print_daemon_v3(event: V3Event, json: bool) -> Result<i32> {
    if json {
        println!("{}", serde_json::to_string_pretty(&event)?);
    } else {
        print!("{}", format_daemon_v3_event(&event)?);
    }
    Ok(0)
}

#[cfg(unix)]
fn negotiate_model_reads() -> Result<V3DaemonSession> {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels])
        .map_err(|_| anyhow!("protocol-v3 model capability declaration is invalid"))?;
    let config = DaemonConfig::from_env()?;
    Ok(DaemonClient::new(config).negotiate_v3(requested)?)
}

#[cfg(not(unix))]
fn negotiate_model_reads() -> Result<crate::daemon::V3DaemonSession> {
    Err(ClientError::UnsupportedPlatform.into())
}

#[cfg(unix)]
fn request_daemon(command: AppCommand) -> Result<AppEvent> {
    let config = DaemonConfig::from_env()?;
    Ok(DaemonClient::new(config).request(command)?)
}

#[cfg(not(unix))]
fn request_daemon(_command: AppCommand) -> Result<AppEvent> {
    Err(ClientError::UnsupportedPlatform.into())
}

fn daemon_claim_status(status: DaemonClaimStatus) -> ClaimStatus {
    match status {
        DaemonClaimStatus::Current => ClaimStatus::Current,
        DaemonClaimStatus::Partial => ClaimStatus::Partial,
        DaemonClaimStatus::Proposed => ClaimStatus::Proposed,
        DaemonClaimStatus::Blocked => ClaimStatus::Blocked,
        DaemonClaimStatus::OutOfScope => ClaimStatus::OutOfScope,
        DaemonClaimStatus::Failed => ClaimStatus::Failed,
        DaemonClaimStatus::Revoked => ClaimStatus::Revoked,
        DaemonClaimStatus::Superseded => ClaimStatus::Superseded,
        DaemonClaimStatus::Expired => ClaimStatus::Expired,
    }
}

fn format_daemon_event(event: &AppEvent) -> Result<String> {
    match event {
        AppEvent::Status(status) => {
            let edition = match status.edition {
                Edition::Standard => "standard",
                Edition::Personal => "personal",
            };
            let state = match status.state {
                RuntimeState::Ready => "ready",
            };
            let capabilities = status
                .capabilities
                .as_slice()
                .iter()
                .map(|capability| match capability {
                    AppCapability::ReadStatus => "read_status",
                    AppCapability::ReadClaims => "read_claims",
                    AppCapability::ReadRun => "read_run",
                    AppCapability::ReadRunEvents => "read_run_events",
                    AppCapability::SubmitRun => "submit_run",
                    AppCapability::CancelRun => "cancel_run",
                    AppCapability::ReadRoutes => "read_routes",
                })
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!(
                "abbeyd: {state} ({edition} edition)\n\
                 protocol: {} · schema: {}\n\
                 build: {} · {} · {}\n\
                 capabilities: {capabilities}\n",
                status.protocol_version,
                status.schema_version,
                status.version,
                status.build_git,
                status.build_target,
            ))
        }
        AppEvent::Claims(snapshot) => {
            let mut rendered = format!("abbeyd claims: {} match(es)\n", snapshot.matched);
            for claim in &snapshot.claims {
                rendered.push_str(&format!(
                    "  [{}] {}\n      {}\n",
                    crate::claims::Status::from(claim.status).label(),
                    claim.name,
                    claim.note,
                ));
                if let Some(instead) = &claim.instead {
                    rendered.push_str(&format!("      instead: {instead}\n"));
                }
            }
            Ok(rendered)
        }
        AppEvent::RouteAudit(page) => {
            let mut rendered = format!(
                "abbeyd routes: {} of at most {} decision(s)\n",
                page.returned, page.limit
            );
            for entry in &page.entries {
                rendered.push_str(&format!(
                    "  {} {}/{} {} {}% {} [{}]\n      alt={} fb={} {}\n",
                    entry.recorded_at,
                    entry.persona,
                    entry.role,
                    entry.model,
                    entry.confidence_percent,
                    entry.stage.as_deref().unwrap_or("-"),
                    entry.workspace.as_deref().unwrap_or("-"),
                    entry.alternate.as_deref().unwrap_or("-"),
                    entry.fallback.as_deref().unwrap_or("-"),
                    entry.reason,
                ));
            }
            Ok(rendered)
        }
        AppEvent::ApprovalRequested(_)
        | AppEvent::RunSubmitted(_)
        | AppEvent::RunStatus(_)
        | AppEvent::CancellationAcknowledged(_)
        | AppEvent::RunEvents(_) => Err(anyhow!(
            "daemon returned an event outside the read-only CLI contract"
        )),
    }
}

fn format_daemon_v3_event(event: &V3Event) -> Result<String> {
    match event {
        V3Event::Negotiated(V3GrantNegotiation {
            protocol_version,
            schema_version,
            granted,
        }) => {
            let capabilities = granted
                .as_slice()
                .iter()
                .map(v3_capability_label)
                .collect::<Vec<_>>()
                .join(", ");
            let capabilities = if capabilities.is_empty() {
                "none"
            } else {
                &capabilities
            };
            Ok(format!(
                "abbeyd protocol-v3 negotiation\n\
                 protocol: {protocol_version} · schema: {schema_version}\n\
                 granted: {capabilities}\n"
            ))
        }
        V3Event::Models(V3EntityPage {
            after,
            through,
            records,
        }) => {
            let mut rendered = format!(
                "abbeyd models: {} record(s) after {after} through {through}\n",
                records.len()
            );
            for record in records {
                rendered.push_str(&format!(
                    "  [{}] {} — {}\n",
                    v3_state_label(record.state),
                    record.id,
                    record.label,
                ));
            }
            Ok(rendered)
        }
        _ => Err(anyhow!(
            "daemon returned an event outside the protocol-v3 CLI contract"
        )),
    }
}

const fn v3_capability_label(capability: &V3Capability) -> &'static str {
    match capability {
        V3Capability::ListTools => "list_tools",
        V3Capability::InvokeTools => "invoke_tools",
        V3Capability::DecideToolApprovals => "decide_tool_approvals",
        V3Capability::CancelTools => "cancel_tools",
        V3Capability::ReadMemory => "read_memory",
        V3Capability::ReadModels => "read_models",
        V3Capability::DownloadModels => "download_models",
        V3Capability::ManageModels => "manage_models",
        V3Capability::ReadTraining => "read_training",
        V3Capability::ManageTraining => "manage_training",
        V3Capability::ReadWorkers => "read_workers",
        V3Capability::CancelJobs => "cancel_jobs",
        V3Capability::ReadClaimsById => "read_claims_by_id",
        V3Capability::PollEvents => "poll_events",
        V3Capability::InferModels => "infer_models",
    }
}

const fn v3_state_label(state: V3OperationState) -> &'static str {
    match state {
        V3OperationState::Available => "available",
        V3OperationState::Queued => "queued",
        V3OperationState::Running => "running",
        V3OperationState::InputRequired => "input_required",
        V3OperationState::Succeeded => "succeeded",
        V3OperationState::Failed => "failed",
        V3OperationState::Denied => "denied",
        V3OperationState::Cancelled => "cancelled",
        V3OperationState::NotDownloaded => "not_downloaded",
    }
}

#[cfg(test)]
mod daemon_tests {
    use super::*;
    use crate::app_core::{
        APP_PROTOCOL_VERSION, APP_SCHEMA_VERSION, CapabilitySet, ClaimsSnapshot, RuntimeStatus,
    };

    #[test]
    fn daemon_human_status_is_concise_and_read_only() {
        let text = format_daemon_event(&AppEvent::Status(RuntimeStatus {
            protocol_version: APP_PROTOCOL_VERSION,
            schema_version: APP_SCHEMA_VERSION,
            edition: Edition::Standard,
            state: RuntimeState::Ready,
            version: "2.6.0".into(),
            build_git: "abc123".into(),
            build_target: "aarch64-apple-darwin".into(),
            capabilities: CapabilitySet::standard(),
            run_routes: Vec::new(),
        }))
        .unwrap();
        assert!(text.contains("abbeyd: ready (standard edition)"));
        assert!(text.contains("capabilities: read_status, read_claims"));
        assert!(!text.contains("shell"));
        assert!(!text.contains("bearer"));
    }

    #[test]
    fn daemon_human_claims_preserve_evidence_status() {
        let text = format_daemon_event(&AppEvent::Claims(ClaimsSnapshot {
            claims: vec![crate::app_core::ClaimRecord {
                name: "three-VM proof".into(),
                status: ClaimStatus::Proposed,
                note: "not production multi-host evidence".into(),
                instead: Some("local multi-process proof".into()),
            }],
            matched: 1,
        }))
        .unwrap();
        assert!(text.contains("[Proposed] three-VM proof"));
        assert!(text.contains("not production multi-host evidence"));
        assert!(text.contains("instead: local multi-process proof"));
    }
}
