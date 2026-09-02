//! Resolve and invoke the selected executor with Abbey defaults.

mod argv;
#[cfg(not(unix))]
mod capture;

use anyhow::{Context, Result, bail};
use argv::{map_exec_err, warn_if_prompt_looks_like_flags};
use fs4::fs_std::FileExt as _;
use std::fs::{File, OpenOptions};
use std::io::{Seek as _, Write as _};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use crate::runtime::supervisor::{
    ProcessSpec, SupervisorLimits, SupervisorOutcome, run_with_checkpoint,
};

pub(crate) use argv::looks_like_flags;
pub use argv::{abi_normalize_model, ollama_normalize_model, truncate_utf8_bytes};

const MAX_LOCAL_TRANSCRIPT_BYTES: u64 = 1024 * 1024;
const MAX_TRANSCRIPT_PROMPT_BYTES: usize = 16 * 1024;
const MAX_TRANSCRIPT_OUTPUT_BYTES: usize = 48 * 1024;
const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

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

mod backend;

pub(crate) use backend::ollama_lists_model;
pub use backend::{AgentBackend, resolve_agent, resolve_agent_for, strip_ansi};

pub use crate::host::{max_prompt_argv_bytes, which_bin};

impl AgentConfig {
    pub fn with_resolved_agent(mut self) -> Result<Self> {
        self.agent_path = resolve_agent()?;
        Ok(self)
    }

    /// The executor binary to spawn: the startup-resolved path when one was
    /// found, otherwise resolved now for the live backend. Late resolution is
    /// what lets every non-generation verb run on a machine with no executor
    /// installed — only a spawn actually requires one.
    fn exec_path(&self) -> Result<PathBuf> {
        if !self.agent_path.as_os_str().is_empty() {
            return Ok(self.agent_path.clone());
        }
        // `ABBEY_AGENT` belongs to the env-chosen backend; a live backend
        // switched away from it (TUI Ctrl-B) resolves its own binary.
        if self.backend == AgentBackend::from_env() {
            resolve_agent()
        } else {
            resolve_agent_for(self.backend)
        }
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
        // `fm`, `abi`, and `ollama` have no server and no chat ids — Abbey
        // mints one locally and backs it with a transcript file: `fm` passes
        // that file to the backend; `abi`/`ollama` replay its tail as a
        // context prefix because those CLIs keep no state between turns.
        if !self.backend.has_server_sessions() {
            let id = uuid::Uuid::new_v4().to_string();
            if let Some(dir) = &self.transcript_dir {
                std::fs::create_dir_all(dir)?;
            }
            return Ok(id);
        }
        let agent = self.exec_path()?;
        let out = Command::new(&agent)
            .arg("create-chat")
            .output()
            .with_context(|| format!("exec {}", agent.display()))?;
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

    /// Record that Claude accepted this session id. Claude owns the actual
    /// transcript; this presence-only marker selects `--resume` on later turns.
    pub fn touch_claude_session_marker(&self, chat_id: &str) {
        let Some(path) = self.transcript_path(chat_id) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, "claude session established\n");
    }

    /// Record one local one-shot turn so the next run can carry bounded context.
    /// Best-effort: a failed write must not fail the run that produced it.
    pub fn append_local_transcript(&self, chat_id: &str, prompt: &[String], output: &str) {
        let Some(path) = self.transcript_path(chat_id) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let prompt = truncate_utf8_bytes(prompt.join(" ").trim(), MAX_TRANSCRIPT_PROMPT_BYTES);
        let output = truncate_utf8_bytes(output.trim(), MAX_TRANSCRIPT_OUTPUT_BYTES);
        let entry = format!("### user\n{}\n### abbey\n{}\n", prompt, output);
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
        {
            if f.lock_exclusive().is_err() {
                return;
            }
            let existing = f.metadata().map_or(0, |metadata| metadata.len());
            if existing.saturating_add(entry.len() as u64) > MAX_LOCAL_TRANSCRIPT_BYTES {
                let _ = fs4::fs_std::FileExt::unlock(&f);
                drop(f);
                let prev = path.with_extension("transcript.prev");
                let _ = std::fs::remove_file(&prev);
                if std::fs::rename(&path, &prev).is_err() {
                    return;
                }
                if let Ok(mut rotated) = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&path)
                {
                    let _ = rotated.write_all(
                        b"### earlier turns omitted at transcript size limit; previous file retained as .transcript.prev\n",
                    );
                    let _ = rotated.write_all(entry.as_bytes());
                }
                return;
            }
            if f.seek(std::io::SeekFrom::End(0)).is_err() {
                let _ = fs4::fs_std::FileExt::unlock(&f);
                return;
            }
            let _ = f.write_all(entry.as_bytes());
            let _ = fs4::fs_std::FileExt::unlock(&f);
        }
    }

    fn lock_local_turn(&self, chat_id: &str) -> Result<Option<File>> {
        let Some(path) = self.transcript_path(chat_id) else {
            return Ok(None);
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let lock_path = path.with_extension("transcript.lock");
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("open conversation turn lock {}", lock_path.display()))?;
        lock.lock_exclusive()
            .with_context(|| format!("lock conversation turn {}", lock_path.display()))?;
        Ok(Some(lock))
    }

    /// Interactive hand-off: inherit stdio (full TUI agent session).
    pub fn run_interactive(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<ExitStatus> {
        // Local one-shot grammars pass the prompt after a real `--` separator,
        // so a leading-dash prompt is text there, not options — no warning.
        if !self.backend.is_oneshot_local() {
            warn_if_prompt_looks_like_flags(prompt_and_rest);
        }
        let agent = self.exec_path()?;
        let args = self.build_args(resume_id, prompt_and_rest);
        let status = Command::new(&agent)
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| map_exec_err(e, &agent))?;
        Ok(status)
    }

    /// Headless run; returns stdout text.
    pub fn run_capture(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Result<(ExitStatus, String, String)> {
        if !self.backend.is_oneshot_local() {
            warn_if_prompt_looks_like_flags(prompt_and_rest);
        }
        let agent = self.exec_path()?;
        let mut cfg = self.clone();
        cfg.print = true;
        let args = cfg.build_args(resume_id, prompt_and_rest);
        #[cfg(unix)]
        {
            let spec = ProcessSpec::inherited(
                agent.clone(),
                args.iter().map(std::ffi::OsString::from).collect(),
            );
            let limits = SupervisorLimits {
                timeout: Duration::from_secs(30 * 60),
                terminate_grace: Duration::from_secs(1),
                stdout_bytes: MAX_CAPTURE_BYTES,
                stderr_bytes: MAX_CAPTURE_BYTES,
                poll_interval: Duration::from_millis(20),
            };
            match run_with_checkpoint(&spec, &limits, || false) {
                Ok(SupervisorOutcome::Exited {
                    status,
                    stdout,
                    stderr,
                }) => Ok((
                    status,
                    String::from_utf8_lossy(&stdout).into_owned(),
                    String::from_utf8_lossy(&stderr).into_owned(),
                )),
                Ok(SupervisorOutcome::TimedOut) => {
                    bail!("agent capture exceeded the 30-minute limit")
                }
                Ok(SupervisorOutcome::StdoutLimit) => {
                    bail!("agent stdout exceeded the {MAX_CAPTURE_BYTES}-byte limit")
                }
                Ok(SupervisorOutcome::StderrLimit) => {
                    bail!("agent stderr exceeded the {MAX_CAPTURE_BYTES}-byte limit")
                }
                Ok(SupervisorOutcome::Cancelled) => bail!("agent capture was cancelled"),
                Err(error) => bail!("supervise {}: {error}", agent.display()),
            }
        }
        #[cfg(not(unix))]
        {
            capture::bounded_capture_output(&agent, &args)
        }
    }

    pub fn passthrough(&self, args: &[String]) -> Result<ExitStatus> {
        let status = Command::new(self.exec_path()?)
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
        if self.backend == AgentBackend::Claude {
            return Ok("opus      Claude Opus (Abbey's default Max binding)\n\
                 sonnet    Claude Sonnet (Abbey's Gemma binding)\n\
                 haiku     Claude Haiku\n\
                 fable     Claude Fable (plan-gated)\n\
                 claude-*  full Claude catalog id, passed through\n"
                .into());
        }
        if self.backend == AgentBackend::Ollama {
            return Ok(format!(
                "{}  default Ollama tag (alias gemma:27b-mlx)\n\
                 gemma4:12b-mlx  smaller Ollama tag\n\
                 <tag>           any tag `ollama list` reports; passed through\n",
                crate::models::OLLAMA_DEFAULT_MODEL
            ));
        }
        let out = Command::new(self.exec_path()?).arg("models").output()?;
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
    // Local one-shot CLIs (`abi complete`, `ollama run`) are non-interactive
    // — always capture, both for clean emit and so the turn can be recorded
    // for Abbey-side continuity.
    if capture_print || cfg.backend.is_oneshot_local() {
        let _turn_lock = if cfg.backend.is_oneshot_local() {
            resume_id
                .filter(|id| !id.is_empty())
                .map(|id| cfg.lock_local_turn(id))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let (st, out, err) = cfg.run_capture(resume_id, prompt_and_rest)?;
        eprint!("{err}");
        if cfg.backend.is_oneshot_local()
            && st.success()
            && let Some(id) = resume_id.filter(|i| !i.is_empty())
        {
            cfg.append_local_transcript(id, prompt_and_rest, &out);
        }
        if cfg.backend == AgentBackend::Claude
            && st.success()
            && let Some(id) = resume_id.filter(|i| !i.is_empty())
        {
            cfg.touch_claude_session_marker(id);
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
    if cfg.backend == AgentBackend::Claude
        && st.success()
        && let Some(id) = resume_id.filter(|i| !i.is_empty())
    {
        cfg.touch_claude_session_marker(id);
    }
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
    fn non_cursor_account_grammars_refuse_cursor_account_surface() {
        assert!(!AgentBackend::Fm.supports_account_surface());
        assert!(!AgentBackend::Abi.supports_account_surface());
        assert!(!AgentBackend::Claude.supports_account_surface());
        assert!(!AgentBackend::Ollama.supports_account_surface());
        assert!(AgentBackend::Cursor.supports_account_surface());
        assert!(AgentBackend::Grok.supports_account_surface());
    }

    #[test]
    fn only_server_backends_get_the_stale_chat_retry() {
        assert!(AgentBackend::Cursor.has_server_sessions());
        assert!(AgentBackend::Grok.has_server_sessions());
        assert!(!AgentBackend::Fm.has_server_sessions());
        assert!(!AgentBackend::Abi.has_server_sessions());
        assert!(!AgentBackend::Claude.has_server_sessions());
        assert!(!AgentBackend::Ollama.has_server_sessions());
        assert!(AgentBackend::Ollama.is_oneshot_local());
        assert!(AgentBackend::Abi.is_oneshot_local());
        assert!(!AgentBackend::Fm.is_oneshot_local());
    }

    #[test]
    fn local_transcript_rollover_keeps_the_previous_file() {
        let dir = std::env::temp_dir().join(format!(
            "abbey-transcript-prev-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = AgentConfig {
            transcript_dir: Some(dir.clone()),
            ..AgentConfig::default()
        };
        cfg.append_local_transcript("kept", &["first".into()], "one");
        let large = "x".repeat(MAX_TRANSCRIPT_OUTPUT_BYTES);
        for index in 0..40 {
            cfg.append_local_transcript("kept", &[format!("large-{index}")], &large);
        }
        let path = dir.join("kept.transcript");
        let prev = dir.join("kept.transcript.prev");
        let current = std::fs::read_to_string(&path).unwrap();
        assert!(
            current.contains("previous file retained as .transcript.prev"),
            "{current}"
        );
        assert!(prev.is_file(), "rolled transcript must be retained");
        let previous = std::fs::read_to_string(prev).unwrap();
        assert!(previous.contains("### user"), "{previous}");
        assert!(std::fs::metadata(path).unwrap().len() <= MAX_LOCAL_TRANSCRIPT_BYTES);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn local_transcript_appends_are_serialized_and_bounded() {
        let dir = std::env::temp_dir().join(format!(
            "abbey-transcript-bound-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = std::sync::Arc::new(AgentConfig {
            transcript_dir: Some(dir.clone()),
            ..AgentConfig::default()
        });
        let mut workers = Vec::new();
        for index in 0..16 {
            let cfg = cfg.clone();
            workers.push(std::thread::spawn(move || {
                cfg.append_local_transcript(
                    "shared",
                    &[format!("prompt-{index}")],
                    &format!("output-{index}"),
                );
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        let path = dir.join("shared.transcript");
        let transcript = std::fs::read_to_string(&path).unwrap();
        assert_eq!(transcript.matches("### user").count(), 16);
        assert_eq!(transcript.matches("### abbey").count(), 16);

        let large = "x".repeat(MAX_TRANSCRIPT_OUTPUT_BYTES * 2);
        for index in 0..32 {
            cfg.append_local_transcript("shared", &[format!("large-{index}")], &large);
        }
        let metadata = std::fs::metadata(path).unwrap();
        assert!(metadata.len() <= MAX_LOCAL_TRANSCRIPT_BYTES);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The `CURSOR_AGENT_CHAT_ID` guard in `AbbeyState::read_chat_for` keys off
    /// this predicate. It once keyed off the process-cached `from_env()`
    /// instead, so a TUI session that *started* on cursor kept adopting the
    /// cursor chat id after Ctrl-B switched to `abi`/`fm` — the transcript
    /// hijack that guard exists to prevent. Passing the live backend is what
    /// makes the switch safe, in both directions.
    #[test]
    fn cursor_chat_env_is_honoured_per_backend_not_per_process() {
        for switched_to in [
            AgentBackend::Fm,
            AgentBackend::Abi,
            AgentBackend::Claude,
            AgentBackend::Ollama,
        ] {
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
    fn transcript_subdirs_separate_local_and_claude_continuity() {
        assert_eq!(AgentBackend::Abi.transcript_subdir(), "abi");
        assert_eq!(AgentBackend::Fm.transcript_subdir(), "fm");
        assert_eq!(AgentBackend::Claude.transcript_subdir(), "claude");
        assert_eq!(AgentBackend::Ollama.transcript_subdir(), "ollama");
        // A mid-session switch must not land abi turns in fm's directory.
        assert_ne!(
            AgentBackend::Abi.transcript_subdir(),
            AgentBackend::Fm.transcript_subdir()
        );
    }

    #[test]
    fn backend_cycle_visits_all_and_wraps() {
        let mut b = AgentBackend::Ollama;
        let mut seen = Vec::new();
        for _ in 0..6 {
            b = b.cycle_next();
            seen.push(b);
        }
        assert_eq!(b, AgentBackend::Ollama, "cycle must wrap to the start");
        for expect in [
            AgentBackend::Grok,
            AgentBackend::Fm,
            AgentBackend::Abi,
            AgentBackend::Claude,
            AgentBackend::Cursor,
            AgentBackend::Ollama,
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
