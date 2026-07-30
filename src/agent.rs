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
    /// Which executor grammar `build_args` speaks.
    pub backend: AgentBackend,
    /// Where `fm` conversation transcripts live (`fm` has no server-side chat
    /// ids, so Abbey maps its own chat id onto a transcript file).
    pub transcript_dir: Option<PathBuf>,
    /// Absolute media paths noted into the prompt (not pixel payloads).
    pub media_note: Option<String>,
    /// Prefer Gemma role when media was attached (cursor binding, not local vision).
    pub media_prefers_gemma: bool,
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

/// Map an Abbey model/alias onto `fm`'s two-model vocabulary (`system` | `pcc`).
///
/// Exact aliases only — substring matching (`cloud`, `private`) would mis-route
/// ordinary cursor-agent ids. Under `fm` the Max/Gemma distinction is carried by
/// the prompt alone, not by the model.
pub fn fm_model(requested: &str) -> &'static str {
    match requested.trim().to_ascii_lowercase().as_str() {
        "pcc" | "private-cloud-compute" | "private_cloud_compute" => "pcc",
        _ => "system",
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
        AgentBackend::Fm => vec![PathBuf::from("/usr/bin/fm")],
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

/// Soft ceiling for a single prompt argv string.
///
/// macOS `ARG_MAX` is typically 256KiB–1MiB and includes the environment.
/// Huge `please-fix` captures (or pasted logs) as one trailing argv used to
/// yield `Argument list too long (os error 7)` and abort the hand-off.
pub const MAX_PROMPT_ARGV_BYTES: usize = 96 * 1024;

/// Truncate `s` on a UTF-8 boundary so its byte length is ≤ `max_bytes`.
pub fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let marker = format!(
        "\n\n… [truncated for OS argv limit; kept {max_bytes} of {} bytes]",
        s.len()
    );
    let budget = max_bytes.saturating_sub(marker.len());
    let mut end = budget.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str(&marker);
    if out.len() > max_bytes {
        let mut e = max_bytes;
        while e > 0 && !out.is_char_boundary(e) {
            e -= 1;
        }
        out.truncate(e);
    }
    out
}

/// Clamp every trailing prompt string so the child argv stays under the OS limit.
pub fn clamp_prompt_args(prompt_and_rest: &[String]) -> Vec<String> {
    prompt_and_rest
        .iter()
        .map(|s| truncate_utf8_bytes(s, MAX_PROMPT_ARGV_BYTES))
        .collect()
}

fn map_exec_err(err: std::io::Error, agent_path: &Path) -> anyhow::Error {
    // E2BIG — Darwin/Linux typically use errno 7.
    if err.raw_os_error() == Some(7) {
        return anyhow::anyhow!(
            "exec {}: Argument list too long (os error 7).\n\
             Abbey clamps prompts to {MAX_PROMPT_ARGV_BYTES} bytes; if this still \
             fires, shrink CURSOR_AGENT_COMPLETED_PATH / please-fix input or unset \
             oversized environment variables.",
            agent_path.display()
        );
    }
    anyhow::Error::new(err).context(format!("exec {}", agent_path.display()))
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

    /// `fm respond` argv. Deliberately built from scratch rather than filtered
    /// from the cursor grammar: `fm` shares none of those flags, and a single
    /// leaked `--force`/`--sandbox` would break the backend the first time a
    /// user passed one.
    fn build_args_fm(&self, resume_id: Option<&str>, prompt_and_rest: &[String]) -> Vec<String> {
        let prompts = clamp_prompt_args(prompt_and_rest);
        let mut args = vec![
            "respond".to_string(),
            "--model".into(),
            fm_model(&self.model).into(),
        ];
        if let Some(mode) = &self.mode {
            // Abbey's ask/plan modes have no `fm` equivalent; express them as
            // instructions so the behaviour survives the backend switch.
            args.push("--instructions".into());
            args.push(match mode.as_str() {
                "ask" => "Answer the question. Do not modify files.".into(),
                "plan" => "Produce a plan only. Do not write the implementation.".into(),
                other => format!("Mode: {other}."),
            });
        }
        if self.print {
            // Streaming interleaves partial output; capture wants one clean body.
            args.push("--no-stream".into());
        }
        if let Some(id) = resume_id.filter(|i| !i.is_empty()) {
            if let Some(path) = self.fm_transcript_path(id) {
                if path.is_file() {
                    args.push("--resume".into());
                    args.push(path.display().to_string());
                }
                args.push("--save-transcript".into());
                args.push(path.display().to_string());
            }
        }
        args.extend(prompts);
        args
    }

    pub fn build_args(&self, resume_id: Option<&str>, prompt_and_rest: &[String]) -> Vec<String> {
        if self.backend == AgentBackend::Fm {
            return self.build_args_fm(resume_id, prompt_and_rest);
        }
        let prompts = clamp_prompt_args(prompt_and_rest);
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
        args.extend(prompts);
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
            .map_err(|e| map_exec_err(e, &self.agent_path))?;
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
    // Headless `--print` without a structured format: capture then colourise
    // fenced code. Inherited stdio stays for interactive sessions and JSON.
    let highlight_print = cfg.print && cfg.output_format.is_none() && crate::highlight::enabled();

    if fresh || cfg.no_resume {
        if fresh {
            let id = cfg.create_chat()?;
            state.save_chat(&id)?;
            eprintln!("abbey: new chat {id}");
            return run_once(cfg, Some(&id), prompt_and_rest, highlight_print);
        }
        return run_once(cfg, None, prompt_and_rest, highlight_print);
    }

    let chat = if let Some(id) = state.read_chat() {
        id
    } else {
        let id = cfg.create_chat()?;
        state.save_chat(&id)?;
        eprintln!("abbey: created chat {id}");
        id
    };

    let code = run_once(cfg, Some(&chat), prompt_and_rest, highlight_print)?;
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
    state.save_chat(&id)?;
    eprintln!("abbey: new chat {id}");
    run_once(cfg, Some(&id), prompt_and_rest, highlight_print)
}

fn run_once(
    cfg: &AgentConfig,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
    highlight_print: bool,
) -> Result<i32> {
    if highlight_print {
        let (st, out, err) = cfg.run_capture(resume_id, prompt_and_rest)?;
        eprint!("{err}");
        crate::highlight::emit_agent_stdout(&out);
        return Ok(st.code().unwrap_or(1));
    }
    let st = cfg.run_interactive(resume_id, prompt_and_rest)?;
    Ok(st.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config with every cursor-agent knob turned on — the `fm` builder must
    /// still emit only flags `fm respond` actually accepts.
    fn maximal_cursor_config() -> AgentConfig {
        AgentConfig {
            agent_path: PathBuf::from("/usr/bin/fm"),
            model: "claude-fable-5-thinking-high".into(),
            auto_review: true,
            trust: true,
            force: true,
            no_resume: true,
            mode: Some("plan".into()),
            print: true,
            output_format: Some("json".into()),
            worktree: Some(Worktree::Named("wt".into())),
            workspace: Some(PathBuf::from("/ws")),
            add_dirs: vec![PathBuf::from("/extra")],
            sandbox: Some("enabled".into()),
            extra_args: vec!["--debug".into(), "--max-turns".into(), "7".into()],
            backend: AgentBackend::Fm,
            transcript_dir: None,
            media_note: None,
            media_prefers_gemma: false,
        }
    }

    #[test]
    fn fm_argv_never_leaks_cursor_flags() {
        let argv = maximal_cursor_config().build_args(None, &["hello".into()]);
        // Every flag `fm respond` knows about.
        let allowed = [
            "respond",
            "--model",
            "system",
            "pcc",
            "--instructions",
            "--no-stream",
            "--resume",
            "--save-transcript",
            "hello",
        ];
        for a in &argv {
            if a.starts_with("--") {
                assert!(
                    allowed.contains(&a.as_str()),
                    "leaked non-fm flag {a:?} into argv {argv:?}"
                );
            }
        }
        for banned in [
            "--auto-review",
            "--trust",
            "--force",
            "--mode",
            "--worktree",
            "--workspace",
            "--add-dir",
            "--sandbox",
            "--output-format",
            "--print",
            "--debug",
            "--max-turns",
        ] {
            assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
        }
    }

    #[test]
    fn fm_collapses_any_cursor_model_id_to_system() {
        assert_eq!(fm_model("claude-fable-5-thinking-high"), "system");
        assert_eq!(fm_model("composer-2.5"), "system");
        assert_eq!(fm_model("auto"), "system");
        assert_eq!(fm_model("pcc"), "pcc");
        assert_eq!(fm_model("private-cloud-compute"), "pcc");
        assert_eq!(fm_model("private_cloud_compute"), "pcc");
        // Substrings must not hijack unrelated aliases.
        assert_eq!(fm_model("Private Cloud Compute"), "system");
        assert_eq!(fm_model("my-cloud-model"), "system");
    }

    #[test]
    fn fm_refuses_account_surface() {
        assert!(!AgentBackend::Fm.supports_account_surface());
        assert!(AgentBackend::Cursor.supports_account_surface());
        assert!(AgentBackend::Grok.supports_account_surface());
    }

    #[test]
    fn fm_mode_becomes_instructions() {
        let mut cfg = maximal_cursor_config();
        cfg.mode = Some("ask".into());
        let argv = cfg.build_args(None, &["q".into()]);
        let i = argv.iter().position(|a| a == "--instructions").unwrap();
        assert!(argv[i + 1].contains("Do not modify files"));
    }

    #[test]
    fn fm_resume_maps_a_chat_id_onto_a_transcript_file() {
        let dir = std::env::temp_dir().join(format!("abbey-fm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = maximal_cursor_config();
        cfg.transcript_dir = Some(dir.clone());

        // No transcript yet: save but do not resume from a file that isn't there.
        let argv = cfg.build_args(Some("chat-1"), &["hi".into()]);
        assert!(!argv.contains(&"--resume".to_string()));
        let save = argv.iter().position(|a| a == "--save-transcript").unwrap();
        assert!(argv[save + 1].ends_with("chat-1.transcript"));

        // Once it exists, resume from it.
        std::fs::write(dir.join("chat-1.transcript"), "{}").unwrap();
        let argv = cfg.build_args(Some("chat-1"), &["hi".into()]);
        let r = argv.iter().position(|a| a == "--resume").unwrap();
        assert!(argv[r + 1].ends_with("chat-1.transcript"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursor_backend_argv_is_unchanged_by_the_fm_split() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Cursor;
        let argv = cfg.build_args(Some("abc"), &["hi".into()]);
        for expected in [
            "--auto-review",
            "--trust",
            "--force",
            "--sandbox",
            "--resume",
        ] {
            assert!(argv.contains(&expected.to_string()), "{expected} missing");
        }
    }

    #[test]
    fn strip_ansi_removes_fm_colour_codes() {
        let coloured = "\u{1b}[38;2;255;107;128mError:\u{1b}[0m System model available";
        assert_eq!(strip_ansi(coloured), "Error: System model available");
    }

    #[test]
    fn truncate_utf8_bytes_respects_char_boundaries_and_cap() {
        let s = "a".repeat(MAX_PROMPT_ARGV_BYTES + 50_000);
        let out = truncate_utf8_bytes(&s, MAX_PROMPT_ARGV_BYTES);
        assert!(out.len() <= MAX_PROMPT_ARGV_BYTES);
        assert!(out.contains("truncated for OS argv limit"));
    }

    #[test]
    fn clamp_prompt_args_caps_each_trailing_string() {
        let huge = "x".repeat(MAX_PROMPT_ARGV_BYTES + 10_000);
        let argv = clamp_prompt_args(&[huge, "ok".into()]);
        assert!(argv[0].len() <= MAX_PROMPT_ARGV_BYTES);
        assert_eq!(argv[1], "ok");
    }

    #[test]
    fn build_args_clamps_a_please_fix_sized_prompt() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Cursor;
        let huge = "y".repeat(MAX_PROMPT_ARGV_BYTES + 80_000);
        let argv = cfg.build_args(None, &[huge]);
        let last = argv.last().expect("prompt");
        assert!(
            last.len() <= MAX_PROMPT_ARGV_BYTES,
            "prompt argv still too long: {}",
            last.len()
        );
    }
}
