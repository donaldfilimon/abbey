use super::{LaneKind, LanePlan, LaneResult};
use crate::agent::AgentConfig;
use crate::models::resolve_model;
use crate::persona;
use crate::roles::{self, WorkerRole};
use std::process::Command;
use std::sync::Arc;
use std::thread;

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
        Ok((status, stdout, stderr)) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Abbey,
            model_or_peer: plan.model.clone(),
            exit: status.code().unwrap_or(1),
            stdout,
            stderr,
        },
        Err(error) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Abbey,
            model_or_peer: plan.model.clone(),
            exit: 1,
            stdout: String::new(),
            stderr: format!("{error:#}"),
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
        Ok(output) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Peer,
            model_or_peer: bin.display().to_string(),
            exit: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => LaneResult {
            name: plan.name.clone(),
            kind: LaneKind::Peer,
            model_or_peer: bin.display().to_string(),
            exit: 1,
            stdout: String::new(),
            stderr: error.to_string(),
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
    let mut results = Vec::with_capacity(plans.len());
    for chunk in plans.chunks(jobs) {
        thread::scope(|scope| {
            let handles = chunk
                .iter()
                .cloned()
                .map(|plan| {
                    let user = Arc::clone(&user);
                    let base = Arc::clone(&base);
                    scope.spawn(move || match plan.kind {
                        LaneKind::Abbey => run_abbey_lane(&base, &plan, &user),
                        LaneKind::Peer => run_peer_lane(&plan, &user),
                    })
                })
                .collect::<Vec<_>>();
            for handle in handles {
                match handle.join() {
                    Ok(result) => results.push(result),
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
    for result in results {
        let kind = match result.kind {
            LaneKind::Abbey => "abbey",
            LaneKind::Peer => "peer",
        };
        println!(
            "===== subagent:{} kind:{} via:{} exit:{} =====",
            result.name, kind, result.model_or_peer, result.exit
        );
        if !result.stdout.trim().is_empty() {
            crate::highlight::emit_agent_stdout(result.stdout.trim_end());
            println!();
        }
        if !result.stderr.trim().is_empty() {
            eprintln!("{}", result.stderr.trim_end());
        }
        println!();
    }
}

/// Abi-persona merge pass over lane results.
pub fn synthesize(
    base: &AgentConfig,
    max_model: &str,
    user: &str,
    results: &[LaneResult],
) -> LaneResult {
    let mut dossier = String::from("Multi-subagent results to reconcile:\n\n");
    for result in results {
        dossier.push_str(&format!(
            "### {}\nexit {}\n{}\n\n",
            result.name,
            result.exit,
            result.stdout.trim()
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
    run_abbey_lane(base, &plan, &format!("{user}\n\n{dossier}"))
}
