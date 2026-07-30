//! Multi-subagent fan-out and local distributed peer agents.
//!
//! **Current:** named Abbey lanes (Max/Gemma/Aviva/…) via cursor-agent `--print`,
//! plus optional same-machine peer CLIs (`gemini -p`, `opencode run`, `claude -p`,
//! `codex exec`) run concurrently and merged. Optional synthesize pass.
//!
//! **Not claimed:** multi-node / multi-GPU / shared-compute mesh. Peers are PATH
//! processes on this host — "distributed" here means across agent binaries, not
//! across machines.

use crate::agent::AgentConfig;
use crate::models::resolve_model;
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::route_log;
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    /// cursor-agent with persona × role wrap
    Abbey,
    /// External peer CLI on PATH
    Peer,
}

#[derive(Debug, Clone)]
pub struct SubagentSpec {
    pub name: &'static str,
    pub kind: LaneKind,
    pub summary: &'static str,
    /// For Abbey lanes
    pub role: WorkerRole,
    pub persona: &'static str,
    /// Extra system framing (reviewer / security / planner)
    pub focus: Option<&'static str>,
    /// Peer binary name
    pub peer_bin: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct LanePlan {
    pub name: String,
    pub kind: LaneKind,
    pub role: WorkerRole,
    pub persona_label: String,
    pub model: String,
    pub focus: Option<String>,
    pub peer_bin: Option<String>,
    pub peer_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LaneResult {
    pub name: String,
    pub kind: LaneKind,
    pub model_or_peer: String,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub lanes: Vec<String>,
    pub peers: Vec<String>,
    pub jobs: usize,
    pub synthesize: bool,
    pub prompt: Vec<String>,
}

const CATALOG: &[SubagentSpec] = &[
    SubagentSpec {
        name: "max",
        kind: LaneKind::Abbey,
        summary: "Implementer — code, tools, math (Max role)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: None,
    },
    SubagentSpec {
        name: "gemma",
        kind: LaneKind::Abbey,
        summary: "Interpreter — tone, visual, clarify (Gemma role)",
        role: WorkerRole::Gemma,
        persona: "abbey",
        focus: None,
        peer_bin: None,
    },
    SubagentSpec {
        name: "aviva",
        kind: LaneKind::Abbey,
        summary: "Terse expert framing (Aviva persona + Max role)",
        role: WorkerRole::Max,
        persona: "aviva",
        focus: None,
        peer_bin: None,
    },
    SubagentSpec {
        name: "abbey",
        kind: LaneKind::Abbey,
        summary: "Warm primary persona + Max role",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: None,
    },
    SubagentSpec {
        name: "abi",
        kind: LaneKind::Abbey,
        summary: "Router/orchestrator framing (Abi persona)",
        role: WorkerRole::Max,
        persona: "abi",
        focus: Some(
            "Focus on intent, risk, tool choice, and how to split work across lanes. \
             Do not implement unless asked.",
        ),
        peer_bin: None,
    },
    SubagentSpec {
        name: "reviewer",
        kind: LaneKind::Abbey,
        summary: "Code-review lens (Max + review focus)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: Some(
            "You are the reviewer subagent. Find bugs, regressions, missing tests, \
             and API mismatches. Prefer concrete file:line notes. Do not rewrite everything.",
        ),
        peer_bin: None,
    },
    SubagentSpec {
        name: "security",
        kind: LaneKind::Abbey,
        summary: "Security-review lens",
        role: WorkerRole::Max,
        persona: "aviva",
        focus: Some(
            "You are the security subagent. Hunt injection, authz gaps, secret leaks, \
             unsafe shell, and dependency risk. Severity-order findings. No theatre.",
        ),
        peer_bin: None,
    },
    SubagentSpec {
        name: "planner",
        kind: LaneKind::Abbey,
        summary: "Plan-only lens (no implementation)",
        role: WorkerRole::Gemma,
        persona: "abi",
        focus: Some(
            "You are the planner subagent. Produce a step-by-step plan with risks and \
             verification. Do not write application code.",
        ),
        peer_bin: None,
    },
    // Local distributed peers (same machine, PATH binaries)
    SubagentSpec {
        name: "gemini",
        kind: LaneKind::Peer,
        summary: "Peer: gemini -p (local PATH)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: Some("gemini"),
    },
    SubagentSpec {
        name: "opencode",
        kind: LaneKind::Peer,
        summary: "Peer: opencode run (local PATH)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: Some("opencode"),
    },
    SubagentSpec {
        name: "claude",
        kind: LaneKind::Peer,
        summary: "Peer: claude -p (local PATH)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: Some("claude"),
    },
    SubagentSpec {
        name: "codex",
        kind: LaneKind::Peer,
        summary: "Peer: codex exec (local PATH)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: Some("codex"),
    },
];

pub fn find_spec(name: &str) -> Option<&'static SubagentSpec> {
    let key = name.trim().to_ascii_lowercase();
    CATALOG.iter().find(|s| s.name == key)
}

pub fn default_lane_names() -> Vec<String> {
    vec!["max".into(), "gemma".into(), "aviva".into()]
}

pub fn print_catalog() {
    println!("abbey subagents — multi-lane + local peer agents");
    println!("note: peers are PATH CLIs on this host; multi-node mesh is Proposed, not Current.\n");
    println!("{:<12} {:<8} detail", "name", "kind");
    for s in CATALOG {
        let kind = match s.kind {
            LaneKind::Abbey => "abbey",
            LaneKind::Peer => "peer",
        };
        let status = if s.kind == LaneKind::Peer {
            match s.peer_bin.and_then(crate::agent::which_bin) {
                Some(p) => format!("present · {}", p.display()),
                None => "missing".into(),
            }
        } else {
            "cursor-agent lane".into()
        };
        println!("{:<12} {:<8} {}", s.name, kind, s.summary);
        println!("{:<12} {:<8} {}", "", "", status);
    }
    println!(
        "\nrun:  abbey subagents run [--lanes max,gemma,reviewer] [--peers gemini,claude] \\\n\
         \t\t[--jobs N] [--synthesize] <prompt…>\n\
         alias: abbey parallel … / swarm / distribute\n\
         env:   ABBEY_SUBAGENT_JOBS (default 4)\n\
         note: --peers is local PATH CLIs only — not a multi-node mesh"
    );
}

fn default_jobs() -> usize {
    crate::platform::default_subagent_jobs()
}

/// Parse CLI/slash args into [`RunOptions`].
pub fn parse_args(args: &[String]) -> Result<RunOptions> {
    let mut opts = RunOptions {
        jobs: default_jobs(),
        ..RunOptions::default()
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "run" | "exec" => {}
            "list" | "ls" | "catalog" | "status" => {
                // Handled by dispatch — treat as empty prompt signal.
                return Ok(opts);
            }
            "--lanes" | "-l" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--lanes needs a comma-separated list");
                };
                opts.lanes = split_csv(v);
            }
            "--peers" | "-P" => {
                // Avoid clashing with clap/global `-p` (--print) in composite CLIs.
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--peers needs a comma-separated list");
                };
                opts.peers = split_csv(v);
            }
            "--jobs" | "-j" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--jobs needs a positive integer");
                };
                opts.jobs = v.parse().context_jobs()?;
                if opts.jobs == 0 {
                    bail!("--jobs must be >= 1");
                }
            }
            "--synthesize" | "--merge" | "-s" => opts.synthesize = true,
            "-h" | "--help" => {
                print_catalog();
                return Ok(opts);
            }
            s if s.starts_with("--lanes=") => {
                opts.lanes = split_csv(s.trim_start_matches("--lanes="));
            }
            s if s.starts_with("--peers=") => {
                opts.peers = split_csv(s.trim_start_matches("--peers="));
            }
            s if s.starts_with("--jobs=") => {
                opts.jobs = s.trim_start_matches("--jobs=").parse().context_jobs()?;
                if opts.jobs == 0 {
                    bail!("--jobs must be >= 1");
                }
            }
            s if s.starts_with('-') => bail!("unknown subagents flag: {s}"),
            s => opts.prompt.push(s.to_string()),
        }
        i += 1;
    }
    Ok(opts)
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

trait ParseJobs {
    fn context_jobs(self) -> Result<usize>;
}
impl ParseJobs for Result<usize, std::num::ParseIntError> {
    fn context_jobs(self) -> Result<usize> {
        self.map_err(|e| anyhow::anyhow!("--jobs: {e}"))
    }
}

pub fn build_plan(opts: &RunOptions, max_model: &str, gemma_model: &str) -> Result<Vec<LanePlan>> {
    let mut names = Vec::new();
    if opts.lanes.is_empty() && opts.peers.is_empty() {
        names.extend(default_lane_names());
    } else {
        names.extend(opts.lanes.iter().cloned());
        names.extend(opts.peers.iter().cloned());
    }
    // Dedup preserving order
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));

    let mut plans = Vec::new();
    for name in names {
        let Some(spec) = find_spec(&name) else {
            bail!(
                "unknown subagent `{name}` — try: abbey subagents list\n\
                 known: {}",
                CATALOG
                    .iter()
                    .map(|s| s.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        let model = match spec.role {
            WorkerRole::Gemma => resolve_model(gemma_model),
            _ => resolve_model(max_model),
        };
        let peer_path = spec.peer_bin.and_then(crate::agent::which_bin);
        if spec.kind == LaneKind::Peer && peer_path.is_none() {
            eprintln!(
                "abbey: peer `{}` not on PATH — skipping",
                spec.peer_bin.unwrap_or(spec.name)
            );
            continue;
        }
        plans.push(LanePlan {
            name: spec.name.into(),
            kind: spec.kind,
            role: spec.role,
            persona_label: spec.persona.into(),
            model,
            focus: spec.focus.map(str::to_string),
            peer_bin: spec.peer_bin.map(str::to_string),
            peer_path,
        });
    }
    if plans.is_empty() {
        bail!("no runnable subagents (all peers missing?)");
    }
    Ok(plans)
}

fn abbey_prompt(plan: &LanePlan, user: &str) -> String {
    let profile =
        persona::parse_persona(&plan.persona_label).unwrap_or(abi_ai::AgentProfile::Abbey);
    let wrapped = persona::wrap_prompt(profile, user);
    let note = roles::role_system_note(plan.role);
    let focus = plan.focus.as_deref().unwrap_or("");
    let focus_block = if focus.is_empty() {
        String::new()
    } else {
        format!("\n\nSubagent focus:\n{focus}\n")
    };
    format!(
        "{note}{focus_block}\n\
         You are subagent `{}` in a multi-agent Abbey run. Be concise; other \
         subagents also answer. Do not assume you are the only worker.\n\n{wrapped}",
        plan.name
    )
}

fn run_abbey_lane(base: &AgentConfig, plan: &LanePlan, user: &str) -> LaneResult {
    let mut cfg = base.clone();
    cfg.model = plan.model.clone();
    cfg.print = true;
    let prompt = abbey_prompt(plan, user);
    match cfg.run_capture(None, &[prompt]) {
        Ok((st, out, err)) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Abbey,
            model_or_peer: plan.model.clone(),
            exit: st.code().unwrap_or(1),
            stdout: out,
            stderr: err,
        },
        Err(e) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Abbey,
            model_or_peer: plan.model.clone(),
            exit: 1,
            stdout: String::new(),
            stderr: format!("{e:#}"),
        },
    }
}

fn run_peer_lane(plan: &LanePlan, user: &str) -> LaneResult {
    let Some(bin) = &plan.peer_path else {
        return LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Peer,
            model_or_peer: plan.peer_bin.clone().unwrap_or_default(),
            exit: 2,
            stdout: String::new(),
            stderr: "peer binary missing".into(),
        };
    };
    let peer = plan.peer_bin.as_deref().unwrap_or(plan.name.as_str());
    let output = match peer {
        "gemini" => Command::new(bin).args(["-p", user]).output(),
        "opencode" => Command::new(bin).args(["run", user]).output(),
        "claude" => Command::new(bin).args(["-p", user]).output(),
        "codex" => Command::new(bin).args(["exec", user]).output(),
        other => {
            return LaneResult {
                name: plan.name.clone(),
                kind: LaneKind::Peer,
                model_or_peer: other.into(),
                exit: 2,
                stdout: String::new(),
                stderr: format!("no argv recipe for peer `{other}`"),
            };
        }
    };
    match output {
        Ok(out) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Peer,
            model_or_peer: bin.display().to_string(),
            exit: out.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(e) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Peer,
            model_or_peer: bin.display().to_string(),
            exit: 1,
            stdout: String::new(),
            stderr: format!("{e}"),
        },
    }
}

/// Run planned lanes with a concurrency cap.
pub fn run_plans(
    base: &AgentConfig,
    plans: &[LanePlan],
    user: &str,
    jobs: usize,
) -> Vec<LaneResult> {
    let jobs = jobs.max(1);
    let user = Arc::new(user.to_string());
    let base = Arc::new(base.clone());
    // Simple pool: chunk into waves of `jobs`.
    let mut results = Vec::with_capacity(plans.len());
    for chunk in plans.chunks(jobs) {
        thread::scope(|scope| {
            let mut handles = Vec::new();
            for plan in chunk {
                let plan = plan.clone();
                let user = Arc::clone(&user);
                let base = Arc::clone(&base);
                handles.push(scope.spawn(move || match plan.kind {
                    LaneKind::Abbey => run_abbey_lane(&base, &plan, &user),
                    LaneKind::Peer => run_peer_lane(&plan, &user),
                }));
            }
            for h in handles {
                match h.join() {
                    Ok(r) => results.push(r),
                    Err(_) => results.push(LaneResult {
                        name: "panic".into(),
                        kind: LaneKind::Abbey,
                        model_or_peer: String::new(),
                        exit: 1,
                        stdout: String::new(),
                        stderr: "subagent thread panicked".into(),
                    }),
                }
            }
        });
    }
    results
}

pub fn print_merged(results: &[LaneResult]) {
    for r in results {
        let kind = match r.kind {
            LaneKind::Abbey => "abbey",
            LaneKind::Peer => "peer",
        };
        println!(
            "===== subagent:{} kind:{} via:{} exit:{} =====",
            r.name, kind, r.model_or_peer, r.exit
        );
        if !r.stdout.trim().is_empty() {
            crate::highlight::emit_agent_stdout(r.stdout.trim_end());
            println!();
        }
        if !r.stderr.trim().is_empty() {
            eprintln!("{}", r.stderr.trim_end());
        }
        println!();
    }
}

fn synthesize(
    base: &AgentConfig,
    max_model: &str,
    user: &str,
    results: &[LaneResult],
) -> LaneResult {
    let mut dossier = String::from("Multi-subagent results to reconcile:\n\n");
    for r in results {
        dossier.push_str(&format!(
            "### {}\nexit {}\n{}\n\n",
            r.name,
            r.exit,
            r.stdout.trim()
        ));
    }
    let plan = LanePlan {
        name: "synthesize".into(),
        kind: LaneKind::Abbey,
        role: WorkerRole::Max,
        persona_label: "abi".into(),
        model: resolve_model(max_model),
        focus: Some(
            "You are the synthesize subagent. Merge the dossier into one coherent answer. \
             Call out conflicts. Prefer concrete next steps. Do not invent work other \
             lanes did not do."
                .into(),
        ),
        peer_bin: None,
        peer_path: None,
    };
    let body = format!("{user}\n\n{dossier}");
    run_abbey_lane(base, &plan, &body)
}

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

/// CLI entry for `abbey subagents …` and enhanced `abbey parallel …`.
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
        bail!("usage: abbey subagents run [--lanes …] [--peers …] [--synthesize] <prompt…>");
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
        "abbey: subagents {correlation}\n  lanes → {}",
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
        eprintln!("abbey: synthesize pass (abi persona)…");
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
             correlation {correlation} — `abbey routes --correlation` (swarm audit)\n\
             honesty: local PATH peers only — not a multi-node agent mesh."
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
        bail!("usage: abbey parallel [--lanes …] [--peers …] [--synthesize] <prompt…>");
    }
    let mut opts = parse_args(args)?;
    if opts.lanes.is_empty() && opts.peers.is_empty() {
        opts.lanes = default_lane_names();
    }
    if opts.prompt.is_empty() {
        bail!("usage: abbey parallel [--lanes …] [--peers …] [--synthesize] <prompt…>");
    }
    run_with_options(cfg, state, &opts, max_model, gemma_model)
}

pub fn status_line() -> String {
    let peers = CATALOG
        .iter()
        .filter(|s| s.kind == LaneKind::Peer)
        .filter(|s| s.peer_bin.and_then(crate::agent::which_bin).is_some())
        .count();
    let abbey_n = CATALOG.iter().filter(|s| s.kind == LaneKind::Abbey).count();
    format!(
        "subagents: {abbey_n} abbey lanes · {peers} peer CLI(s) on PATH (local, not multi-node)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_core_and_peers() {
        assert!(find_spec("max").is_some());
        assert!(find_spec("reviewer").is_some());
        assert!(find_spec("gemini").is_some_and(|s| s.kind == LaneKind::Peer));
        assert!(find_spec("nope").is_none());
    }

    #[test]
    fn parse_lanes_peers_synthesize() {
        let args = vec![
            "run".into(),
            "--lanes".into(),
            "max,reviewer".into(),
            "--peers".into(),
            "gemini".into(),
            "--synthesize".into(),
            "--jobs".into(),
            "2".into(),
            "fix".into(),
            "it".into(),
        ];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.lanes, vec!["max", "reviewer"]);
        assert_eq!(opts.peers, vec!["gemini"]);
        assert!(opts.synthesize);
        assert_eq!(opts.jobs, 2);
        assert_eq!(opts.prompt, vec!["fix", "it"]);
    }

    #[test]
    fn build_plan_defaults_three_lanes() {
        let opts = RunOptions {
            prompt: vec!["x".into()],
            ..RunOptions::default()
        };
        let plans = build_plan(&opts, "fable", "gemini").unwrap();
        let names: Vec<_> = plans.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["max", "gemma", "aviva"]);
    }

    #[test]
    fn build_plan_rejects_unknown() {
        let opts = RunOptions {
            lanes: vec!["nope".into()],
            prompt: vec!["x".into()],
            ..RunOptions::default()
        };
        assert!(build_plan(&opts, "fable", "gemini").is_err());
    }
}
