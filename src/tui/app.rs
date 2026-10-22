//! Chat-first TUI state. Drawing lives in `render.rs`, keys in `keymap.rs`,
//! the terminal loop in `run_loop.rs`.

use super::completion::{Completion, FileIndex};
use super::composer::{Composer, History};
use super::keymap::EscWindow;
use super::theme::{TerminalBrightness, Theme, ThemeId};
use super::transcript::Transcript;
use super::worker::{self, JoinOutcome, RunHandle, RunKind, SlashRoute};
use crate::agent::{AgentBackend, AgentConfig};
use crate::models;
use crate::state::AbbeyState;
use anyhow::Result;
use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Panel {
    Memory,
    Routes,
    Skills,
    Doctor,
    Personas,
    Claims,
}

impl Panel {
    pub(crate) const ALL: [Panel; 6] = [
        Panel::Memory,
        Panel::Routes,
        Panel::Skills,
        Panel::Doctor,
        Panel::Personas,
        Panel::Claims,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Panel::Memory => "Memory",
            Panel::Routes => "Routes",
            Panel::Skills => "Skills",
            Panel::Doctor => "Doctor",
            Panel::Personas => "Personas",
            Panel::Claims => "Claims",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Overlay {
    None,
    Palette {
        query: String,
        idx: usize,
    },
    Help,
    Panel(Panel),
    ModelPicker {
        idx: usize,
    },
    ProviderPicker {
        idx: usize,
        choices: Vec<ProviderChoice>,
    },
    ResumePicker {
        idx: usize,
    },
    Confirm {
        command: Vec<String>,
    },
    Search {
        query: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Suspend {
    Editor,
    InteractiveSlash(String),
}

/// How long the TUI waits for an interrupted run before it gives up.
///
/// An interrupt asks the worker to stop; it does not promise that the
/// worker stops. A worker that never reports would otherwise leave the
/// activity row spinning for the rest of the session, so the wait is
/// bounded and the app says that it is giving up.
pub(crate) const INTERRUPT_GRACE: Duration = Duration::from_secs(3);

/// How long a shutdown waits for a background worker before it gives up.
///
/// The worker is asked to stop exactly as the interrupt path asks it to
/// stop, so it gets the same time to notice the cancellation and unwind:
/// the interrupt path's own grace. A shorter bound here would report an
/// abandonment for a worker that was about to finish; a longer one would
/// keep the terminal in raw mode for a worker that never will. A worker
/// that misses the bound is detached — it finishes on its own — and the
/// app says that it gave up on it.
pub(crate) const SHUTDOWN_JOIN_GRACE: Duration = Duration::from_secs(3);

/// The status line keeps at most this many characters.
///
/// The status is one row of the terminal. An unbounded status pushes the
/// outcome off the screen after a few appends.
const STATUS_MAX_CHARS: usize = 200;

/// How long a test waits for the background refresh before it gives up.
///
/// The refresh worker opens the memory backend and probes the executor,
/// so it can take a moment. A worker that wedges must not hang a test,
/// so the wait is bounded and the caller is told it timed out.
#[cfg(test)]
const REFRESH_JOIN_GRACE: Duration = Duration::from_secs(10);

pub struct App {
    pub state: AbbeyState,
    pub cfg: AgentConfig,
    pub theme_id: ThemeId,
    pub theme: Theme,
    pub transcript: Transcript,
    pub composer: Composer,
    pub history_log: History,
    pub files: FileIndex,
    pub completion: Option<Completion>,
    pub completion_idx: usize,
    pub completion_basis: Option<(String, usize)>,
    pub prediction: super::prediction_owner::PredictionOwner,
    /// A clipboard copy in flight, if any.
    pub clipboard: Option<super::clipboard::PendingCopy>,
    pub run: Option<RunHandle>,
    pub(crate) pending_model_mutation: Option<ModelMutation>,
    pub queued: Vec<String>,
    pub overlay: Overlay,
    pub panel_view: super::overlays::PanelView,
    pub scroll_from_bottom: usize,
    pub status: String,
    pub should_quit: bool,
    pub ctrl_c_armed: bool,
    pub last_esc: Option<EscWindow>,
    pub last_submitted: Option<String>,
    pub pending_suspend: Option<Suspend>,
    pub tick: u64,
    pub claims_lines: Vec<String>,
    pub route_lines: Vec<String>,
    pub doctor_lines: Vec<String>,
    pub history: Vec<crate::state::HistoryEntry>,
    pub aliases: Vec<(String, String)>,
    pub live_models: Vec<String>,
    pub persona_lines: Vec<String>,
    pub memory_lines: Vec<String>,
    pub skill_lines: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum ModelMutation {
    Set,
    Clear,
}

impl ModelMutation {
    pub(crate) fn for_local(args: &[String]) -> Option<Self> {
        let [input] = args else { return None };
        let (name, rest) = crate::slash::parse_slash(input)?;
        let name = crate::slash_alias::resolve_name(name).unwrap_or(name);
        match (name, rest.trim()) {
            ("model", "") | ("think" | "thinking", "") => None,
            ("model", "clear") => Some(Self::Clear),
            ("model" | "think" | "thinking", _) => Some(Self::Set),
            _ => None,
        }
    }
}

/// A picker-open snapshot, not a readiness or account-entitlement claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderChoice {
    pub backend: AgentBackend,
    pub executable: std::result::Result<std::path::PathBuf, String>,
}

pub(crate) const PROVIDERS: [AgentBackend; 6] = [
    AgentBackend::Ollama,
    AgentBackend::Grok,
    AgentBackend::Fm,
    AgentBackend::Abi,
    AgentBackend::Claude,
    AgentBackend::Cursor,
];

// Keep the displayed selection in the exact vocabulary accepted by fm_model.
fn fm_selection(model: &str) -> String {
    match model.trim().to_ascii_lowercase().as_str() {
        "pcc" | "private-cloud-compute" | "private_cloud_compute" => "pcc",
        _ => "system",
    }
    .into()
}

impl App {
    /// Build the app without asking the terminal what it looks like.
    ///
    /// A caller that cannot query the terminal gets the dark palette. The
    /// real entry point asks, because a dark palette in a light terminal
    /// profile is illegible.
    ///
    /// This is what the test fixtures use, so they do not depend on a
    /// terminal answering.
    #[cfg(test)]
    pub fn new(state: AbbeyState, cfg: AgentConfig) -> Result<Self> {
        Self::new_with_brightness(state, cfg, TerminalBrightness::Dark)
    }

    /// Build the app with the terminal's reported appearance in hand.
    ///
    /// The brightness only decides the theme when the user has not chosen
    /// one; see [`ThemeId::resolve`].
    pub fn new_with_brightness(
        state: AbbeyState,
        mut cfg: AgentConfig,
        brightness: TerminalBrightness,
    ) -> Result<Self> {
        // Startup already resolved CLI flags and backend-specific persisted models.
        // Re-reading here would discard explicit selections (and ABI literal IDs).
        if cfg.backend == AgentBackend::Fm {
            cfg.model = fm_selection(&cfg.model);
        }
        let theme_id = ThemeId::resolve(&state.state_dir, brightness);
        let history_log = History::load(&state.state_dir);
        let files = FileIndex::load(&state.cwd);
        let app = Self {
            history: state.history(40),
            aliases: models::alias_table()
                .iter()
                .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
                .collect(),
            state,
            cfg,
            theme_id,
            theme: Theme::from_id(theme_id),
            transcript: Transcript::default(),
            composer: Composer::default(),
            history_log,
            files,
            completion: None,
            completion_idx: 0,
            completion_basis: None,
            prediction: super::prediction_owner::PredictionOwner::default(),
            clipboard: None,
            run: None,
            pending_model_mutation: None,
            queued: Vec::new(),
            overlay: Overlay::None,
            panel_view: super::overlays::PanelView::default(),
            scroll_from_bottom: 0,
            status: "Enter send · Shift-Enter newline · Esc interrupt · Ctrl-K palette · F1 help"
                .into(),
            should_quit: false,
            ctrl_c_armed: false,
            last_esc: None,
            last_submitted: None,
            pending_suspend: None,
            tick: 0,
            claims_lines: Vec::new(),
            route_lines: Vec::new(),
            doctor_lines: Vec::new(),
            live_models: Vec::new(),
            persona_lines: Vec::new(),
            memory_lines: Vec::new(),
            skill_lines: Vec::new(),
        };
        // The panels are filled by a background refresh that the event
        // loop starts: the doctor panel's `--version` probe and the
        // memory backend's open are slow, and they must not run on the
        // thread that draws.
        Ok(app)
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Wait for the background refresh in flight to finish.
    ///
    /// The refresh worker writes into the state directory: the memory
    /// backend's open creates the store there. A test that removes that
    /// directory has to wait for the worker first, or it removes a
    /// directory a worker is still writing into.
    ///
    /// The wait is bounded — a worker that wedges must not hang a test —
    /// and the answer says whether the worker finished. The collected
    /// data is dropped: the caller wants the worker done, not the
    /// panels filled.
    #[cfg(test)]
    pub(crate) fn join_refresh(&mut self) -> bool {
        let deadline = std::time::Instant::now() + REFRESH_JOIN_GRACE;
        loop {
            let poll = self.panel_view.refresh.as_ref().map(|w| w.poll());
            match poll {
                // Nothing is in flight.
                None => return true,
                Some(super::refresh::RefreshPoll::Pending) => {}
                Some(
                    super::refresh::RefreshPoll::Finished(_)
                    | super::refresh::RefreshPoll::Failed(_),
                ) => {
                    // The worker is done with the state directory: it
                    // either collected its data or it died. The data is
                    // dropped with it.
                    self.panel_view.refresh = None;
                    return true;
                }
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Move to the next palette, without choosing it.
    ///
    /// Cycling is a preview: it does not persist the palette. A persisted
    /// palette wins over the terminal's own brightness for every later
    /// session, so a stray cycle key must not quietly decide that for
    /// the user. Choosing a palette in the palette overlay is the
    /// deliberate choice, and that one is persisted.
    pub fn cycle_theme(&mut self) {
        self.theme_id = self.theme_id.cycle();
        self.theme = Theme::from_id(self.theme_id);
        self.status = format!("theme → {} (not saved)", self.theme_id.as_str());
    }

    /// Choose a palette and remember it.
    ///
    /// This is the explicit choice: the palette is written to the state
    /// directory, so it survives the session and keeps winning over the
    /// terminal's reported brightness.
    pub fn select_theme(&mut self, id: ThemeId) {
        self.theme_id = id;
        self.theme = Theme::from_id(id);
        match ThemeId::save(&self.state.state_dir, id) {
            Ok(()) => self.status = format!("theme → {} (saved)", id.as_str()),
            Err(error) => self.status = format!("theme → {} (not saved: {error})", id.as_str()),
        }
    }

    /// Copy the last reply to the host's clipboard.
    ///
    /// The copy runs on a worker, so a clipboard command that hangs cannot
    /// freeze the TUI's event loop. The status line says what happened in
    /// every case — copied, no clipboard command on this host, or the
    /// command's own refusal. It never reports a success that did not
    /// happen.
    pub fn copy_last_reply(&mut self) {
        let text = self.transcript.last_reply();
        if text.is_empty() {
            self.status = "nothing to copy: no reply yet".into();
            return;
        }
        // A copy already in flight is *not* joined here: joining would
        // block the event loop on a clipboard command that may be hung,
        // which is the whole reason the copy runs on a worker. Dropping the
        // handle detaches that worker; it finishes on its own, and its
        // result is discarded because the receiver is gone.
        self.clipboard = Some(super::clipboard::PendingCopy::start(text));
        self.status = "copying the last reply…".into();
    }

    /// Collect a finished copy, if there is one.
    ///
    /// Called from the pump, so the status line learns the outcome without
    /// the event loop ever waiting for the clipboard. A worker that died
    /// without an answer is a failure, not a copy that is still running:
    /// the status must not stay on "copying…" for the rest of the session.
    pub fn poll_clipboard(&mut self) {
        let Some(pending) = &self.clipboard else {
            return;
        };
        let outcome = match pending.poll() {
            super::clipboard::CopyPoll::Pending => return,
            super::clipboard::CopyPoll::Finished(outcome) => outcome,
            super::clipboard::CopyPoll::Failed(reason) => {
                self.clipboard = None;
                self.status = format!("the clipboard copy failed: {reason}");
                return;
            }
        };
        self.clipboard = None;
        self.status = match outcome {
            super::clipboard::CopyOutcome::Copied => {
                format!("copied the last reply ({})", outcome.describe())
            }
            _ => outcome.describe(),
        };
    }

    /// Switch to the next executor backend whose binary actually resolves.
    pub fn cycle_backend(&mut self) {
        if self.is_running() {
            self.status = "provider unchanged: interrupt or finish the active run first".into();
            return;
        }
        let mut next = self.cfg.backend;
        for _ in 0..5 {
            next = next.cycle_next();
            let Ok(path) = crate::agent::resolve_agent_for(next) else {
                continue;
            };
            self.select_provider(next, path);
            return;
        }
        self.status = "backend: no other executor resolvable on this host".into();
    }

    pub(crate) fn open_provider_picker(&mut self) -> Overlay {
        if self.is_running() {
            self.status = "provider unchanged: interrupt or finish the active run first".into();
            return Overlay::None;
        }
        let choices = PROVIDERS
            .into_iter()
            .map(|backend| ProviderChoice {
                backend,
                executable: if backend == self.cfg.backend
                    && crate::host::is_executable(&self.cfg.agent_path)
                {
                    Ok(self.cfg.agent_path.clone())
                } else {
                    crate::agent::resolve_agent_for(backend).map_err(|e| e.to_string())
                },
            })
            .collect();
        self.status =
            "provider: installed means CLI found, not model/account ready · /doctor".into();
        Overlay::ProviderPicker {
            choices,
            idx: PROVIDERS
                .iter()
                .position(|b| *b == self.cfg.backend)
                .unwrap_or(0),
        }
    }

    pub(crate) fn select_provider(&mut self, backend: AgentBackend, path: std::path::PathBuf) {
        if self.is_running() {
            self.status = "provider unchanged: interrupt or finish the active run first".into();
            return;
        }
        let reset_model = if self.cfg.backend == backend {
            None
        } else if backend == AgentBackend::Fm {
            Some("system")
        } else if self.cfg.backend == AgentBackend::Fm {
            Some("auto")
        } else {
            None
        };
        if let Some(model) = reset_model {
            // Persist the exact local choice, so a later local command refresh
            // cannot resurrect a cloud selection, vendor alias, or Apple-only
            // model in another provider. Commit before changing live identity.
            if let Err(e) = self.state.save_model_literal(model) {
                self.status = format!("provider unchanged: could not select {model}: {e}");
                return;
            }
            self.cfg.model = model.into();
        }
        self.cfg.backend = backend;
        self.cfg.agent_path = path;
        self.cfg.transcript_dir = Some(self.state.state_dir.join(backend.transcript_subdir()));
        self.live_models.clear();
        // The panels are filled in the background: the doctor panel's
        // `--version` probe must not run on the thread that draws.
        self.request_refresh();
        self.status = if backend == AgentBackend::Fm {
            format!(
                "provider → Apple Foundation Models · {} · readiness: /doctor",
                self.cfg.model
            )
        } else {
            format!("backend → {}", backend.label())
        };
    }

    /// Refresh the panels without blocking the UI thread.
    ///
    /// The collection runs on a worker thread and the result is applied
    /// on the UI thread when it lands, so a refresh never delays the
    /// frame the user is reading.
    pub fn refresh_all(&mut self) {
        self.status = "refreshing…".into();
        self.request_refresh();
    }

    /// Submit the composer: slash is routed, `!cmd` confirms, anything else is a turn.
    pub fn submit(&mut self) {
        let text = self.composer.text.trim_end().to_string();
        if text.trim().is_empty() {
            return;
        }
        if self.is_running() {
            self.queued.push(text);
            self.composer.take();
            self.completion = None;
            self.status = format!("queued ({})", self.queued.len());
            return;
        }
        self.composer.take();
        self.completion = None;
        self.submit_text(text);
    }

    fn submit_text(&mut self, text: String) {
        self.history_log.push(&text);
        self.last_submitted = Some(text.clone());
        self.scroll_from_bottom = 0;
        if let Some(cmd) = text.strip_prefix('!') {
            let command = super::command::split(cmd);
            if !command.is_empty() {
                self.overlay = Overlay::Confirm { command };
            }
            return;
        }
        if text.starts_with('/') {
            self.run_slash(&text);
            return;
        }
        self.transcript.push_user(&text);
        self.start(RunKind::Prompt { text, fresh: false });
    }

    pub(crate) fn start(&mut self, kind: RunKind) {
        if self.is_running() {
            self.status = "a run is already active".into();
            return;
        }
        if let Err(error) = self.prediction.dismiss() {
            self.transcript.push_error(format!("prediction: {error:#}"));
            return;
        }
        let model_mutation = match &kind {
            RunKind::Local(args) => ModelMutation::for_local(args),
            _ => None,
        };
        match worker::spawn_run(self.cfg.clone(), self.state.clone(), kind) {
            Ok(handle) => {
                self.pending_model_mutation = model_mutation;
                self.transcript.begin_turn();
                self.run = Some(handle);
                self.status = "running · Esc to interrupt".into();
            }
            Err(e) => self
                .transcript
                .push_error(format!("could not start run: {e}")),
        }
    }

    fn run_slash(&mut self, text: &str) {
        self.transcript.push_user(text);
        match worker::route_slash(text) {
            SlashRoute::Agent => self.start(RunKind::AgentSlash(text.to_string())),
            SlashRoute::Interactive => {
                self.pending_suspend = Some(Suspend::InteractiveSlash(text.to_string()));
            }
            SlashRoute::Local => self.run_local(&[text.to_string()]),
        }
    }

    /// Captured `abbey …` child; output lands in the transcript.
    pub(crate) fn run_local(&mut self, args: &[String]) {
        self.start(RunKind::Local(args.to_vec()));
    }

    /// Execute a confirmed `!cmd` through the OS allowlist gate.
    pub(crate) fn run_confirmed(&mut self, command: Vec<String>) {
        let mut args = vec!["os".to_string(), "execute".into(), "--confirm".into()];
        args.extend(command);
        self.run_local(&args);
    }

    /// Apply at most 128 events per tick, then finalize only after the queue
    /// is drained. Completion is a worker outcome, never an executor event.
    pub fn pump(&mut self) {
        // Collect a finished clipboard copy before anything else, so the
        // status line is never a frame behind.
        self.poll_clipboard();
        // Apply a background refresh that has landed. The collection ran
        // on its own thread; only the write-back happens here. A worker
        // that died is reported: "still refreshing" and "the worker is
        // gone" are different facts, and the status line must not spin
        // on the first one for the rest of the session.
        let refreshed = self.panel_view.refresh.as_ref().map(|w| w.poll());
        match refreshed {
            Some(super::refresh::RefreshPoll::Finished(payload)) => {
                self.panel_view.refresh = None;
                self.apply_refresh(payload);
                if self.status.starts_with("refreshing") {
                    self.status = "refreshed".into();
                }
            }
            Some(super::refresh::RefreshPoll::Failed(reason)) => {
                self.panel_view.refresh = None;
                self.status = format!("the panel refresh failed: {reason}");
            }
            Some(super::refresh::RefreshPoll::Pending) | None => {}
        }
        let allowed = !self.is_running() && matches!(self.overlay, Overlay::None);
        match self.prediction.pump(
            &self.composer.text,
            self.composer.cursor,
            &self.cfg,
            self.tick,
            allowed,
        ) {
            Ok(Some(hint)) => {
                self.completion = super::completion::complete(
                    &self.composer.text,
                    self.composer.cursor,
                    self.history_log.entries(),
                    &self.files,
                    Some(hint),
                );
                self.completion_idx = 0;
                self.completion_basis = Some((self.composer.text.clone(), self.composer.cursor));
            }
            Ok(None) => {}
            Err(error) => self.transcript.push_error(format!("prediction: {error:#}")),
        }
        use crate::stream::StreamEvent;
        use std::sync::mpsc::TryRecvError;
        let Some(run) = &mut self.run else {
            return;
        };
        // An interrupt asks the worker to stop; it does not promise that
        // the worker stops. Give up on a worker that has not reported by
        // the end of the grace, so the app cannot spin on it forever.
        if run.completion.is_none()
            && let Some(requested) = run.cancel_requested
            && requested.elapsed() >= INTERRUPT_GRACE
        {
            run.abandon();
            run.completion = Some((
                130,
                Some(format!(
                    "the interrupt did not finish within {}s; the app gave up on it",
                    INTERRUPT_GRACE.as_secs()
                )),
            ));
        }
        if run.completion.is_none() {
            match run.done.try_recv() {
                Ok(result) => run.completion = Some(result),
                Err(TryRecvError::Disconnected) => {
                    run.cancel.cancel();
                    let message = run
                        .join()
                        .err()
                        .unwrap_or("run worker disconnected before completion");
                    run.completion = Some((1, Some(message.into())));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        // Observe completion before draining events: the worker sends its
        // outcome last. Joining first also closes every event producer after
        // a forced cancellation, so an empty queue cannot race a final send.
        if run.completion.is_some()
            && let Err(message) = run.join()
        {
            run.completion = Some((1, Some(message.into())));
        }
        let completed_before_drain = run.completion.is_some();
        let mut drained = false;
        for _ in 0..128 {
            // An abandoned worker still holds the event sender, so the
            // queue can never report "drained". The run is over either
            // way: stop waiting for events it will not send.
            if run.abandoned {
                drained = true;
                break;
            }
            match run.events.try_recv() {
                Ok(StreamEvent::Done { .. }) => {}
                Ok(ev) => {
                    if !self.transcript.apply(ev) {
                        run.cancel.cancel();
                        run.completion = Some((
                            1,
                            Some("active transcript exceeded its retention limit".into()),
                        ));
                    }
                }
                Err(_) => {
                    drained = true;
                    break;
                }
            }
        }
        if !drained || !completed_before_drain {
            return;
        }
        let mut run = self.run.take().expect("active run");
        let (mut code, mut err) = run.completion.take().expect("worker completed");
        if let Err(message) = run.join() {
            code = 1;
            err = Some(message.into());
        }
        // An interrupt received before finalization also stops the queue even
        // if the executor happened to exit successfully at the same instant.
        if code == 0 && run.cancel.is_cancelled() {
            code = 130;
        }
        if let Some(e) = err
            && !matches!(self.transcript.cells.last(), Some(super::transcript::Cell::Error(s)) if s == &e)
        {
            self.transcript.push_error(e);
        }
        self.transcript.apply(StreamEvent::Done { exit: code });
        self.status = format!("done · exit {code} · {}s", run.started.elapsed().as_secs());
        self.history = self.state.history(40);
        let model_mutation = self.pending_model_mutation.take();
        if run.local {
            if code == 0
                && let Some(mutation) = model_mutation
            {
                // Ambient ABBEY_MODEL is a startup preference, not the result
                // of a live selection. Only explicit successful mutations may
                // replace the model; failed/cancelled runs retain it.
                let selected = match mutation {
                    ModelMutation::Clear => Ok("auto".into()),
                    ModelMutation::Set => {
                        std::fs::File::open(&self.state.model_file).and_then(|file| {
                            let mut text = String::new();
                            file.take(64 * 1024 + 1).read_to_string(&mut text)?;
                            if text.len() > 64 * 1024 {
                                return Err(std::io::Error::other("selected model exceeds limit"));
                            }
                            Ok(text.lines().next().unwrap_or("").trim().to_string())
                        })
                    }
                };
                match selected {
                    Ok(model) if !model.is_empty() => {
                        self.cfg.model = if self.cfg.backend == AgentBackend::Fm {
                            fm_selection(&model)
                        } else {
                            model
                        };
                    }
                    _ => self
                        .transcript
                        .push_error("could not read selected model; live model unchanged"),
                }
            }
            if code == 0 {
                self.refresh_all();
                self.status = format!("done · exit {code} · {}s", run.started.elapsed().as_secs());
            } else {
                self.request_refresh();
            }
        } else {
            self.request_refresh();
        }
        if code != 0 || self.should_quit {
            self.discard_queue();
        } else if !self.queued.is_empty() {
            let next = self.queued.remove(0);
            self.submit_text(next);
        }
    }

    pub(crate) fn discard_queue(&mut self) {
        let count = self.queued.len();
        self.queued.clear();
        if count > 0 {
            let notice = format!("discarded {count} queued prompt(s)");
            self.transcript.push_notice(&notice);
            self.status = append_status(&self.status, &notice);
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        // The prediction workers are joined here, not on the UI thread:
        // the app is leaving, so nothing is stale any more.
        let prediction = self.prediction.shutdown();
        self.discard_queue();
        // Join a clipboard copy that is still running, so the worker is not
        // left writing to a clipboard command while the terminal is going
        // away. The wait is bounded: a hung clipboard command must not
        // keep the terminal in raw mode for ever.
        if let Some(pending) = self.clipboard.take() {
            let _ = pending.join_within(super::clipboard::JOIN_GRACE);
        }
        // The background refresh owns the doctor's `--version` probe. The
        // app is leaving, so the probe must not outlive the terminal: the
        // supervisor kills it on its own deadline, and this join waits for
        // that. Without the join the app can exit while the probe is still
        // running, and the process outlives the terminal it was started
        // under.
        if let Some(refresh) = self.panel_view.refresh.take()
            && !refresh.join_within(super::refresh::REFRESH_JOIN_GRACE)
        {
            let message = format!(
                "the refresh worker did not finish within {}s; the app gave up on it",
                super::refresh::REFRESH_JOIN_GRACE.as_secs()
            );
            self.status = append_status(&self.status, &message);
            self.transcript.push_error(message);
        }
        let mut run_result = Ok(());
        if let Some(mut run) = self.run.take() {
            run.cancel.cancel();
            match run.join_within(SHUTDOWN_JOIN_GRACE) {
                JoinOutcome::Finished => {}
                JoinOutcome::Panicked => {
                    run_result = Err(anyhow::Error::msg("run worker panicked"));
                }
                JoinOutcome::Abandoned => {
                    // The worker did not stop. The app gives up on it
                    // rather than waiting for ever — the terminal is
                    // still in raw mode — and says so: the status line
                    // and the transcript carry the outcome, and the
                    // shutdown reports it instead of a quiet success.
                    let message = format!(
                        "the run worker did not finish within {}s; the app gave up on it",
                        SHUTDOWN_JOIN_GRACE.as_secs()
                    );
                    self.status = message.clone();
                    self.transcript.push_error(message.clone());
                    run_result = Err(anyhow::Error::msg(message));
                }
            }
        }
        // A prediction worker that would not stop is reported the same
        // way, but it does not fail the shutdown: the rerank is optional
        // background work, and the user's turn is not what failed.
        let abandoned = self.prediction.abandoned();
        if abandoned > 0 {
            let message = format!(
                "{abandoned} prediction worker(s) did not finish within {}s; the app gave up on them",
                SHUTDOWN_JOIN_GRACE.as_secs()
            );
            self.status = append_status(&self.status, &message);
            self.transcript.push_error(message);
        }
        prediction?;
        run_result?;
        Ok(())
    }
}

/// Append a segment to the status line, keeping the line bounded.
///
/// The status is one row of the terminal. An unbounded status pushes the
/// outcome off the screen after a few appends, so the line is trimmed
/// to its newest words: the tail is what just happened.
fn append_status(status: &str, segment: &str) -> String {
    let line = format!("{status} · {segment}");
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= STATUS_MAX_CHARS {
        return line;
    }
    let mut bounded = String::from("…");
    bounded.extend(&chars[chars.len() - (STATUS_MAX_CHARS - 1)..]);
    bounded
}
