//! Parallel agentic lanes — fan-out across roles / peer tools, then merge.

use crate::agent::AgentConfig;
use crate::models::resolve_model;
use crate::persona;
use crate::roles::{self, WorkerRole};
use crate::state::AbbeyState;
use anyhow::Result;
use std::thread;

#[derive(Debug, Clone)]
pub struct LaneSpec {
    pub name: String,
    pub role: WorkerRole,
    pub model: String,
    pub persona_label: String,
}

#[derive(Debug, Clone)]
pub struct LaneResult {
    pub name: String,
    pub model: String,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn default_lanes(user: &str, cfg_max: &str, cfg_gemma: &str) -> Vec<LaneSpec> {
    let persona = persona::select_persona(user);
    vec![
        LaneSpec {
            name: "max".into(),
            role: WorkerRole::Max,
            model: resolve_model(cfg_max),
            persona_label: persona.label().into(),
        },
        LaneSpec {
            name: "gemma".into(),
            role: WorkerRole::Gemma,
            model: resolve_model(cfg_gemma),
            persona_label: persona.label().into(),
        },
        LaneSpec {
            name: "aviva".into(),
            role: WorkerRole::Max,
            model: resolve_model(cfg_max),
            persona_label: "aviva".into(),
        },
    ]
}

fn lane_prompt(spec: &LaneSpec, user: &str) -> String {
    let profile =
        persona::parse_persona(&spec.persona_label).unwrap_or(abi_ai::AgentProfile::Abbey);
    let wrapped = persona::wrap_prompt(profile, user);
    let note = roles::role_system_note(spec.role);
    format!(
        "{note}\n\nYou are lane `{}` in a parallel Abbey run. Be concise; other lanes also answer.\n\n{wrapped}",
        spec.name
    )
}

/// Run lanes in parallel via cursor-agent `--print` capture.
pub fn run_lanes(base: &AgentConfig, lanes: &[LaneSpec], user: &str) -> Result<Vec<LaneResult>> {
    let user = user.to_string();
    thread::scope(|scope| {
        let mut handles = Vec::new();
        for lane in lanes {
            let lane = lane.clone();
            let mut cfg = base.clone();
            cfg.model = lane.model.clone();
            cfg.print = true;
            let prompt = lane_prompt(&lane, &user);
            handles.push(scope.spawn(move || -> Result<LaneResult> {
                let (st, out, err) = cfg.run_capture(None, &[prompt])?;
                Ok(LaneResult {
                    name: lane.name,
                    model: lane.model,
                    exit: st.code().unwrap_or(1),
                    stdout: out,
                    stderr: err,
                })
            }));
        }
        let mut results = Vec::new();
        for h in handles {
            match h.join() {
                Ok(Ok(r)) => results.push(r),
                Ok(Err(e)) => results.push(LaneResult {
                    name: "error".into(),
                    model: String::new(),
                    exit: 1,
                    stdout: String::new(),
                    stderr: format!("{e:#}"),
                }),
                Err(_) => results.push(LaneResult {
                    name: "panic".into(),
                    model: String::new(),
                    exit: 1,
                    stdout: String::new(),
                    stderr: "lane thread panicked".into(),
                }),
            }
        }
        Ok(results)
    })
}

pub fn print_merged(results: &[LaneResult]) {
    for r in results {
        println!(
            "===== lane:{} model:{} exit:{} =====",
            r.name, r.model, r.exit
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
    println!(
        "===== merge note =====\n\
         Parallel lanes completed. Prefer Max for code/tools, Gemma for tone/visual, \
         Aviva lane for terse expert framing. Abi (you) reconciles conflicts."
    );
}

pub fn run_parallel_cli(
    cfg: &AgentConfig,
    _state: &AbbeyState,
    prompt: &[String],
    max_model: &str,
    gemma_model: &str,
) -> Result<i32> {
    let user = prompt.join(" ");
    if user.trim().is_empty() {
        anyhow::bail!("usage: abbey parallel <prompt…>");
    }
    let lanes = default_lanes(&user, max_model, gemma_model);
    eprintln!(
        "abbey: parallel lanes → {}",
        lanes
            .iter()
            .map(|l| format!("{}({})", l.name, l.model))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let results = run_lanes(cfg, &lanes, &user)?;
    print_merged(&results);
    let worst = results.iter().map(|r| r.exit).max().unwrap_or(1);
    Ok(worst)
}
