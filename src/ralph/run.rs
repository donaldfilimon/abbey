//! Ralph loop iteration execution, gate probing and route logging.

use crate::actions::{RunSpec, run_agent};
use crate::improve::gate::{self, GateReport};
use crate::route_log::{self, RouteRecord};
use anyhow::Context;
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::*;

pub(super) fn cmd_run(
    cfg: &AgentConfig,
    state: &AbbeyState,
    root: &Path,
    opts: &RalphOpts,
) -> Result<i32> {
    scaffold_workspace(root, false)?;
    let mut ralph_state = load_state(root);
    if let Some(reason) = &ralph_state.halted_reason {
        bail!(
            "ralph circuit breaker is open ({reason}); inspect `.ralph/errors.log` and run `abbey ralph reset` to resume"
        );
    }

    let hint = if opts.rest.is_empty() {
        None
    } else {
        Some(opts.rest.join(" "))
    };

    if !opts.confirm {
        let (task, prompt) = build_iteration_prompt(root, Some(state), hint.as_deref())?;
        println!(
            "abbey ralph: dry-run preview (pass --confirm to execute fresh-context iterations)\n\
             selected single task: {task}\n\
             max_iterations={} · max_failures={} · max_minutes={}\n\n\
             --- iteration prompt preview ---\n{}",
            opts.max_iterations,
            opts.max_failures,
            opts.max_minutes,
            prompt.trim()
        );
        return Ok(0);
    }

    let correlation = Uuid::new_v4().to_string();
    let started = Instant::now();
    let budget = Duration::from_secs(opts.max_minutes.saturating_mul(60));

    for round in 1..=opts.max_iterations {
        cfg.check_cancelled()?;
        if started.elapsed() >= budget {
            ralph_state.halted_reason = Some(format!(
                "wall-clock budget ({} min) exhausted",
                opts.max_minutes
            ));
            save_state(root, &ralph_state)?;
            println!(
                "abbey ralph: stopped — {}",
                ralph_state.halted_reason.as_deref().unwrap_or_default()
            );
            return Ok(1);
        }

        let items = load_plan_items(root);
        if items.iter().all(|i| i.done) && hint.is_none() {
            println!("abbey ralph: all tasks in fix_plan.md are complete.");
            return Ok(0);
        }

        let (task, prompt) = build_iteration_prompt(root, Some(state), hint.as_deref())?;
        ralph_state.iteration += 1;
        if ralph_state.active_task.as_deref() != Some(task.as_str()) {
            ralph_state.active_task = Some(task.clone());
            ralph_state.consecutive_failures = 0;
        }
        save_state(root, &ralph_state)?;

        let mut iter_cfg = cfg.clone();
        iter_cfg.force = true;
        iter_cfg.print = true;
        let _ = run_agent(&mut iter_cfg, state, &[prompt], RunSpec::fresh())?;

        let gate_report = run_workspace_gate(root)?;
        log_ralph_route(
            state,
            &correlation,
            ralph_state.iteration,
            gate_report.ok,
            &gate_report.kinds_csv(),
        )?;

        if gate_report.ok {
            let _ = mark_task_completed(root, &task);
            ralph_state.completed_tasks += 1;
            ralph_state.consecutive_failures = 0;
            ralph_state.last_gate_ok = Some(true);
            ralph_state.last_gate_kinds = "ok".into();
            ralph_state.halted_reason = None;
            save_state(root, &ralph_state)?;
            let _ = record_learning(
                root,
                Some(state),
                &format!(
                    "Iteration {} verified `{task}` green against `{}`.",
                    ralph_state.iteration, gate_report.cmd
                ),
            );
            println!(
                "abbey ralph: iteration {} (round {round}) GREEN — completed `{task}`",
                ralph_state.iteration
            );
            if opts.max_iterations == 1 {
                return Ok(0);
            }
        } else {
            ralph_state.consecutive_failures += 1;
            ralph_state.last_gate_ok = Some(false);
            ralph_state.last_gate_kinds = gate_report.kinds_csv();
            record_failure_reflection(
                root,
                Some(state),
                ralph_state.iteration,
                &task,
                &gate_report.kinds_csv(),
                &gate_report.excerpt,
            )?;
            if ralph_state.consecutive_failures >= opts.max_failures {
                let reason = format!(
                    "task `{task}` failed {} consecutive iterations (kinds: {})",
                    ralph_state.consecutive_failures,
                    gate_report.kinds_csv()
                );
                ralph_state.halted_reason = Some(reason.clone());
                save_state(root, &ralph_state)?;
                eprintln!("abbey ralph: circuit breaker tripped — {reason}");
                return Ok(1);
            }
            save_state(root, &ralph_state)?;
        }
    }
    Ok(0)
}

fn run_workspace_gate(root: &Path) -> Result<GateReport> {
    if gate::resolve_check_cmd(root).is_ok() {
        return gate::run_gate(root);
    }
    // Fallback for standard Rust workspaces without ./check.sh.
    let started = Instant::now();
    let out = std::process::Command::new("cargo")
        .args(["test", "--locked"])
        .current_dir(root)
        .output()
        .context("spawn `cargo test --locked`")?;
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    let ok = out.status.success();
    let kinds = if ok {
        Vec::new()
    } else {
        gate::classify_failures(&combined)
    };
    Ok(GateReport {
        ok,
        exit: out.status.code().unwrap_or(1),
        excerpt: gate::clamp_excerpt(&combined),
        kinds,
        elapsed_ms: started.elapsed().as_millis(),
        cmd: "cargo test --locked".into(),
    })
}

fn log_ralph_route(
    state: &AbbeyState,
    correlation: &str,
    iteration: usize,
    ok: bool,
    kinds: &str,
) -> Result<()> {
    let cwd = state.cwd.display().to_string();
    let stage = format!("ralph-iter-{iteration}");
    let rec = RouteRecord::new(
        &cwd,
        "abbey",
        "max",
        "ralph-loop",
        format!(
            "ralph-loop iter={iteration} gate={} kinds={kinds}",
            if ok { "ok" } else { "fail" }
        ),
        if ok { 0.92 } else { 0.55 },
    )
    .in_stage(correlation, &stage)
    .with_routing(
        Some("gemma".into()),
        Some("ralph-loop fresh-context iteration".into()),
    );
    route_log::append_route_record(&state.state_dir, &rec)
}
