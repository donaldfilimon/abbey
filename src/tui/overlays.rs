//! Command palette, help, panels, and pickers over the chat transcript.

use super::app::{App, Overlay, Panel};
use super::widgets;
use crate::slash::SLASH_CATALOG;
use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

#[derive(Debug, Default)]
pub(crate) struct PanelView {
    pub filter: String,
    pub editing: bool,
    pub row: usize,
}

fn panel_rows(app: &App, panel: Panel) -> Vec<String> {
    let query = app.panel_view.filter.to_lowercase();
    panel_lines(app, panel)
        .into_iter()
        .filter(|row| row.to_lowercase().contains(&query))
        .collect()
}

fn next_index(index: usize, count: usize) -> usize {
    index.saturating_add(1).min(count.saturating_sub(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    Slash(&'static str),
    NewChat,
    PleaseFix,
    CycleBackend,
    Refresh,
    CycleTheme,
    OpenPanel(Panel),
    ModelPicker,
    ResumePicker,
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub struct PaletteItem {
    pub id: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub action: PaletteAction,
}

const BUILTIN: &[PaletteItem] = &[
    PaletteItem {
        id: "new",
        label: "New chat",
        detail: "Start a fresh agent session",
        action: PaletteAction::NewChat,
    },
    PaletteItem {
        id: "fix",
        label: "Please-fix",
        detail: "Fix last failure / piped error",
        action: PaletteAction::PleaseFix,
    },
    PaletteItem {
        id: "refresh",
        label: "Refresh",
        detail: "Reload doctor / memory / skills",
        action: PaletteAction::Refresh,
    },
    PaletteItem {
        id: "theme",
        label: "Cycle theme",
        detail: "ink → violet → mono",
        action: PaletteAction::CycleTheme,
    },
    PaletteItem {
        id: "backend",
        label: "Cycle backend",
        detail: "ollama → grok → fm → abi → claude → cursor (skips unresolvable)",
        action: PaletteAction::CycleBackend,
    },
    PaletteItem {
        id: "doctor",
        label: "Open Doctor",
        detail: "Diagnostics panel",
        action: PaletteAction::OpenPanel(Panel::Doctor),
    },
    PaletteItem {
        id: "quit",
        label: "Quit",
        detail: "Leave the TUI",
        action: PaletteAction::Quit,
    },
    PaletteItem {
        id: "model",
        label: "Switch model",
        detail: "Pick from the live model list",
        action: PaletteAction::ModelPicker,
    },
    PaletteItem {
        id: "resume",
        label: "Resume chat",
        detail: "Pick a previous conversation",
        action: PaletteAction::ResumePicker,
    },
    PaletteItem {
        id: "memory",
        label: "Memory",
        detail: "Memory layers panel",
        action: PaletteAction::OpenPanel(Panel::Memory),
    },
    PaletteItem {
        id: "routes",
        label: "Routes",
        detail: "Route audit tail",
        action: PaletteAction::OpenPanel(Panel::Routes),
    },
    PaletteItem {
        id: "skills",
        label: "Skills",
        detail: "Skills and plugins inventory",
        action: PaletteAction::OpenPanel(Panel::Skills),
    },
    PaletteItem {
        id: "personas",
        label: "Personas",
        detail: "Persona and role bindings",
        action: PaletteAction::OpenPanel(Panel::Personas),
    },
    PaletteItem {
        id: "claims",
        label: "Claims",
        detail: "Capability ledger",
        action: PaletteAction::OpenPanel(Panel::Claims),
    },
];

pub fn palette_items() -> Vec<PaletteItem> {
    let mut items = BUILTIN.to_vec();
    for c in SLASH_CATALOG {
        items.push(PaletteItem {
            id: c.name,
            label: c.name,
            detail: c.help,
            action: PaletteAction::Slash(c.name),
        });
    }
    items
}

pub fn fuzzy_filter(items: &[PaletteItem], query: &str) -> Vec<PaletteItem> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .copied()
        .filter(|it| {
            it.id.to_ascii_lowercase().contains(&q)
                || it.label.to_ascii_lowercase().contains(&q)
                || it.detail.to_ascii_lowercase().contains(&q)
        })
        .collect()
}

pub fn help_lines() -> Vec<&'static str> {
    vec![
        "Abbey chat — keys",
        "",
        "  Enter                 send",
        "  Shift/Alt-Enter, ^J   newline",
        "  Esc                   interrupt, or Esc Esc to recall",
        "  Ctrl-C                clear, then quit",
        "  Ctrl-K                palette",
        "  Ctrl-R                history search",
        "  Ctrl-G                $EDITOR",
        "  Ctrl-O                expand tool or thinking",
        "  Shift-Tab             permission mode",
        "  Ctrl-B / Ctrl-T       backend / theme",
        "  Ctrl-N / Ctrl-P       new chat / please-fix",
        "  Ctrl-L                refresh",
        "  PgUp PgDn wheel       scroll",
        "  @ / / !cmd            files, slash, allowlisted OS (asks first)",
    ]
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn panel_lines(app: &App, p: Panel) -> Vec<String> {
    match p {
        Panel::Memory => app.memory_lines.clone(),
        Panel::Routes => app.route_lines.clone(),
        Panel::Skills => app.skill_lines.clone(),
        Panel::Doctor => app.doctor_lines.clone(),
        Panel::Personas => app.persona_lines.clone(),
        Panel::Claims => app.claims_lines.clone(),
    }
}

fn model_rows(app: &App) -> Vec<String> {
    if app.live_models.is_empty() {
        app.aliases
            .iter()
            .map(|(a, b)| format!("{a}  {b}"))
            .collect()
    } else {
        app.live_models.clone()
    }
}

fn list(f: &mut Frame, area: Rect, app: &App, title: &str, rows: &[String], sel: Option<usize>) {
    let r = centered(
        area,
        area.width.saturating_sub(8).min(100),
        area.height.saturating_sub(4).min(30),
    );
    let lines: Vec<Line<'static>> = rows
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if Some(i) == sel {
                Line::styled(s.clone(), widgets::list_highlight_style(&app.theme))
            } else {
                Line::raw(s.clone())
            }
        })
        .collect();
    let skip = sel.map_or(0, |s| {
        s.saturating_sub(usize::from(r.height.saturating_sub(3)))
    });
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>())
            .wrap(Wrap { trim: false })
            .block(widgets::rounded_block(title, &app.theme, true)),
        r,
    );
}

pub(crate) fn draw(f: &mut Frame, area: Rect, app: &App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => list(
            f,
            area,
            app,
            "Help · Esc",
            &help_lines()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            None,
        ),
        Overlay::Palette { query, idx } => {
            let items = fuzzy_filter(&palette_items(), query);
            let mut rows = vec![format!("> {query}")];
            rows.extend(items.iter().map(|i| format!("{}  {}", i.label, i.detail)));
            let sel = idx.saturating_add(1).min(rows.len().saturating_sub(1));
            list(f, area, app, "Command palette", &rows, Some(sel));
        }
        Overlay::Panel(p) => {
            let rows = panel_rows(app, *p);
            let selected = app.panel_view.row.min(rows.len().saturating_sub(1));
            // Begin at the selected logical row so preceding wrapped entries
            // cannot push the current item out of the physical viewport.
            list(
                f,
                area,
                app,
                &format!(
                    "{} · /{} · ↑↓ Home/End · Esc",
                    p.title(),
                    app.panel_view.filter
                ),
                &rows[selected..],
                Some(0),
            );
        }
        Overlay::ModelPicker { idx } => {
            let rows = model_rows(app);
            let sel = (*idx).min(rows.len().saturating_sub(1));
            list(f, area, app, "Model · Enter select", &rows, Some(sel));
        }
        Overlay::ResumePicker { idx } => {
            let rows: Vec<String> = app
                .history
                .iter()
                .map(|h| format!("{}  {}  {}", h.timestamp, h.chat_id, h.cwd))
                .collect();
            let sel = (*idx).min(rows.len().saturating_sub(1));
            list(f, area, app, "Resume · Enter select", &rows, Some(sel));
        }
        Overlay::Confirm { command } => list(
            f,
            area,
            app,
            "Run OS command?",
            &[
                format!("abbey os execute --confirm {}", command.join(" ")),
                String::new(),
                "Allowlist only. y = run · any other key = cancel".into(),
            ],
            None,
        ),
        Overlay::Search { query } => {
            let hit = app
                .history_log
                .search(query)
                .unwrap_or("(no match)")
                .to_string();
            list(
                f,
                area,
                app,
                "History search · Enter accept",
                &[format!("> {query}"), hit],
                None,
            );
        }
    }
}

impl App {
    pub(crate) fn overlay_nav(&mut self, o: Overlay, code: KeyCode) -> Overlay {
        match (o, code) {
            (Overlay::Palette { mut query, .. }, KeyCode::Char(c)) => {
                query.push(c);
                Overlay::Palette { query, idx: 0 }
            }
            (Overlay::Palette { mut query, .. }, KeyCode::Backspace) => {
                query.pop();
                Overlay::Palette { query, idx: 0 }
            }
            (Overlay::Palette { query, idx }, KeyCode::Down) => Overlay::Palette {
                idx: next_index(idx, fuzzy_filter(&palette_items(), &query).len()),
                query,
            },
            (Overlay::Palette { query, idx }, KeyCode::Up) => Overlay::Palette {
                query,
                idx: idx.saturating_sub(1),
            },
            (Overlay::Palette { query, idx }, KeyCode::Enter) => {
                let items = fuzzy_filter(&palette_items(), &query);
                match items
                    .get(idx.min(items.len().saturating_sub(1)))
                    .map(|i| i.action)
                {
                    Some(a) => self.palette_action(a),
                    None => Overlay::None,
                }
            }
            (Overlay::ModelPicker { idx }, KeyCode::Down) => Overlay::ModelPicker {
                idx: next_index(idx, model_rows(self).len()),
            },
            (Overlay::ModelPicker { idx }, KeyCode::Up) => Overlay::ModelPicker {
                idx: idx.saturating_sub(1),
            },
            (Overlay::ModelPicker { idx }, KeyCode::Enter) => {
                if let Some(row) =
                    model_rows(self).get(idx.min(model_rows(self).len().saturating_sub(1)))
                {
                    let name = row.split_whitespace().next().unwrap_or("").to_string();
                    self.run_local(&[format!("/model {name}")]);
                }
                Overlay::None
            }
            (Overlay::ResumePicker { idx }, KeyCode::Down) => Overlay::ResumePicker {
                idx: next_index(idx, self.history.len()),
            },
            (Overlay::ResumePicker { idx }, KeyCode::Up) => Overlay::ResumePicker {
                idx: idx.saturating_sub(1),
            },
            (Overlay::ResumePicker { idx }, KeyCode::Enter) => {
                if let Some(h) = self
                    .history
                    .get(idx.min(self.history.len().saturating_sub(1)))
                {
                    let id = h.chat_id.clone();
                    match self.state.save_chat(&id) {
                        Ok(()) => self.transcript.push_notice(format!("resumed chat {id}")),
                        Err(e) => self.transcript.push_error(format!("{e:#}")),
                    }
                }
                Overlay::None
            }
            (Overlay::Panel(p), KeyCode::Tab) => {
                let i = Panel::ALL.iter().position(|x| *x == p).unwrap_or(0);
                self.open_panel(Panel::ALL[(i + 1) % Panel::ALL.len()])
            }
            (Overlay::Panel(p), code) => {
                self.navigate_panel(p, code);
                Overlay::Panel(p)
            }
            (Overlay::Help, _) => Overlay::None,
            (other, _) => other,
        }
    }

    pub(crate) fn open_panel(&mut self, p: Panel) -> Overlay {
        self.panel_view = PanelView::default();
        if p == Panel::Claims && self.claims_lines.is_empty() {
            self.claims_lines = crate::claims::CLAIMS
                .iter()
                .map(|c| format!("{} · {} — {}", c.status.label(), c.name, c.note))
                .collect();
        }
        Overlay::Panel(p)
    }

    fn navigate_panel(&mut self, p: Panel, code: KeyCode) {
        if self.panel_view.editing {
            match code {
                KeyCode::Enter => self.panel_view.editing = false,
                KeyCode::Backspace => {
                    self.panel_view.filter.pop();
                    self.panel_view.row = 0;
                }
                KeyCode::Char(c) => {
                    if self.panel_view.filter.len() + c.len_utf8() <= 256 {
                        self.panel_view.filter.push(c);
                    }
                    self.panel_view.row = 0;
                }
                _ => {}
            }
            return;
        }
        let last = panel_rows(self, p).len().saturating_sub(1);
        self.panel_view.row = self.panel_view.row.min(last);
        match code {
            KeyCode::Char('/') => self.panel_view.editing = true,
            KeyCode::Home => self.panel_view.row = 0,
            KeyCode::End => self.panel_view.row = last,
            KeyCode::Up | KeyCode::Char('k') => {
                self.panel_view.row = self.panel_view.row.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.panel_view.row = next_index(self.panel_view.row, last.saturating_add(1))
            }
            KeyCode::PageUp => self.panel_view.row = self.panel_view.row.saturating_sub(10),
            KeyCode::PageDown => {
                self.panel_view.row = self.panel_view.row.saturating_add(10).min(last)
            }
            _ => {}
        }
    }

    fn palette_action(&mut self, a: PaletteAction) -> Overlay {
        use super::worker::RunKind;
        match a {
            PaletteAction::Slash(name) => {
                self.composer.set(format!("/{name} "));
                Overlay::None
            }
            PaletteAction::NewChat => {
                self.transcript.push_notice("new chat");
                self.start(RunKind::Prompt {
                    text: String::new(),
                    fresh: true,
                });
                Overlay::None
            }
            PaletteAction::PleaseFix => {
                let input = self.composer.take();
                self.start(RunKind::PleaseFix(input));
                Overlay::None
            }
            PaletteAction::CycleBackend => {
                self.cycle_backend();
                Overlay::None
            }
            PaletteAction::Refresh => {
                self.refresh_all();
                Overlay::None
            }
            PaletteAction::CycleTheme => {
                self.cycle_theme();
                Overlay::None
            }
            PaletteAction::OpenPanel(p) => self.open_panel(p),
            PaletteAction::ModelPicker => Overlay::ModelPicker { idx: 0 },
            PaletteAction::ResumePicker => {
                self.history = self.state.history(40);
                Overlay::ResumePicker { idx: 0 }
            }
            PaletteAction::Quit => {
                self.should_quit = true;
                Overlay::None
            }
        }
    }
}
