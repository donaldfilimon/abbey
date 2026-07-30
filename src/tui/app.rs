//! TUI application state and event loop.

use crate::agent::AgentConfig;
use crate::config;
use crate::inventory;
use crate::learn;
use crate::memory;
use crate::models;
use crate::persona;
use crate::roles;
use crate::state::AbbeyState;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::stdout;
use std::time::Duration;

pub use super::tabs::{PendingAction, Tab};

pub struct App {
    pub state: AbbeyState,
    pub cfg: AgentConfig,
    pub tab: Tab,
    pub input: String,
    pub cursor: usize,
    pub status: String,
    pub list_idx: usize,
    pub should_quit: bool,
    pub pending: PendingAction,
    pub last_agent_code: Option<i32>,
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
        let mut app = Self {
            state,
            cfg,
            tab: Tab::Home,
            input: String::new(),
            cursor: 0,
            status: "Enter run · /help · Ctrl-n new · Ctrl-p fix · Tab panels · q quit".into(),
            list_idx: 0,
            should_quit: false,
            pending: PendingAction::None,
            last_agent_code: None,
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
        Ok(app)
    }

    pub fn refresh_personas(&mut self) {
        let ac = config::AbbeyConfig::load().unwrap_or_default();
        let mut lines = persona::persona_status_lines("");
        lines.extend(roles::role_status_lines(&ac.roles.max, &ac.roles.gemma));
        lines.push(format!("default_role: {}", ac.default_role));
        lines.push(
            "routing: route_decision → route.jsonl (alt/fb audit only; no auto second agent)"
                .into(),
        );
        lines.push("Tip: /max · /gemma · /routes · hybrid-loop · /persona aviva".into());
        self.persona_lines = lines;
    }

    pub fn refresh_memory(&mut self) {
        let backend = config::AbbeyConfig::load()
            .unwrap_or_default()
            .memory_backend;
        let mut lines = vec![memory::backend_status(&self.state.state_dir, &backend)];
        // Redraw path: never block the render loop on another process's lock.
        let opened = memory::open_backend_with_timeout(
            &self.state.state_dir,
            &backend,
            Duration::from_millis(250),
        );
        match opened {
            Ok(mem) => {
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
                // Interpretable 3-D map preview (topic × recency × consolidation).
                if let Ok(records) = mem.filter(None, None, 12) {
                    if records.is_empty() {
                        lines.push(
                            "map: empty — teach with `abbey memory put --tag <subject>`".into(),
                        );
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
            // A locked or broken store is not an empty one — say which.
            Err(e) => lines.push(format!("unavailable: {e}")),
        }
        lines.push("CLI: abbey learn correction|preference|digest|review|stats".into());
        self.memory_lines = lines;
    }

    pub fn refresh_skills(&mut self) {
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
        self.skill_lines = lines;
    }

    pub fn refresh_doctor(&mut self) {
        let chat = self.state.read_chat().unwrap_or_else(|| "(none)".into());
        let ac = config::AbbeyConfig::load().unwrap_or_default();
        let mut lines = crate::build_info::lines();
        lines.extend([
            format!("agent:     {}", self.cfg.agent_path.display()),
            format!("agent ver: {}", self.cfg.agent_version()),
            format!("model:     {}", self.cfg.model),
            format!("chat:      {chat}"),
            format!("chat file: {}", self.state.active_chat_file().display()),
            format!("per-cwd:   {}", self.state.per_cwd),
            format!("cwd:       {}", self.state.cwd.display()),
            format!("state:     {}", self.state.state_dir.display()),
            format!("auto-review: {}", self.cfg.auto_review),
            format!("trust:     {}", self.cfg.trust),
            format!("force:     {}", self.cfg.force),
            format!("no-resume: {}", self.cfg.no_resume),
            "personas:   Abbey · Aviva · Abi (abi-ai)".into(),
            "roles:      Max→technical · Gemma→visual (cursor-agent bindings)".into(),
            memory::backend_status(&self.state.state_dir, &ac.memory_backend),
            memory::feature_status(),
            config::wdbx_cli_status(&ac),
            "os-control: abbey os dry-run|execute --confirm (cross-platform allowlist)".into(),
            "subagents:  abbey subagents run --lanes max,reviewer [--peers gemini]".into(),
            "parallel:   alias of subagents with Max+Gemma+Aviva defaults".into(),
            "learn:      abbey learn correction|preference|routes|digest|review|stats".into(),
        ]);
        self.doctor_lines = lines;
        self.history = self.state.history(40);
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

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.input[..self.cursor]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        let start = self.cursor - prev;
        self.input.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    fn delete(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let next = self.input[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        self.input
            .replace_range(self.cursor..self.cursor + next, "");
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.input[..self.cursor]
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        self.cursor -= prev;
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let next = self.input[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        self.cursor += next;
    }

    pub fn list_len(&self) -> usize {
        match self.tab {
            Tab::Home => 0,
            Tab::Chats => self.history.len(),
            Tab::Personas => self.persona_lines.len(),
            Tab::Memory => self.memory_lines.len(),
            Tab::Skills => self.skill_lines.len(),
            Tab::Models => {
                if self.live_models.is_empty() {
                    self.aliases.len()
                } else {
                    self.live_models.len()
                }
            }
            Tab::Doctor => self.doctor_lines.len(),
        }
    }

    fn select_list_item(&mut self) {
        match self.tab {
            Tab::Chats => {
                if let Some(e) = self.history.get(self.list_idx) {
                    if let Err(err) = self.state.save_chat(&e.chat_id) {
                        self.status = format!("save chat failed: {err}");
                    } else {
                        self.status = format!("active chat → {}", e.chat_id);
                        self.refresh_doctor();
                    }
                }
            }
            Tab::Models => {
                if !self.live_models.is_empty() {
                    if let Some(line) = self.live_models.get(self.list_idx) {
                        let id = line.split_whitespace().next().unwrap_or(line);
                        let id = models::resolve_model(id);
                        self.cfg.model = id.clone();
                        let _ = self.state.save_model(&id);
                        self.status = format!("model → {id}");
                        self.refresh_doctor();
                    }
                } else if let Some((alias, full)) = self.aliases.get(self.list_idx) {
                    self.cfg.model = full.clone();
                    let _ = self.state.save_model(full);
                    self.status = format!("model → {alias} ({full})");
                    self.refresh_doctor();
                }
            }
            Tab::Personas => {
                self.refresh_personas();
                self.status = "personas refreshed".into();
            }
            Tab::Memory => {
                self.refresh_memory();
                let _ = learn::status(&self.state);
                self.status = "memory refreshed".into();
            }
            Tab::Skills => {
                self.refresh_skills();
                self.status = "skills refreshed".into();
            }
            _ => {}
        }
    }

    fn on_tab_enter(&mut self, tab: Tab) {
        self.tab = tab;
        self.list_idx = 0;
        match tab {
            Tab::Models if self.live_models.is_empty() => self.refresh_models_live(),
            Tab::Doctor => self.refresh_doctor(),
            Tab::Personas => self.refresh_personas(),
            Tab::Memory => self.refresh_memory(),
            Tab::Skills => self.refresh_skills(),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        if (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Tab => {
                self.on_tab_enter(self.tab.next());
                return;
            }
            KeyCode::BackTab => {
                self.on_tab_enter(self.tab.prev());
                return;
            }
            _ => {}
        }

        if self.tab == Tab::Home {
            match key.code {
                KeyCode::Char('q') if key.modifiers.is_empty() && self.input.is_empty() => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Esc => {
                    if !self.input.is_empty() {
                        self.input.clear();
                        self.cursor = 0;
                    } else {
                        self.should_quit = true;
                    }
                    return;
                }
                KeyCode::Enter => {
                    let t = self.input.trim().to_string();
                    if t.starts_with('/') {
                        self.pending = PendingAction::Slash(t);
                    } else {
                        self.pending = PendingAction::RunSession { fresh: false };
                    }
                    return;
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.pending = PendingAction::RunSession { fresh: true };
                    return;
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.pending = PendingAction::RunPleaseFix;
                    return;
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.refresh_doctor();
                    self.refresh_personas();
                    self.refresh_memory();
                    self.refresh_skills();
                    self.status = "refreshed".into();
                    return;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.insert_char(c);
                    return;
                }
                KeyCode::Backspace => {
                    self.backspace();
                    return;
                }
                KeyCode::Delete => {
                    self.delete();
                    return;
                }
                KeyCode::Left => {
                    self.move_left();
                    return;
                }
                KeyCode::Right => {
                    self.move_right();
                    return;
                }
                KeyCode::Home => {
                    self.cursor = 0;
                    return;
                }
                KeyCode::End => {
                    self.cursor = self.input.len();
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('1') => self.on_tab_enter(Tab::Home),
            KeyCode::Char('2') => self.on_tab_enter(Tab::Chats),
            KeyCode::Char('3') => self.on_tab_enter(Tab::Personas),
            KeyCode::Char('4') => self.on_tab_enter(Tab::Memory),
            KeyCode::Char('5') => self.on_tab_enter(Tab::Skills),
            KeyCode::Char('6') => self.on_tab_enter(Tab::Models),
            KeyCode::Char('7') => self.on_tab_enter(Tab::Doctor),
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.list_len();
                if len > 0 {
                    self.list_idx = (self.list_idx + 1).min(len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list_idx = self.list_idx.saturating_sub(1);
            }
            KeyCode::Enter => {
                if matches!(
                    self.tab,
                    Tab::Chats | Tab::Models | Tab::Personas | Tab::Memory | Tab::Skills
                ) {
                    self.select_list_item();
                } else if self.tab == Tab::Home {
                    let t = self.input.trim().to_string();
                    if t.starts_with('/') {
                        self.pending = PendingAction::Slash(t);
                    } else {
                        self.pending = PendingAction::RunSession { fresh: false };
                    }
                }
            }
            KeyCode::Char('n') => {
                self.pending = PendingAction::RunSession { fresh: true };
            }
            KeyCode::Char('p') => {
                self.pending = PendingAction::RunPleaseFix;
            }
            KeyCode::Char('r') => {
                self.refresh_doctor();
                self.refresh_personas();
                self.refresh_memory();
                self.refresh_skills();
                self.status = "refreshed".into();
            }
            _ => {}
        }
    }
}

pub fn run_tui(state: AbbeyState, cfg: AgentConfig) -> Result<i32> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(state, cfg)?;
    let mut code = 0i32;

    let result = (|| -> Result<i32> {
        loop {
            terminal.draw(|f| super::ui::draw(f, &app))?;

            match app.pending {
                PendingAction::None => {}
                action => {
                    app.pending = PendingAction::None;
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
                            match crate::dispatch_slash(&cmd, &app.state, &mut app.cfg) {
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
                    app.refresh_doctor();
                    app.refresh_memory();
                    code = run_code;

                    enable_raw_mode()?;
                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                    terminal.hide_cursor()?;
                    terminal.clear()?;
                }
            }

            if app.should_quit {
                break;
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    app.handle_key(key);
                }
            }
        }
        Ok(code)
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}
