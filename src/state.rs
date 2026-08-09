//! XDG state: chat ids (global + per-cwd), model, history.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
mod conversation;
#[cfg(not(unix))]
#[path = "state/conversation_portable.rs"]
mod conversation;

#[cfg(unix)]
pub(crate) use conversation::lock_legacy_capture;

#[derive(Debug, Clone)]
pub struct AbbeyState {
    pub state_dir: PathBuf,
    pub chat_file: PathBuf,
    pub model_file: PathBuf,
    pub history_file: PathBuf,
    pub cwd_dir: PathBuf,
    pub per_cwd: bool,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub chat_id: String,
    pub cwd: String,
}

impl AbbeyState {
    pub fn load() -> Result<Self> {
        // Both the override variable and the default root belong to the active
        // edition (`edition::ACTIVE`), so a personal build can never adopt the
        // safe build's state — not even from an exported ABBEY_STATE_DIR.
        let edition = crate::edition::ACTIVE;
        let state_dir = std::env::var_os(edition.state_dir_env())
            .map(PathBuf::from)
            .or_else(|| edition.default_state_root())
            .with_context(|| format!("cannot resolve {}", edition.state_dir_env()))?;

        // These name individual state *files*, so they are edition-scoped too.
        // Scoping only the root was not enough: one exported ABBEY_CHAT_FILE
        // otherwise gave both editions the same chat id (observed, then fixed).
        // The safe edition's names are unchanged (`ABBEY_CHAT_FILE`, …).
        let chat_file = std::env::var_os(edition.scoped_env("CHAT_FILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("chat-id"));
        let model_file = std::env::var_os(edition.scoped_env("MODEL_FILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("model"));
        let history_file = std::env::var_os(edition.scoped_env("HISTORY_FILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("history.log"));
        let cwd_dir = state_dir.join("by-cwd");

        fs::create_dir_all(&state_dir)?;
        fs::create_dir_all(&cwd_dir)?;

        let per_cwd = std::env::var("ABBEY_PER_CWD")
            .map(|v| v != "0")
            .unwrap_or(true);

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        Ok(Self {
            state_dir,
            chat_file,
            model_file,
            history_file,
            cwd_dir,
            per_cwd,
            cwd,
        })
    }

    fn cwd_key(cwd: &Path) -> String {
        let s = cwd.to_string_lossy();
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                out.push(ch);
            } else {
                out.push('_');
            }
        }
        out.chars().take(180).collect()
    }

    pub fn active_chat_file(&self) -> PathBuf {
        if self.per_cwd {
            self.cwd_dir.join(Self::cwd_key(&self.cwd))
        } else {
            self.chat_file.clone()
        }
    }

    /// Active chat id as seen by `backend`.
    ///
    /// The backend is passed in rather than read from
    /// `AgentBackend::from_env()`, which resolves once per process: the TUI's
    /// Ctrl-B switch changes `AgentConfig::backend` at runtime, so consulting
    /// the cached value here would let a session that *started* on cursor keep
    /// adopting `CURSOR_AGENT_CHAT_ID` after switching to `abi`/`fm`, which is
    /// exactly the hijack described below.
    pub fn read_chat_for(&self, backend: crate::agent::AgentBackend) -> Option<String> {
        // Recover the canonical commit's compatibility mirrors before backend
        // selection. An inherited Cursor id may win below, but it must not
        // indefinitely strand a committed journal from an earlier process.
        let mirrored = match conversation::read_chat(self) {
            Ok(mirrored) => mirrored,
            Err(_) => return None,
        };
        // `CURSOR_AGENT_CHAT_ID` lets Abbey join the cursor session it was
        // launched from — but it is a *cursor* chat id. Under a backend with
        // no server sessions (`fm`, `abi`) it names nothing real, and adopting
        // it hijacks the transcript this run should continue. Found live:
        // running `abbey -c` under `abi` inside a cursor session resumed the
        // cursor id, so every turn wrote a fresh transcript and continuity
        // silently never happened.
        if backend.has_server_sessions()
            && let Ok(id) = std::env::var("CURSOR_AGENT_CHAT_ID")
        {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return Some(id);
            }
        }
        mirrored
    }

    pub fn save_chat(&self, id: &str) -> Result<()> {
        conversation::save_chat(self, id)
    }

    pub fn clear_chat(&self, all: bool) -> Result<()> {
        conversation::clear_chat(self, all)
    }

    pub fn history(&self, n: usize) -> Vec<HistoryEntry> {
        conversation::history(self, n).unwrap_or_default()
    }

    pub fn read_model(&self) -> String {
        if let Ok(m) = std::env::var("ABBEY_MODEL") {
            let m = m.trim();
            if !m.is_empty() {
                return crate::models::resolve_model(m);
            }
        }
        if let Some(m) = read_first_line(&self.model_file) {
            return crate::models::resolve_model(&m);
        }
        "auto".into()
    }

    /// Raw model-file / `ABBEY_MODEL` text without cursor alias expansion.
    ///
    /// Used by `ABBEY_BACKEND=abi` so a persisted bare `claude-*` catalog id is
    /// not rewritten into a Cursor `*-thinking-*` binding (which abi treats as
    /// local, not live).
    pub fn read_model_raw(&self) -> String {
        if let Ok(m) = std::env::var("ABBEY_MODEL") {
            let m = m.trim();
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(m) = read_first_line(&self.model_file) {
            return m;
        }
        "local".into()
    }

    pub fn save_model(&self, model: &str) -> Result<()> {
        let m = crate::models::resolve_model(model);
        conversation::write_model(self, &m)
    }

    /// Persist a model tag without cursor `resolve_model` expansion.
    pub fn save_model_literal(&self, model: &str) -> Result<()> {
        let m = model.trim();
        anyhow::ensure!(!m.is_empty(), "model id must not be empty");
        conversation::write_model(self, m)
    }

    pub fn clear_model(&self) -> Result<()> {
        conversation::clear_model(self)
    }

    pub(crate) fn compact_history(&self, keep: usize) -> Result<usize> {
        conversation::compact_history(self, keep)
    }

    pub(crate) fn ensure_conversation_ready(&self) -> Result<()> {
        conversation::ensure_ready(self)
    }
}

fn read_first_line(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}
