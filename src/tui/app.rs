//! TUI application state and event loop.

use crate::agent::AgentConfig;
use crate::models;
use crate::state::AbbeyState;
use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::stdout;
use std::time::Duration;

use super::theme::{Theme, ThemeId};

pub use super::tabs::{Focus, OverlayKind, PendingAction, Tab};

pub struct App {
    pub state: AbbeyState,
    pub cfg: AgentConfig,
    pub tab: Tab,
    pub focus: Focus,
    pub theme_id: ThemeId,
    pub theme: Theme,
    pub input: String,
    pub cursor: usize,
    pub status: String,
    pub list_idx: usize,
    pub scroll: usize,
    pub filter: String,
    pub filtering: bool,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    pub tick: u64,
    pub overlay: OverlayKind,
    pub overlay_query: String,
    pub overlay_idx: usize,
    pub should_quit: bool,
    pub pending: PendingAction,
    pub last_agent_code: Option<i32>,
    /// Compact tail of `route.jsonl` for the Home Routes pane (audit only).
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
        let history = state.history(40);
        let aliases = models::alias_table()
            .iter()
            .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
            .collect();
        let theme_id = ThemeId::resolve(&state.state_dir);
        let mut app = Self {
            state,
            cfg,
            tab: Tab::Home,
            focus: Focus::Prompt,
            theme_id,
            theme: Theme::from_id(theme_id),
            input: String::new(),
            cursor: 0,
            status: "Enter run · ` focus · Ctrl-K palette · Ctrl-T theme · ? help".into(),
            list_idx: 0,
            scroll: 0,
            filter: String::new(),
            filtering: false,
            input_history: Vec::new(),
            history_idx: None,
            tick: 0,
            overlay: OverlayKind::None,
            overlay_query: String::new(),
            overlay_idx: 0,
            should_quit: false,
            pending: PendingAction::None,
            last_agent_code: None,
            route_lines: Vec::new(),
            doctor_lines: Vec::new(),
            history,
            aliases,
            live_models: Vec::new(),
            persona_lines: Vec::new(),
            memory_lines: Vec::new(),
            skill_lines: Vec::new(),
        };
        app.refresh_doctor();
        app.refresh_personas();
        app.refresh_memory();
        app.refresh_skills();
        // Non-cursor backends answer `models` statically (no exec) — prefetch
        // so the Models tab never falls back to the cursor alias table there.
        if !app.cfg.backend.supports_account_surface() {
            app.refresh_models_live();
        }
        Ok(app)
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
    ///
    /// Unresolvable backends (no `fm` on this OS, `abi` only a shell alias)
    /// are skipped with no state change; if nothing else resolves the current
    /// backend stays and the status line says so. Chat ids are per-backend
    /// artifacts — the next run under a server backend simply resumes or
    /// mints as usual, and `fm`/`abi` mint locally.
    pub fn cycle_backend(&mut self) {
        let mut next = self.cfg.backend;
        for _ in 0..3 {
            next = next.cycle_next();
            let Ok(path) = crate::agent::resolve_agent_for(next) else {
                continue;
            };
            self.cfg.backend = next;
            self.cfg.agent_path = path;
            // Transcripts are per-backend; without this a switch to abi would
            // append abi turns into the fm directory chosen at startup.
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
        self.status = "refreshed".into();
    }

    pub fn filtered_lines(&self) -> Vec<String> {
        let raw: Vec<String> = match self.tab {
            Tab::Home => self
                .history
                .iter()
                .map(|e| format!("{}  {}  {}", e.timestamp, e.chat_id, e.cwd))
                .collect(),
            Tab::Chats => self
                .history
                .iter()
                .map(|e| format!("{}  {}  {}", e.timestamp, e.chat_id, e.cwd))
                .collect(),
            Tab::Personas => self.persona_lines.clone(),
            Tab::Memory => self.memory_lines.clone(),
            Tab::Skills => self.skill_lines.clone(),
            Tab::Models => {
                if self.live_models.is_empty() {
                    self.aliases
                        .iter()
                        .map(|(a, full)| format!("{a:<12} {full}"))
                        .collect()
                } else {
                    self.live_models.clone()
                }
            }
            Tab::Doctor => self.doctor_lines.clone(),
        };
        let f = self.filter.trim().to_ascii_lowercase();
        if f.is_empty() || !matches!(self.tab, Tab::Home | Tab::Chats | Tab::Models | Tab::Skills) {
            return raw;
        }
        raw.into_iter()
            .filter(|l| l.to_ascii_lowercase().contains(&f))
            .collect()
    }

    pub fn list_len(&self) -> usize {
        self.filtered_lines().len()
    }

    pub fn ensure_visible(&mut self, viewport: usize) {
        let len = self.list_len();
        if len == 0 {
            self.list_idx = 0;
            self.scroll = 0;
            return;
        }
        if self.list_idx >= len {
            self.list_idx = len - 1;
        }
        if self.list_idx < self.scroll {
            self.scroll = self.list_idx;
        } else if viewport > 0 && self.list_idx >= self.scroll + viewport {
            self.scroll = self.list_idx + 1 - viewport;
        }
    }

    pub fn kpi_chips(&self) -> Vec<(String, String)> {
        let chat = self
            .state
            .read_chat_for(self.cfg.backend)
            .map(|c| c.chars().take(8).collect::<String>())
            .unwrap_or_else(|| "—".into());
        let persona = self
            .persona_lines
            .first()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("abbey")
            .to_string();
        let role = self
            .persona_lines
            .iter()
            .find(|l| l.starts_with("default_role:"))
            .map(|l| l.trim_start_matches("default_role:").trim().to_string())
            .unwrap_or_else(|| "auto".into());
        let mem = self
            .memory_lines
            .first()
            .map(|s| {
                if s.len() > 18 {
                    format!("{}…", &s[..16])
                } else {
                    s.clone()
                }
            })
            .unwrap_or_else(|| "—".into());
        let last = self
            .last_agent_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".into());
        vec![
            ("backend".into(), self.cfg.backend.label().to_string()),
            ("model".into(), self.cfg.model.clone()),
            ("chat".into(), chat),
            ("persona".into(), persona),
            ("role".into(), role),
            ("last".into(), last),
            ("mem".into(), mem),
        ]
    }
}

pub fn run_tui(state: AbbeyState, cfg: AgentConfig) -> Result<i32> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(state, cfg)?;
    let mut code = 0i32;

    let result = (|| -> Result<i32> {
        loop {
            app.ensure_visible(12);
            terminal.draw(|f| super::ui::draw(f, &app))?;

            match app.pending {
                PendingAction::None => {}
                action => {
                    app.pending = PendingAction::None;
                    disable_raw_mode()?;
                    execute!(
                        terminal.backend_mut(),
                        LeaveAlternateScreen,
                        DisableMouseCapture
                    )?;
                    terminal.show_cursor()?;

                    let prompt: Vec<String> = if app.input.trim().is_empty() {
                        Vec::new()
                    } else {
                        vec![app.input.trim().to_string()]
                    };

                    let mut was_slash = false;
                    let run_code = match action {
                        PendingAction::RunSession { fresh } => {
                            let spec = if fresh {
                                crate::actions::RunSpec::fresh()
                            } else {
                                crate::actions::RunSpec::resume()
                            };
                            crate::actions::run_agent(&mut app.cfg, &app.state, &prompt, spec)?
                        }
                        PendingAction::RunPleaseFix => {
                            let text = crate::please_fix::build_prompt_soft(&app.input);
                            crate::actions::run_agent(
                                &mut app.cfg,
                                &app.state,
                                &[text],
                                crate::actions::RunSpec::max(),
                            )?
                        }
                        PendingAction::Slash(cmd) => {
                            was_slash = true;
                            match crate::slash_dispatch::dispatch_slash(
                                &cmd,
                                &app.state,
                                &mut app.cfg,
                            ) {
                                Ok(c) => {
                                    app.status = format!("{cmd} → exit {c}");
                                    c
                                }
                                Err(e) => {
                                    app.status = format!("slash error: {e}");
                                    1
                                }
                            }
                        }
                        PendingAction::None => 0,
                    };

                    app.last_agent_code = Some(run_code);
                    if !was_slash {
                        app.status = format!("agent exited {run_code} · Enter to run again");
                    }
                    app.input.clear();
                    app.cursor = 0;
                    app.overlay = OverlayKind::None;
                    app.refresh_doctor();
                    app.refresh_memory();
                    code = run_code;

                    enable_raw_mode()?;
                    execute!(
                        terminal.backend_mut(),
                        EnterAlternateScreen,
                        EnableMouseCapture
                    )?;
                    terminal.hide_cursor()?;
                    terminal.clear()?;
                }
            }

            if app.should_quit {
                break;
            }

            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => app.handle_key(key),
                    Event::Mouse(m) => app.handle_mouse(m.kind),
                    _ => {}
                }
            }
            app.tick = app.tick.wrapping_add(1);
        }
        Ok(code)
    })();

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
