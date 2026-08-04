//! Input handling for the TUI: key events, palette/overlay actions, the
//! line editor, prompt history, and mouse scroll.
//!
//! Split out of `app.rs` per the repo's file-size rule (split before the
//! 800-line soft ceiling, not after). Same `impl App`, different file.

use crate::learn;
use crate::models;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

use super::app::App;
use super::overlay::{self, PaletteAction};
use super::tabs::{Focus, OverlayKind, PendingAction, Tab};

impl App {
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

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
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

    pub(super) fn handle_mouse(&mut self, kind: MouseEventKind) {
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
}
