//! Goal-driven smart improve loop — ledger pick + local subagents + check.sh bar.
//!
//! **Current:** same-host diagnose/implement lanes via [`crate::subagents`], hard
//! stop when `./check.sh` is green and there is no remaining open slice (or
//! stabilize-only). One `--confirm` unlocks Max `force` apply for the whole run.
//!
//! **Not claimed:** multi-node mesh, unrestricted OS execute, auto-closing
//! `tasks/goals.md` to `done` (goal-ledger iron rule: green ≠ done).

mod gate;
mod ledger;
mod report;

use crate::agent::AgentConfig;
use crate::config::AbbeyConfig;
use crate::route_log::{self, RouteRecord};
use crate::state::AbbeyState;
use crate::subagents::{self, LaneResult, RunOptions};
use anyhow::{Result, bail};
use gate::{GateReport, run_gate, wants_security_lane};
use ledger::{GoalStatus, Ledger, WorkFocus, pick_work, require_tasks_root};
use report::{RunReport, latest_report_path, report_path, write_report};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Status,
    Plan,
    Run,
    Help,
}

#[derive(Debug, Clone)]
struct ImproveOpts {
    verb: Verb,
    confirm: bool,
    max_rounds: usize,
    max_minutes: u64,
    jobs: usize,
    lanes: Vec<String>,
    peers: Vec<String>,
    synthesize: bool,
    gate_only: bool,
    /// Prefer ledger slices; still require green gate to exit 0.
    ledger_only: bool,
    goal_hint: Vec<String>,
}

impl Default for ImproveOpts {
    fn default() -> Self {
        Self {
            verb: Verb::Help,
            confirm: false,
            max_rounds: 5,
            max_minutes: 60,
            jobs: crate::platform::default_subagent_jobs(),
            lanes: vec!["reviewer".into(), "gemma".into()],
            peers: Vec::new(),
            synthesize: false,
            gate_only: false,
            ledger_only: false,
            goal_hint: Vec::new(),
        }
    }
}

pub fn dispatch(cfg: &AgentConfig, state: &AbbeyState, args: &[String]) -> Result<i32> {
    let opts = parse_args(args)?;
    match opts.verb {
        Verb::Help => {
            print_help();
            Ok(0)
        }
        Verb::Status => cmd_status(state, &opts),
        Verb::Plan => cmd_plan(state, &opts),
        Verb::Run => cmd_run(cfg, state, &opts),
    }
}

pub fn status_line() -> String {
    "improve:   ledger + check.sh bar · local subagents · needs --confirm to apply \
     (not multi-node)"
        .into()
}

fn print_help() {
    println!(
        "abbey improve — goal-driven smart improve (local subagents + ./check.sh)\n\
         \n\
         abbey improve status                 # ledger + gate hint\n\
         abbey improve plan [goal…]           # pick slice; print prompts (no apply)\n\
         abbey improve run [goal…] [flags]    # bounded diagnose→implement→gate loop\n\
         \n\
         flags:\n\
           --confirm              unlock Max force apply for this run (once)\n\
           --max-rounds N         default 5\n\
           --max-minutes M        default 60\n\
           --jobs N               diagnose fan-out concurrency\n\
           --lanes a,b            diagnose lanes (default reviewer,gemma)\n\
           --peers a,b            optional same-host PATH peers\n\
           --synthesize           Abi merge after diagnose\n\
           --gate-only            heal ./check.sh only (ignore open todos)\n\
           --ledger-only          drive open slices; still require green gate\n\
         \n\
         honesty: same-host lanes/peers + local ABI process proof only — production multi-host is Proposed.\n\
         Abbey does not patch files itself; apply goes through cursor-agent Max.\n\
         OS allowlist execute still needs its own --confirm.\n\
         goals.md is never auto-marked done — close the ledger after evidence.\n\
         env: ABBEY_CHECK_CMD overrides ./check.sh"
    );
}

fn parse_args(args: &[String]) -> Result<ImproveOpts> {
    let mut opts = ImproveOpts::default();
    if args.is_empty() {
        return Ok(opts);
    }
    let mut i = match args[0].as_str() {
        "status" | "st" => {
            opts.verb = Verb::Status;
            1
        }
        "plan" => {
            opts.verb = Verb::Plan;
            1
        }
        "run" => {
            opts.verb = Verb::Run;
            1
        }
        "-h" | "--help" | "help" => {
            opts.verb = Verb::Help;
            return Ok(opts);
        }
        // Bare flags → run
        s if s.starts_with('-') => {
            opts.verb = Verb::Run;
            0
        }
        other => {
            bail!("improve: unknown verb `{other}` — try status|plan|run (see `abbey improve`)")
        }
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--confirm" | "-y" => opts.confirm = true,
            "--synthesize" | "-s" => opts.synthesize = true,
            "--gate-only" => opts.gate_only = true,
            "--ledger-only" => opts.ledger_only = true,
            "--max-rounds" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--max-rounds needs a positive integer");
                };
                opts.max_rounds = parse_positive(v, "--max-rounds")?;
            }
            "--max-minutes" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--max-minutes needs a positive integer");
                };
                opts.max_minutes = parse_positive(v, "--max-minutes")? as u64;
            }
            "--jobs" => {
                i += 1;
                opts.jobs = crate::slash::parse_jobs_value(args.get(i).map(String::as_str))?;
            }
            "--lanes" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--lanes needs a csv list");
                };
                opts.lanes = split_csv(v);
            }
            "--peers" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("--peers needs a csv list");
                };
                opts.peers = split_csv(v);
            }
            s if s.starts_with("--max-rounds=") => {
                opts.max_rounds =
                    parse_positive(s.trim_start_matches("--max-rounds="), "--max-rounds")?;
            }
            s if s.starts_with("--max-minutes=") => {
                opts.max_minutes =
                    parse_positive(s.trim_start_matches("--max-minutes="), "--max-minutes")? as u64;
            }
            s if s.starts_with("--jobs=") => {
                opts.jobs = crate::slash::parse_jobs_value(Some(s.trim_start_matches("--jobs=")))?;
            }
            s if s.starts_with("--lanes=") => {
                opts.lanes = split_csv(s.trim_start_matches("--lanes="));
            }
            s if s.starts_with("--peers=") => {
                opts.peers = split_csv(s.trim_start_matches("--peers="));
            }
            "-h" | "--help" => {
                opts.verb = Verb::Help;
                return Ok(opts);
            }
            s if s.starts_with('-') => bail!("improve: unknown flag `{s}`"),
            s => opts.goal_hint.push(s.to_string()),
        }
        i += 1;
    }
    if opts.gate_only && opts.ledger_only {
        bail!("improve: --gate-only and --ledger-only are mutually exclusive");
    }
    Ok(opts)
}

fn parse_positive(s: &str, flag: &str) -> Result<usize> {
    let n: usize = s.parse().map_err(|e| anyhow::anyhow!("{flag}: {e}"))?;
    if n == 0 {
        bail!("{flag} must be >= 1");
    }
    Ok(n)
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

fn cmd_status(state: &AbbeyState, opts: &ImproveOpts) -> Result<i32> {
    let root = require_tasks_root(&state.cwd)?;
    let ledger = Ledger::load(&root)?;
    println!("abbey improve status");
    println!("root:    {}", root.display());
    // Name the exact files, so a bare-looking ledger points at which path missed.
    // Labelled by filename, not "goals"/"todos" — those would collide with the
    // count line below and read as duplicate output.
    for (label, path) in [
        ("goals.md", &ledger.goals_path),
        ("todo.md ", &ledger.todo_path),
    ] {
        let mark = if path.exists() { "" } else { "  (missing)" };
        println!("{label}: {}{mark}", path.display());
    }
    println!(
        "goals:   {} open / {} total · todos: {} open / {} total",
        ledger.open_goals().count(),
        ledger.goals.len(),
        ledger.open_todo_count(),
        ledger.todos.len()
    );
    println!(
        "actionable: {} goals · {} todos (blocked work stays visible)",
        ledger
            .goals
            .iter()
            .filter(|g| matches!(g.status, GoalStatus::Todo | GoalStatus::InProgress))
            .count(),
        ledger.actionable_todo_count()
    );
    for g in &ledger.goals {
        let mark = if g.status.is_open() { "·" } else { "✓" };
        println!("  {mark} [{}] {}", g.status.label(), g.title);
    }
    if let Some(t) = ledger.next_actionable_todo() {
        println!("next actionable todo: [{}] {}", t.section, t.text);
    } else if ledger.open_todo_count() > 0 {
        println!("next actionable todo: none (remaining open items are blocked)");
    }
    let hint = opts.goal_hint.first().map(String::as_str);
    let focus = pick_work(&ledger, hint, opts.gate_only)?;
    println!("focus:   {}", focus.summary());
    println!(
        "gate:    {} (run `abbey improve run` to execute; --confirm to apply)",
        gate::resolve_check_cmd(&root)
            .map(|(p, a)| {
                if a.is_empty() {
                    p
                } else {
                    format!("{p} {}", a.join(" "))
                }
            })
            .unwrap_or_else(|_| "(missing check.sh)".into())
    );
    println!("honesty: local subagents only · goals.md never auto-done");
    if let Some(prev) = latest_report_path(&state.state_dir) {
        println!("last report: {}", prev.display());
    }
    Ok(0)
}

fn cmd_plan(state: &AbbeyState, opts: &ImproveOpts) -> Result<i32> {
    let root = require_tasks_root(&state.cwd)?;
    let ledger = Ledger::load(&root)?;
    let hint = join_hint(&opts.goal_hint);
    let focus = pick_work(&ledger, hint.as_deref(), opts.gate_only)?;
    let diag = diagnose_prompt(&focus, None);
    let impl_p = implement_prompt(&focus, "(diagnose output here)", None, opts.confirm);
    println!("abbey improve plan");
    println!("focus: {}", focus.summary());
    println!(
        "apply: {}",
        if opts.confirm {
            "would force (pass --confirm on run)"
        } else {
            "diagnose-only unless run --confirm"
        }
    );
    println!("\n===== diagnose prompt =====\n{diag}");
    println!("\n===== implement prompt (preview) =====\n{impl_p}");
    Ok(0)
}

fn cmd_run(cfg: &AgentConfig, state: &AbbeyState, opts: &ImproveOpts) -> Result<i32> {
    let root = require_tasks_root(&state.cwd)?;
    let ledger = Ledger::load(&root)?;
    let hint = join_hint(&opts.goal_hint);
    let mut focus = pick_work(&ledger, hint.as_deref(), opts.gate_only)?;
    let correlation = Uuid::new_v4().to_string();
    let started = Instant::now();
    let budget = Duration::from_secs(opts.max_minutes.saturating_mul(60));
    let ac = AbbeyConfig::load().unwrap_or_default();

    eprintln!(
        "abbey: improve {correlation}\n  focus → {}\n  confirm={} max_rounds={} max_minutes={} jobs={}\n  \
         honesty: local PATH peers only — not a multi-node mesh",
        focus.summary(),
        opts.confirm,
        opts.max_rounds,
        opts.max_minutes,
        opts.jobs
    );
    if !opts.confirm {
        eprintln!("abbey: diagnose-only (no Max force). Re-run with --confirm to apply fixes.");
    }

    let mut report = RunReport::new(&correlation, &focus, opts);

    // Round 0: gate first so diagnose is failure-grounded.
    eprintln!("abbey: improve gate (round 0)…");
    let gate0 = run_gate(&root)?;
    log_improve_stage(
        state,
        &correlation,
        "round-0-gate",
        if gate0.ok { "ok" } else { "fail" },
        &gate0.kinds_csv(),
    )?;
    report.note_gate(0, &gate0);
    print_gate_summary(&gate0);
    let mut last_gate = Some(gate0.clone());

    if gate0.ok && focus_complete(&focus, &ledger, opts) {
        report.outcome = "already production-ready (gate green · no open slice)".into();
        write_report(state, &report)?;
        println!("abbey: improve done — {}", report.outcome);
        return Ok(0);
    }

    // If gate is green and we're not ledger-driving, stabilize focus is already done.
    if gate0.ok && matches!(focus, WorkFocus::Stabilize) && !opts.ledger_only {
        report.outcome = "gate green · stabilize focus complete".into();
        write_report(state, &report)?;
        println!("abbey: improve done — {}", report.outcome);
        return Ok(0);
    }

    let max_rounds = if opts.confirm {
        opts.max_rounds
    } else {
        // Diagnose-only: one advise round, no implement loop.
        1
    };

    for round in 1..=max_rounds {
        if started.elapsed() >= budget {
            report.outcome = format!(
                "stopped: wall-clock budget ({} min) exhausted",
                opts.max_minutes
            );
            write_report(state, &report)?;
            eprintln!("abbey: {}", report.outcome);
            return Ok(1);
        }

        // Refresh focus each round unless gate-only.
        if !opts.gate_only {
            let fresh = Ledger::load(&root).unwrap_or_else(|_| ledger.clone());
            focus = pick_work(&fresh, hint.as_deref(), opts.gate_only)?;
        }

        let gate_excerpt = last_gate
            .as_ref()
            .filter(|g| !g.ok)
            .map(|g| g.excerpt.as_str());
        let kinds = last_gate
            .as_ref()
            .map(|g| g.kinds.as_slice())
            .unwrap_or(&[]);

        // ---- diagnose ----
        let mut lanes = opts.lanes.clone();
        if wants_security_lane(kinds) && !lanes.iter().any(|l| l == "security") {
            lanes.push("security".into());
        }
        let diag_prompt = diagnose_prompt(&focus, gate_excerpt);
        eprintln!(
            "abbey: improve diagnose (round {round}) lanes={}…",
            lanes.join(",")
        );
        let diag_opts = RunOptions {
            lanes,
            peers: opts.peers.clone(),
            jobs: opts.jobs,
            synthesize: opts.synthesize,
            prompt: vec![diag_prompt.clone()],
        };
        let diag_results = run_diagnose(cfg, &diag_opts, &ac.roles.max, &ac.roles.gemma)?;
        subagents::print_merged(&diag_results);
        log_improve_stage(
            state,
            &correlation,
            &format!("round-{round}-diagnose"),
            "subagents",
            &diag_results
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(","),
        )?;
        let dossier = merge_dossier(&diag_results);
        report.note_diagnose(round, &diag_results);

        if !opts.confirm {
            report.outcome =
                "diagnose-only complete — pass --confirm to apply Max force rounds".into();
            println!(
                "\n===== improve advise (no apply) =====\n{}",
                dossier.trim()
            );
            write_report(state, &report)?;
            println!("abbey: {}", report.outcome);
            println!(
                "correlation {correlation} — report → {}",
                report_path(&state.state_dir, &correlation).display()
            );
            return Ok(0);
        }

        // ---- implement (force) ----
        let impl_prompt = implement_prompt(&focus, &dossier, gate_excerpt, true);
        eprintln!("abbey: improve implement (round {round}) lane=max force…");
        let impl_result = run_implement(cfg, &ac.roles.max, &impl_prompt)?;
        subagents::print_merged(std::slice::from_ref(&impl_result));
        log_improve_stage(
            state,
            &correlation,
            &format!("round-{round}-implement"),
            "max",
            &format!("exit={}", impl_result.exit),
        )?;
        report.note_implement(round, &impl_result);

        // ---- gate ----
        eprintln!("abbey: improve gate (round {round})…");
        let g = run_gate(&root)?;
        log_improve_stage(
            state,
            &correlation,
            &format!("round-{round}-gate"),
            if g.ok { "ok" } else { "fail" },
            &g.kinds_csv(),
        )?;
        report.note_gate(round, &g);
        print_gate_summary(&g);
        last_gate = Some(g.clone());

        let fresh = Ledger::load(&root).unwrap_or_else(|_| ledger.clone());
        if g.ok && (opts.gate_only || focus_complete(&focus, &fresh, opts)) {
            report.outcome =
                format!("production-ready after round {round} (gate green · focus satisfied)");
            write_report(state, &report)?;
            println!("abbey: improve done — {}", report.outcome);
            println!(
                "correlation {correlation} — close goals.md yourself after evidence · report → {}",
                report_path(&state.state_dir, &correlation).display()
            );
            return Ok(0);
        }
        if g.ok && matches!(focus, WorkFocus::Stabilize) && !opts.ledger_only {
            report.outcome = format!("gate green after round {round}");
            write_report(state, &report)?;
            println!("abbey: improve done — {}", report.outcome);
            return Ok(0);
        }
    }

    report.outcome = format!(
        "stopped: max-rounds={} exhausted (gate {})",
        opts.max_rounds,
        last_gate
            .as_ref()
            .map(|g| if g.ok { "green" } else { "red" })
            .unwrap_or("?")
    );
    write_report(state, &report)?;
    eprintln!("abbey: {}", report.outcome);
    println!(
        "correlation {correlation} — report → {}",
        report_path(&state.state_dir, &correlation).display()
    );
    Ok(1)
}

fn focus_complete(focus: &WorkFocus, ledger: &Ledger, opts: &ImproveOpts) -> bool {
    if opts.gate_only {
        return true; // gate green checked by caller
    }
    match focus {
        WorkFocus::Stabilize => true,
        WorkFocus::Goal { .. } => {
            if opts.ledger_only {
                ledger.open_goals().next().is_none() && ledger.open_todo_count() == 0
            } else {
                // Default: gate green is enough when no open goal/todo remains.
                ledger.pick_goal(None).is_none() && ledger.next_actionable_todo().is_none()
            }
        }
    }
}

fn join_hint(parts: &[String]) -> Option<String> {
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn diagnose_prompt(focus: &WorkFocus, gate_excerpt: Option<&str>) -> String {
    let mut s = String::from(
        "You are a diagnose lane in `abbey improve` (local multi-subagent; not multi-node).\n\
         Do NOT apply code changes. Find concrete bugs, gate failures, and the smallest \
         next fix. Prefer file:line notes and a short ordered plan.\n\n",
    );
    s.push_str(&format!("Work focus: {}\n", focus.summary()));
    if let WorkFocus::Goal { body, todo, .. } = focus {
        if !body.trim().is_empty() {
            s.push_str("\n--- goal body ---\n");
            s.push_str(body);
            s.push_str("\n--- end goal body ---\n");
        }
        if let Some(t) = todo {
            s.push_str(&format!("\nPriority todo checkbox: {t}\n"));
        }
    }
    if let Some(ex) = gate_excerpt {
        s.push_str("\n--- ./check.sh failure excerpt ---\n");
        s.push_str(ex);
        s.push_str("\n--- end excerpt ---\n");
    } else {
        s.push_str("\nGate is currently green or not yet failed; focus on the ledger slice.\n");
    }
    s.push_str(
        "\nOutput: (1) top findings (2) ordered fix plan (3) acceptance = ./check.sh green.\n",
    );
    s
}

fn implement_prompt(
    focus: &WorkFocus,
    dossier: &str,
    gate_excerpt: Option<&str>,
    confirmed: bool,
) -> String {
    let mut s = String::from(
        "You are the Max implement lane in `abbey improve`.\n\
         Apply the smallest fix that advances the focus and turns ./check.sh green.\n\
         Do not mark tasks/goals.md done. Do not claim multi-node mesh.\n\
         Keep claims honesty (Current vs Proposed vs OOS).\n\n",
    );
    if confirmed {
        s.push_str("CONFIRM: user passed --confirm — you may edit the tree (force).\n\n");
    } else {
        s.push_str("NO CONFIRM — propose only; do not edit.\n\n");
    }
    s.push_str(&format!("Work focus: {}\n\n", focus.summary()));
    s.push_str("--- diagnose dossier ---\n");
    s.push_str(dossier.trim());
    s.push_str("\n--- end dossier ---\n");
    if let Some(ex) = gate_excerpt {
        s.push_str("\n--- ./check.sh failure excerpt ---\n");
        s.push_str(ex);
        s.push_str("\n--- end excerpt ---\n");
    }
    s.push_str(
        "\nAfter edits, mentally verify: cargo fmt/clippy/test and file-size guard \
         (./check.sh). Summarize what you changed.\n",
    );
    s
}

fn run_diagnose(
    cfg: &AgentConfig,
    opts: &RunOptions,
    max_model: &str,
    gemma_model: &str,
) -> Result<Vec<LaneResult>> {
    let plans = subagents::build_plan(opts, max_model, gemma_model)?;
    let user = opts.prompt.join(" ");
    let mut results = subagents::run_plans(cfg, &plans, &user, opts.jobs);
    if opts.synthesize {
        let syn = subagents::synthesize(cfg, max_model, &user, &results);
        results.push(syn);
    }
    Ok(results)
}

fn run_implement(cfg: &AgentConfig, max_model: &str, prompt: &str) -> Result<LaneResult> {
    let opts = RunOptions {
        lanes: vec!["max".into()],
        peers: Vec::new(),
        jobs: 1,
        synthesize: false,
        prompt: vec![prompt.to_string()],
    };
    let plans = subagents::build_plan(&opts, max_model, max_model)?;
    let mut force_cfg = cfg.clone();
    force_cfg.force = true;
    force_cfg.print = true;
    let results = subagents::run_plans(&force_cfg, &plans, prompt, 1);
    Ok(results.into_iter().next().unwrap_or(LaneResult {
        name: "max".into(),
        kind: subagents::LaneKind::Abbey,
        model_or_peer: max_model.into(),
        exit: 1,
        stdout: String::new(),
        stderr: "no implement result".into(),
    }))
}

fn merge_dossier(results: &[LaneResult]) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&format!(
            "### {} (exit {})\n{}\n\n",
            r.name,
            r.exit,
            r.stdout.trim()
        ));
    }
    out
}

fn print_gate_summary(g: &GateReport) {
    if g.ok {
        println!(
            "===== gate:ok cmd:{} elapsed_ms:{} =====",
            g.cmd, g.elapsed_ms
        );
    } else {
        println!(
            "===== gate:FAIL exit:{} kinds:{} cmd:{} elapsed_ms:{} =====",
            g.exit,
            g.kinds_csv(),
            g.cmd,
            g.elapsed_ms
        );
        if !g.excerpt.trim().is_empty() {
            eprintln!("{}", g.excerpt.trim_end());
        }
    }
}

fn log_improve_stage(
    state: &AbbeyState,
    correlation: &str,
    stage: &str,
    role: &str,
    detail: &str,
) -> Result<()> {
    let cwd = state.cwd.display().to_string();
    let rec = RouteRecord::new(
        &cwd,
        "abbey",
        role,
        "improve",
        format!("improve stage={stage} {detail}"),
        0.8,
    )
    .in_stage(correlation, stage)
    .with_routing(
        Some("subagents".into()),
        Some("improve loop (local only; audit)".into()),
    );
    route_log::append_route_record(&state.state_dir, &rec)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_run_confirm_and_caps() {
        let args = vec![
            "run".into(),
            "--confirm".into(),
            "--max-rounds".into(),
            "3".into(),
            "--max-minutes=15".into(),
            "--gate-only".into(),
            "Smart".into(),
        ];
        let o = parse_args(&args).unwrap();
        assert_eq!(o.verb, Verb::Run);
        assert!(o.confirm);
        assert_eq!(o.max_rounds, 3);
        assert_eq!(o.max_minutes, 15);
        assert!(o.gate_only);
        assert_eq!(o.goal_hint, vec!["Smart".to_string()]);
    }

    #[test]
    fn parse_rejects_zero_rounds() {
        let args = vec!["run".into(), "--max-rounds=0".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn parse_status_default_no_confirm() {
        let o = parse_args(&["status".into()]).unwrap();
        assert_eq!(o.verb, Verb::Status);
        assert!(!o.confirm);
    }

    #[test]
    fn parse_rejects_gate_and_ledger() {
        let args = vec!["run".into(), "--gate-only".into(), "--ledger-only".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn diagnose_prompt_includes_focus() {
        let focus = WorkFocus::Stabilize;
        let p = diagnose_prompt(&focus, Some("error: boom"));
        assert!(p.contains("stabilize"));
        assert!(p.contains("error: boom"));
        assert!(p.contains("Do NOT apply"));
    }
}
