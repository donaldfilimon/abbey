//! Background-data refresh for the Personas/Memory/Skills/Doctor panels.
//!
//! The refresh runs on a worker thread. The doctor panel's `--version`
//! probe spawns a subprocess with a half-second budget, the model list
//! spawns another one, and the memory panel opens the memory backend
//! with a quarter-second budget and then runs a reflection and four
//! filters over it. All of that used to run on the UI thread, in the
//! middle of the frame the user is reading the answer on.
//!
//! The collection is therefore a pure function of the config and the
//! state: it can run on any thread, and the app applies the result on
//! the UI thread when it arrives.

use crate::agent::{AgentBackend, AgentConfig};
use crate::config;
use crate::inventory;
use crate::memory;
use crate::persona;
use crate::roles;
use crate::state::AbbeyState;
use std::time::Duration;

use super::app::App;

/// The panel data one refresh collected.
#[derive(Debug)]
pub(crate) struct RefreshPayload {
    pub doctor_lines: Vec<String>,
    pub persona_lines: Vec<String>,
    pub memory_lines: Vec<String>,
    pub skill_lines: Vec<String>,
    pub route_lines: Vec<String>,
    pub live_models: Vec<String>,
    pub history: Vec<crate::state::HistoryEntry>,
}

/// A poll for a finished refresh.
///
/// "Not finished yet" and "the worker is gone" are different facts, and
/// the TUI has to say so: a worker that died without an answer would
/// otherwise leave the status line on "refreshing…" for the rest of the
/// session.
#[derive(Debug)]
pub(crate) enum RefreshPoll {
    /// The refresh is still running.
    Pending,
    /// The refresh finished with this data.
    Finished(RefreshPayload),
    /// The worker stopped without an answer.
    Failed(String),
}

/// How long the shutdown path waits for a running refresh.
///
/// The refresh's own slowest step is the doctor's `--version` probe, which
/// the supervisor kills after 500 ms plus a 100 ms terminate grace. The
/// grace covers that deadline, so the probe is dead by the time the
/// terminal is restored.
pub(crate) const REFRESH_JOIN_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// A refresh running on its own thread.
///
/// The worker is never joined on the UI thread: a refresh that is still
/// running is left to finish on its own, and its result is discarded
/// when a newer refresh has replaced it. The *shutdown* path does join
/// it — see [`RefreshWorker::join_within`] — because the refresh owns a
/// process that must not outlive the terminal.
#[derive(Debug)]
pub(crate) struct RefreshWorker {
    payload: std::sync::mpsc::Receiver<RefreshPayload>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RefreshWorker {
    /// Start a refresh that runs `work` on its own thread.
    pub(crate) fn start(work: impl FnOnce() -> RefreshPayload + Send + 'static) -> Self {
        let (tx, payload) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("abbey-tui-refresh".into())
            .spawn(move || {
                let _ = tx.send(work());
            })
            .expect("the refresh worker thread starts");
        Self {
            payload,
            thread: Some(thread),
        }
    }

    /// The collected data, if the refresh has finished.
    ///
    /// [`RefreshPoll::Pending`] means it is still running: the caller
    /// keeps its "refreshing…" status and asks again on the next pump.
    /// [`RefreshPoll::Failed`] means the worker is gone: it panicked, or
    /// it was killed. That is not a refresh that is still running, and
    /// the caller must not keep waiting for an answer that can never
    /// arrive.
    pub(crate) fn poll(&self) -> RefreshPoll {
        match self.payload.try_recv() {
            Ok(payload) => RefreshPoll::Finished(payload),
            Err(std::sync::mpsc::TryRecvError::Empty) => RefreshPoll::Pending,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                RefreshPoll::Failed("the refresh worker stopped without an answer".to_owned())
            }
        }
    }

    /// Wait for the worker to finish, at most `grace`.
    ///
    /// The refresh owns processes the app started — the doctor's
    /// `--version` probe runs under the supervisor, and the supervisor
    /// kills it on its own deadline. The app is leaving, so it must not
    /// leave that probe running: the terminal is about to be restored and
    /// the process must not outlive it. So the shutdown path joins the
    /// worker here, and the grace covers the probe's own deadline
    /// (500 ms plus its 100 ms terminate grace).
    ///
    /// Returns whether the worker finished within the grace. A worker
    /// that did not is abandoned, and the app says so rather than
    /// pretending the teardown was clean.
    pub(crate) fn join_within(mut self, grace: std::time::Duration) -> bool {
        use std::sync::mpsc::RecvTimeoutError;
        // The worker sends its payload and then drops the sender, so a
        // receive either produces the payload or reports a disconnect.
        // Either one means the thread has reached its end.
        let finished = match self.payload.recv_timeout(grace) {
            Ok(_) | Err(RecvTimeoutError::Disconnected) => true,
            Err(RecvTimeoutError::Timeout) => false,
        };
        if finished && let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        finished
    }
}

/// Collect every panel's data.
///
/// This is the whole refresh, expressed as a function of the config and
/// the state. It reads files, spawns the agent's `--version`, and opens
/// the memory backend, so it belongs on a worker thread.
pub(crate) fn collect(cfg: &AgentConfig, state: &AbbeyState) -> RefreshPayload {
    RefreshPayload {
        doctor_lines: doctor_lines(cfg, state),
        persona_lines: persona_lines(),
        memory_lines: memory_lines(state),
        skill_lines: skill_lines(),
        route_lines: route_lines(state),
        live_models: live_models(cfg),
        history: state.history(40),
    }
}

/// Doctor line naming the active executor backend.
pub fn backend_doctor_line(backend: AgentBackend) -> String {
    match backend {
        AgentBackend::Cursor => "backend:   cursor-agent (ABBEY_BACKEND=cursor)".into(),
        AgentBackend::Ollama => {
            "backend:   ollama — local `ollama run`, default gemma4:26b-mlx (default)".into()
        }
        AgentBackend::Grok => "backend:   grok (ABBEY_BACKEND=grok)".into(),
        AgentBackend::Fm => {
            "backend:   fm — on-device Apple Foundation Models (ABBEY_BACKEND=fm)".into()
        }
        AgentBackend::Abi => {
            "backend:   abi — `abi complete`, local persona-template; claude-*/live \
             opt into Anthropic transport (ABBEY_BACKEND=abi)"
                .into()
        }
        AgentBackend::Claude => {
            "backend:   claude — Claude Code CLI, no cursor-agent (ABBEY_BACKEND=claude)".into()
        }
    }
}

/// Doctor line for Max/Gemma — bindings only exist where cursor model ids do.
pub fn roles_doctor_line(backend: AgentBackend) -> String {
    if matches!(backend, AgentBackend::Fm | AgentBackend::Abi) {
        format!(
            "roles:      Max/Gemma are prompt-only under `{}` (no per-role model ids)",
            backend.label()
        )
    } else {
        format!(
            "roles:      Max→technical · Gemma→visual (model bindings via {})",
            backend.label()
        )
    }
}

fn persona_lines() -> Vec<String> {
    let ac = config::AbbeyConfig::load().unwrap_or_default();
    let mut lines = persona::persona_status_lines("");
    lines.extend(roles::role_status_lines(&ac.roles.max, &ac.roles.gemma));
    lines.push(format!("default_role: {}", ac.default_role));
    lines.push(
        "routing: route_decision → route.jsonl (alt/fb audit only; no auto second agent)".into(),
    );
    lines.push("Tip: /max · /gemma · /routes · hybrid-loop · /persona aviva".into());
    lines
}

fn memory_lines(state: &AbbeyState) -> Vec<String> {
    let abbey_cfg = config::AbbeyConfig::load().unwrap_or_default();
    let backend = abbey_cfg.memory_backend.clone();
    let mut lines = vec![memory::backend_status(&state.state_dir, &backend)];
    let opened =
        memory::open_backend_with_timeout(&state.state_dir, &backend, Duration::from_millis(250));
    match opened {
        Ok(mem) => {
            match memory::build_embedder(&abbey_cfg.embeddings) {
                Ok(embedder) if embedder.space().provider == "none" => {
                    lines.push("semantic disabled (lexical search remains available)".into());
                }
                Ok(embedder) => match memory::embedding_status(mem.as_ref(), embedder.as_ref()) {
                    Ok(status) => lines.push(format!(
                        "semantic {} {}d ready={} pending={}",
                        embedder.space().provider,
                        embedder.space().dimension,
                        status.ready,
                        status.pending()
                    )),
                    Err(error) => lines.push(format!("semantic index unavailable: {error}")),
                },
                Err(error) => lines.push(format!("semantic provider unavailable: {error}")),
            }
            for layer in ["stm", "ltm", "activity", "train_candidate"] {
                let n = mem
                    .filter(Some(layer), None, 500)
                    .map(|v| v.len())
                    .unwrap_or(0);
                lines.push(format!("{layer:<16} {n}"));
            }
            if let Ok(report) = mem.reflect() {
                lines.push(format!(
                    "reflect low={} dups={} superseded={}",
                    report.low_confidence.len(),
                    report.duplicate_summaries.len(),
                    report.superseded.len()
                ));
            }
            if let Ok(prefs) = mem.filter(Some("ltm"), Some("preference"), 10) {
                for p in prefs.into_iter().take(5) {
                    lines.push(format!("pref: {}", p.summary));
                }
            }
            if let Ok(records) = mem.filter(None, None, 12) {
                if records.is_empty() {
                    lines.push("map: empty — teach with `abbey memory put --tag <subject>`".into());
                } else {
                    lines.push("map  topic×recency×depth (CLI: abbey memory map|near)".into());
                    for r in &records {
                        let p = memory::coordinates(r);
                        lines.push(format!(
                            "  {:>5.0} {:>6.2} {:>5.2}  {:<14} {}",
                            p.x,
                            p.y,
                            p.z,
                            memory::primary_topic(r),
                            r.summary
                        ));
                    }
                }
            }
        }
        Err(e) => lines.push(format!("unavailable: {e}")),
    }
    lines.push("CLI/TUI: abbey learn ...|review|stats|improve (TUI-owned direct)".into());
    lines
}

fn skill_lines() -> Vec<String> {
    let mut lines = Vec::new();
    if let Ok(skills) = inventory::list_skills() {
        for s in skills.into_iter().take(80) {
            if s.description.is_empty() {
                lines.push(s.name);
            } else {
                lines.push(format!("{} — {}", s.name, s.description));
            }
        }
    }
    for t in inventory::list_agent_tools() {
        let mark = if t.path.is_some() { "✓" } else { "·" };
        lines.push(format!("{mark} tool:{:<12} {}", t.name, t.kind));
    }
    if lines.is_empty() {
        lines.push("(no skills/tools found)".into());
    }
    lines
}

fn route_lines(state: &AbbeyState) -> Vec<String> {
    crate::route_log::recent_routes(&state.state_dir, 8)
        .map(|v| {
            v.iter()
                .rev()
                .map(crate::route_log::compact_route_line)
                .collect()
        })
        .unwrap_or_default()
}

fn live_models(cfg: &AgentConfig) -> Vec<String> {
    // A backend with its own account surface is never asked for a model
    // list: that surface is the vendor's, and the picker shows the alias
    // table instead.
    if cfg.backend.supports_account_surface() {
        return Vec::new();
    }
    match cfg.list_models_text() {
        Ok(text) => text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn doctor_lines(cfg: &AgentConfig, state: &AbbeyState) -> Vec<String> {
    let chat = state
        .read_chat_for(cfg.backend)
        .unwrap_or_else(|| "(none)".into());
    let ac = config::AbbeyConfig::load().unwrap_or_default();
    let mut lines = crate::build_info::lines();
    lines.extend([
        backend_doctor_line(cfg.backend),
        format!("agent:     {}", cfg.agent_path.display()),
        format!("agent ver: {}", cfg.agent_version()),
        format!("model:     {}", cfg.model),
        // The live backend plus the live model, resolved with the spawn
        // path's own rules — the same helper the CLI doctor prints, so the
        // two surfaces cannot disagree. Read-only: nothing is persisted
        // and routing is untouched.
        crate::effective_model::effective_model_line(cfg.backend, &cfg.model),
        format!("chat:      {chat}"),
        format!("chat file: {}", state.active_chat_file().display()),
        format!("per-cwd:   {}", state.per_cwd),
        format!("cwd:       {}", state.cwd.display()),
        format!("state:     {}", state.state_dir.display()),
        format!("auto-review: {}", cfg.auto_review),
        format!("trust:     {}", cfg.trust),
        format!("force:     {}", cfg.force),
        format!("no-resume: {}", cfg.no_resume),
        "personas:   Abbey · Aviva · Abi (abi-ai)".into(),
        roles_doctor_line(cfg.backend),
        memory::backend_status(&state.state_dir, &ac.memory_backend),
        memory::feature_status(),
        config::wdbx_cli_status(&ac),
        "os-control: abbey os dry-run|execute --confirm (cross-platform allowlist)".into(),
        "subagents:  abbey subagents run --lanes max,reviewer [--peers gemini]".into(),
        "parallel:   alias of subagents with Max+Gemma+Aviva defaults".into(),
        // Enhanced self-learn visibility in TUI doctor (uses same curation_summary as CLI doctor).
        format!(
            "learn cur:  {}",
            crate::learn::curation_summary(state).unwrap_or_else(|_| "n/a".into())
        ),
        "learn:      abbey learn correction|preference|routes|digest|review|stats|improve (TUI owned)"
            .into(),
    ]);
    lines
}

impl App {
    /// Start a background refresh, replacing any refresh in flight.
    ///
    /// The refresh in flight is not joined: it cannot be cancelled, and
    /// waiting for it would put the very freeze this worker exists to
    /// remove back on the UI thread. Its result is discarded when it
    /// lands.
    pub(crate) fn request_refresh(&mut self) {
        let cfg = self.cfg.clone();
        let state = self.state.clone();
        self.panel_view.refresh = Some(RefreshWorker::start(move || collect(&cfg, &state)));
    }

    /// Start a background refresh with an injected collection.
    ///
    /// For a test that needs a refresh which is slow, or which produces
    /// a known result.
    #[cfg(test)]
    pub(crate) fn request_refresh_with(
        &mut self,
        work: impl FnOnce() -> RefreshPayload + Send + 'static,
    ) {
        self.panel_view.refresh = Some(RefreshWorker::start(work));
    }

    /// Apply a refresh that has finished.
    pub(crate) fn apply_refresh(&mut self, payload: RefreshPayload) {
        self.doctor_lines = payload.doctor_lines;
        self.persona_lines = payload.persona_lines;
        self.memory_lines = payload.memory_lines;
        self.skill_lines = payload.skill_lines;
        self.route_lines = payload.route_lines;
        // A backend with its own account surface has no model list to
        // collect: the picker shows the alias table instead.
        if !self.cfg.backend.supports_account_surface() {
            self.live_models = payload.live_models;
        }
        self.history = payload.history;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_names_every_backend() {
        assert!(backend_doctor_line(AgentBackend::Cursor).contains("cursor-agent"));
        assert!(backend_doctor_line(AgentBackend::Grok).contains("grok"));
        assert!(backend_doctor_line(AgentBackend::Fm).contains("on-device"));
        let abi = backend_doctor_line(AgentBackend::Abi);
        assert!(abi.contains("abi complete") && abi.contains("ABBEY_BACKEND=abi"));
        let claude = backend_doctor_line(AgentBackend::Claude);
        assert!(claude.contains("Claude Code") && claude.contains("ABBEY_BACKEND=claude"));
        let ollama = backend_doctor_line(AgentBackend::Ollama);
        assert!(ollama.contains("ollama") && ollama.contains("gemma4:26b-mlx"));
    }

    #[test]
    fn roles_line_claims_bindings_only_under_cursor_model_ids() {
        for b in [
            AgentBackend::Cursor,
            AgentBackend::Grok,
            AgentBackend::Claude,
            AgentBackend::Ollama,
        ] {
            assert!(roles_doctor_line(b).contains("bindings"));
        }
        for b in [AgentBackend::Fm, AgentBackend::Abi] {
            let line = roles_doctor_line(b);
            assert!(
                line.contains("prompt-only") && !line.contains("bindings"),
                "under {} the TUI must not claim per-role model bindings: {line}",
                b.label()
            );
        }
    }

    /// The TUI doctor panel and the CLI doctor print the *same* effective
    /// model line, from the same helper — so the two surfaces can never
    /// disagree about what a turn would use right now.
    ///
    /// The stale-model case is the interesting one: after a Ctrl-B provider
    /// switch the live `cfg.model` can still hold another backend's id, and
    /// the panel must report what the turn would *actually* use.
    #[test]
    fn doctor_panel_prints_the_shared_effective_model_line() {
        let mut app = crate::tui::tests::scratch_app("effective", AgentBackend::Ollama);
        app.cfg.model = "claude-opus-5-thinking-high".into();
        app.apply_refresh(collect(&app.cfg, &app.state));
        let expected =
            crate::effective_model::effective_model_line(app.cfg.backend, &app.cfg.model);
        assert_eq!(
            expected,
            "effective model: gemma4:26b-mlx  (live: claude-opus-5-thinking-high, backend: ollama)"
        );
        assert!(
            app.doctor_lines.iter().any(|line| line == &expected),
            "doctor panel must carry the shared line {expected}: {:?}",
            app.doctor_lines
        );
    }
}
