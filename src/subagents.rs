//! Multi-subagent fan-out and local peer agents.
//!
//! Abbey lanes and optional peer CLIs run on this host. This is deliberately
//! separate from [`crate::mesh`], which wraps ABI's authenticated local
//! multi-process WDBX proof. Neither surface proves a production multi-host or
//! shared-compute agent mesh.

mod catalog;
mod execute;
mod parsing;

use crate::agent::AgentConfig;
use crate::route_log;
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use uuid::Uuid;

pub use catalog::{
    LaneKind, LanePlan, LaneResult, build_plan, default_lane_names, find_spec, print_catalog,
    status_line,
};
pub use execute::{print_merged, run_plans, synthesize};
pub use parsing::{RunOptions, parse_args};

fn record_swarm(state: &AbbeyState, correlation: &str, plans: &[LanePlan], results: &[LaneResult]) {
    let reason = format!(
        "subagents lanes={} results={}",
        plans
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        results.len()
    );
    let mut rec = route_log::RouteRecord::new(
        state.cwd.display().to_string(),
        "abi",
        "max",
        plans
            .first()
            .map(|p| p.model.clone())
            .unwrap_or_else(|| "multi".into()),
        reason,
        0.7,
    )
    .with_routing(Some("gemma".into()), Some("synthesize-or-manual".into()));
    rec.correlation = Some(correlation.into());
    rec.tools.push("subagents".into());
    if plans.iter().any(|p| p.kind == LaneKind::Peer) {
        rec.tools.push("peer".into());
    }
    let _ = route_log::append_route_record(&state.state_dir, &rec);
}

/// CLI entry for `abbey subagents ...` and enhanced `abbey parallel ...`.
pub fn dispatch(
    cfg: &AgentConfig,
    state: &AbbeyState,
    args: &[String],
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    if args.is_empty()
        || matches!(
            args.first().map(String::as_str),
            Some("list" | "ls" | "catalog" | "status" | "-h" | "--help")
        )
    {
        print_catalog();
        return Ok(0);
    }

    let opts = parse_args(args)?;
    if opts.prompt.is_empty() {
        print_catalog();
        return Ok(0);
    }
    run_with_options(cfg, state, &opts, max_model, gemma_model)
}

pub fn run_with_options(
    cfg: &AgentConfig,
    state: &AbbeyState,
    opts: &RunOptions,
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    let user = opts.prompt.join(" ");
    if user.trim().is_empty() {
        bail!("usage: abbey subagents run [--lanes ...] [--peers ...] [--synthesize] <prompt...>");
    }
    if cfg.backend.is_on_device()
        && opts
            .peers
            .iter()
            .any(|p| find_spec(p).is_some_and(|s| s.kind == LaneKind::Peer))
    {
        eprintln!(
            "abbey: peer lanes need external CLIs; under ABBEY_BACKEND=fm only Abbey lanes run"
        );
    }

    let plans = build_plan(opts, max_model, gemma_model)?;
    let correlation = Uuid::new_v4().to_string();
    eprintln!(
        "abbey: subagents {correlation}\n  lanes -> {}",
        plans
            .iter()
            .map(|p| match p.kind {
                LaneKind::Abbey => format!("{}(abbey:{})", p.name, p.model),
                LaneKind::Peer =>
                    format!("{}(peer:{})", p.name, p.peer_bin.as_deref().unwrap_or("?")),
            })
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!("  jobs={} synthesize={}", opts.jobs, opts.synthesize);

    let mut results = run_plans(cfg, &plans, &user, opts.jobs);
    print_merged(&results);

    if opts.synthesize {
        eprintln!("abbey: synthesize pass (abi persona)...");
        let syn = synthesize(cfg, max_model, &user, &results);
        println!(
            "===== subagent:synthesize kind:abbey via:{} exit:{} =====",
            syn.model_or_peer, syn.exit
        );
        if !syn.stdout.trim().is_empty() {
            crate::highlight::emit_agent_stdout(syn.stdout.trim_end());
            println!();
        }
        if !syn.stderr.trim().is_empty() {
            eprintln!("{}", syn.stderr.trim_end());
        }
        results.push(syn);
    } else {
        println!(
            "===== merge note =====\n\
             Multi-subagent run finished. Prefer Max for code, Gemma for tone/visual, \
             Aviva for terse expert, reviewer/security for audit, peers for second opinions.\n\
             Re-run with --synthesize for an Abi merge pass.\n\
             correlation {correlation} - `abbey routes --correlation` (swarm audit)\n\
             honesty: local PATH peers only - not a multi-node agent mesh."
        );
    }

    record_swarm(state, &correlation, &plans, &results);
    let worst = results.iter().map(|r| r.exit).max().unwrap_or(1);
    Ok(worst)
}

/// Backward-compatible `abbey parallel <prompt>` (+ optional flags before prompt).
pub fn run_parallel_compat(
    cfg: &AgentConfig,
    state: &AbbeyState,
    args: &[String],
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    if args.is_empty() {
        bail!("usage: abbey parallel [--lanes ...] [--peers ...] [--synthesize] <prompt...>");
    }
    let mut opts = parse_args(args)?;
    if opts.lanes.is_empty() && opts.peers.is_empty() {
        opts.lanes = default_lane_names();
    }
    if opts.prompt.is_empty() {
        bail!("usage: abbey parallel [--lanes ...] [--peers ...] [--synthesize] <prompt...>");
    }
    run_with_options(cfg, state, &opts, max_model, gemma_model)
}
