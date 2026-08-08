use super::RunOptions;
use crate::models::resolve_model;
use crate::roles::WorkerRole;
use anyhow::{Result, bail};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    /// Abbey's selected agent backend with persona x role wrapping.
    Abbey,
    /// External peer CLI on PATH, still on this host.
    Peer,
}

#[derive(Debug, Clone)]
pub struct SubagentSpec {
    pub name: &'static str,
    pub kind: LaneKind,
    pub summary: &'static str,
    pub role: WorkerRole,
    pub persona: &'static str,
    pub focus: Option<&'static str>,
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

const CATALOG: &[SubagentSpec] = &[
    SubagentSpec {
        name: "max",
        kind: LaneKind::Abbey,
        summary: "Implementer - code, tools, math (Max role)",
        role: WorkerRole::Max,
        persona: "abbey",
        focus: None,
        peer_bin: None,
    },
    SubagentSpec {
        name: "gemma",
        kind: LaneKind::Abbey,
        summary: "Interpreter - tone, visual, clarify (Gemma role)",
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
    println!("abbey subagents - multi-lane + local peer agents");
    println!("note: peers are PATH CLIs on this host; no production multi-host mesh is claimed.\n");
    println!("{:<12} {:<8} detail", "name", "kind");
    for spec in CATALOG {
        let kind = match spec.kind {
            LaneKind::Abbey => "abbey",
            LaneKind::Peer => "peer",
        };
        let status = if spec.kind == LaneKind::Peer {
            match spec.peer_bin.and_then(crate::agent::which_bin) {
                Some(path) => format!("present - {}", path.display()),
                None => "missing".into(),
            }
        } else {
            "Abbey backend lane".into()
        };
        println!("{:<12} {:<8} {}", spec.name, kind, spec.summary);
        println!("{:<12} {:<8} {}", "", "", status);
    }
    println!(
        "{}",
        concat!(
            "\nrun:  abbey subagents run [--lanes max,gemma,reviewer] ",
            "[--peers gemini,claude] \\\n",
            "\t\t[--jobs N] [--synthesize] <prompt...>\n",
            "alias: abbey parallel ... / swarm / distribute\n",
            "env:   ABBEY_SUBAGENT_JOBS (default 4)\n",
            "note: --peers is local PATH CLIs only - not a multi-node mesh"
        )
    );
}

pub fn build_plan(opts: &RunOptions, max_model: &str, gemma_model: &str) -> Result<Vec<LanePlan>> {
    let mut names = Vec::new();
    if opts.lanes.is_empty() && opts.peers.is_empty() {
        names.extend(default_lane_names());
    } else {
        names.extend(opts.lanes.iter().cloned());
        names.extend(opts.peers.iter().cloned());
    }
    let mut seen = std::collections::HashSet::new();
    names.retain(|name| seen.insert(name.clone()));

    let mut plans = Vec::new();
    for name in names {
        let Some(spec) = find_spec(&name) else {
            bail!(
                "unknown subagent `{name}` - try: abbey subagents list\nknown: {}",
                CATALOG
                    .iter()
                    .map(|spec| spec.name)
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
                "abbey: peer `{}` not on PATH - skipping",
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

pub fn status_line() -> String {
    let peers = CATALOG
        .iter()
        .filter(|spec| spec.kind == LaneKind::Peer)
        .filter(|spec| spec.peer_bin.and_then(crate::agent::which_bin).is_some())
        .count();
    let abbey_n = CATALOG
        .iter()
        .filter(|spec| spec.kind == LaneKind::Abbey)
        .count();
    format!(
        "subagents: {abbey_n} abbey lanes - {peers} peer CLI(s) on PATH (local, not multi-node)"
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
    fn build_plan_defaults_three_lanes() {
        let opts = RunOptions {
            prompt: vec!["x".into()],
            ..RunOptions::default()
        };
        let plans = build_plan(&opts, "fable", "gemini").unwrap();
        let names: Vec<_> = plans.iter().map(|plan| plan.name.as_str()).collect();
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
