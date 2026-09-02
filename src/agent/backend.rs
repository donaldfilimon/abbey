//! Executor backend selection and binary resolution.
//!
//! Split out of `agent/mod.rs` per the repo's file-size rule. The invariants
//! live here: backend precedence (backend env > config > legacy agent path > ollama >
//! first other installed executor, cursor last), the once-per-process cache,
//! and the rule that no backend is a hard requirement.

use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::host::which_bin;
use crate::runtime::supervisor::{
    ProcessSpec, SupervisorLimits, SupervisorOutcome, run_with_checkpoint,
};

/// Backend preference, or auto path via `ABBEY_AGENT`.
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
    /// Claude Code CLI (`claude`) with its own session store and argv grammar.
    Claude,
    /// Local Ollama CLI (`ollama run`) — preferred default. One-shot with
    /// Abbey-side transcript continuity; default model is the local
    /// `gemma4:26b-mlx` tag (alias `gemma:27b-mlx`).
    Ollama,
}

impl AgentBackend {
    /// Parse a backend token from env or config. `None` for unknown/empty.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cursor" | "cursor-agent" => Some(Self::Cursor),
            "grok" | "grok-build" | "xai" => Some(Self::Grok),
            "fm" | "apple" | "foundation" | "on-device" => Some(Self::Fm),
            "abi" | "abi-cli" => Some(Self::Abi),
            "claude" | "claude-code" => Some(Self::Claude),
            "ollama" | "ollama-cli" => Some(Self::Ollama),
            _ => None,
        }
    }

    /// Backend precedence: `ABBEY_BACKEND` env > config `backend` key >
    /// legacy `ABBEY_AGENT` cursor path > ollama when resolvable >
    /// grok/fm/abi/claude, then cursor last.
    ///
    /// A *set but unknown* env value still means ollama — it does not fall
    /// through to the config key, so a typo in the env var cannot silently
    /// activate a config-chosen backend.
    ///
    /// Resolved once per process and cached: neither the environment nor the
    /// config file changes mid-run, and `read_chat` (hence the TUI's per-frame
    /// draw) calls this — without the cache every frame re-read `config.toml`
    /// from disk. The TUI's Ctrl-B switch sets `AgentConfig::backend`
    /// directly and does not depend on this.
    pub fn from_env() -> Self {
        Self::from_env_with_source().0
    }

    /// Where the cached backend choice came from, for `doctor` honesty:
    /// `"default"`, or the auto-fallback note when ollama is absent.
    /// Env/config sources keep their existing richer formatting in `doctor`.
    pub fn from_env_source() -> &'static str {
        Self::from_env_with_source().1
    }

    fn from_env_with_source() -> (Self, &'static str) {
        static RESOLVED: std::sync::OnceLock<(AgentBackend, &'static str)> =
            std::sync::OnceLock::new();
        *RESOLVED.get_or_init(Self::resolve_from_env_and_config)
    }

    fn resolve_from_env_and_config() -> (Self, &'static str) {
        let env_backend = std::env::var("ABBEY_BACKEND").ok();
        let config_backend = crate::config::AbbeyConfig::load()
            .ok()
            .and_then(|c| c.backend);
        Self::select_from_sources(
            env_backend.as_deref(),
            config_backend.as_deref(),
            legacy_agent_path().is_some(),
        )
        .unwrap_or_else(Self::default_with_fallback)
    }

    /// `ABBEY_BACKEND` wins over a config key; a config key wins over the
    /// legacy `ABBEY_AGENT` cursor path. Unknown env still selects ollama so a
    /// typo cannot fall through to config. `None` means the unchosen default.
    fn select_from_sources(
        env_backend: Option<&str>,
        config_backend: Option<&str>,
        has_legacy_agent: bool,
    ) -> Option<(Self, &'static str)> {
        if let Some(value) = env_backend.filter(|value| !value.trim().is_empty()) {
            return Some((Self::parse(value).unwrap_or(Self::Ollama), "env"));
        }
        if let Some(configured) = config_backend.filter(|value| !value.trim().is_empty()) {
            let parsed = Self::parse(configured).unwrap_or_else(|| {
                eprintln!(
                    "abbey: config `backend = {configured:?}` is not one of \
                     ollama|cursor|grok|fm|abi|claude — using ollama"
                );
                Self::Ollama
            });
            return Some((parsed, "config"));
        }
        if has_legacy_agent {
            return Some((Self::Cursor, "ABBEY_AGENT"));
        }
        None
    }

    /// The unchosen default: ollama when it resolves, otherwise the first
    /// other installed executor in the fixed TUI cycle order, with cursor
    /// last. ollama is preferred, never required — a machine with only
    /// `abi` (or `grok`/`fm`) still works out of the box. The legacy
    /// `ABBEY_AGENT` path is handled before this unchosen-default path.
    fn default_with_fallback() -> (Self, &'static str) {
        Self::pick_default_backend(&|backend| resolve_agent_for(backend).is_ok(), &|| {
            resolve_agent_for(Self::Ollama).is_ok_and(|path| ollama_default_ready(&path))
        })
    }

    fn pick_default_backend(
        resolves: &dyn Fn(AgentBackend) -> bool,
        ollama_ready: &dyn Fn() -> bool,
    ) -> (Self, &'static str) {
        let ollama_installed = resolves(Self::Ollama);
        if ollama_installed && ollama_ready() {
            return (Self::Ollama, "default");
        }
        for candidate in [Self::Grok, Self::Fm, Self::Abi, Self::Claude, Self::Cursor] {
            if resolves(candidate) {
                return (
                    candidate,
                    if ollama_installed {
                        "auto — ollama daemon/default model unavailable"
                    } else {
                        "auto — ollama not installed"
                    },
                );
            }
        }
        // Nothing installed: stay on ollama so the eventual spawn-time error
        // names the preferred install first.
        (Self::Ollama, "default — no ready executor found")
    }

    /// State subdirectory holding this backend's conversation transcripts.
    ///
    /// Shared by `apply_global_flags` and the TUI's backend switch so a
    /// mid-session switch cannot write one backend's transcripts into
    /// another's directory.
    pub fn transcript_subdir(self) -> &'static str {
        match self {
            Self::Abi => "abi",
            Self::Claude => "claude",
            Self::Ollama => "ollama",
            _ => "fm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            Self::Grok => "grok",
            Self::Fm => "fm",
            Self::Abi => "abi",
            Self::Claude => "claude",
            Self::Ollama => "ollama",
        }
    }

    /// Whether this backend is verifiably on-device for every supported model.
    ///
    /// `abi` is deliberately excluded: its default transport is local, but a
    /// `claude-*`/`live` model selects a real network transport, so the label
    /// would be conditional — and a conditional "on-device" reads as a promise.
    pub fn is_on_device(self) -> bool {
        matches!(self, Self::Fm)
    }

    /// Local one-shot CLIs with Abbey-side transcript continuity.
    ///
    /// `abi complete` and `ollama run` have no server session: Abbey captures
    /// the turn and replays a bounded transcript tail on the next prompt.
    pub fn is_oneshot_local(self) -> bool {
        matches!(self, Self::Abi | Self::Ollama)
    }

    /// Account / session-list / MCP surface. Claude has its own commands, but
    /// does not accept cursor-agent's account verbs.
    pub fn supports_account_surface(self) -> bool {
        matches!(self, Self::Cursor | Self::Grok)
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
            Self::Ollama => Self::Grok,
            Self::Grok => Self::Fm,
            Self::Fm => Self::Abi,
            Self::Abi => Self::Claude,
            Self::Claude => Self::Cursor,
            Self::Cursor => Self::Ollama,
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
    let backend = AgentBackend::from_env();
    if backend == AgentBackend::Cursor
        && let Some(path) = legacy_agent_path()
    {
        return Ok(path);
    }
    resolve_agent_for(backend)
}

fn legacy_agent_path() -> Option<PathBuf> {
    let value = std::env::var_os("ABBEY_AGENT")?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_file().then_some(path)
}

fn ollama_default_ready(path: &Path) -> bool {
    ollama_lists_model(path, crate::models::OLLAMA_DEFAULT_MODEL)
}

/// True only when `ollama list` already contains `model`. Used to refuse
/// `ollama run`, which would otherwise pull a missing tag.
pub(crate) fn ollama_lists_model(path: &Path, model: &str) -> bool {
    let spec = ProcessSpec::inherited(path.to_path_buf(), vec![OsString::from("list")]);
    let limits = SupervisorLimits {
        timeout: Duration::from_millis(750),
        terminate_grace: Duration::from_millis(100),
        stdout_bytes: 64 * 1024,
        stderr_bytes: 4 * 1024,
        poll_interval: Duration::from_millis(10),
    };
    let Ok(SupervisorOutcome::Exited { status, stdout, .. }) =
        run_with_checkpoint(&spec, &limits, || false)
    else {
        return false;
    };
    status.success()
        && String::from_utf8_lossy(&stdout)
            .lines()
            .skip(1)
            .any(|line| line.split_whitespace().next() == Some(model))
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
        AgentBackend::Claude => "claude",
        AgentBackend::Ollama => "ollama",
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
                 CLI (macOS 26+). Unset ABBEY_BACKEND=fm to use the default ollama backend."
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
        AgentBackend::Claude => {
            if let Some(path) = which_bin("claude") {
                return Ok(path);
            }
            bail!(
                "`claude` not found — ABBEY_BACKEND=claude needs the Claude Code CLI on \
                 PATH. Install it from https://claude.com/claude-code, or select another \
                 backend."
            );
        }
        AgentBackend::Ollama => {
            if let Some(path) = which_bin("ollama") {
                return Ok(path);
            }
            bail!(
                "`ollama` not found — ABBEY_BACKEND=ollama needs the Ollama CLI on PATH \
                 (https://ollama.com). The default model is gemma4:26b-mlx \
                 (alias gemma:27b-mlx)."
            );
        }
    }
    bail!(
        "{} not found — generation needs an executor backend. Install any of \
         ollama, grok, fm, abi, claude, or cursor-agent \
         (ABBEY_BACKEND=ollama|cursor|grok|fm|abi|claude picks \
         one explicitly, ABBEY_AGENT points at a binary directly); local verbs \
         work without one",
        backend.label()
    );
}

fn fs_readlink(path: &Path) -> Result<String> {
    Ok(std::fs::read_link(path)?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_backend_prefers_ollama_but_never_requires_cursor() {
        // ollama wins whenever it resolves, even if cursor-agent is also present.
        let (b, src) = AgentBackend::pick_default_backend(&|_| true, &|| true);
        assert_eq!(b, AgentBackend::Ollama);
        assert_eq!(src, "default");

        // cursor present without ollama is a last-resort auto fallback, never
        // the preferred default.
        let (b, src) =
            AgentBackend::pick_default_backend(&|b| matches!(b, AgentBackend::Cursor), &|| false);
        assert_eq!(b, AgentBackend::Cursor);
        assert!(
            src.starts_with("auto"),
            "cursor without ollama must be an auto fallback: {src}"
        );

        // otherwise the first other installed executor serves, in the fixed
        // cycle order, and the source string says the choice was automatic.
        let (b, src) =
            AgentBackend::pick_default_backend(&|b| matches!(b, AgentBackend::Abi), &|| false);
        assert_eq!(b, AgentBackend::Abi);
        assert!(
            src.starts_with("auto"),
            "auto choice must be visible: {src}"
        );

        let (b, _) = AgentBackend::pick_default_backend(
            &|b| matches!(b, AgentBackend::Grok | AgentBackend::Abi),
            &|| false,
        );
        assert_eq!(b, AgentBackend::Grok, "cycle order breaks the tie");

        let (b, src) =
            AgentBackend::pick_default_backend(&|b| matches!(b, AgentBackend::Claude), &|| false);
        assert_eq!(b, AgentBackend::Claude);
        assert!(src.starts_with("auto"));

        // Nothing installed: stay on ollama so the spawn-time error names the
        // preferred install first — never a panic, never a random pick, never
        // cursor-agent.
        let (b, src) = AgentBackend::pick_default_backend(&|_| false, &|| false);
        assert_eq!(b, AgentBackend::Ollama);
        assert_eq!(src, "default — no ready executor found");

        let (b, src) = AgentBackend::pick_default_backend(
            &|backend| matches!(backend, AgentBackend::Ollama | AgentBackend::Abi),
            &|| false,
        );
        assert_eq!(b, AgentBackend::Abi);
        assert!(src.contains("default model unavailable"));
    }

    #[test]
    fn claude_backend_aliases_parse() {
        assert_eq!(AgentBackend::parse("claude"), Some(AgentBackend::Claude));
        assert_eq!(
            AgentBackend::parse("Claude-Code"),
            Some(AgentBackend::Claude)
        );
    }

    #[test]
    fn ollama_backend_aliases_parse() {
        assert_eq!(AgentBackend::parse("ollama"), Some(AgentBackend::Ollama));
        assert_eq!(
            AgentBackend::parse("Ollama-CLI"),
            Some(AgentBackend::Ollama)
        );
    }

    #[cfg(unix)]
    #[test]
    fn automatic_ollama_probe_requires_the_default_model() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = std::env::temp_dir().join(format!(
            "abbey-ollama-probe-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf 'NAME ID SIZE\\n{} digest 1GB\\n'\n",
                crate::models::OLLAMA_DEFAULT_MODEL
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(ollama_default_ready(&path));

        std::fs::write(
            &path,
            "#!/bin/sh\nprintf 'NAME ID SIZE\\nother:latest digest 1GB\\n'\n",
        )
        .unwrap();
        assert!(!ollama_default_ready(&path));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn configured_backend_outranks_the_legacy_agent_path() {
        assert_eq!(
            AgentBackend::select_from_sources(Some("abi"), Some("ollama"), true),
            Some((AgentBackend::Abi, "env"))
        );
        assert_eq!(
            AgentBackend::select_from_sources(None, Some("ollama"), true),
            Some((AgentBackend::Ollama, "config"))
        );
        assert_eq!(
            AgentBackend::select_from_sources(None, None, true),
            Some((AgentBackend::Cursor, "ABBEY_AGENT"))
        );
        assert_eq!(AgentBackend::select_from_sources(None, None, false), None);
        assert_eq!(
            AgentBackend::select_from_sources(Some("not-a-backend"), Some("claude"), true),
            Some((AgentBackend::Ollama, "env"))
        );
    }
}
