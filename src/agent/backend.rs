//! Executor backend selection and binary resolution.
//!
//! Split out of `agent/mod.rs` per the repo's file-size rule. The invariants
//! live here: backend precedence (env > config > cursor when resolvable >
//! first other installed executor), the once-per-process cache, and the rule
//! that no backend — cursor-agent included — is a hard requirement.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::host::which_bin;

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

    /// Backend precedence: `ABBEY_BACKEND` env > config `backend` key >
    /// cursor when resolvable > the first other installed executor.
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
        Self::from_env_with_source().0
    }

    /// Where the cached backend choice came from, for `doctor` honesty:
    /// `"default"`, or the auto-fallback note when cursor-agent is absent.
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
        if let Ok(v) = std::env::var("ABBEY_BACKEND")
            && !v.trim().is_empty()
        {
            return (Self::parse(&v).unwrap_or(Self::Cursor), "env");
        }
        let Some(configured) = crate::config::AbbeyConfig::load()
            .ok()
            .and_then(|c| c.backend)
        else {
            return Self::default_with_fallback();
        };
        let parsed = Self::parse(&configured).unwrap_or_else(|| {
            // Silently falling back would make a typo'd config key look like
            // it worked — the user asked for a specific executor and got
            // cursor-agent instead, with no way to tell.
            eprintln!(
                "abbey: config `backend = {configured:?}` is not one of \
                 cursor|grok|fm|abi — using cursor-agent"
            );
            Self::Cursor
        });
        (parsed, "config")
    }

    /// The unchosen default: cursor when it (or an explicit `ABBEY_AGENT`
    /// binary) resolves, otherwise the first other installed executor in the
    /// fixed TUI cycle order. cursor-agent is preferred, never required —
    /// a machine with only `abi` (or `grok`/`fm`) works out of the box.
    /// `doctor` reports the auto choice; generation on a machine with no
    /// executor at all still fails at spawn time with the full remedy list.
    fn default_with_fallback() -> (Self, &'static str) {
        // An ABBEY_AGENT override names a concrete binary for the default
        // backend — honour it before probing anything.
        if std::env::var("ABBEY_AGENT")
            .map(|p| Path::new(&p).is_file())
            .unwrap_or(false)
        {
            return (Self::Cursor, "default");
        }
        Self::pick_default_backend(&|b| resolve_agent_for(b).is_ok())
    }

    fn pick_default_backend(resolves: &dyn Fn(AgentBackend) -> bool) -> (Self, &'static str) {
        if resolves(Self::Cursor) {
            return (Self::Cursor, "default");
        }
        for candidate in [Self::Grok, Self::Fm, Self::Abi] {
            if resolves(candidate) {
                return (candidate, "auto — cursor-agent not installed");
            }
        }
        // Nothing installed: stay on cursor so the eventual spawn-time error
        // names the preferred install first.
        (Self::Cursor, "default")
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
        "{} not found — generation needs an executor backend. Install any of \
         cursor-agent, grok, fm, or abi (ABBEY_BACKEND=cursor|grok|fm|abi picks \
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
    fn default_backend_prefers_cursor_but_never_requires_it() {
        // cursor wins whenever it resolves…
        let (b, src) = AgentBackend::pick_default_backend(&|_| true);
        assert_eq!(b, AgentBackend::Cursor);
        assert_eq!(src, "default");

        // …otherwise the first other installed executor serves, in the fixed
        // cycle order, and the source string says the choice was automatic.
        let (b, src) = AgentBackend::pick_default_backend(&|b| matches!(b, AgentBackend::Abi));
        assert_eq!(b, AgentBackend::Abi);
        assert!(
            src.starts_with("auto"),
            "auto choice must be visible: {src}"
        );

        let (b, _) = AgentBackend::pick_default_backend(&|b| {
            matches!(b, AgentBackend::Grok | AgentBackend::Abi)
        });
        assert_eq!(b, AgentBackend::Grok, "cycle order breaks the tie");

        // Nothing installed: stay on cursor so the spawn-time error names the
        // preferred install first — never a panic, never a random pick.
        let (b, src) = AgentBackend::pick_default_backend(&|_| false);
        assert_eq!(b, AgentBackend::Cursor);
        assert_eq!(src, "default");
    }
}
