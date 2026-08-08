//! XDG state: chat ids (global + per-cwd), model, history.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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
        let state_dir = std::env::var_os("ABBEY_STATE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                // Unix: XDG state, else ~/.local/state/abbey (do not steal macOS
                // Application Support via data_local_dir — that relocates existing stores).
                // Windows: LocalAppData\abbey.
                #[cfg(windows)]
                {
                    dirs::data_local_dir()
                        .map(|d| d.join("abbey"))
                        .or_else(|| dirs::home_dir().map(|h| h.join("AppData\\Local\\abbey")))
                }
                #[cfg(not(windows))]
                {
                    dirs::state_dir()
                        .map(|d| d.join("abbey"))
                        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state/abbey")))
                }
            })
            .context("cannot resolve ABBEY_STATE_DIR")?;

        let chat_file = std::env::var_os("ABBEY_CHAT_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("chat-id"));
        let model_file = std::env::var_os("ABBEY_MODEL_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("model"));
        let history_file = std::env::var_os("ABBEY_HISTORY_FILE")
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
        // `CURSOR_AGENT_CHAT_ID` lets Abbey join the cursor session it was
        // launched from — but it is a *cursor* chat id. Under a backend with
        // no server sessions (`fm`, `abi`) it names nothing real, and adopting
        // it hijacks the transcript this run should continue. Found live:
        // running `abbey -c` under `abi` inside a cursor session resumed the
        // cursor id, so every turn wrote a fresh transcript and continuity
        // silently never happened.
        if backend.has_server_sessions() {
            if let Ok(id) = std::env::var("CURSOR_AGENT_CHAT_ID") {
                let id = id.trim().to_string();
                if !id.is_empty() {
                    return Some(id);
                }
            }
        }
        let file = self.active_chat_file();
        if let Some(id) = read_first_line(&file) {
            return Some(id);
        }
        if self.per_cwd {
            return read_first_line(&self.chat_file);
        }
        None
    }

    pub fn save_chat(&self, id: &str) -> Result<()> {
        let id = id.trim();
        if id.is_empty() {
            bail!("empty chat id");
        }
        let file = self.active_chat_file();
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file, format!("{id}\n"))?;
        // Mirror for shell-integration / zsh wrapper
        fs::write(&self.chat_file, format!("{id}\n"))?;
        fs::write(
            self.chat_file.with_extension("export"),
            format!("ABBEY_CHAT_ID={id}\n"),
        )?;
        self.append_history(id)?;
        Ok(())
    }

    pub fn clear_chat(&self, all: bool) -> Result<()> {
        let file = self.active_chat_file();
        let _ = fs::remove_file(&file);
        if all || !self.per_cwd {
            let _ = fs::remove_file(&self.chat_file);
            let _ = fs::remove_file(self.chat_file.with_extension("export"));
        }
        if all {
            if let Ok(entries) = fs::read_dir(&self.cwd_dir) {
                for e in entries.flatten() {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        Ok(())
    }

    fn append_history(&self, id: &str) -> Result<()> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_file)?;
        writeln!(
            f,
            "{}\t{}\t{}",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            id,
            self.cwd.display()
        )?;
        Ok(())
    }

    pub fn history(&self, n: usize) -> Vec<HistoryEntry> {
        let Ok(text) = fs::read_to_string(&self.history_file) else {
            return Vec::new();
        };
        text.lines()
            .rev()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                Some(HistoryEntry {
                    timestamp: parts.next()?.to_string(),
                    chat_id: parts.next()?.to_string(),
                    cwd: parts.next().unwrap_or("").to_string(),
                })
            })
            .take(n)
            .collect()
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
        fs::write(&self.model_file, format!("{m}\n"))?;
        Ok(())
    }

    /// Persist a model tag without cursor `resolve_model` expansion.
    pub fn save_model_literal(&self, model: &str) -> Result<()> {
        let m = model.trim();
        anyhow::ensure!(!m.is_empty(), "model id must not be empty");
        fs::write(&self.model_file, format!("{m}\n"))?;
        Ok(())
    }

    pub fn clear_model(&self) -> Result<()> {
        let _ = fs::remove_file(&self.model_file);
        Ok(())
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
