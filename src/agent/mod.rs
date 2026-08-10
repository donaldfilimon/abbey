//! Resolve and invoke cursor-agent with Abbey defaults.

mod argv;

use anyhow::{Context, Result, bail};
use argv::{map_exec_err, warn_if_prompt_looks_like_flags};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

pub(crate) use argv::looks_like_flags;
pub use argv::{abi_normalize_model, truncate_utf8_bytes};

/// Grok `--worktree` with optional name (`-w` vs `-w mybranch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Worktree {
    /// Pass `--worktree` with no name (agent picks).
    Auto,
    /// Pass `--worktree <name>`.
    Named(String),
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // mirrors cursor-agent / env knobs 1:1
pub struct AgentConfig {
    pub agent_path: PathBuf,
    pub model: String,
    pub auto_review: bool,
    pub trust: bool,
    pub force: bool,
    pub no_resume: bool,
    pub mode: Option<String>,
    pub print: bool,
    pub output_format: Option<String>,
    pub worktree: Option<Worktree>,
    pub workspace: Option<PathBuf>,
    pub add_dirs: Vec<PathBuf>,
    pub sandbox: Option<String>,
    pub extra_args: Vec<String>,
    /// Which executor grammar `build_args` speaks.
    pub backend: AgentBackend,
    /// Where `fm` conversation transcripts live (`fm` has no server-side chat
    /// ids, so Abbey maps its own chat id onto a transcript file).
    pub transcript_dir: Option<PathBuf>,
    /// Absolute media paths noted into the prompt (not pixel payloads).
    pub media_note: Option<String>,
    /// Prefer Gemma role when media was attached (cursor binding, not local vision).
    pub media_prefers_gemma: bool,
    /// Force capture+re-emit for `--print` even when stdout is not a TTY (CoT save).
    pub force_capture: bool,
    /// When set, captured stdout is also written here as a CoT transcript.
    pub cot_path: Option<PathBuf>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            agent_path: PathBuf::new(),
            model: "auto".into(),
            auto_review: env_flag("ABBEY_AUTO_REVIEW", true),
            trust: env_flag("ABBEY_TRUST", true),
            force: env_flag("ABBEY_FORCE", false),
            no_resume: env_flag("ABBEY_NO_RESUME", false),
            mode: None,
            print: false,
            output_format: None,
            worktree: None,
            workspace: None,
            add_dirs: Vec::new(),
            sandbox: None,
            extra_args: Vec::new(),
            backend: AgentBackend::from_env(),
            transcript_dir: None,
            media_note: None,
            media_prefers_gemma: false,
            force_capture: false,
            cot_path: None,
        }
    }
}

impl AgentConfig {
    /// Build the least-authority, one-shot configuration used by Abbey's
    /// provider-contract adapters. The caller owns the executable, backend,
    /// model, workspace, environment, and limits at startup; none are derived
    /// from a model request or ambient Abbey runtime flags.
    pub(crate) fn fixed_provider_recipe(
        agent_path: PathBuf,
        backend: AgentBackend,
        model: String,
    ) -> Self {
        Self {
            agent_path,
            model,
            auto_review: false,
            trust: false,
            force: false,
            no_resume: true,
            mode: None,
            print: true,
            output_format: None,
            worktree: None,
            workspace: None,
            add_dirs: Vec::new(),
            sandbox: None,
            extra_args: Vec::new(),
            backend,
            transcript_dir: None,
            media_note: None,
            media_prefers_gemma: false,
            force_capture: false,
            cot_path: None,
        }
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        _ => default,
    }
}

/// Backend preference: `cursor` (default), `grok`, or auto path via `ABBEY_AGENT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentBackend {
    Cursor,
    Grok,
    /// Apple Foundation Models CLI (`fm`) — on-device, no network, no third-party agent.
    Fm,
    /// The sibling ABI framework's CLI (`abi complete`) — one-shot completion:
    /// deterministic persona-template locally by default, Anthropic live
    /// transport when a `claude-*`/`live` model is requested (abi credentials).
    Abi,
}

impl AgentBackend {
    /// Parse a backend token from env or config. `None` for unknown/empty.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cursor" | "cursor-agent" => Some(Self::Cursor),
            "grok" | "grok-build" | "xai" => Some(Self::Grok),
            "fm" | "apple" | "foundation" | "on-device" => Some(Self::Fm),
            "abi" | "abi-cli" => Some(Self::Abi),
            _ => None,
        }
    }

    /// Backend precedence: `ABBEY_BACKEND` env > config `backend` key > cursor.
    ///
    /// A *set but unknown* env value still means cursor (long-standing
    /// behaviour) — it does not fall through to the config key, so a typo in
    /// the env var cannot silently activate a config-chosen backend.
    ///
    /// Resolved once per process and cached: neither the environment nor the
    /// config file changes mid-run, and `read_chat` (hence the TUI's per-frame
    /// draw) calls this — without the cache every frame re-read `config.toml`
    /// from disk. The TUI's Ctrl-B switch sets `AgentConfig::backend`
    /// directly and does not depend on this.
    pub fn from_env() -> Self {
        static RESOLVED: std::sync::OnceLock<AgentBackend> = std::sync::OnceLock::new();
        *RESOLVED.get_or_init(Self::resolve_from_env_and_config)
    }

    fn resolve_from_env_and_config() -> Self {
        if let Ok(v) = std::env::var("ABBEY_BACKEND")
            && !v.trim().is_empty()
        {
            return Self::parse(&v).unwrap_or(Self::Cursor);
        }
        let Some(configured) = crate::config::AbbeyConfig::load()
            .ok()
            .and_then(|c| c.backend)
        else {
            return Self::Cursor;
        };
        Self::parse(&configured).unwrap_or_else(|| {
            // Silently falling back would make a typo'd config key look like
            // it worked — the user asked for a specific executor and got
            // cursor-agent instead, with no way to tell.
            eprintln!(
                "abbey: config `backend = {configured:?}` is not one of \
                 cursor|grok|fm|abi — using cursor-agent"
            );
            Self::Cursor
        })
    }

    /// State subdirectory holding this backend's conversation transcripts.
    ///
    /// Shared by `apply_global_flags` and the TUI's backend switch so a
    /// mid-session switch cannot write one backend's transcripts into
    /// another's directory.
    pub fn transcript_subdir(self) -> &'static str {
        match self {
            Self::Abi => "abi",
            _ => "fm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            Self::Grok => "grok",
            Self::Fm => "fm",
            Self::Abi => "abi",
        }
    }

    /// Whether this backend runs entirely on the local machine.
    ///
    /// `abi` is deliberately excluded: its default transport is local, but a
    /// `claude-*`/`live` model selects a real network transport, so the label
    /// would be conditional — and a conditional "on-device" reads as a promise.
    pub fn is_on_device(self) -> bool {
        matches!(self, Self::Fm)
    }

    /// Account / session-list / MCP surface — `fm` and `abi` have none of these.
    pub fn supports_account_surface(self) -> bool {
        !matches!(self, Self::Fm | Self::Abi)
    }

    /// Server-side chat sessions (`create-chat` / `--resume <id>`).
    ///
    /// Both `fm` and `abi` back Abbey's chat id with a local transcript file
    /// instead: `fm` natively (`--resume`/`--save-transcript`), `abi`
    /// Abbey-side (the id names the transcript whose tail is replayed as a
    /// context prefix). Neither has a *server* session, which is what this
    /// predicate is about — it gates `create-chat` and the stale-chat retry.
    pub fn has_server_sessions(self) -> bool {
        matches!(self, Self::Cursor | Self::Grok)
    }

    /// The next backend in the fixed TUI switch order (wraps around).
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Cursor => Self::Grok,
            Self::Grok => Self::Fm,
            Self::Fm => Self::Abi,
            Self::Abi => Self::Cursor,
        }
    }
}

/// Strip ANSI SGR sequences — every `fm` subcommand colourizes its output, so
/// probes must de-colour before matching.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // Consume "[ ... <final byte in @..~>"
        if chars.next() != Some('[') {
            continue;
        }
        for c in chars.by_ref() {
            if ('\u{40}'..='\u{7e}').contains(&c) {
                break;
            }
        }
    }
    out
}

pub fn resolve_agent() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ABBEY_AGENT") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    resolve_agent_for(AgentBackend::from_env())
}

/// Resolve the executor binary for a specific backend.
///
/// Unlike [`resolve_agent`], this ignores the `ABBEY_AGENT` path override —
/// that override belongs to the env-chosen backend; honouring it while
/// *switching* backends would hand every backend the same binary.
pub fn resolve_agent_for(backend: AgentBackend) -> Result<PathBuf> {
    let home = dirs::home_dir().context("HOME")?;
    // `abi` resolution matches the WDBX bridge: config `abi_bin` /
    // ABBEY_ABI_BIN first, then known install paths, then PATH. Done *before*
    // the shared candidate scan so a present cursor-agent can never win.
    if backend == AgentBackend::Abi {
        let from_cfg = crate::config::AbbeyConfig::load()
            .ok()
            .and_then(|c| crate::config::resolve_abi_bin(&c));
        if let Some(path) = from_cfg {
            return Ok(path);
        }
    }
    let key = match backend {
        AgentBackend::Grok => "grok",
        AgentBackend::Cursor => "cursor",
        AgentBackend::Fm => "fm",
        AgentBackend::Abi => "abi",
    };
    let candidates = crate::host::agent_candidate_paths(key, &home);
    for c in &candidates {
        if !c.is_file() {
            continue;
        }
        if c.file_name().and_then(|s| s.to_str()) == Some("agent")
            && let Ok(target) = fs_readlink(c)
        {
            // Skip Grok Build's `agent` when we want Cursor.
            if backend == AgentBackend::Cursor && !target.contains("cursor-agent") {
                continue;
            }
        }
        return Ok(c.clone());
    }
    // Fallbacks on PATH
    match backend {
        AgentBackend::Cursor => {
            if let Some(path) = which_bin("cursor-agent") {
                return Ok(path);
            }
        }
        AgentBackend::Grok => {
            if let Some(path) = which_bin("grok") {
                return Ok(path);
            }
        }
        AgentBackend::Fm => {
            if let Some(path) = which_bin("fm") {
                return Ok(path);
            }
            bail!(
                "`fm` not found — the on-device backend needs the Apple Foundation Models \
                 CLI (macOS 26+). Unset ABBEY_BACKEND=fm to use cursor-agent."
            );
        }
        AgentBackend::Abi => {
            if let Some(path) = which_bin("abi") {
                return Ok(path);
            }
            bail!(
                "`abi` not found — ABBEY_BACKEND=abi needs a real `abi` binary (a shell \
                 alias will not do). Build it with `cargo build -p abi-cli` in ../abi, \
                 then set ABBEY_ABI_BIN or `abi_bin` in config.toml."
            );
        }
    }
    bail!(
        "{} not found (set ABBEY_AGENT or ABBEY_BACKEND=cursor|grok)",
        backend.label()
    );
}

fn fs_readlink(path: &Path) -> Result<String> {
    Ok(std::fs::read_link(path)?.to_string_lossy().into_owned())
}

pub use crate::host::{max_prompt_argv_bytes, which_bin};

impl AgentConfig {
    pub fn with_resolved_agent(mut self) -> Result<Self> {
        self.agent_path = resolve_agent()?;
        Ok(self)
    }

    pub fn agent_version(&self) -> String {
        Command::new(&self.agent_path)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into())
    }

    pub fn create_chat(&self) -> Result<String> {
        // `fm` and `abi` have no server and no chat ids — Abbey mints one
        // locally and backs it with a transcript file: `fm` passes that file
        // to the backend, `abi` replays its tail as a context prefix because
        // `abi complete` itself keeps no state between turns.
        if !self.backend.has_server_sessions() {
            let id = uuid::Uuid::new_v4().to_string();
            if let Some(dir) = &self.transcript_dir {
                std::fs::create_dir_all(dir)?;
            }
            return Ok(id);
        }
        let out = Command::new(&self.agent_path)
            .arg("create-chat")
            .output()
            .with_context(|| format!("exec {}", self.agent_path.display()))?;
        if !out.status.success() {
            bail!(
                "create-chat failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if id.is_empty() {
            bail!("create-chat returned empty id");
        }
        Ok(id)
    }

    /// Transcript file backing one Abbey chat id (`fm` natively via
    /// `--resume`/`--save-transcript`; `abi` Abbey-side).
    pub fn transcript_path(&self, chat_id: &str) -> Option<PathBuf> {
        let dir = self.transcript_dir.as_ref()?;
        Some(dir.join(format!("{chat_id}.transcript")))
    }

    /// Record one abi turn so the next run can carry bounded context.
    /// Best-effort: a failed write must not fail the run that produced it.
    pub fn append_abi_transcript(&self, chat_id: &str, prompt: &[String], output: &str) {
        let Some(path) = self.transcript_path(chat_id) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let entry = format!(
            "### user\n{}\n### abbey\n{}\n",
            prompt.join(" ").trim(),
            output.trim()
        );
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }

    /// Interactive hand-off: inherit stdio (full TUI agent session).
    pub fn run_interactive(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<ExitStatus> {
        // The abi grammar passes the prompt after a real `--` separator, so a
        // leading-dash prompt is text there, not options — no warning needed.
        if self.backend != AgentBackend::Abi {
            warn_if_prompt_looks_like_flags(prompt_and_rest);
        }
        let args = self.build_args(resume_id, prompt_and_rest);
        let status = Command::new(&self.agent_path)
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| map_exec_err(e, &self.agent_path))?;
        Ok(status)
    }

    /// Headless run; returns stdout text.
    pub fn run_capture(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<(ExitStatus, String, String)> {
        if self.backend != AgentBackend::Abi {
            warn_if_prompt_looks_like_flags(prompt_and_rest);
        }
        let mut cfg = self.clone();
        cfg.print = true;
        let args = cfg.build_args(resume_id, prompt_and_rest);
        let out = Command::new(&cfg.agent_path)
            .args(&args)
            .output()
            .map_err(|e| map_exec_err(e, &cfg.agent_path))?;
        Ok((
            out.status,
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }

    pub fn passthrough(&self, args: &[String]) -> Result<ExitStatus> {
        let status = Command::new(&self.agent_path)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        Ok(status)
    }

    pub fn list_models_text(&self) -> Result<String> {
        if self.backend == AgentBackend::Fm {
            return Ok("system  on-device Apple Foundation Model\npcc     Apple Foundation Model on Private Cloud Compute\n".into());
        }
        if self.backend == AgentBackend::Abi {
            return Ok(
                "local     deterministic persona-template completion (abi-ai, no network)\n\
                 claude-*  Anthropic via `abi complete --live` (needs abi credentials)\n\
                 live      Anthropic live transport with abi's default model\n"
                    .into(),
            );
        }
        let out = Command::new(&self.agent_path).arg("models").output()?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Ask `fm` what it can actually serve.
    ///
    /// `fm available` prints an error about Private Cloud Compute *and*
    /// "System model available" in the same run, and exits non-zero — so this
    /// parses de-coloured stdout rather than trusting the exit code.
    pub fn fm_availability(&self) -> String {
        let Ok(out) = Command::new(&self.agent_path).arg("available").output() else {
            return "fm: not runnable".into();
        };
        let text = strip_ansi(&format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
        let lower = text.to_ascii_lowercase();
        let system = lower.contains("system model available");
        let pcc = lower.contains("private cloud compute") && !lower.contains("not available");
        format!(
            "on-device system model: {} · private cloud compute: {}",
            if system { "available" } else { "unavailable" },
            if pcc { "available" } else { "unavailable" },
        )
    }
}

/// Resilient session: resume or create; on failure create once and retry.
pub fn run_resilient(
    cfg: &AgentConfig,
    state: &crate::state::AbbeyState,
    fresh: bool,
    prompt_and_rest: &[String],
) -> Result<i32> {
    // Headless `--print` (or forced CoT capture): capture then re-emit.
    // Inherited stdio stays for interactive sessions and JSON.
    let capture_print = cfg.print
        && cfg.output_format.is_none()
        && (cfg.force_capture || crate::highlight::enabled() || cfg.cot_path.is_some());

    if fresh || cfg.no_resume {
        state.ensure_conversation_ready(cfg.backend)?;
        if fresh {
            let id = cfg.create_chat()?;
            state.save_chat(&id)?;
            eprintln!("abbey: new chat {id}");
            return run_once(cfg, Some(&id), prompt_and_rest, capture_print);
        }
        return run_once(cfg, None, prompt_and_rest, capture_print);
    }

    let chat = if let Some(id) = state.resolve_chat_for(cfg.backend)? {
        id
    } else {
        state.ensure_conversation_ready(cfg.backend)?;
        let id = cfg.create_chat()?;
        state.save_chat(&id)?;
        eprintln!("abbey: created chat {id}");
        id
    };

    let code = run_once(cfg, Some(&chat), prompt_and_rest, capture_print)?;
    if code == 0 {
        state.save_chat(&chat)?;
        return Ok(0);
    }

    // The retry exists because a server-side chat can go stale. `fm` and `abi`
    // have no server: a non-zero exit is a real failure (bad argument,
    // unavailable model, missing credentials), and retrying would silently
    // abandon the transcript / burn a new chat on every invocation.
    if !cfg.backend.has_server_sessions() {
        return Ok(code);
    }
    eprintln!("abbey: resume of {chat} failed (exit {code}); creating a new chat…");
    state.ensure_conversation_ready(cfg.backend)?;
    let id = cfg.create_chat()?;
    eprintln!("abbey: new chat {id}");
    let retry_code = run_once(cfg, Some(&id), prompt_and_rest, capture_print)?;

    // Persist the new chat only if it actually worked. When the retry fails too
    // the cause is not a stale chat — it is something a new chat cannot fix
    // (account/plan rejection, a bad flag, the agent being down). Saving the
    // fresh id anyway would make the *next* invocation resume a chat that has
    // never succeeded, fail, and mint another: one orphan chat per command,
    // compounding, with the user's real transcript abandoned. Same reasoning as
    // the `fm` early-return above.
    state.save_chat(chat_to_persist(&chat, &id, retry_code))?;
    if retry_code != 0 {
        eprintln!("abbey: new chat also failed (exit {retry_code}); keeping {chat}");
    }
    Ok(retry_code)
}

/// Which chat id stays in state after a resume failure forced a retry.
///
/// Keeping the freshly created chat is only right when it worked; otherwise the
/// failure is not staleness and the new id would poison the next invocation.
fn chat_to_persist<'a>(original: &'a str, retried: &'a str, retry_code: i32) -> &'a str {
    if retry_code == 0 { retried } else { original }
}

fn run_once(
    cfg: &AgentConfig,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
    capture_print: bool,
) -> Result<i32> {
    // `abi complete` is one-shot and non-interactive — always capture, both
    // for clean emit and so the turn can be recorded for Abbey-side
    // continuity (the context prefix the next build_args_abi reads).
    if capture_print || cfg.backend == AgentBackend::Abi {
        let (st, out, err) = cfg.run_capture(resume_id, prompt_and_rest)?;
        eprint!("{err}");
        if cfg.backend == AgentBackend::Abi
            && st.success()
            && let Some(id) = resume_id.filter(|i| !i.is_empty())
        {
            cfg.append_abi_transcript(id, prompt_and_rest, &out);
        }
        if let Some(path) = &cfg.cot_path {
            if let Err(e) = crate::surfaces::save_cot(path, &out) {
                eprintln!("abbey: cot save failed: {e:#}");
            } else {
                eprintln!("abbey: cot transcript → {}", path.display());
            }
        }
        if cfg.cot_path.is_some() {
            let _ = crate::output::print(crate::surfaces::render_cot(&out));
        } else {
            crate::highlight::emit_agent_stdout(&out);
        }
        return Ok(st.code().unwrap_or(1));
    }
    let st = cfg.run_interactive(resume_id, prompt_and_rest)?;
    Ok(st.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_retry_keeps_the_original_chat() {
        // A successful retry means the old chat really was stale — adopt the new one.
        assert_eq!(chat_to_persist("old", "new", 0), "new");
        // A failed retry means the cause was not staleness (plan/auth rejection,
        // bad flag, agent down). Adopting "new" would make the next invocation
        // resume a chat that has never worked and mint yet another.
        assert_eq!(chat_to_persist("old", "new", 1), "old");
        assert_eq!(chat_to_persist("old", "new", 2), "old");
    }

    #[test]
    fn fm_and_abi_refuse_account_surface() {
        assert!(!AgentBackend::Fm.supports_account_surface());
        assert!(!AgentBackend::Abi.supports_account_surface());
        assert!(AgentBackend::Cursor.supports_account_surface());
        assert!(AgentBackend::Grok.supports_account_surface());
    }

    #[test]
    fn only_server_backends_get_the_stale_chat_retry() {
        assert!(AgentBackend::Cursor.has_server_sessions());
        assert!(AgentBackend::Grok.has_server_sessions());
        assert!(!AgentBackend::Fm.has_server_sessions());
        assert!(!AgentBackend::Abi.has_server_sessions());
    }

    /// The `CURSOR_AGENT_CHAT_ID` guard in `AbbeyState::read_chat_for` keys off
    /// this predicate. It once keyed off the process-cached `from_env()`
    /// instead, so a TUI session that *started* on cursor kept adopting the
    /// cursor chat id after Ctrl-B switched to `abi`/`fm` — the transcript
    /// hijack that guard exists to prevent. Passing the live backend is what
    /// makes the switch safe, in both directions.
    #[test]
    fn cursor_chat_env_is_honoured_per_backend_not_per_process() {
        for switched_to in [AgentBackend::Fm, AgentBackend::Abi] {
            assert!(
                !switched_to.has_server_sessions(),
                "{switched_to:?} must not adopt CURSOR_AGENT_CHAT_ID"
            );
        }
        for switched_to in [AgentBackend::Cursor, AgentBackend::Grok] {
            assert!(
                switched_to.has_server_sessions(),
                "{switched_to:?} must still adopt a real cursor session id"
            );
        }
    }

    #[test]
    fn transcript_subdir_separates_abi_from_fm() {
        assert_eq!(AgentBackend::Abi.transcript_subdir(), "abi");
        assert_eq!(AgentBackend::Fm.transcript_subdir(), "fm");
        // A mid-session switch must not land abi turns in fm's directory.
        assert_ne!(
            AgentBackend::Abi.transcript_subdir(),
            AgentBackend::Fm.transcript_subdir()
        );
    }

    #[test]
    fn backend_cycle_visits_all_and_wraps() {
        let mut b = AgentBackend::Cursor;
        let mut seen = Vec::new();
        for _ in 0..4 {
            b = b.cycle_next();
            seen.push(b);
        }
        assert_eq!(b, AgentBackend::Cursor, "cycle must wrap to the start");
        for expect in [
            AgentBackend::Grok,
            AgentBackend::Fm,
            AgentBackend::Abi,
            AgentBackend::Cursor,
        ] {
            assert!(seen.contains(&expect), "{expect:?} missing from cycle");
        }
    }

    #[test]
    fn strip_ansi_removes_fm_colour_codes() {
        let coloured = "\u{1b}[38;2;255;107;128mError:\u{1b}[0m System model available";
        assert_eq!(strip_ansi(coloured), "Error: System model available");
    }
}
