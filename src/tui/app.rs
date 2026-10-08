//! Chat-first TUI state. Drawing lives in `render.rs`, keys in `keymap.rs`,
//! the terminal loop in `run_loop.rs`.

use super::completion::{Completion, FileIndex};
use super::composer::{Composer, History};
use super::theme::{Theme, ThemeId};
use super::transcript::Transcript;
use super::worker::{self, RunHandle, RunKind, SlashRoute};
use crate::agent::AgentConfig;
use crate::models;
use crate::state::AbbeyState;
use anyhow::Result;
use std::time::Instant;

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
    Palette { query: String, idx: usize },
    Help,
    Panel(Panel),
    ModelPicker { idx: usize },
    ResumePicker { idx: usize },
    Confirm { command: Vec<String> },
    Search { query: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Suspend {
    Editor,
    InteractiveSlash(String),
}

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
    pub run: Option<RunHandle>,
    pub queued: Vec<String>,
    pub overlay: Overlay,
    pub panel_view: super::overlays::PanelView,
    pub scroll_from_bottom: usize,
    pub status: String,
    pub should_quit: bool,
    pub ctrl_c_armed: bool,
    pub last_esc: Option<Instant>,
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

impl App {
    pub fn new(state: AbbeyState, mut cfg: AgentConfig) -> Result<Self> {
        cfg.model = state.read_model();
        let theme_id = ThemeId::resolve(&state.state_dir);
        let history_log = History::load(&state.state_dir);
        let files = FileIndex::load(&state.cwd);
        let mut app = Self {
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
            run: None,
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
        app.refresh_doctor();
        app.refresh_personas();
        app.refresh_memory();
        app.refresh_skills();
        if !app.cfg.backend.supports_account_surface() {
            app.refresh_models_live();
        }
        Ok(app)
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn cycle_theme(&mut self) {
        self.theme_id = self.theme_id.cycle();
        self.theme = Theme::from_id(self.theme_id);
        let _ = ThemeId::save(&self.state.state_dir, self.theme_id);
        self.status = format!("theme → {}", self.theme_id.as_str());
    }

    pub fn refresh_models_live(&mut self) {
        if let Ok(text) = self.cfg.list_models_text() {
            self.live_models = text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
        }
    }

    /// Switch to the next executor backend whose binary actually resolves.
    pub fn cycle_backend(&mut self) {
        let mut next = self.cfg.backend;
        for _ in 0..5 {
            next = next.cycle_next();
            let Ok(path) = crate::agent::resolve_agent_for(next) else {
                continue;
            };
            self.cfg.backend = next;
            self.cfg.agent_path = path;
            self.cfg.transcript_dir = Some(self.state.state_dir.join(next.transcript_subdir()));
            self.live_models.clear();
            if !next.supports_account_surface() {
                self.refresh_models_live();
            }
            self.refresh_doctor();
            self.status = format!("backend → {}", next.label());
            return;
        }
        self.status = "backend: no other executor resolvable on this host".into();
    }

    pub fn refresh_all(&mut self) {
        self.refresh_doctor();
        self.refresh_personas();
        self.refresh_memory();
        self.refresh_skills();
        self.history = self.state.history(40);
        self.status = "refreshed".into();
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
            let command: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
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
        match worker::spawn_run(self.cfg.clone(), self.state.clone(), kind) {
            Ok(handle) => {
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
        if run.local {
            self.cfg.model = self.state.read_model();
            if code == 0 {
                self.refresh_all();
                self.status = format!("done · exit {code} · {}s", run.started.elapsed().as_secs());
            } else {
                self.refresh_memory();
            }
        } else {
            self.refresh_memory();
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
            self.status.push_str(&format!(" · {notice}"));
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        let prediction = self.prediction.dismiss();
        self.discard_queue();
        if let Some(mut run) = self.run.take() {
            run.cancel.cancel();
            let run_result = run.join().map_err(anyhow::Error::msg);
            prediction?;
            run_result?;
        } else {
            prediction?;
        }
        Ok(())
    }
}
