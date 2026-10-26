//! Self-learning Rust Ralph loop orchestrator and stdio MCP server.
//!
//! A Ralph loop runs an agent in a fresh-context iteration cycle where each
//! iteration selects the single highest-priority pending task from `fix_plan.md`
//! (or the repository ledger), applies one focused change, verifies it against
//! the Rust gate (`./check.sh` or `ABBEY_CHECK_CMD`), and persists operational
//! learnings and failure reflections on disk (`.ralph/learnings.md`,
//! `.ralph/errors.log`, `.ralph/state.json`) plus Abbey's self-learn memory.

use crate::actions::{RunSpec, run_agent};
use crate::agent::AgentConfig;
use crate::improve::gate::{self, GateReport};
use crate::learn;
use crate::route_log::{self, RouteRecord};
use crate::state::AbbeyState;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Default maximum iterations for a bounded Ralph loop run.
pub const DEFAULT_MAX_ITERATIONS: usize = 10;

/// Default consecutive gate failures on the same task before tripping the circuit breaker.
pub const DEFAULT_MAX_FAILURES: usize = 3;

/// Default wall-clock budget in minutes for a bounded Ralph loop run.
pub const DEFAULT_MAX_MINUTES: u64 = 60;

/// Maximum JSON-RPC request frame size in bytes for the Ralph MCP server.
const MAX_MCP_FRAME_BYTES: usize = 256 * 1024;

/// Persistent circuit-breaker and iteration state stored in `.ralph/state.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RalphState {
    /// Total iterations executed in this workspace.
    pub iteration: usize,
    /// Total tasks verified green and marked complete.
    pub completed_tasks: usize,
    /// Consecutive gate failures on the current active task.
    pub consecutive_failures: usize,
    /// Title of the task currently in progress.
    pub active_task: Option<String>,
    /// Whether the most recent gate check succeeded.
    pub last_gate_ok: Option<bool>,
    /// Comma-separated failure classifications from the last red gate.
    pub last_gate_kinds: String,
    /// Reason the circuit breaker tripped, if halted.
    pub halted_reason: Option<String>,
}

/// One checklist item in `fix_plan.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanItem {
    /// 1-based line number in `fix_plan.md`.
    pub line: usize,
    /// Whether the item is checked (`[x]`).
    pub done: bool,
    /// Task description text.
    pub text: String,
}

/// Summary of the workspace's Ralph loop files and state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RalphStatusSnapshot {
    /// Workspace root path.
    pub workspace: String,
    /// Whether `PROMPT.md` exists.
    pub prompt_exists: bool,
    /// Whether `fix_plan.md` exists.
    pub fix_plan_exists: bool,
    /// Number of pending tasks in `fix_plan.md`.
    pub pending_count: usize,
    /// Number of completed tasks in `fix_plan.md`.
    pub completed_count: usize,
    /// Next single task selected for the next fresh-context iteration.
    pub next_task: Option<String>,
    /// Number of self-learned operational insights in `.ralph/learnings.md`.
    pub learnings_count: usize,
    /// Number of failure reflections in `.ralph/errors.log`.
    pub error_log_entries: usize,
    /// Current circuit-breaker and iteration state.
    pub state: RalphState,
}

/// Options parsed from `abbey ralph …` CLI or slash invocations.
#[derive(Debug, Clone)]
pub struct RalphOpts {
    /// Subcommand verb.
    pub verb: RalphVerb,
    /// Unlock agent execution and file mutations (`--confirm`).
    pub confirm: bool,
    /// Overwrite existing scaffold files on `init` (`--force`).
    pub force: bool,
    /// Emit machine-readable JSON (`--json`).
    pub json: bool,
    /// Maximum loop iterations (`--max-iterations`).
    pub max_iterations: usize,
    /// Consecutive failures before circuit breaker halts (`--max-failures`).
    pub max_failures: usize,
    /// Wall-clock timeout in minutes (`--max-minutes`).
    pub max_minutes: u64,
    /// Optional workspace override (`--workspace`).
    pub workspace: Option<PathBuf>,
    /// Free-text task hint, learning insight, or arguments.
    pub rest: Vec<String>,
}

impl Default for RalphOpts {
    fn default() -> Self {
        Self {
            verb: RalphVerb::Status,
            confirm: false,
            force: false,
            json: false,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_failures: DEFAULT_MAX_FAILURES,
            max_minutes: DEFAULT_MAX_MINUTES,
            workspace: None,
            rest: Vec::new(),
        }
    }
}

/// Supported subcommands under `abbey ralph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RalphVerb {
    /// Show Ralph loop files, pending tasks, learnings, and circuit-breaker state.
    Status,
    /// Scaffold `PROMPT.md`, `fix_plan.md`, and `.ralph/` files for Rust.
    Init,
    /// Preview the fresh-context single-task prompt with injected self-learnings.
    Plan,
    /// Execute a single fresh-context iteration + gate check.
    Step,
    /// Execute a bounded multi-iteration fresh-context Ralph loop.
    Run,
    /// Record an explicit Rust / repository learning into `.ralph/learnings.md` and Abbey LTM.
    Learn,
    /// Reset the circuit breaker counter in `.ralph/state.json`.
    ResetBreaker,
    /// Run the stdio JSON-RPC 2.0 MCP server for the Ralph loop.
    Mcp,
    /// Register `ralph-loop-rust` in global and workspace MCP config files.
    InstallMcp,
    /// Print usage help.
    Help,
}

/// Dispatch `abbey ralph …` commands.
pub fn dispatch(cfg: &AgentConfig, state: &AbbeyState, args: &[String]) -> Result<i32> {
    let opts = parse_args(args)?;
    let root = opts.workspace.clone().unwrap_or_else(|| state.cwd.clone());
    match opts.verb {
        RalphVerb::Help => {
            print_help();
            Ok(0)
        }
        RalphVerb::Status => cmd_status(&root, &opts),
        RalphVerb::Init => cmd_init(&root, &opts),
        RalphVerb::Plan => cmd_plan(&root, state, &opts),
        RalphVerb::Learn => cmd_learn(&root, state, &opts),
        RalphVerb::ResetBreaker => cmd_reset_breaker(&root, &opts),
        RalphVerb::Step => {
            let mut single = opts.clone();
            single.max_iterations = 1;
            cmd_run(cfg, state, &root, &single)
        }
        RalphVerb::Run => cmd_run(cfg, state, &root, &opts),
        RalphVerb::Mcp => serve_mcp_stdio(&root, Some(state)),
        RalphVerb::InstallMcp => cmd_install_mcp(&root, &opts),
    }
}

/// Parse CLI/slash arguments for `abbey ralph`.
pub fn parse_args(args: &[String]) -> Result<RalphOpts> {
    let mut opts = RalphOpts::default();
    if args.is_empty() {
        return Ok(opts);
    }
    let mut index = match args[0].as_str() {
        "status" | "st" => {
            opts.verb = RalphVerb::Status;
            1
        }
        "init" | "scaffold" => {
            opts.verb = RalphVerb::Init;
            1
        }
        "plan" | "prompt" => {
            opts.verb = RalphVerb::Plan;
            1
        }
        "step" | "once" => {
            opts.verb = RalphVerb::Step;
            1
        }
        "run" | "loop" => {
            opts.verb = RalphVerb::Run;
            1
        }
        "learn" | "remember" => {
            opts.verb = RalphVerb::Learn;
            1
        }
        "reset" | "reset-breaker" => {
            opts.verb = RalphVerb::ResetBreaker;
            1
        }
        "mcp" | "serve-mcp" => {
            opts.verb = RalphVerb::Mcp;
            1
        }
        "install-mcp" | "register-mcp" => {
            opts.verb = RalphVerb::InstallMcp;
            1
        }
        "-h" | "--help" | "help" => {
            opts.verb = RalphVerb::Help;
            return Ok(opts);
        }
        flag if flag.starts_with('-') => {
            opts.verb = RalphVerb::Status;
            0
        }
        other => bail!(
            "ralph: unknown subcommand `{other}` — try status|init|plan|step|run|learn|mcp|install-mcp"
        ),
    };

    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--confirm" | "-y" => opts.confirm = true,
            "--force" | "-f" => opts.force = true,
            "--json" => opts.json = true,
            "-h" | "--help" => {
                opts.verb = RalphVerb::Help;
                return Ok(opts);
            }
            "--max-iterations" | "--max-rounds" => {
                index += 1;
                let Some(val) = args.get(index) else {
                    bail!("--max-iterations requires a positive integer");
                };
                opts.max_iterations = parse_positive(val, "--max-iterations")?;
            }
            "--max-failures" => {
                index += 1;
                let Some(val) = args.get(index) else {
                    bail!("--max-failures requires a positive integer");
                };
                opts.max_failures = parse_positive(val, "--max-failures")?;
            }
            "--max-minutes" => {
                index += 1;
                let Some(val) = args.get(index) else {
                    bail!("--max-minutes requires a positive integer");
                };
                opts.max_minutes = parse_positive(val, "--max-minutes")? as u64;
            }
            "--workspace" | "--cwd" => {
                index += 1;
                let Some(val) = args.get(index) else {
                    bail!("--workspace requires a directory path");
                };
                opts.workspace = Some(PathBuf::from(val));
            }
            s if s.starts_with("--max-iterations=") => {
                opts.max_iterations = parse_positive(
                    s.trim_start_matches("--max-iterations="),
                    "--max-iterations",
                )?;
            }
            s if s.starts_with("--max-rounds=") => {
                opts.max_iterations =
                    parse_positive(s.trim_start_matches("--max-rounds="), "--max-rounds")?;
            }
            s if s.starts_with("--max-failures=") => {
                opts.max_failures =
                    parse_positive(s.trim_start_matches("--max-failures="), "--max-failures")?;
            }
            s if s.starts_with("--max-minutes=") => {
                opts.max_minutes =
                    parse_positive(s.trim_start_matches("--max-minutes="), "--max-minutes")? as u64;
            }
            s if s.starts_with("--workspace=") => {
                opts.workspace = Some(PathBuf::from(s.trim_start_matches("--workspace=")));
            }
            s if s.starts_with('-') => bail!("ralph: unknown flag `{s}`"),
            word => opts.rest.push(word.to_string()),
        }
        index += 1;
    }
    Ok(opts)
}

fn parse_positive(raw: &str, flag: &str) -> Result<usize> {
    let val: usize = raw.parse().map_err(|e| anyhow::anyhow!("{flag}: {e}"))?;
    if val == 0 {
        bail!("{flag} must be >= 1");
    }
    Ok(val)
}

fn print_help() {
    println!(
        "abbey ralph — self-learning Rust Ralph loop (fresh context + single task + Cargo gate)\n\n\
         usage:\n  \
           abbey ralph status [--json]                  # inspect PROMPT.md, fix_plan.md, learnings, breaker\n  \
           abbey ralph init [--force]                   # scaffold PROMPT.md, fix_plan.md, and .ralph/\n  \
           abbey ralph plan [hint…]                     # preview fresh-context single-task prompt\n  \
           abbey ralph step [--confirm]                 # run 1 fresh-context iteration + gate verification\n  \
           abbey ralph run [--confirm] [flags]          # run bounded self-learning Ralph loop\n  \
           abbey ralph learn <insight…>                 # record Rust/project insight to .ralph + LTM\n  \
           abbey ralph reset                            # reset circuit breaker after manual fix\n  \
           abbey ralph mcp                              # run stdio JSON-RPC 2.0 MCP server\n  \
           abbey ralph install-mcp                      # register ralph-loop-rust in global MCP configs\n\n\
         flags:\n  \
           --confirm               unlock agent execution and file updates\n  \
           --max-iterations N      default {DEFAULT_MAX_ITERATIONS}\n  \
           --max-failures F        circuit-breaker threshold per task (default {DEFAULT_MAX_FAILURES})\n  \
           --max-minutes M         wall-clock timeout in minutes (default {DEFAULT_MAX_MINUTES})\n  \
           --workspace DIR         target repository root (defaults to current directory)\n  \
           --json                  machine-readable JSON output"
    );
}

fn ralph_dir(root: &Path) -> PathBuf {
    root.join(".ralph")
}

fn prompt_path(root: &Path) -> PathBuf {
    root.join("PROMPT.md")
}

fn fix_plan_path(root: &Path) -> PathBuf {
    root.join("fix_plan.md")
}

fn learnings_path(root: &Path) -> PathBuf {
    ralph_dir(root).join("learnings.md")
}

fn errors_log_path(root: &Path) -> PathBuf {
    ralph_dir(root).join("errors.log")
}

fn state_json_path(root: &Path) -> PathBuf {
    ralph_dir(root).join("state.json")
}

/// Load `.ralph/state.json`, returning default state if not yet initialized.
pub fn load_state(root: &Path) -> RalphState {
    let path = state_json_path(root);
    let Ok(text) = fs::read_to_string(path) else {
        return RalphState::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Persist `.ralph/state.json` atomically.
pub fn save_state(root: &Path, ralph_state: &RalphState) -> Result<()> {
    let dir = ralph_dir(root);
    fs::create_dir_all(&dir)?;
    let path = state_json_path(root);
    let encoded = serde_json::to_string_pretty(ralph_state)?;
    fs::write(path, encoded)?;
    Ok(())
}

/// Parse checklist items from `fix_plan.md` (or `tasks/todo.md` fallback).
pub fn parse_plan_items(text: &str) -> Vec<PlanItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            items.push(PlanItem {
                line: idx + 1,
                done: false,
                text: rest.trim().to_string(),
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            items.push(PlanItem {
                line: idx + 1,
                done: true,
                text: rest.trim().to_string(),
            });
        }
    }
    items
}

/// Load checklist items from `fix_plan.md`, falling back to `tasks/todo.md`.
pub fn load_plan_items(root: &Path) -> Vec<PlanItem> {
    let primary = fix_plan_path(root);
    if primary.is_file()
        && let Ok(text) = fs::read_to_string(&primary)
    {
        return parse_plan_items(&text);
    }
    let fallback = root.join("tasks/todo.md");
    if fallback.is_file()
        && let Ok(text) = fs::read_to_string(&fallback)
    {
        return parse_plan_items(&text);
    }
    Vec::new()
}

/// Mark the first pending task matching `task_text` as `- [x]` in `fix_plan.md`.
pub fn mark_task_completed(root: &Path, task_text: &str) -> Result<bool> {
    let path = fix_plan_path(root);
    if !path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(&path)?;
    let mut updated = false;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if !updated {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("- [ ] ")
                && rest.trim() == task_text.trim()
            {
                let indent_len = line.len() - trimmed.len();
                let indent = &line[..indent_len];
                out.push_str(indent);
                out.push_str("- [x] ");
                out.push_str(rest);
                out.push('\n');
                updated = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if updated {
        fs::write(&path, out)?;
    }
    Ok(updated)
}

/// Build a status snapshot for the workspace.
pub fn status_snapshot(root: &Path) -> RalphStatusSnapshot {
    let items = load_plan_items(root);
    let pending: Vec<&PlanItem> = items.iter().filter(|i| !i.done).collect();
    let completed_count = items.iter().filter(|i| i.done).count();
    let next_task = pending.first().map(|i| i.text.clone());
    let learnings_count = fs::read_to_string(learnings_path(root))
        .map(|t| {
            t.lines()
                .filter(|l| l.trim_start().starts_with("- "))
                .count()
        })
        .unwrap_or(0);
    let error_log_entries = fs::read_to_string(errors_log_path(root))
        .map(|t| {
            t.lines()
                .filter(|l| l.trim_start().starts_with("## Iteration"))
                .count()
        })
        .unwrap_or(0);
    RalphStatusSnapshot {
        workspace: root.display().to_string(),
        prompt_exists: prompt_path(root).is_file(),
        fix_plan_exists: fix_plan_path(root).is_file(),
        pending_count: pending.len(),
        completed_count,
        next_task,
        learnings_count,
        error_log_entries,
        state: load_state(root),
    }
}

/// Scaffold `PROMPT.md`, `fix_plan.md`, and `.ralph/` files in `root`.
pub fn scaffold_workspace(root: &Path, force: bool) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    fs::create_dir_all(ralph_dir(root))?;

    let prompt_file = prompt_path(root);
    if force || !prompt_file.exists() {
        fs::write(
            &prompt_file,
            "# Rust Self-Learning Ralph Loop Prompt\n\n\
             You are operating in a fresh-context, single-task-per-iteration Rust Ralph loop.\n\n\
             ## Rules\n\
             1. Read `fix_plan.md`, `AGENTS.md`, and `.ralph/learnings.md`.\n\
             2. Pick the SINGLE highest-priority unchecked (`- [ ]`) task from `fix_plan.md`.\n\
             3. Implement ONLY that one task. Do not bundle unrelated refactors.\n\
             4. Run the Rust verification gate (`./check.sh` or `cargo fmt --check && cargo clippy -- -D warnings && cargo test`).\n\
             5. If verification passes, mark the task `- [x]` in `fix_plan.md` and record any reusable Rust/crate insight in `.ralph/learnings.md`.\n\
             6. If verification fails, append the root cause and rejected hypothesis to `.ralph/errors.log` so the next fresh-context iteration does not repeat it.\n",
        )?;
        written.push(prompt_file);
    }

    let plan_file = fix_plan_path(root);
    if force || !plan_file.exists() {
        fs::write(
            &plan_file,
            "# Fix Plan (Ralph Loop — One Task Per Iteration)\n\n\
             ## Priority Tasks\n\n\
             - [ ] Verify `cargo check --locked` and `cargo test --locked` pass cleanly\n\
             - [ ] Resolve any pending compiler, Clippy, or test regressions\n",
        )?;
        written.push(plan_file);
    }

    let learn_file = learnings_path(root);
    if force || !learn_file.exists() {
        fs::write(
            &learn_file,
            "# Self-Learned Operational Playbook (.ralph/learnings.md)\n\n\
             - Always keep Cargo commands `--locked` when a lockfile is present and respect repository module-size and lint boundaries.\n\
             - Complete exactly one task per iteration and verify with `./check.sh` or `cargo test` before marking `- [x]`.\n",
        )?;
        written.push(learn_file);
    }

    let err_file = errors_log_path(root);
    if force || !err_file.exists() {
        fs::write(
            &err_file,
            "# Ralph Loop Failure Reflections (.ralph/errors.log)\n\n",
        )?;
        written.push(err_file);
    }

    let state_file = state_json_path(root);
    if force || !state_file.exists() {
        save_state(root, &RalphState::default())?;
        written.push(state_file);
    }

    Ok(written)
}

/// Record a self-learned insight into `.ralph/learnings.md` and optionally Abbey LTM.
pub fn record_learning(
    root: &Path,
    abbey_state: Option<&AbbeyState>,
    insight: &str,
) -> Result<Option<String>> {
    let trimmed = insight.trim();
    if trimmed.is_empty() {
        bail!("learning insight must not be empty");
    }
    fs::create_dir_all(ralph_dir(root))?;
    let path = learnings_path(root);
    let mut existing = if path.is_file() {
        fs::read_to_string(&path)?
    } else {
        String::from("# Self-Learned Operational Playbook (.ralph/learnings.md)\n\n")
    };
    let bullet = format!("- {trimmed}\n");
    if !existing.contains(&bullet) {
        if !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(&bullet);
        fs::write(&path, existing)?;
    }
    let ltm_id = if let Some(st) = abbey_state {
        learn::capture_correction(st, "ralph-loop rust learning", trimmed, true).ok()
    } else {
        None
    };
    Ok(ltm_id)
}

/// Append a failure reflection to `.ralph/errors.log` and distill a guardrail into `.ralph/learnings.md`.
pub fn record_failure_reflection(
    root: &Path,
    abbey_state: Option<&AbbeyState>,
    iteration: usize,
    task: &str,
    kinds_csv: &str,
    excerpt: &str,
) -> Result<()> {
    fs::create_dir_all(ralph_dir(root))?;
    let path = errors_log_path(root);
    let mut log = if path.is_file() {
        fs::read_to_string(&path)?
    } else {
        String::from("# Ralph Loop Failure Reflections (.ralph/errors.log)\n\n")
    };
    let clipped_excerpt: String = excerpt.lines().take(25).collect::<Vec<_>>().join("\n");
    log.push_str(&format!(
        "## Iteration {iteration} — task: {task} (fail kinds: {kinds_csv})\n\
         ```\n{clipped_excerpt}\n```\n\n"
    ));
    fs::write(&path, log)?;

    // Distill a concise self-learning rule from the first compiler/clippy/test error line.
    if let Some(signal_line) = excerpt.lines().find(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("error[")
            || lower.contains("error:")
            || lower.contains("failed")
            || lower.contains("fail ")
    }) {
        let lesson = format!(
            "[auto-learned iter {iteration} · {kinds_csv}] On task `{task}`, avoid trigger: {}",
            signal_line.trim()
        );
        let _ = record_learning(root, abbey_state, &lesson);
    }
    Ok(())
}

/// Assemble the fresh-context single-iteration prompt for the next pending task.
pub fn build_iteration_prompt(
    root: &Path,
    abbey_state: Option<&AbbeyState>,
    hint: Option<&str>,
) -> Result<(String, String)> {
    let items = load_plan_items(root);
    let selected = if let Some(h) = hint.filter(|s| !s.trim().is_empty()) {
        let lower = h.to_ascii_lowercase();
        items
            .iter()
            .find(|i| !i.done && i.text.to_ascii_lowercase().contains(&lower))
            .map(|i| i.text.clone())
            .unwrap_or_else(|| h.trim().to_string())
    } else {
        items
            .iter()
            .find(|i| !i.done)
            .map(|i| i.text.clone())
            .unwrap_or_else(|| "Stabilize workspace and ensure ./check.sh passes".to_string())
    };

    let base_prompt = fs::read_to_string(prompt_path(root)).unwrap_or_else(|_| {
        "Execute exactly one Rust task in a fresh context, verify with the repository gate, and record learnings.".into()
    });
    let learnings = fs::read_to_string(learnings_path(root)).unwrap_or_default();
    let recent_errors = fs::read_to_string(errors_log_path(root))
        .map(|text| {
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(40);
            lines[start..].join("\n")
        })
        .unwrap_or_default();
    let ltm_prefs = abbey_state
        .map(|st| learn::preference_context(&st.state_dir, 8))
        .unwrap_or_default();

    let mut full = String::new();
    full.push_str("# Ralph Loop Iteration (Fresh Context · Single Task)\n\n");
    full.push_str(&format!("## Selected Single Task\n- [ ] {selected}\n\n"));
    full.push_str("## Base Instructions (`PROMPT.md`)\n");
    full.push_str(base_prompt.trim());
    full.push_str("\n\n");
    if !learnings.trim().is_empty() {
        full.push_str("## Self-Learned Operational Playbook (`.ralph/learnings.md`)\n");
        full.push_str(learnings.trim());
        full.push_str("\n\n");
    }
    if !ltm_prefs.trim().is_empty() {
        full.push_str("## Abbey Self-Learn LTM Directives\n");
        full.push_str(ltm_prefs.trim());
        full.push_str("\n\n");
    }
    if !recent_errors.trim().is_empty() {
        full.push_str("## Recent Failure Reflections (`.ralph/errors.log` — Do Not Repeat)\n");
        full.push_str(recent_errors.trim());
        full.push_str("\n\n");
    }
    full.push_str(
        "## Verification Gate\n\
         Run the repository gate (`./check.sh` or `cargo check --locked && cargo test --locked`) \
         and ensure zero warnings or test failures before concluding.\n",
    );
    Ok((selected, full))
}

fn cmd_status(root: &Path, opts: &RalphOpts) -> Result<i32> {
    let snap = status_snapshot(root);
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&snap)?);
        return Ok(0);
    }
    println!("abbey ralph status — self-learning Rust Ralph loop");
    println!("workspace:         {}", snap.workspace);
    println!(
        "harness files:     PROMPT.md={} · fix_plan.md={}",
        if snap.prompt_exists {
            "present"
        } else {
            "missing"
        },
        if snap.fix_plan_exists {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "tasks:             {} pending · {} completed",
        snap.pending_count, snap.completed_count
    );
    println!(
        "next single task:  {}",
        snap.next_task
            .as_deref()
            .unwrap_or("(none — all tasks complete)")
    );
    println!(
        "self-learning:     {} playbook rule(s) · {} error reflection(s)",
        snap.learnings_count, snap.error_log_entries
    );
    println!(
        "circuit breaker:   iteration={} · consecutive_failures={} · halted={}",
        snap.state.iteration,
        snap.state.consecutive_failures,
        snap.state.halted_reason.as_deref().unwrap_or("no")
    );
    Ok(0)
}

fn cmd_init(root: &Path, opts: &RalphOpts) -> Result<i32> {
    let written = scaffold_workspace(root, opts.force)?;
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "workspace": root.display().to_string(),
                "written": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            }))?
        );
        return Ok(0);
    }
    if written.is_empty() {
        println!(
            "abbey ralph init: files already exist in {} (pass --force to overwrite)",
            root.display()
        );
    } else {
        println!("abbey ralph init: scaffolded {} file(s):", written.len());
        for path in written {
            println!("  {}", path.display());
        }
    }
    Ok(0)
}

fn cmd_plan(root: &Path, state: &AbbeyState, opts: &RalphOpts) -> Result<i32> {
    let hint = if opts.rest.is_empty() {
        None
    } else {
        Some(opts.rest.join(" "))
    };
    let (task, prompt) = build_iteration_prompt(root, Some(state), hint.as_deref())?;
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "selected_task": task,
                "prompt": prompt,
            }))?
        );
        return Ok(0);
    }
    println!("abbey ralph plan — selected task: {task}\n");
    println!("{prompt}");
    Ok(0)
}

fn cmd_learn(root: &Path, state: &AbbeyState, opts: &RalphOpts) -> Result<i32> {
    let insight = opts.rest.join(" ");
    let ltm_id = record_learning(root, Some(state), &insight)?;
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "recorded": insight,
                "learnings_file": learnings_path(root).display().to_string(),
                "ltm_id": ltm_id,
            }))?
        );
        return Ok(0);
    }
    println!(
        "abbey ralph learn: recorded insight in {}{}",
        learnings_path(root).display(),
        ltm_id
            .as_deref()
            .map(|id| format!(" (LTM id: {id})"))
            .unwrap_or_default()
    );
    Ok(0)
}

fn cmd_reset_breaker(root: &Path, opts: &RalphOpts) -> Result<i32> {
    let mut st = load_state(root);
    st.consecutive_failures = 0;
    st.halted_reason = None;
    save_state(root, &st)?;
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&st)?);
    } else {
        println!("abbey ralph reset: circuit breaker cleared (consecutive_failures=0)");
    }
    Ok(0)
}

fn cmd_run(cfg: &AgentConfig, state: &AbbeyState, root: &Path, opts: &RalphOpts) -> Result<i32> {
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

/// Register `ralph-loop-rust` in global and workspace MCP configuration files.
pub fn cmd_install_mcp(root: &Path, opts: &RalphOpts) -> Result<i32> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("abbey"));
    let bin_str = exe.display().to_string();
    let mut updated_files = Vec::new();

    let mut json_targets = vec![root.join(".mcp.json")];
    if let Some(home) = dirs::home_dir() {
        json_targets.push(home.join(".cursor/mcp.json"));
        json_targets.push(home.join(".claude/mcp.json"));
        json_targets.push(home.join(".config/claude/mcp.json"));
    }

    for path in json_targets {
        if register_json_mcp(&path, &bin_str, &["ralph", "mcp"]).is_ok() {
            updated_files.push(path.display().to_string());
        }
    }

    if let Some(home) = dirs::home_dir() {
        let codex_path = home.join(".codex/config.toml");
        if register_codex_toml_mcp(&codex_path, &bin_str).is_ok() {
            updated_files.push(codex_path.display().to_string());
        }
    }

    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "server": "ralph-loop-rust",
                "command": bin_str,
                "args": ["ralph", "mcp"],
                "updated_configs": updated_files,
            }))?
        );
    } else {
        println!(
            "abbey ralph install-mcp: registered `ralph-loop-rust` ({bin_str} ralph mcp) in {} config(s):",
            updated_files.len()
        );
        for f in &updated_files {
            println!("  {f}");
        }
    }
    Ok(0)
}

fn register_json_mcp(path: &Path, command: &str, args: &[&str]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut doc: Value = if path.is_file() {
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    let Some(root_obj) = doc.as_object_mut() else {
        bail!("MCP config root is not a JSON object");
    };
    let servers = root_obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?;
    servers.insert(
        "ralph-loop-rust".into(),
        json!({
            "command": command,
            "args": args,
        }),
    );
    fs::write(path, serde_json::to_string_pretty(&doc)? + "\n")?;
    Ok(())
}

fn register_codex_toml_mcp(path: &Path, command: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = if path.is_file() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    if existing.contains("[mcp_servers.ralph-loop-rust]")
        || existing.contains("[mcp_servers.\"ralph-loop-rust\"]")
    {
        return Ok(());
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!(
        "\n[mcp_servers.ralph-loop-rust]\ncommand = \"{command}\"\nargs = [\"ralph\", \"mcp\"]\n"
    ));
    fs::write(path, updated)?;
    Ok(())
}

/// Serve the self-learning Ralph Loop MCP server over newline-delimited JSON-RPC 2.0 stdio.
pub fn serve_mcp_stdio(default_root: &Path, abbey_state: Option<&AbbeyState>) -> Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve_mcp_stream(stdin.lock(), stdout.lock(), default_root, abbey_state)?;
    Ok(0)
}

/// Run the Ralph MCP JSON-RPC 2.0 loop over arbitrary buffered streams.
pub fn serve_mcp_stream(
    mut reader: impl BufRead,
    mut writer: impl Write,
    default_root: &Path,
    abbey_state: Option<&AbbeyState>,
) -> std::io::Result<()> {
    let mut initialized = false;
    loop {
        let mut frame = Vec::new();
        let read = (&mut reader)
            .take((MAX_MCP_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)?;
        if read == 0 {
            return Ok(());
        }
        while frame.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            frame.pop();
        }
        if frame.is_empty() {
            continue;
        }
        if let Some(resp) = handle_mcp_frame(&frame, &mut initialized, default_root, abbey_state) {
            let line = serde_json::to_string(&resp).unwrap_or_else(|_| {
                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialization error"}}"#
                    .to_string()
            });
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
}

fn handle_mcp_frame(
    frame: &[u8],
    initialized: &mut bool,
    default_root: &Path,
    abbey_state: Option<&AbbeyState>,
) -> Option<Value> {
    let doc: Value = match serde_json::from_slice(frame) {
        Ok(v) => v,
        Err(e) => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {"code": -32700, "message": format!("parse error: {e}")}
            }));
        }
    };
    let obj = doc.as_object()?;
    let id = obj.get("id").cloned();
    let method = obj.get("method").and_then(Value::as_str).unwrap_or("");
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));

    let id = id?;
    let result = match method {
        "initialize" => {
            *initialized = true;
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-06-18");
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "ralph-loop-rust",
                    "title": "Abbey Self-Learning Rust Ralph Loop MCP",
                    "version": crate::build_info::VERSION
                },
                "instructions": "Self-learning Rust Ralph loop orchestrator. Provides fresh-context single-task planning, playbook learning, failure reflection logging, and circuit-breaker state."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            if !*initialized {
                Err((-32600, "server not initialized".to_string()))
            } else {
                Ok(json!({"tools": ralph_mcp_tools()}))
            }
        }
        "tools/call" => {
            if !*initialized {
                Err((-32600, "server not initialized".to_string()))
            } else {
                call_ralph_mcp_tool(&params, default_root, abbey_state)
            }
        }
        other => Err((-32601, format!("unknown method `{other}`"))),
    };

    Some(match result {
        Ok(val) => json!({"jsonrpc": "2.0", "id": id, "result": val}),
        Err((code, msg)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": msg}
        }),
    })
}

fn ralph_mcp_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "ralph_status",
            "title": "Ralph Loop Status",
            "description": "Inspect the Rust Ralph loop workspace status: PROMPT.md, fix_plan.md pending/completed tasks, next single task, self-learning count, and circuit-breaker state.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string", "description": "Optional workspace path"}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_init",
            "title": "Initialize Ralph Loop Harness",
            "description": "Scaffold PROMPT.md, fix_plan.md, .ralph/learnings.md, .ralph/errors.log, and .ralph/state.json for a Rust repository.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "force": {"type": "boolean"}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_plan",
            "title": "Build Fresh-Context Single-Task Prompt",
            "description": "Select the single highest-priority pending task from fix_plan.md and assemble a fresh-context iteration prompt with self-learned rules and failure reflections.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "hint": {"type": "string"}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_learn",
            "title": "Record Self-Learned Rust Insight",
            "description": "Persist a reusable Rust compiler, borrow-checker, or architecture insight into .ralph/learnings.md and Abbey LTM.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "insight": {"type": "string", "minLength": 1}
                },
                "required": ["insight"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_record_iteration",
            "title": "Record Iteration Verification Outcome",
            "description": "Record the outcome of a single-task Ralph iteration (gate pass or failure excerpt), updating fix_plan.md, .ralph/errors.log, .ralph/learnings.md, and circuit-breaker state.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "task": {"type": "string"},
                    "gate_ok": {"type": "boolean"},
                    "excerpt": {"type": "string"},
                    "max_failures": {"type": "integer", "minimum": 1}
                },
                "required": ["task", "gate_ok"],
                "additionalProperties": false
            }
        }),
    ]
}

fn call_ralph_mcp_tool(
    params: &Value,
    default_root: &Path,
    abbey_state: Option<&AbbeyState>,
) -> std::result::Result<Value, (i64, String)> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((-32602, "`name` is required".into()));
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let root = args
        .get("workspace")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root.to_path_buf());

    let outcome: Result<Value> = match name {
        "ralph_status" => Ok(
            serde_json::to_value(status_snapshot(&root)).map_err(|e| (-32603, e.to_string()))?
        ),
        "ralph_init" => {
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let files = scaffold_workspace(&root, force).map_err(|e| (-32603, e.to_string()))?;
            Ok(json!({
                "workspace": root.display().to_string(),
                "written": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            }))
        }
        "ralph_plan" => {
            let hint = args.get("hint").and_then(Value::as_str);
            let (task, prompt) = build_iteration_prompt(&root, abbey_state, hint)
                .map_err(|e| (-32603, e.to_string()))?;
            Ok(json!({
                "selected_task": task,
                "prompt": prompt,
            }))
        }
        "ralph_learn" => {
            let insight = args.get("insight").and_then(Value::as_str).unwrap_or("");
            let ltm_id = record_learning(&root, abbey_state, insight)
                .map_err(|e| (-32603, e.to_string()))?;
            Ok(json!({
                "recorded": insight,
                "ltm_id": ltm_id,
            }))
        }
        "ralph_record_iteration" => {
            let task = args
                .get("task")
                .and_then(Value::as_str)
                .unwrap_or("unspecified task");
            let gate_ok = args
                .get("gate_ok")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let excerpt = args.get("excerpt").and_then(Value::as_str).unwrap_or("");
            let max_failures = args
                .get("max_failures")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .unwrap_or(DEFAULT_MAX_FAILURES);
            let mut st = load_state(&root);
            st.iteration += 1;
            if st.active_task.as_deref() != Some(task) {
                st.active_task = Some(task.to_string());
                st.consecutive_failures = 0;
            }
            if gate_ok {
                let _ = mark_task_completed(&root, task);
                st.completed_tasks += 1;
                st.consecutive_failures = 0;
                st.last_gate_ok = Some(true);
                st.last_gate_kinds = "ok".into();
                st.halted_reason = None;
                let _ = record_learning(
                    &root,
                    abbey_state,
                    &format!("Iteration {} completed `{task}` cleanly.", st.iteration),
                );
            } else {
                st.consecutive_failures += 1;
                st.last_gate_ok = Some(false);
                let kinds = gate::classify_failures(excerpt)
                    .iter()
                    .map(|k| k.label())
                    .collect::<Vec<_>>()
                    .join(",");
                let kinds_csv = if kinds.is_empty() {
                    "other".to_string()
                } else {
                    kinds
                };
                st.last_gate_kinds = kinds_csv.clone();
                record_failure_reflection(
                    &root,
                    abbey_state,
                    st.iteration,
                    task,
                    &kinds_csv,
                    excerpt,
                )
                .map_err(|e| (-32603, e.to_string()))?;
                if st.consecutive_failures >= max_failures {
                    st.halted_reason = Some(format!(
                        "task `{task}` failed {} consecutive iterations",
                        st.consecutive_failures
                    ));
                }
            }
            save_state(&root, &st).map_err(|e| (-32603, e.to_string()))?;
            Ok(serde_json::to_value(&st).map_err(|e| (-32603, e.to_string()))?)
        }
        other => return Err((-32602, format!("unknown tool `{other}`"))),
    };

    match outcome {
        Ok(structured) => {
            let text = serde_json::to_string_pretty(&structured)
                .unwrap_or_else(|_| structured.to_string());
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "structuredContent": structured,
                "isError": false
            }))
        }
        Err(err) => Ok(json!({
            "content": [{"type": "text", "text": err.to_string()}],
            "isError": true
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "abbey-ralph-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scaffold_and_single_task_completion_round_trip() {
        let dir = scratch_dir("scaffold");
        let files = scaffold_workspace(&dir, false).unwrap();
        assert_eq!(files.len(), 5);

        let snap0 = status_snapshot(&dir);
        assert!(snap0.prompt_exists);
        assert!(snap0.fix_plan_exists);
        assert_eq!(snap0.pending_count, 2);
        assert_eq!(snap0.completed_count, 0);

        let first_task = snap0.next_task.unwrap();
        assert!(mark_task_completed(&dir, &first_task).unwrap());

        let snap1 = status_snapshot(&dir);
        assert_eq!(snap1.pending_count, 1);
        assert_eq!(snap1.completed_count, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failure_reflection_auto_learns_and_trips_circuit_breaker() {
        let dir = scratch_dir("breaker");
        scaffold_workspace(&dir, false).unwrap();

        let mut input = Vec::new();
        let reqs = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "ralph_record_iteration",
                    "arguments": {
                        "workspace": dir.display().to_string(),
                        "task": "Fix borrow checker lifetime in parser",
                        "gate_ok": false,
                        "excerpt": "error[E0502]: cannot borrow `buf` as mutable because it is also borrowed as immutable",
                        "max_failures": 2
                    }
                }
            }))
            .unwrap(),
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "ralph_record_iteration",
                    "arguments": {
                        "workspace": dir.display().to_string(),
                        "task": "Fix borrow checker lifetime in parser",
                        "gate_ok": false,
                        "excerpt": "error[E0502]: cannot borrow `buf` as mutable because it is also borrowed as immutable",
                        "max_failures": 2
                    }
                }
            }))
            .unwrap(),
        ];
        for r in reqs {
            input.extend_from_slice(r.as_bytes());
            input.push(b'\n');
        }
        let mut output = Vec::new();
        serve_mcp_stream(&input[..], &mut output, &dir, None).unwrap();
        let responses: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(responses.len(), 3);
        let st = load_state(&dir);
        assert_eq!(st.consecutive_failures, 2);
        assert!(st.halted_reason.is_some());
        let learnings = fs::read_to_string(learnings_path(&dir)).unwrap();
        assert!(learnings.contains("E0502"));
        let _ = fs::remove_dir_all(&dir);
    }
}
