//! Clap CLI — Grok Build / Codex / Claude Code parity surface.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Parser)]
#[command(
    name = "abbey",
    version = VERSION,
    about = "Abbey — agent CLI/TUI with Grok Build · Codex · Claude Code parity",
    long_about = "Abbey wraps cursor-agent with resilient chats and a ratatui TUI.\n\
Parity targets: Grok Build (session/TUI/worktree), Codex (exec/review/doctor/mcp),\n\
Claude Code (slash commands, plan/ask, init/diff/pr/commit/review).\n\
Run with no args to open the TUI. Type /help inside the TUI for slash commands."
)]
pub struct Cli {
    /// Model id or alias (fable, opus, grok, codex, …)
    #[arg(short = 'm', long = "model", global = true, env = "ABBEY_MODEL")]
    pub model: Option<String>,

    /// Execution mode
    #[arg(long = "mode", global = true, value_enum)]
    pub mode: Option<ExecMode>,

    /// Plan mode shorthand
    #[arg(long = "plan", global = true)]
    pub plan: bool,

    /// Ask mode shorthand
    #[arg(long = "ask", global = true)]
    pub ask: bool,

    /// Headless --print (Claude `-p` / Grok `--single`)
    #[arg(short = 'p', long = "print", global = true)]
    pub print: bool,

    /// Output format with --print
    #[arg(long = "output-format", global = true)]
    pub output_format: Option<String>,

    /// Force / yolo / always-approve (Codex bypass / Claude skip-permissions)
    #[arg(
        short = 'f',
        long = "force",
        alias = "yolo",
        visible_alias = "always-approve",
        global = true
    )]
    pub force: bool,

    /// Continue most recent chat (Grok/Claude `-c`)
    #[arg(short = 'c', long = "continue", global = true)]
    pub continue_session: bool,

    /// Isolated git worktree (Grok `--worktree`)
    #[arg(short = 'w', long = "worktree", global = true, num_args = 0..=1, default_missing_value = "")]
    pub worktree: Option<String>,

    /// Working directory / workspace root (Grok `--cwd`)
    #[arg(long = "cwd", visible_alias = "workspace", global = true)]
    pub workspace: Option<PathBuf>,

    /// Extra workspace root (Claude `--add-dir`)
    #[arg(long = "add-dir", global = true)]
    pub add_dirs: Vec<PathBuf>,

    /// Attach an image path for the agent to read (not encoded by Abbey)
    #[arg(long = "image", global = true, value_name = "PATH")]
    pub images: Vec<PathBuf>,

    /// Attach a video path for the agent to read (not encoded by Abbey)
    #[arg(long = "video", global = true, value_name = "PATH")]
    pub videos: Vec<PathBuf>,

    /// Attach image or video by extension (same as --image/--video)
    #[arg(long = "media", global = true, value_name = "PATH")]
    pub media: Vec<PathBuf>,

    /// Forward `--approve-mcps` so cursor-agent auto-approves MCP tool servers
    #[arg(long = "approve-mcps", global = true)]
    pub approve_mcps: bool,

    /// Thinking effort alias → `-m fable-thinking-<level>` (Cursor model id)
    #[arg(long = "thinking", global = true, value_name = "LEVEL")]
    pub thinking: Option<String>,

    /// Sandbox: enabled|disabled
    #[arg(long = "sandbox", global = true)]
    pub sandbox: Option<String>,

    /// Debug logging (Grok/Claude `--debug`)
    #[arg(long = "debug", global = true)]
    pub debug: bool,

    /// Max agent turns (Grok `--max-turns`)
    #[arg(long = "max-turns", global = true)]
    pub max_turns: Option<u32>,

    /// Force open TUI even when a prompt is given
    #[arg(long = "tui", global = true)]
    pub tui: bool,

    /// Skip TUI; run headless CLI path
    #[arg(long = "no-tui", global = true)]
    pub no_tui: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Prompt words (when no subcommand)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub prompt: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExecMode {
    Ask,
    Plan,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    Pwsh,
    Elvish,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Open the Abbey TUI (default with no args)
    Tui,
    /// Force a brand-new chat, then start
    New {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Resume current chat (Grok/Claude continue)
    #[command(visible_alias = "resume")]
    Continue {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// New chat that asks agent to continue prior context (Claude /fork)
    Fork {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Fix last failed command / piped error
    #[command(visible_alias = "fix")]
    PleaseFix {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Read-only Q&A mode (Claude ask)
    Ask {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Plan mode (Claude /plan, Grok plan)
    Plan {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Headless single-shot (Claude -p, Codex exec, Grok --single)
    #[command(visible_alias = "p", visible_alias = "e", visible_alias = "exec")]
    Print {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Show git diff (Claude /diff)
    Diff {
        #[arg(long, short)]
        staged: bool,
    },
    /// Review git diff (Codex review / Claude /review)
    Review {
        #[arg(long, short)]
        staged: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        note: Vec<String>,
    },
    /// Security-focused review of git diff (Claude /security-review)
    #[command(name = "security-review", visible_alias = "sec")]
    SecurityReview {
        #[arg(long, short)]
        staged: bool,
    },
    /// Conventional commit message for staged diff (Claude /commit)
    Commit,
    /// Draft PR title + body (Claude /pr)
    Pr,
    /// Scaffold AGENTS.md from a project scan (Claude /init)
    Init {
        /// Overwrite existing AGENTS.md
        #[arg(long, short = 'f')]
        force: bool,
        /// Print generated markdown without writing
        #[arg(long, short = 'p')]
        print: bool,
        /// Ask cursor-agent to refine the scaffold after the local scan
        #[arg(long)]
        agent: bool,
    },
    /// Create git branch
    Branch { name: String },
    /// Explain a path or topic
    Explain {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        question: Vec<String>,
    },
    /// Show or set default model
    Model { name: Option<String> },
    /// List model aliases
    #[command(visible_alias = "alias")]
    Aliases,
    /// Paths, model, chat, knobs (Codex doctor)
    #[command(visible_alias = "which", visible_alias = "info")]
    Doctor,
    /// Rich debug diagnostics
    Debug,
    /// Print active chat id
    #[command(visible_alias = "id")]
    ChatId,
    /// Drop saved chat id (Claude /clear)
    #[command(visible_alias = "reset", visible_alias = "rewind")]
    Clear {
        #[arg(long)]
        all: bool,
    },
    /// Trim local history log (Claude /compact)
    Compact {
        #[arg(default_value_t = 50)]
        keep: usize,
    },
    /// Recent chat log (Claude /tasks)
    #[command(visible_alias = "hist", visible_alias = "tasks")]
    History {
        #[arg(default_value_t = 20)]
        n: usize,
    },
    /// Chat id + history, or knowledge-store ops (`search`/`put`/…)
    Memory {
        #[command(subcommand)]
        cmd: Option<MemoryCmd>,
    },
    /// Show or set persona (abbey|aviva|abi|auto)
    Persona { name: Option<String> },
    /// Show or set worker role (max|gemma|auto)
    Role { name: Option<String> },
    /// Recent hybrid routing records
    Routes {
        #[arg(default_value_t = 10)]
        n: usize,
        /// Show only the stages of one correlated run (see `abbey hybrid-loop`)
        #[arg(long)]
        correlation: Option<String>,
    },
    /// Cross-platform OS control (dry-run default; execute --confirm)
    Os {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show OS-control allowlist (alias of `os allowlist`)
    #[command(visible_alias = "policy")]
    Allowlist,
    /// WDBX bridge: `abi wdbx …` passthrough (query gets --json)
    Wdbx {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Gemma interpret → Max implement, linked in the route log
    #[command(name = "hybrid-loop", visible_alias = "loop")]
    HybridLoop {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Generate an image via cursor-agent tools (not a local vision model)
    #[command(visible_alias = "gen-image")]
    Imagine {
        /// Output path (default: ./abbey-gen-<timestamp>.png)
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// Aspect ratio hint: 1:1 · 16:9 · 9:16 · 4:3 · 3:4 · auto
        #[arg(long)]
        aspect: Option<String>,
        /// Edit an existing image instead of generating from scratch
        #[arg(long = "edit", value_name = "PATH")]
        edit: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Generate media via cursor-agent tools (`image` | `video`)
    Generate {
        #[command(subcommand)]
        cmd: GenerateCmd,
    },
    /// Structured reasoning with a Cursor thinking model
    Reason {
        /// Thinking effort: low|medium|high|xhigh|max (default xhigh)
        #[arg(long = "thinking", value_name = "LEVEL")]
        thinking: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// High-quality macOS voice I/O (Premium/Enhanced TTS + on-device STT)
    Voice {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Speak text (alias for `abbey voice speak`)
    Speak {
        #[arg(short = 'v', long = "voice")]
        voice: Option<String>,
        #[arg(short = 'r', long = "rate")]
        rate: Option<u32>,
        #[arg(short = 'o', long = "out")]
        out: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Self-learn: correction|preference|routes|digest|export|review|stats (LoRA out of scope)
    Learn {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Alias for `learn review` (train_candidate curation)
    #[command(name = "learn-review", visible_alias = "train-review")]
    LearnReview {
        #[arg(default_value_t = 50)]
        n: usize,
    },
    /// Alias for `learn stats` (train_candidate counts)
    #[command(name = "learn-stats", visible_alias = "train-stats")]
    LearnStats,
    /// Parallel Max/Gemma/Aviva lanes (alias of `subagents run` defaults)
    Parallel {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Multi-subagent fan-out + local peer agents (not multi-node)
    #[command(visible_alias = "swarm", visible_alias = "distribute")]
    Subagents {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Goal-driven improve loop: ledger + local subagents + ./check.sh bar
    #[command(visible_alias = "smart-improve", visible_alias = "prod-ready")]
    Improve {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Discover peer agentic tools on PATH
    #[command(visible_alias = "tools")]
    Agents,
    /// List local skills (~/.grok|claude|cursor|abi-mega)
    Skills,
    /// Plugins inventory / passthrough (cursor-agent or abi)
    #[command(visible_alias = "plug")]
    Plugins {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show force/trust/sandbox knobs (Claude /permissions)
    Permissions,
    /// Show Abbey env config (`--init` scaffolds config.toml if absent)
    Config {
        /// Write an annotated default config.toml when none exists yet.
        #[arg(long)]
        init: bool,
    },
    /// Cost tracking unavailable note (Claude /cost)
    Cost,
    /// List slash commands
    #[command(name = "slash-help")]
    SlashHelp,
    /// Create empty chat and print id
    CreateChat,
    /// Cursor auth status
    #[command(visible_alias = "whoami")]
    Status,
    /// List models from account
    Models,
    /// Open agent chat picker
    Ls,
    /// Login (cursor-agent passthrough)
    Login,
    /// Logout (cursor-agent passthrough)
    Logout,
    /// MCP config inventory + cursor-agent mcp management
    Mcp {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Agent Client Protocol peer inventory / launch (not an ACP host)
    Acp {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Syntax-highlight a file or stdin (auto on -p/print fences when TTY)
    #[command(visible_alias = "hl")]
    Highlight {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Claims gate: Current / Proposed / Out of scope (embeddings, LoRA, multi-node…)
    #[command(visible_alias = "roadmap", visible_alias = "scope")]
    Claims {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Host OS/arch targets + thread/GPU/NPU/TPU inventory (detect-only)
    #[command(visible_alias = "targets")]
    Platform {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Alias for `platform compute` (threads + accelerator detect)
    Compute {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Vision/media honesty (path attach + agent gen; local weights OOS)
    Vision {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// CoT transcript viewer (not an Abbey CoT engine)
    #[command(name = "cot")]
    Cot {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Tool responsibility matrix (Abbey is not a tool runtime)
    Runtime {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Out-of-scope honesty index (lora|weights|accel|shell|host)
    #[command(name = "oos", visible_alias = "deferred")]
    Oos {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// LoRA honesty (learn curation Current; runners OOS)
    Lora {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Local weights honesty (bindings/fm Current; bundled weights OOS)
    Weights {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// NPU/TPU/GPU honesty (host detect Current; Abbey accel runtime OOS)
    #[command(visible_alias = "npu", visible_alias = "tpu")]
    Accel {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Unrestricted-shell honesty (allowlist Current; autonomous OS OOS)
    #[command(visible_alias = "unrestricted")]
    Shell {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// MCP/ACP host honesty (inventory Current; Abbey-as-host OOS)
    #[command(visible_alias = "tool-host")]
    Host {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Plugin management
    Plugin {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Generate shell completions (Grok completions / Codex completion)
    Completion { shell: Shell },
    /// Pass-through to cursor-agent
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Subcommand)]
pub enum GenerateCmd {
    /// Image generation / edit (same as `abbey imagine`)
    Image {
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        #[arg(long)]
        aspect: Option<String>,
        #[arg(long = "edit", value_name = "PATH")]
        edit: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
    /// Video generation (best-effort; requires an agent/MCP video tool)
    Video {
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum MemoryCmd {
    /// Chat id + recent history (default)
    Chat,
    /// Store a memory record
    Put {
        summary: String,
        #[arg(long, default_value = "stm")]
        retention: String,
        #[arg(long, default_value = "")]
        payload: String,
        #[arg(long, default_value = "abbey cli")]
        provenance: String,
        /// Subject tag — the 3-D map's topic axis groups by this (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Source category such as session, route, or import.
        #[arg(long, default_value = "session")]
        source: String,
        /// Source-specific stable reference.
        #[arg(long)]
        source_ref: Option<String>,
        /// Project root/name override (defaults to the current Git root).
        #[arg(long)]
        project: Option<String>,
        /// Record timestamp in RFC 3339 form.
        #[arg(long)]
        timestamp: Option<String>,
    },
    /// Get by id
    Get { id: String },
    /// Keyword search
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Promote retention layer
    Promote {
        id: String,
        #[arg(default_value = "ltm")]
        retention: String,
    },
    /// Mark a memory obsolete (never deletes — provenance is preserved)
    Invalidate { id: String },
    /// Replace a memory with a corrected one, marking the old obsolete
    Supersede {
        /// Record being replaced
        old_id: String,
        /// Summary of the replacement record
        summary: String,
        #[arg(long, default_value = "stm")]
        retention: String,
        #[arg(long, default_value = "")]
        payload: String,
        #[arg(long, default_value = "abbey cli supersede")]
        provenance: String,
        /// Subject tag for the replacement (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Reflection report (duplicates / low confidence / superseded)
    Reflect,
    /// 3-D memory map: topic × recency × consolidation
    Map {
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Only this retention layer
        #[arg(long)]
        layer: Option<String>,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Memories nearest another one in the 3-D map
    Near {
        id: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Lexical similarity search (feature-hash cosine — not learned semantics)
    ///
    /// Ranks by shared character n-grams, so it tolerates typos and word order
    /// where `search` (substring) misses. Pass `--id` to anchor on a record.
    Similar {
        /// Free-text query; omit when using --id
        #[arg(default_value = "")]
        query: String,
        /// Anchor on an existing record instead of free text
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Export layer as JSONL (train_candidate requires provenance on records)
    Export {
        #[arg(long, default_value = "ltm")]
        layer: String,
        #[command(flatten)]
        filter: MemoryFilterArgs,
    },
    /// Proposed: semantic embeddings (honest refuse)
    #[command(visible_alias = "embedding", visible_alias = "vector")]
    Embed {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _args: Vec<String>,
    },
}

/// Filters shared by memory query surfaces.
#[derive(Debug, Clone, Default, Args)]
pub struct MemoryFilterArgs {
    /// Subject tag.
    #[arg(long)]
    pub tag: Option<String>,
    /// Source category.
    #[arg(long)]
    pub source: Option<String>,
    /// Exact source reference.
    #[arg(long)]
    pub source_ref: Option<String>,
    /// Exact project identifier/root.
    #[arg(long)]
    pub project: Option<String>,
    /// Inclusive RFC 3339 lower timestamp bound.
    #[arg(long)]
    pub since: Option<String>,
    /// Inclusive RFC 3339 upper timestamp bound.
    #[arg(long)]
    pub until: Option<String>,
}
