//! Self-learning Rust Ralph loop orchestrator and stdio MCP server.
//!
//! A Ralph loop runs an agent in a fresh-context iteration cycle where each
//! iteration selects the single highest-priority pending task from `fix_plan.md`
//! (or the repository ledger), applies one focused change, verifies it against
//! the Rust gate (`./check.sh` or `ABBEY_CHECK_CMD`), and persists operational
//! learnings and failure reflections on disk (`.ralph/learnings.md`,
//! `.ralph/errors.log`, `.ralph/state.json`) plus Abbey's self-learn memory.

use crate::agent::AgentConfig;
use crate::learn;
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

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

mod mcp;
mod run;

pub use mcp::{cmd_install_mcp, serve_mcp_stdio, serve_mcp_stream};
use run::cmd_run;

#[cfg(test)]
mod tests;
