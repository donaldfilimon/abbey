//! Key, paste and mouse handling for the chat TUI.

use super::app::{App, Overlay, Suspend};
use super::completion;
use super::permission;
use super::worker::RunKind;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use std::time::{Duration, Instant};

const ESC_ESC_WINDOW: Duration = Duration::from_millis(600);

impl App {
    pub fn handle_paste(&mut self, text: &str) {
        if matches!(self.overlay, Overlay::None) {
            self.composer.insert_str(text);
            self.update_completion();
        }
    }

    pub fn handle_mouse(&mut self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::ScrollUp => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(3);
            }
            MouseEventKind::ScrollDown => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(3);
            }
            _ => {}
        }
    }

    pub(crate) fn update_completion(&mut self) {
        let allowed = !self.is_running() && matches!(self.overlay, Overlay::None);
        if let Err(error) = self.prediction.sync(
            &self.composer.text,
            self.composer.cursor,
            &self.cfg,
            self.tick,
            allowed,
        ) {
            self.transcript.push_error(format!("prediction: {error:#}"));
        }
        self.completion_basis = Some((self.composer.text.clone(), self.composer.cursor));
        self.completion = completion::complete(
            &self.composer.text,
            self.composer.cursor,
            self.history_log.entries(),
            &self.files,
            self.prediction.boost(),
        );
        self.completion_idx = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.completion_basis.as_ref()
            != Some(&(self.composer.text.clone(), self.composer.cursor))
        {
            self.completion = None;
        }
        let draft = (self.composer.text.clone(), self.composer.cursor);
        self.handle_key_inner(key);
        if draft != (self.composer.text.clone(), self.composer.cursor) {
            self.update_completion();
        }
    }

    fn handle_key_inner(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if !(ctrl && key.code == KeyCode::Char('c')) {
            self.ctrl_c_armed = false;
        }
        if !matches!(self.overlay, Overlay::None) {
            self.handle_overlay_key(key);
            return;
        }
        match (key.code, ctrl) {
            (KeyCode::Char('c'), true) => {
                if self.composer.is_empty() && self.ctrl_c_armed {
                    if let Some(run) = &self.run {
                        run.cancel.cancel();
                    }
                    self.should_quit = true;
                } else {
                    self.composer.take();
                    self.completion = None;
                    self.ctrl_c_armed = true;
                    self.status = "Ctrl-C again to quit".into();
                }
            }
            (KeyCode::Char('k'), true) => {
                self.overlay = Overlay::Palette {
                    query: String::new(),
                    idx: 0,
                };
            }
            (KeyCode::Char('t'), true) => self.cycle_theme(),
            (KeyCode::Char('b'), true) if !self.is_running() => self.cycle_backend(),
            (KeyCode::Char('o'), true) => self.transcript.toggle_last_expandable(),
            (KeyCode::Char('r'), true) => {
                self.overlay = Overlay::Search {
                    query: String::new(),
                };
            }
            (KeyCode::Char('g'), true) => self.pending_suspend = Some(Suspend::Editor),
            (KeyCode::Char('n'), true) if !self.is_running() => {
                self.transcript.push_notice("new chat");
                let text = self.composer.take();
                self.start(RunKind::Prompt { text, fresh: true });
            }
            (KeyCode::Char('p'), true) if !self.is_running() => {
                let input = self.composer.take();
                self.start(RunKind::PleaseFix(input));
            }
            (KeyCode::Char('l'), true) => self.refresh_all(),
            (KeyCode::Char('j'), true) => self.composer.newline(),
            (KeyCode::Char('a'), true) => self.composer.line_start(),
            (KeyCode::Char('e'), true) => self.composer.line_end(),
            (KeyCode::Char('u'), true) => self.composer.kill_line_before(),
            (KeyCode::Char('w'), true) => self.composer.kill_word_before(),
            (KeyCode::F(1), _) => self.overlay = Overlay::Help,
            (KeyCode::Char('?'), false) if self.composer.is_empty() => self.overlay = Overlay::Help,
            (KeyCode::BackTab, _) => {
                self.status = format!("permission → {}", permission::cycle(&mut self.cfg));
            }
            (KeyCode::Esc, _) => self.on_esc(),
            (KeyCode::Enter, _)
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                self.composer.newline();
            }
            (KeyCode::Enter, _) if self.is_running() => self.submit(),
            (KeyCode::Enter, _)
                if crate::slash::parse_slash(&self.composer.text).is_some_and(|(name, _)| {
                    let name = crate::slash_alias::resolve_name(name).unwrap_or(name);
                    crate::slash::lookup(name).is_some()
                }) =>
            {
                self.submit()
            }
            (KeyCode::Enter, _) | (KeyCode::Tab, _) if self.completion.is_some() => {
                self.accept_completion();
            }
            (KeyCode::Enter, _) => self.submit(),
            (KeyCode::Up, _) if self.completion.is_some() => {
                self.completion_idx = self.completion_idx.saturating_sub(1);
            }
            (KeyCode::Down, _) if self.completion.is_some() => {
                let count = self.completion.as_ref().map_or(0, |c| match c {
                    completion::Completion::Slash(rows) => rows.len(),
                    completion::Completion::File { items, .. } => items.len(),
                });
                self.completion_idx = self
                    .completion_idx
                    .saturating_add(1)
                    .min(count.saturating_sub(1));
            }
            (KeyCode::Up, _) => {
                if !self.composer.up()
                    && let Some(prev) = self.history_log.prev(&self.composer.text)
                {
                    self.composer.set(prev);
                }
            }
            (KeyCode::Down, _) => {
                if !self.composer.down()
                    && let Some(next) = self.history_log.next()
                {
                    self.composer.set(next);
                }
            }
            (KeyCode::Left, _) if key.modifiers.contains(KeyModifiers::ALT) => {
                self.composer.word_left();
            }
            (KeyCode::Right, _) if key.modifiers.contains(KeyModifiers::ALT) => {
                self.composer.word_right();
            }
            (KeyCode::Left, _) => self.composer.left(),
            (KeyCode::Right, _) => self.composer.right(),
            (KeyCode::Home, _) => self.composer.line_start(),
            (KeyCode::End, _) => self.composer.line_end(),
            (KeyCode::PageUp, _) => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(10);
            }
            (KeyCode::PageDown, _) => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(10);
            }
            (KeyCode::Backspace, _) => {
                self.composer.backspace();
                self.update_completion();
            }
            (KeyCode::Delete, _) => self.composer.delete(),
            (KeyCode::Char(c), false) => {
                self.composer.insert_char(c);
                self.update_completion();
            }
            _ => {}
        }
    }

    fn on_esc(&mut self) {
        if let Err(error) = self.prediction.dismiss() {
            self.transcript.push_error(format!("prediction: {error:#}"));
        }
        if self.completion.take().is_some() {
            return;
        }
        if let Some(run) = &self.run {
            run.cancel.cancel();
            self.status = "interrupting…".into();
            return;
        }
        let now = Instant::now();
        if self
            .last_esc
            .is_some_and(|t| now.duration_since(t) < ESC_ESC_WINDOW)
        {
            if let Some(prev) = self.last_submitted.clone() {
                self.composer.set(prev);
            }
            self.last_esc = None;
        } else {
            self.last_esc = Some(now);
        }
    }

    fn accept_completion(&mut self) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let (text, cursor) = completion::accept(
            &self.composer.text,
            self.composer.cursor,
            &c,
            self.completion_idx,
        );
        self.composer.text = text;
        self.composer.cursor = cursor;
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        self.overlay = match (overlay, key.code) {
            (_, KeyCode::Esc) => Overlay::None,
            (Overlay::Confirm { command }, KeyCode::Char('y' | 'Y')) => {
                self.transcript
                    .push_user(&format!("!{}", command.join(" ")));
                self.run_confirmed(command);
                Overlay::None
            }
            (Overlay::Confirm { .. }, _) => {
                self.status = "command not run".into();
                Overlay::None
            }
            (Overlay::Search { mut query }, KeyCode::Char(c)) => {
                query.push(c);
                Overlay::Search { query }
            }
            (Overlay::Search { mut query }, KeyCode::Backspace) => {
                query.pop();
                Overlay::Search { query }
            }
            (Overlay::Search { query }, KeyCode::Enter) => {
                if let Some(hit) = self.history_log.search(&query).map(str::to_string) {
                    self.composer.set(hit);
                }
                Overlay::None
            }
            (other, code) => self.overlay_nav(other, code),
        };
    }
}
