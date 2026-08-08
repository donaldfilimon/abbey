//! Ratatui drawing for Abbey TUI.

use super::app::{App, Focus, OverlayKind, Tab};
use super::overlay;
use super::widgets::{
    accent_style, dim_style, draw_kpi_strip, draw_vertical_scrollbar, list_highlight_style,
    rounded_block,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph, Tabs, Wrap};

pub fn draw(f: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(if app.tab == Tab::Home { 1 } else { 0 }),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Fill background
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(app.theme.bg).fg(app.theme.fg)),
        f.area(),
    );

    draw_header(f, root[0], app);
    if app.tab == Tab::Home {
        let chips = app.kpi_chips();
        let refs: Vec<(&str, &str)> = chips
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        draw_kpi_strip(f, root[1], &refs, &app.theme);
    }
    match app.tab {
        Tab::Home => draw_home(f, root[2], app),
        Tab::Chats => draw_list_tab(f, root[2], app, " Chats · Enter activate · / filter "),
        Tab::Personas => draw_list_tab(f, root[2], app, " Personas · Max/Gemma roles "),
        Tab::Memory => draw_list_tab(f, root[2], app, " Memory · map · self-learn "),
        Tab::Skills => draw_list_tab(f, root[2], app, " Skills · plugins · peers "),
        Tab::Models => draw_list_tab(f, root[2], app, " Models · Enter select "),
        Tab::Doctor => draw_list_tab(f, root[2], app, " Doctor "),
    }
    draw_input(f, root[3], app);
    draw_status(f, root[4], app);

    if app.overlay != OverlayKind::None {
        overlay::draw_overlay(
            f,
            f.area(),
            app.overlay,
            &app.overlay_query,
            app.overlay_idx,
            &app.theme,
            &app.input,
        );
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let pulse = if app.tick % 20 < 10 {
        app.theme.header_pulse
    } else {
        app.theme.accent
    };
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            let selected = *t == app.tab;
            Line::from(Span::styled(
                format!(" {} ", t.title()),
                if selected {
                    Style::default()
                        .fg(app.theme.bg)
                        .bg(pulse)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.theme.fg_dim)
                },
            ))
        })
        .collect();

    let brand = if app.tick % 20 < 10 {
        Style::default()
            .fg(app.theme.header_pulse)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD)
    };

    let tabs = Tabs::new(titles)
        .select(Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0))
        .block(
            rounded_block("", &app.theme, false)
                .title(Span::styled(" ✦ Abbey ", brand))
                .title(Span::styled(
                    format!(" {} ", app.theme_id.as_str()),
                    Style::default().fg(app.theme.fg_dim),
                )),
        )
        .divider(Span::styled("│", Style::default().fg(app.theme.border)));
    f.render_widget(tabs, area);
}

fn draw_home(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let chat = app
        .state
        .read_chat_for(app.cfg.backend)
        .unwrap_or_else(|| "—".into());
    let body = vec![
        Line::from(vec![
            Span::styled("model  ", dim_style(&app.theme)),
            Span::styled(
                app.cfg.model.clone(),
                Style::default()
                    .fg(app.theme.ok)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("chat   ", dim_style(&app.theme)),
            Span::styled(chat, Style::default().fg(app.theme.accent)),
        ]),
        Line::from(vec![
            Span::styled("cwd    ", dim_style(&app.theme)),
            Span::raw(short_path(&app.state.cwd.display().to_string())),
        ]),
        Line::from(""),
        Line::from(Span::styled("Keys", accent_style(&app.theme))),
        Line::from(Span::styled(
            "  Enter /slash     run or execute slash",
            dim_style(&app.theme),
        )),
        Line::from(Span::styled(
            "  `  Ctrl-L        toggle prompt ↔ panel",
            dim_style(&app.theme),
        )),
        Line::from(Span::styled(
            "  Ctrl-K / Ctrl-T  palette / theme",
            dim_style(&app.theme),
        )),
        Line::from(Span::styled(
            "  Tab 1-7 · ?      tabs · help",
            dim_style(&app.theme),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Composer-first · dual focus · dashboard chips.",
            dim_style(&app.theme),
        )),
    ];

    let left = Paragraph::new(body)
        .block(rounded_block(
            " Session ",
            &app.theme,
            app.focus == Focus::Prompt,
        ))
        .wrap(Wrap { trim: false });
    f.render_widget(left, chunks[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);
    draw_recent_list(f, right[0], app);
    draw_routes_pane(f, right[1], app);
}

/// Tail of the routing audit (persona/role/model/confidence per run) — the
/// same records `abbey routes` prints, compacted for a half-width pane.
fn draw_routes_pane(f: &mut Frame, area: Rect, app: &App) {
    let lines: Vec<Line> = if app.route_lines.is_empty() {
        vec![Line::from(Span::styled(
            "(no routes yet — run a prompt; audit only, no auto second agent)",
            dim_style(&app.theme),
        ))]
    } else {
        app.route_lines
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), dim_style(&app.theme))))
            .collect()
    };
    let p = Paragraph::new(lines)
        .block(rounded_block(" Routes · audit ", &app.theme, false))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn draw_recent_list(f: &mut Frame, area: Rect, app: &App) {
    let lines = app.filtered_lines();
    let focused = app.focus == Focus::Panel;
    let items: Vec<ListItem> = lines
        .iter()
        .enumerate()
        .skip(app.scroll)
        .map(|(i, line)| {
            let style = if focused && i == app.list_idx {
                list_highlight_style(&app.theme)
            } else {
                Style::default().fg(app.theme.fg)
            };
            // Prefer cyan-ish accent for id portion when we can split
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", parts[0].get(11..19).unwrap_or(parts[0])),
                        dim_style(&app.theme),
                    ),
                    Span::styled(
                        parts[1].chars().take(8).collect::<String>(),
                        Style::default().fg(app.theme.accent),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        short_path(parts.get(2..).map(|p| p.join(" ")).as_deref().unwrap_or("")),
                        dim_style(&app.theme),
                    ),
                ]))
                .style(style)
            } else {
                ListItem::new(line.clone()).style(style)
            }
        })
        .collect();

    let title = if app.filter.is_empty() {
        " Recent chats ".to_string()
    } else {
        format!(" Recent · /{} ", app.filter)
    };
    let block = rounded_block(&title, &app.theme, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(List::new(items), inner);
    draw_vertical_scrollbar(
        f,
        area,
        lines.len(),
        inner.height as usize,
        app.scroll,
        &app.theme,
    );
}

fn draw_list_tab(f: &mut Frame, area: Rect, app: &App, title: &str) {
    let lines = app.filtered_lines();
    let focused = app.focus == Focus::Panel;
    let title = if app.filtering || !app.filter.is_empty() {
        format!("{title} filter:{}/ ", app.filter)
    } else {
        title.to_string()
    };
    let items: Vec<ListItem> = lines
        .iter()
        .enumerate()
        .skip(app.scroll)
        .map(|(i, line)| {
            let style = if focused && i == app.list_idx {
                list_highlight_style(&app.theme)
            } else {
                Style::default().fg(app.theme.fg)
            };
            ListItem::new(line.clone()).style(style)
        })
        .collect();

    let block = rounded_block(&title, &app.theme, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(List::new(items), inner);
    draw_vertical_scrollbar(
        f,
        area,
        lines.len(),
        inner.height as usize,
        app.scroll,
        &app.theme,
    );
}

fn draw_input(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Prompt;
    let title = if focused {
        " Prompt · Enter run · /slash · ↑↓ history "
    } else {
        " Prompt (Ctrl-L / ` to focus) "
    };

    let caret_on = app.tick.is_multiple_of(2);
    let mut spans = Vec::new();
    let (before, after) = app.input.split_at(app.cursor.min(app.input.len()));
    spans.push(Span::raw(before.to_string()));
    if let Some(ch) = after.chars().next() {
        let style = if caret_on && focused {
            Style::default().bg(app.theme.accent).fg(app.theme.bg)
        } else {
            Style::default().fg(app.theme.fg)
        };
        spans.push(Span::styled(ch.to_string(), style));
        spans.push(Span::raw(after[ch.len_utf8()..].to_string()));
    } else if caret_on && focused {
        spans.push(Span::styled(
            " ",
            Style::default().bg(app.theme.accent).fg(app.theme.bg),
        ));
    }

    let border = if focused {
        app.theme.prompt_border
    } else {
        app.theme.border
    };
    let p = Paragraph::new(Line::from(spans))
        .block(rounded_block(title, &app.theme, focused).border_style(Style::default().fg(border)));
    f.render_widget(p, area);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let exit = app
        .last_agent_code
        .map(|c| {
            let color = if c == 0 {
                app.theme.ok
            } else {
                app.theme.error
            };
            Span::styled(format!(" last={c}"), Style::default().fg(color))
        })
        .unwrap_or_else(|| Span::raw(""));
    let filter = if app.filter.is_empty() {
        Span::raw("")
    } else {
        Span::styled(
            format!(" filter={} ", app.filter),
            Style::default().fg(app.theme.warn),
        )
    };
    // Off-default executors get the warn colour: which binary runs the next
    // prompt is the one thing a TUI user must never be surprised by.
    let backend = app.cfg.backend;
    let backend_span = Span::styled(
        format!("{} ", backend.label()),
        if backend == crate::agent::AgentBackend::Cursor {
            dim_style(&app.theme)
        } else {
            Style::default().fg(app.theme.warn)
        },
    );
    let line = Line::from(vec![
        Span::styled(" ▸ ", accent_style(&app.theme)),
        Span::styled(
            format!("{} ", app.focus.label()),
            Style::default().fg(app.theme.accent_dim),
        ),
        backend_span,
        Span::styled(format!("{} ", app.theme_id.as_str()), dim_style(&app.theme)),
        filter,
        Span::raw(app.status.clone()),
        exit,
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(app.theme.bg)),
        area,
    );
}

fn short_path(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    if let Some(home) = dirs::home_dir() {
        let hs = home.to_string_lossy();
        if let Some(rest) = p.strip_prefix(hs.as_ref()) {
            return format!("~{rest}");
        }
    }
    if p.len() > 36 {
        format!("…{}", &p[p.len() - 34..])
    } else {
        p.to_string()
    }
}
