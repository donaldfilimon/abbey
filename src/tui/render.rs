//! Layout: header, transcript, completion menu, composer, status line.

use super::app::{App, Overlay};
use super::completion::Completion;
use super::{overlays, permission, widgets};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub(crate) fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let composer_rows = u16::try_from(app.composer.text.split('\n').count())
        .unwrap_or(1)
        .clamp(1, 8)
        + 2;
    let menu_rows = match &app.completion {
        Some(Completion::Slash(p)) => u16::try_from(p.len().min(6)).unwrap_or(0),
        Some(Completion::File { items, .. }) => u16::try_from(items.len().min(6)).unwrap_or(0),
        None => 0,
    };
    let [header, body, menu, composer, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(menu_rows),
        Constraint::Length(composer_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(f, header, app);
    draw_transcript(f, body, app);
    draw_menu(f, menu, app);
    draw_composer(f, composer, app);
    draw_status(f, status, app);
    if !matches!(app.overlay, Overlay::None) {
        overlays::draw(f, area, app);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let accent = widgets::accent_style(&app.theme);
    let dim = widgets::dim_style(&app.theme);
    let line = Line::from(vec![
        Span::styled(
            " abbey ",
            accent.add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled("· ", dim),
        Span::raw(app.cfg.backend.label().to_string()),
        Span::styled(" · ", dim),
        Span::raw(app.cfg.model.clone()),
        Span::styled(" · perm ", dim),
        Span::raw(permission::label(&app.cfg)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_transcript(f: &mut Frame, area: Rect, app: &App) {
    if area.width < 2 || area.height == 0 {
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
    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .enumerate()
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

fn draw_composer(f: &mut Frame, area: Rect, app: &App) {
    let title = if app.is_running() {
        "queue a message"
    } else {
        "message"
    };
    let block = widgets::rounded_block(title, &app.theme, true);
    let inner = block.inner(area);
    let lines: Vec<Line<'static>> = app
        .composer
        .text
        .split('\n')
        .map(|l| Line::raw(l.to_string()))
        .collect();
    let (row, col) = app.composer.cursor_row_col();
    let visible = usize::from(inner.height.max(1));
    let skip = row.saturating_sub(visible - 1);
    f.render_widget(
        Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>()).block(block),
        area,
    );
    let x = inner.x
        + u16::try_from(col)
            .unwrap_or(0)
            .min(inner.width.saturating_sub(1));
    let y = inner.y + u16::try_from(row - skip).unwrap_or(0);
    if matches!(app.overlay, Overlay::None) {
        f.set_cursor_position((x, y));
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let dim = widgets::dim_style(&app.theme);
    let mut spans = Vec::new();
    if let Some(run) = &app.run {
        let frame = SPINNER[usize::try_from(app.tick / 2).unwrap_or(0) % SPINNER.len()];
        spans.push(Span::styled(
            format!(" {frame} {}s ", run.started.elapsed().as_secs()),
            widgets::accent_style(&app.theme),
        ));
    }
    let tokens = app
        .transcript
        .usage
        .map_or_else(|| "n/a".to_string(), |(i, o)| format!("{i}↑ {o}↓"));
    spans.push(Span::styled(format!(" tokens {tokens} "), dim));
    if !app.queued.is_empty() {
        spans.push(Span::styled(format!(" queued {} ", app.queued.len()), dim));
    }
    spans.push(Span::raw(format!(" {}", app.status)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
