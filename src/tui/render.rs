//! Chat-first, size-adaptive presentation. No provider or execution authority lives here.

use super::app::{App, Overlay};
use super::completion::Completion;
use super::{overlays, permission, widgets};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub(crate) fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    if area.is_empty() {
        return;
    }
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
        area,
    );
    let bordered = area.height >= 5 && area.width >= 4;
    let input_width = area.width.saturating_sub(if bordered { 2 } else { 0 });
    let draft = ComposerLayout::new(&app.composer.text, app.composer.cursor, input_width);
    let header_rows = if area.height >= 10 {
        2
    } else {
        u16::from(area.height >= 4)
    };
    let activity_rows = u16::from(area.height >= 8);
    let composer_rows = if bordered {
        u16::try_from(draft.lines.len())
            .unwrap_or(u16::MAX)
            .clamp(1, (area.height / 3).clamp(1, 8))
            + 2
    } else {
        u16::from(area.height >= 2)
    };
    let requested_menu = match &app.completion {
        Some(Completion::Slash(p)) => u16::try_from(p.len().min(6)).unwrap_or(0),
        Some(Completion::File { items, .. }) => u16::try_from(items.len().min(6)).unwrap_or(0),
        None => 0,
    };
    let menu_rows = requested_menu.min(
        area.height
            .saturating_sub(header_rows + activity_rows + composer_rows + 3),
    );
    let [header, body, activity, menu, composer, status] = Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Min(0),
        Constraint::Length(activity_rows),
        Constraint::Length(menu_rows),
        Constraint::Length(composer_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(f, header, app);
    draw_transcript(f, body, app);
    draw_activity(f, activity, app);
    draw_menu(f, menu, app);
    draw_composer(f, composer, app, draft, bordered);
    draw_status(f, status, app);
    if !matches!(app.overlay, Overlay::None) {
        overlays::draw(f, area, app);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let accent = widgets::accent_style(&app.theme);
    let dim = widgets::dim_style(&app.theme);
    let identity = Line::from(vec![
        Span::styled(
            " abbey ",
            accent.add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled("· ", dim),
        Span::raw(app.cfg.backend.label().to_string()),
        Span::styled(" · ", dim),
        Span::raw(clean_label(&app.cfg.model)),
    ]);
    let workspace = app
        .state
        .cwd
        .file_name()
        .unwrap_or(app.state.cwd.as_os_str())
        .to_string_lossy();
    let context = Line::from(vec![
        Span::styled(format!(" {}", compact_label(&workspace, 28)), dim),
        Span::styled("  ·  permission: ", dim),
        Span::styled(permission::label(&app.cfg), dim),
        Span::styled("  ·  Ctrl-K palette  Ctrl-S providers  F1 help", dim),
    ]);
    f.render_widget(Paragraph::new(vec![identity, context]), area);
}

fn clean_label(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn compact_label(text: &str, width: usize) -> String {
    let clean = clean_label(text);
    if clean.width() <= width {
        return clean;
    }
    let span = Span::raw(clean);
    let mut label = String::new();
    let mut cells = 0;
    for grapheme in span.styled_graphemes(Style::default()) {
        let next = grapheme.symbol.width();
        if cells + next >= width {
            break;
        }
        cells += next;
        label.push_str(grapheme.symbol);
    }
    label.push('…');
    label
}

fn draw_transcript(f: &mut Frame, area: Rect, app: &App) {
    if area.width < 2 || area.height == 0 {
        return;
    }
    if app.transcript.cells.is_empty() {
        draw_welcome(f, area, app);
        return;
    }
    let lines = app
        .transcript
        .lines(&app.theme, area.width.saturating_sub(1));
    let height = usize::from(area.height);
    let max_scroll = lines.len().saturating_sub(height);
    let from_bottom = app.scroll_from_bottom.min(max_scroll);
    let start = lines.len().saturating_sub(height + from_bottom);
    let visible: Vec<Line<'static>> = lines.into_iter().skip(start).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);
    if from_bottom > 0 {
        let hint = Rect {
            x: area.x,
            y: area.bottom().saturating_sub(1),
            width: area.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Line::styled(
                format!("↓ {from_bottom} lines below · PgDn"),
                widgets::dim_style(&app.theme),
            )),
            hint,
        );
    }
}

fn draw_welcome(f: &mut Frame, area: Rect, app: &App) {
    let inset = u16::from(area.width >= 40) * 2;
    let top = u16::from(area.height >= 10);
    let area = Rect::new(
        area.x + inset,
        area.y + top,
        area.width.saturating_sub(inset * 2),
        area.height.saturating_sub(top),
    );
    let dim = widgets::dim_style(&app.theme);
    let mut lines = vec![
        Line::styled(
            "What would you like to work on?",
            widgets::accent_style(&app.theme),
        ),
        Line::styled(
            "Ask a question, discuss code, or plan your next change.",
            dim,
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Ctrl-K", widgets::accent_style(&app.theme)),
            Span::raw("  Commands & local panels"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl-S", widgets::accent_style(&app.theme)),
            Span::raw("  Choose a provider"),
        ]),
        Line::from(vec![
            Span::styled("F1", widgets::accent_style(&app.theme)),
            Span::raw("      Keyboard help · / commands · @ files"),
        ]),
        Line::raw(""),
        Line::styled(
            "Responses use the selected provider; tools depend on that provider.",
            dim,
        ),
        Line::styled(
            "Apple Foundation Models: on-device chat on supported Macs, not a tool host.",
            dim,
        ),
    ];
    if area.height < 7 {
        lines = vec![
            lines.remove(0),
            Line::styled("Ctrl-K palette · Ctrl-S providers · F1 help", dim),
        ];
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_menu(f: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 {
        return;
    }
    let rows: Vec<(String, String)> = match &app.completion {
        Some(Completion::Slash(p)) => p
            .iter()
            .map(|p| (format!("/{}", p.name), p.help.to_string()))
            .collect(),
        Some(Completion::File { items, .. }) => items
            .iter()
            .map(|i| (format!("@{i}"), String::new()))
            .collect(),
        None => Vec::new(),
    };
    let sel = app.completion_idx.min(rows.len().saturating_sub(1));
    let start = sel.saturating_sub(usize::from(area.height).saturating_sub(1));
    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .enumerate()
        .skip(start)
        .take(usize::from(area.height))
        .map(|(i, (a, b))| {
            let style = if i == sel {
                widgets::list_highlight_style(&app.theme)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(format!(" {a} "), style),
                Span::styled(b, widgets::dim_style(&app.theme)),
            ])
        })
        .collect();
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

/// Shared wrapping for both sizing and painting, including a real end-of-line cursor cell.
struct ComposerLayout {
    lines: Vec<Line<'static>>,
    row: usize,
    col: usize,
}

impl ComposerLayout {
    fn new(text: &str, cursor: usize, width: u16) -> Self {
        // styled_graphemes omits controls. Normalize first so byte accounting
        // still matches the cursor after pasted tabs or other control bytes.
        let cursor = display_input(text.get(..cursor).unwrap_or(text)).len();
        let text = display_input(text);
        let width = usize::from(width.max(1));
        let mut rows = vec![String::new()];
        let (mut col, mut byte) = (0, 0);
        let mut position = None;
        // Ratatui's graphemes keep combining marks and ZWJ emoji together.
        for (line_idx, hard_line) in text.split('\n').enumerate() {
            if line_idx > 0 {
                if cursor == byte - 1 {
                    if col == width {
                        // The hard newline creates this cursor row below. Do
                        // not insert a second, cursor-dependent blank row.
                        position = Some((rows.len(), 0));
                    } else {
                        position = Some((rows.len() - 1, col));
                    }
                }
                rows.push(String::new());
                col = 0;
            }
            let span = Span::raw(hard_line);
            for grapheme in span.styled_graphemes(Style::default()) {
                let symbol = grapheme.symbol;
                let rendered = if symbol.width() > width {
                    "�".to_string()
                } else {
                    symbol.to_string()
                };
                let cells = rendered.width();
                if col + cells > width || (col == width && cells > 0) {
                    rows.push(String::new());
                    col = 0;
                }
                if (byte..byte + symbol.len()).contains(&cursor) {
                    position = Some((rows.len() - 1, col));
                }
                rows.last_mut().expect("one row").push_str(&rendered);
                col += cells;
                byte += symbol.len();
            }
            // Account for the hard newline, handled at the start of the next iteration.
            byte += 1;
        }
        if position.is_none() {
            if col == width {
                rows.push(String::new());
                col = 0;
            }
            position = Some((rows.len() - 1, col));
        }
        let (row, col) = position.expect("cursor position");
        Self {
            lines: rows.into_iter().map(Line::raw).collect(),
            row,
            col,
        }
    }
}

fn display_input(text: &str) -> String {
    let mut display = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' => display.push_str("    "),
            '\n' => display.push('\n'),
            c if c.is_control() => display.push('�'),
            c => display.push(c),
        }
    }
    display
}

fn draw_composer(f: &mut Frame, area: Rect, app: &App, draft: ComposerLayout, bordered: bool) {
    if area.is_empty() {
        return;
    }
    let title = if app.is_running() {
        " Queue message · Enter queues "
    } else {
        " Message "
    };
    let block = widgets::rounded_block(title, &app.theme, true).title_bottom(Line::styled(
        " Enter send · Shift-Enter newline ",
        widgets::dim_style(&app.theme),
    ));
    let inner = if bordered { block.inner(area) } else { area };
    if bordered {
        f.render_widget(block, area);
    }
    if inner.is_empty() {
        return;
    }
    let visible = usize::from(inner.height);
    let skip = draft.row.saturating_sub(visible - 1);
    let lines = if app.composer.is_empty() {
        vec![Line::styled(
            if app.is_running() {
                "Add a follow-up…"
            } else {
                "Ask anything… (/ commands, @ files)"
            },
            widgets::dim_style(&app.theme),
        )]
    } else {
        draft.lines.into_iter().skip(skip).take(visible).collect()
    };
    f.render_widget(Paragraph::new(lines), inner);
    let x = inner.x + u16::try_from(draft.col).unwrap_or(0).min(inner.width - 1);
    let y = inner.y
        + u16::try_from(draft.row - skip)
            .unwrap_or(0)
            .min(inner.height - 1);
    if matches!(app.overlay, Overlay::None) {
        f.set_cursor_position((x, y));
    }
}

fn draw_activity(f: &mut Frame, area: Rect, app: &App) {
    let dim = widgets::dim_style(&app.theme);
    let mut spans = Vec::new();
    if let Some(run) = &app.run {
        let frame = SPINNER[usize::try_from(app.tick / 2).unwrap_or(0) % SPINNER.len()];
        spans.push(Span::styled(
            format!(
                " {frame} Running · {}s · Esc cancel ",
                run.started.elapsed().as_secs()
            ),
            widgets::accent_style(&app.theme),
        ));
    } else {
        spans.push(Span::styled(" Ready ", widgets::accent_style(&app.theme)));
    }
    if !app.queued.is_empty() {
        spans.push(Span::styled(
            format!("· queued ({}) ", app.queued.len()),
            widgets::accent_style(&app.theme),
        ));
    }
    let tokens = app
        .transcript
        .usage
        .map_or_else(|| "n/a".to_string(), |(i, o)| format!("{i}↑ {o}↓"));
    spans.push(Span::styled(format!("· tokens {tokens}"), dim));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    // Keep the actual lifecycle outcome first, including on narrow terminals.
    f.render_widget(
        Paragraph::new(app.status.as_str()).style(widgets::dim_style(&app.theme)),
        area,
    );
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
