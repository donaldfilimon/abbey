//! Resolve and invoke cursor-agent with Abbey defaults.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

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
}

impl AgentBackend {
    pub fn from_env() -> Self {
        match std::env::var("ABBEY_BACKEND")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "grok" | "grok-build" | "xai" => Self::Grok,
            _ => Self::Cursor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            Self::Grok => "grok",
        }
    }
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
    let candidates: Vec<PathBuf> = match backend {
        AgentBackend::Grok => vec![
            home.join(".grok/bin/grok"),
            home.join(".local/bin/grok"),
            PathBuf::from("/opt/homebrew/bin/grok"),
        ],
        AgentBackend::Cursor => vec![
            home.join(".local/bin/cursor-agent"),
            home.join(".local/bin/agent"),
        ],
    };
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
    }
    bail!(
        "{} not found (set ABBEY_AGENT or ABBEY_BACKEND=cursor|grok)",
        backend.label()
    );
}

fn fs_readlink(path: &Path) -> Result<String> {
    Ok(std::fs::read_link(path)?.to_string_lossy().into_owned())
}

/// First matching executable on `PATH`.
pub fn which_bin(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(bin);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

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

    pub fn build_args(&self, resume_id: Option<&str>, prompt_and_rest: &[String]) -> Vec<String> {
        let mut args = Vec::new();
        args.push("--model".into());
        args.push(self.model.clone());
        if self.auto_review {
            args.push("--auto-review".into());
        }
        if self.trust {
            args.push("--trust".into());
        }
        if self.force {
            args.push("--force".into());
        }
        if let Some(mode) = &self.mode {
            args.push("--mode".into());
            args.push(mode.clone());
        }
        if self.print {
            args.push("--print".into());
            if let Some(fmt) = &self.output_format {
                args.push("--output-format".into());
                args.push(fmt.clone());
            }
        }
        if let Some(wt) = &self.worktree {
            args.push("--worktree".into());
            if let Worktree::Named(name) = wt {
                args.push(name.clone());
            }
        }
        if let Some(ws) = &self.workspace {
            args.push("--workspace".into());
            args.push(ws.display().to_string());
        }
        for d in &self.add_dirs {
            args.push("--add-dir".into());
            args.push(d.display().to_string());
        }
        if let Some(sb) = &self.sandbox {
            args.push("--sandbox".into());
            args.push(sb.clone());
        }
        args.extend(self.extra_args.iter().cloned());
        if let Some(id) = resume_id {
            if !id.is_empty() {
                args.push("--resume".into());
                args.push(id.to_string());
            }
        }
        args.extend(prompt_and_rest.iter().cloned());
        args
    }

    /// Interactive hand-off: inherit stdio (full TUI agent session).
    pub fn run_interactive(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<ExitStatus> {
        let args = self.build_args(resume_id, prompt_and_rest);
        let status = Command::new(&self.agent_path)
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("exec {}", self.agent_path.display()))?;
        Ok(status)
    }

    /// Headless run; returns stdout text.
    pub fn run_capture(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<(ExitStatus, String, String)> {
        let mut cfg = self.clone();
        cfg.print = true;
        let args = cfg.build_args(resume_id, prompt_and_rest);
        let out = Command::new(&cfg.agent_path)
            .args(&args)
            .output()
            .with_context(|| format!("exec {}", cfg.agent_path.display()))?;
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
        let out = Command::new(&self.agent_path).arg("models").output()?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// Resilient session: resume or create; on failure create once and retry.
pub fn run_resilient(
    cfg: &AgentConfig,
    state: &crate::state::AbbeyState,
    fresh: bool,
    prompt_and_rest: &[String],
) -> Result<i32> {
    if fresh || cfg.no_resume {
        if fresh {
            let id = cfg.create_chat()?;
            state.save_chat(&id)?;
            eprintln!("abbey: new chat {id}");
            let st = cfg.run_interactive(Some(&id), prompt_and_rest)?;
            return Ok(st.code().unwrap_or(1));
        }
        let st = cfg.run_interactive(None, prompt_and_rest)?;
        return Ok(st.code().unwrap_or(1));
    }

    let chat = if let Some(id) = state.read_chat() {
        id
    } else {
        let id = cfg.create_chat()?;
        state.save_chat(&id)?;
        eprintln!("abbey: created chat {id}");
        id
    };

    let st = cfg.run_interactive(Some(&chat), prompt_and_rest)?;
    if st.success() {
        state.save_chat(&chat)?;
        return Ok(0);
    }

    let code = st.code().unwrap_or(1);
    eprintln!("abbey: resume of {chat} failed (exit {code}); creating a new chat…");
    let id = cfg.create_chat()?;
    state.save_chat(&id)?;
    eprintln!("abbey: new chat {id}");
    let st = cfg.run_interactive(Some(&id), prompt_and_rest)?;
    Ok(st.code().unwrap_or(1))
}
