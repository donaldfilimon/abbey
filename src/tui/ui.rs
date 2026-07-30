//! Ratatui drawing for Abbey TUI.

use super::app::{App, Tab};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap};

const ACCENT: Color = Color::Rgb(180, 120, 255); // abbey violet
const DIM: Color = Color::Rgb(120, 120, 140);
const OK: Color = Color::Rgb(120, 220, 160);
const WARN: Color = Color::Rgb(240, 180, 80);

pub fn draw(f: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, root[0], app);
    match app.tab {
        Tab::Home => draw_home(f, root[1], app),
        Tab::Chats => draw_chats(f, root[1], app),
        Tab::Personas => draw_lines_panel(
            f,
            root[1],
            app,
            &app.persona_lines,
            " Personas · Max/Gemma roles ",
        ),
        Tab::Memory => draw_lines_panel(
            f,
            root[1],
            app,
            &app.memory_lines,
            " Memory · self-learn LTM ",
        ),
        Tab::Skills => draw_lines_panel(
            f,
            root[1],
            app,
            &app.skill_lines,
            " Skills · plugins · peer tools ",
        ),
        Tab::Models => draw_models(f, root[1], app),
        Tab::Doctor => draw_doctor(f, root[1], app),
    }
    draw_input(f, root[2], app);
    draw_status(f, root[3], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            let selected = *t == app.tab;
            Line::from(Span::styled(
                format!(" {} ", t.title()),
                if selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DIM)
                },
            ))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .select(Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT))
                .title(Span::styled(
                    " ✦ Abbey ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )),
        )
        .divider(Span::raw("│"));
    f.render_widget(tabs, area);
}

fn draw_home(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    let chat = app.state.read_chat().unwrap_or_else(|| "—".into());
    let body = vec![
        Line::from(vec![
            Span::styled("model  ", Style::default().fg(DIM)),
            Span::styled(
                app.cfg.model.clone(),
                Style::default().fg(OK).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("chat   ", Style::default().fg(DIM)),
            Span::styled(chat, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("cwd    ", Style::default().fg(DIM)),
            Span::raw(app.state.cwd.display().to_string()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Keys",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from("  Enter          run session (or execute /slash)"),
        Line::from("  /help          Claude Code–style slash catalog"),
        Line::from("  Ctrl-n         new chat"),
        Line::from("  Ctrl-p         please-fix"),
        Line::from("  Tab / 1-7      Home Chats Personas Memory Skills Models Doctor"),
        Line::from("  Esc / q        quit (empty input)"),
        Line::from(""),
        Line::from(Span::styled(
            "Hybrid: Abbey/Aviva/Abi · Max/Gemma · parallel · OS control · self-learn.",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "Enter launches cursor-agent with persona/role wrap; TUI restores on exit.",
            Style::default().fg(DIM),
        )),
    ];

    let left = Paragraph::new(body)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Session ")
                .border_style(Style::default().fg(DIM)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(left, chunks[0]);

    let recent: Vec<ListItem> = app
        .history
        .iter()
        .take(12)
        .map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", &e.timestamp[11..19.min(e.timestamp.len())]),
                    Style::default().fg(DIM),
                ),
                Span::styled(
                    e.chat_id.chars().take(8).collect::<String>(),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" "),
                Span::styled(short_path(&e.cwd), Style::default().fg(DIM)),
            ]))
        })
        .collect();

    let right = List::new(recent).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Recent chats ")
            .border_style(Style::default().fg(DIM)),
    );
    f.render_widget(right, chunks[1]);
}

fn draw_lines_panel(f: &mut Frame, area: Rect, app: &App, lines: &[String], title: &str) {
    let items: Vec<ListItem> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let style = if i == app.list_idx {
                Style::default()
                    .bg(Color::Rgb(40, 30, 60))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(line.clone()).style(style)
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title.to_string())
            .border_style(Style::default().fg(ACCENT)),
    );
    f.render_widget(list, area);
}

fn draw_chats(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .history
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let style = if i == app.list_idx {
                Style::default()
                    .bg(Color::Rgb(40, 30, 60))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  ", e.timestamp), Style::default().fg(DIM)),
                Span::styled(e.chat_id.clone(), Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::raw(e.cwd.clone()),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Chat history · Enter to activate · j/k move ")
            .border_style(Style::default().fg(ACCENT)),
    );
    f.render_widget(list, area);
}

fn draw_models(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = if !app.live_models.is_empty() {
        app.live_models
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let style = if i == app.list_idx {
                    Style::default()
                        .bg(Color::Rgb(40, 30, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(line.clone()).style(style)
            })
            .collect()
    } else {
        app.aliases
            .iter()
            .enumerate()
            .map(|(i, (a, full))| {
                let style = if i == app.list_idx {
                    Style::default()
                        .bg(Color::Rgb(40, 30, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{a:<12}"),
                        Style::default().fg(OK).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(full.clone(), Style::default().fg(DIM)),
                ]))
                .style(style)
            })
            .collect()
    };

    let title = if app.live_models.is_empty() {
        " Model aliases · Enter to select · (live list empty / offline) "
    } else {
        " Models (from cursor-agent) · Enter to select "
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(ACCENT)),
    );
    f.render_widget(list, area);
}

fn draw_doctor(f: &mut Frame, area: Rect, app: &App) {
    let lines: Vec<Line> = app
        .doctor_lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let style = if i == app.list_idx {
                Style::default().bg(Color::Rgb(40, 30, 60))
            } else {
                Style::default()
            };
            Line::from(Span::styled(l.clone(), style))
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Doctor ")
                .border_style(Style::default().fg(WARN)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn draw_input(f: &mut Frame, area: Rect, app: &App) {
    let title = match app.tab {
        Tab::Home => " Prompt ",
        _ => " Prompt (switch to Home to type · Enter runs) ",
    };

    // Show cursor as inverted char
    let mut spans = Vec::new();
    let (before, after) = app.input.split_at(app.cursor.min(app.input.len()));
    spans.push(Span::raw(before.to_string()));
    if let Some(ch) = after.chars().next() {
        spans.push(Span::styled(
            ch.to_string(),
            Style::default().bg(ACCENT).fg(Color::Black),
        ));
        spans.push(Span::raw(after[ch.len_utf8()..].to_string()));
    } else {
        spans.push(Span::styled(
            " ",
            Style::default().bg(ACCENT).fg(Color::Black),
        ));
    }

    let p = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(p, area);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let exit = app
        .last_agent_code
        .map(|c| format!(" last={c}"))
        .unwrap_or_default();
    let line = Line::from(vec![
        Span::styled(" ▸ ", Style::default().fg(ACCENT)),
        Span::raw(app.status.clone()),
        Span::styled(exit, Style::default().fg(DIM)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn short_path(p: &str) -> String {
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
