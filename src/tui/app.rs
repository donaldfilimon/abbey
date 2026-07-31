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
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::stdout;
use std::time::Duration;

use super::overlay::{self, PaletteAction};
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

    pub fn cycle_theme(&mut self) {
        self.theme_id = self.theme_id.cycle();
        self.theme = Theme::from_id(self.theme_id);
        let _ = ThemeId::save(&self.state.state_dir, self.theme_id);
        self.status = format!("theme → {}", self.theme_id.as_str());
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

    fn push_history(&mut self) {
        let t = self.input.trim();
        if t.is_empty() {
            return;
        }
        if self.input_history.last().map(|s| s.as_str()) != Some(t) {
            self.input_history.push(t.to_string());
        }
        if self.input_history.len() > 100 {
            self.input_history.remove(0);
        }
        self.history_idx = None;
    }

    fn history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        let next = match self.history_idx {
            None => self.input_history.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_idx = Some(next);
        self.input = self.input_history[next].clone();
        self.cursor = self.input.len();
        self.sync_slash_overlay();
    }

    fn history_down(&mut self) {
        let Some(i) = self.history_idx else {
            return;
        };
        if i + 1 >= self.input_history.len() {
            self.history_idx = None;
            self.input.clear();
            self.cursor = 0;
        } else {
            self.history_idx = Some(i + 1);
            self.input = self.input_history[i + 1].clone();
            self.cursor = self.input.len();
        }
        self.sync_slash_overlay();
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.sync_slash_overlay();
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
        self.sync_slash_overlay();
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
        self.sync_slash_overlay();
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

    fn sync_slash_overlay(&mut self) {
        if self.overlay == OverlayKind::Palette || self.overlay == OverlayKind::Help {
            return;
        }
        let t = self.input.trim_start();
        if t.starts_with('/') && !t.contains(char::is_whitespace) {
            self.overlay = OverlayKind::SlashSuggest;
            self.overlay_idx = 0;
        } else if self.overlay == OverlayKind::SlashSuggest {
            self.overlay = OverlayKind::None;
        }
    }

    fn accept_slash_suggestion(&mut self) {
        let prefix = self
            .input
            .split_whitespace()
            .next()
            .unwrap_or("/")
            .trim_start_matches('/');
        let suggestions = overlay::slash_suggestions(prefix);
        if let Some(cmd) = suggestions.get(self.overlay_idx) {
            self.input = format!("/{} ", cmd.name);
            self.cursor = self.input.len();
            self.overlay = OverlayKind::None;
            self.overlay_idx = 0;
        }
    }

    fn select_list_item(&mut self) {
        let lines = self.filtered_lines();
        match self.tab {
            Tab::Home | Tab::Chats => {
                if let Some(line) = lines.get(self.list_idx) {
                    // timestamp  chat_id  cwd — chat_id is 2nd field
                    let parts: Vec<_> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let chat_id = parts[1];
                        if let Err(err) = self.state.save_chat(chat_id) {
                            self.status = format!("save chat failed: {err}");
                        } else {
                            self.status = format!("active chat → {chat_id}");
                            self.refresh_doctor();
                        }
                    }
                }
            }
            Tab::Models => {
                if let Some(line) = lines.get(self.list_idx) {
                    let id = line.split_whitespace().next().unwrap_or(line);
                    let id = models::resolve_model(id);
                    self.cfg.model = id.clone();
                    let _ = self.state.save_model(&id);
                    self.status = format!("model → {id}");
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
            Tab::Doctor => {}
        }
    }

    fn on_tab_enter(&mut self, tab: Tab) {
        self.tab = tab;
        self.list_idx = 0;
        self.scroll = 0;
        self.filter.clear();
        self.filtering = false;
        match tab {
            Tab::Models => {
                if self.live_models.is_empty() {
                    self.refresh_models_live();
                }
            }
            Tab::Doctor => self.refresh_doctor(),
            Tab::Personas => self.refresh_personas(),
            Tab::Memory => self.refresh_memory(),
            Tab::Skills => self.refresh_skills(),
            Tab::Chats | Tab::Home => {
                self.refresh_doctor();
            }
        }
    }

    fn close_overlay(&mut self) -> bool {
        if self.overlay != OverlayKind::None {
            self.overlay = OverlayKind::None;
            self.overlay_query.clear();
            self.overlay_idx = 0;
            return true;
        }
        false
    }

    fn apply_palette_action(&mut self, action: PaletteAction) {
        self.close_overlay();
        match action {
            PaletteAction::Slash(name) => {
                self.pending = PendingAction::Slash(format!("/{name}"));
            }
            PaletteAction::NewChat => {
                self.pending = PendingAction::RunSession { fresh: true };
            }
            PaletteAction::PleaseFix => {
                self.pending = PendingAction::RunPleaseFix;
            }
            PaletteAction::Refresh => self.refresh_all(),
            PaletteAction::CycleTheme => self.cycle_theme(),
            PaletteAction::GotoDoctor => self.on_tab_enter(Tab::Doctor),
            PaletteAction::Quit => self.should_quit = true,
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
            }
            KeyCode::Enter => {
                let items = overlay::fuzzy_filter(&overlay::palette_items(), &self.overlay_query);
                if let Some(it) = items.get(self.overlay_idx) {
                    self.apply_palette_action(it.action);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let n = overlay::fuzzy_filter(&overlay::palette_items(), &self.overlay_query).len();
                if n > 0 {
                    self.overlay_idx = (self.overlay_idx + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.overlay_idx = self.overlay_idx.saturating_sub(1);
            }
            KeyCode::Backspace => {
                self.overlay_query.pop();
                self.overlay_idx = 0;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.overlay_query.push(c);
                self.overlay_idx = 0;
            }
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

        if self.overlay == OverlayKind::Palette {
            self.handle_palette_key(key);
            return;
        }
        if self.overlay == OverlayKind::Help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                self.close_overlay();
            }
            return;
        }

        // Global chords
        match key.code {
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.overlay = OverlayKind::Palette;
                self.overlay_query.clear();
                self.overlay_idx = 0;
                return;
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cycle_theme();
                return;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.focus = self.focus.toggle();
                self.status = format!("focus → {}", self.focus.label());
                return;
            }
            KeyCode::Char('`') if key.modifiers.is_empty() && self.focus == Focus::Panel => {
                self.focus = Focus::Prompt;
                self.status = "focus → prompt".into();
                return;
            }
            KeyCode::Char('`') if key.modifiers.is_empty() && self.focus == Focus::Prompt => {
                // Only toggle when input empty so backticks can be typed? Prefer always toggle
                // when input empty; otherwise insert.
                if self.input.is_empty() {
                    self.focus = Focus::Panel;
                    self.status = "focus → panel".into();
                    return;
                }
            }
            KeyCode::F(1) => {
                self.overlay = OverlayKind::Help;
                return;
            }
            KeyCode::Tab => {
                if self.overlay == OverlayKind::SlashSuggest {
                    self.accept_slash_suggestion();
                    return;
                }
                self.on_tab_enter(self.tab.next());
                return;
            }
            KeyCode::BackTab => {
                self.on_tab_enter(self.tab.prev());
                return;
            }
            _ => {}
        }

        if self.filtering && self.focus == Focus::Panel {
            match key.code {
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filter.clear();
                    self.list_idx = 0;
                    return;
                }
                KeyCode::Enter => {
                    self.filtering = false;
                    return;
                }
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.list_idx = 0;
                    return;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.filter.push(c);
                    self.list_idx = 0;
                    return;
                }
                _ => {}
            }
        }

        if self.focus == Focus::Prompt {
            match key.code {
                KeyCode::Char('q') if key.modifiers.is_empty() && self.input.is_empty() => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('?') if self.input.is_empty() => {
                    self.overlay = OverlayKind::Help;
                    return;
                }
                KeyCode::Esc => {
                    if self.close_overlay() {
                        return;
                    }
                    if !self.input.is_empty() {
                        self.input.clear();
                        self.cursor = 0;
                        self.overlay = OverlayKind::None;
                    } else {
                        self.should_quit = true;
                    }
                    return;
                }
                KeyCode::Enter => {
                    if self.overlay == OverlayKind::SlashSuggest {
                        self.accept_slash_suggestion();
                        return;
                    }
                    self.submit_prompt();
                    return;
                }
                KeyCode::Up => {
                    if self.overlay == OverlayKind::SlashSuggest {
                        self.overlay_idx = self.overlay_idx.saturating_sub(1);
                    } else {
                        self.history_up();
                    }
                    return;
                }
                KeyCode::Down => {
                    if self.overlay == OverlayKind::SlashSuggest {
                        let prefix = self
                            .input
                            .split_whitespace()
                            .next()
                            .unwrap_or("/")
                            .trim_start_matches('/');
                        let n = overlay::slash_suggestions(prefix).len();
                        if n > 0 {
                            self.overlay_idx = (self.overlay_idx + 1).min(n - 1);
                        }
                    } else {
                        self.history_down();
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
                    self.refresh_all();
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

        // Panel focus
        match key.code {
            KeyCode::Esc => {
                if self.close_overlay() {
                    return;
                }
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.filtering = false;
                    self.list_idx = 0;
                } else {
                    self.focus = Focus::Prompt;
                    self.status = "focus → prompt".into();
                }
            }
            KeyCode::Char('1') => self.on_tab_enter(Tab::Home),
            KeyCode::Char('2') => self.on_tab_enter(Tab::Chats),
            KeyCode::Char('3') => self.on_tab_enter(Tab::Personas),
            KeyCode::Char('4') => self.on_tab_enter(Tab::Memory),
            KeyCode::Char('5') => self.on_tab_enter(Tab::Skills),
            KeyCode::Char('6') => self.on_tab_enter(Tab::Models),
            KeyCode::Char('7') => self.on_tab_enter(Tab::Doctor),
            KeyCode::Char('/') => {
                self.filtering = true;
                self.filter.clear();
                self.status = "filter…".into();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.list_len();
                if len > 0 {
                    self.list_idx = (self.list_idx + 1).min(len - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list_idx = self.list_idx.saturating_sub(1);
            }
            KeyCode::PageDown => {
                let len = self.list_len();
                if len > 0 {
                    self.list_idx = (self.list_idx + 10).min(len - 1);
                }
            }
            KeyCode::PageUp => {
                self.list_idx = self.list_idx.saturating_sub(10);
            }
            KeyCode::Home => self.list_idx = 0,
            KeyCode::End => {
                let len = self.list_len();
                if len > 0 {
                    self.list_idx = len - 1;
                }
            }
            KeyCode::Enter => self.select_list_item(),
            KeyCode::Char('n') => {
                self.pending = PendingAction::RunSession { fresh: true };
            }
            KeyCode::Char('p') => {
                self.pending = PendingAction::RunPleaseFix;
            }
            KeyCode::Char('r') => self.refresh_all(),
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
    }

    fn submit_prompt(&mut self) {
        let t = self.input.trim().to_string();
        self.push_history();
        if t.starts_with('/') {
            self.pending = PendingAction::Slash(t);
        } else {
            self.pending = PendingAction::RunSession { fresh: false };
        }
    }

    fn handle_mouse(&mut self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::ScrollDown => {
                let len = self.list_len();
                if len > 0 {
                    self.list_idx = (self.list_idx + 1).min(len - 1);
                }
            }
            MouseEventKind::ScrollUp => {
                self.list_idx = self.list_idx.saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn kpi_chips(&self) -> Vec<(String, String)> {
        let chat = self
            .state
            .read_chat()
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
