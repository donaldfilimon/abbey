//! Resolve and invoke cursor-agent with Abbey defaults.

mod argv;

use anyhow::{Context, Result, bail};
use argv::{map_exec_err, warn_if_prompt_looks_like_flags};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

pub use argv::truncate_utf8_bytes;

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
}

impl AgentBackend {
    pub fn from_env() -> Self {
        match std::env::var("ABBEY_BACKEND")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "grok" | "grok-build" | "xai" => Self::Grok,
            "fm" | "apple" | "foundation" | "on-device" => Self::Fm,
            _ => Self::Cursor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            Self::Grok => "grok",
            Self::Fm => "fm",
        }
    }

    /// Whether this backend runs entirely on the local machine.
    pub fn is_on_device(self) -> bool {
        matches!(self, Self::Fm)
    }

    /// Account / session-list / MCP surface — `fm` has none of these.
    pub fn supports_account_surface(self) -> bool {
        !matches!(self, Self::Fm)
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
    let home = dirs::home_dir().context("HOME")?;
    let backend = AgentBackend::from_env();
    let key = match backend {
        AgentBackend::Grok => "grok",
        AgentBackend::Cursor => "cursor",
        AgentBackend::Fm => "fm",
    };
    let candidates = crate::host::agent_candidate_paths(key, &home);
    for c in &candidates {
        if !c.is_file() {
            continue;
        }
        if c.file_name().and_then(|s| s.to_str()) == Some("agent") {
            if let Ok(target) = fs_readlink(c) {
                // Skip Grok Build's `agent` when we want Cursor.
                if backend == AgentBackend::Cursor && !target.contains("cursor-agent") {
                    continue;
                }
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
        // `fm` has no server and no chat ids — Abbey mints one locally and
        // backs it with a transcript file.
        if self.backend == AgentBackend::Fm {
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

    /// Transcript file backing one Abbey chat id under the `fm` backend.
    pub fn fm_transcript_path(&self, chat_id: &str) -> Option<PathBuf> {
        let dir = self.transcript_dir.as_ref()?;
        Some(dir.join(format!("{chat_id}.transcript")))
    }

    /// Interactive hand-off: inherit stdio (full TUI agent session).
    pub fn run_interactive(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<ExitStatus> {
        warn_if_prompt_looks_like_flags(prompt_and_rest);
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
        warn_if_prompt_looks_like_flags(prompt_and_rest);
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
        if fresh {
            let id = cfg.create_chat()?;
            state.save_chat(&id)?;
            eprintln!("abbey: new chat {id}");
            return run_once(cfg, Some(&id), prompt_and_rest, capture_print);
        }
        return run_once(cfg, None, prompt_and_rest, capture_print);
    }

    let chat = if let Some(id) = state.read_chat() {
        id
    } else {
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

    // The retry exists because a server-side chat can go stale. `fm` has no
    // server: a non-zero exit is a real failure (bad argument, unavailable
    // model), and retrying would silently abandon the transcript and burn a new
    // chat on every invocation.
    if cfg.backend == AgentBackend::Fm {
        return Ok(code);
    }
    eprintln!("abbey: resume of {chat} failed (exit {code}); creating a new chat…");
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
    if capture_print {
        let (st, out, err) = cfg.run_capture(resume_id, prompt_and_rest)?;
        eprint!("{err}");
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
    fn fm_refuses_account_surface() {
        assert!(!AgentBackend::Fm.supports_account_surface());
        assert!(AgentBackend::Cursor.supports_account_surface());
        assert!(AgentBackend::Grok.supports_account_surface());
    }

    #[test]
    fn strip_ansi_removes_fm_colour_codes() {
        let coloured = "\u{1b}[38;2;255;107;128mError:\u{1b}[0m System model available";
        assert_eq!(strip_ansi(coloured), "Error: System model available");
    }
}
