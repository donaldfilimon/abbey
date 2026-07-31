//! Command palette, help, and slash autocomplete overlays.

use crate::slash::{SLASH_CATALOG, SlashCmd};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph, Wrap};

use super::tabs::OverlayKind;
use super::theme::Theme;
use super::widgets::{dim_style, list_highlight_style, rounded_block};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    Slash(&'static str),
    NewChat,
    PleaseFix,
    Refresh,
    CycleTheme,
    GotoDoctor,
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
        id: "doctor",
        label: "Open Doctor",
        detail: "Diagnostics panel",
        action: PaletteAction::GotoDoctor,
    },
    PaletteItem {
        id: "quit",
        label: "Quit",
        detail: "Leave the TUI",
        action: PaletteAction::Quit,
    },
];

/// Slash catalog entries whose name starts with `prefix` (leading `/` stripped).
pub fn slash_suggestions(prefix: &str) -> Vec<&'static SlashCmd> {
    let p = prefix.trim().trim_start_matches('/').to_ascii_lowercase();
    SLASH_CATALOG
        .iter()
        .filter(|c| p.is_empty() || c.name.starts_with(p.as_str()))
        .collect()
}

/// Built-in actions plus every slash command as a palette row.
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

/// Case-insensitive substring filter on id/label/detail.
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
        "Abbey TUI — keys",
        "",
        "  ` / Ctrl-L     toggle focus  prompt ↔ panel",
        "  Tab / S-Tab    next / previous tab",
        "  1-7            jump to tab",
        "  Ctrl-K         command palette",
        "  Ctrl-T         cycle theme (ink / violet / mono)",
        "  F1 / ?         help (empty prompt)",
        "  /              filter lists (panel focus)",
        "  ↑↓             history (prompt) · move (panel)",
        "  Enter          run / slash / select",
        "  Ctrl-n         new chat",
        "  Ctrl-p         please-fix",
        "  Ctrl-r         refresh",
        "  Esc            close overlay · clear · quit",
        "  Ctrl-q/c       quit",
        "",
        "Composer stays available on every tab. Panels are secondary.",
    ]
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

/// Draw the active overlay centered over `area`.
pub fn draw_overlay(
    f: &mut Frame,
    area: Rect,
    kind: OverlayKind,
    query: &str,
    idx: usize,
    theme: &Theme,
    slash_input: &str,
) {
    match kind {
        OverlayKind::None => {}
        OverlayKind::Help => draw_help(f, area, theme),
        OverlayKind::Palette => draw_palette(f, area, query, idx, theme),
        OverlayKind::SlashSuggest => draw_slash(f, area, slash_input, idx, theme),
    }
}

fn draw_help(f: &mut Frame, area: Rect, theme: &Theme) {
    let lines: Vec<Line> = help_lines()
        .into_iter()
        .map(|l| {
            if l.starts_with("Abbey") {
                Line::from(Span::styled(
                    l,
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(l, Style::default().fg(theme.fg)))
            }
        })
        .collect();
    let rect = centered(area, 56, (lines.len() as u16).saturating_add(2).min(24));
    f.render_widget(Clear, rect);
    let block = rounded_block(" Help · Esc ", theme, true);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme.bg).fg(theme.fg))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_palette(f: &mut Frame, area: Rect, query: &str, idx: usize, theme: &Theme) {
    let filtered = fuzzy_filter(&palette_items(), query);
    let rect = centered(area, 64, 18);
    f.render_widget(Clear, rect);
    let title = format!(" Palette · {query} ");
    let block = rounded_block(&title, theme, true);
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(theme.accent)),
            Span::raw(query.to_string()),
            Span::styled("█", Style::default().fg(theme.accent)),
        ])),
        chunks[0],
    );

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let style = if i == idx {
                list_highlight_style(theme)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<14}", it.label),
                    style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(it.detail.to_string(), dim_style(theme)),
            ]))
            .style(style)
        })
        .collect();
    f.render_widget(
        List::new(items).style(Style::default().bg(theme.bg)),
        chunks[1],
    );
}

fn draw_slash(f: &mut Frame, area: Rect, slash_input: &str, idx: usize, theme: &Theme) {
    let prefix = slash_input
        .split_whitespace()
        .next()
        .unwrap_or("/")
        .trim_start_matches('/');
    let suggestions = slash_suggestions(prefix);
    if suggestions.is_empty() {
        return;
    }
    let height = (suggestions.len() as u16).saturating_add(2).min(12);
    let width = 52u16.min(area.width.saturating_sub(2));
    // Anchor just above the bottom (composer region).
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height.saturating_add(4)));
    let rect = Rect::new(area.x + 1, y, width, height);
    f.render_widget(Clear, rect);
    let block = rounded_block(" /slash ", theme, true);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let items: Vec<ListItem> = suggestions
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let style = if i == idx {
                list_highlight_style(theme)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("/{:<14}", c.name),
                    style.fg(theme.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(c.help.to_string(), dim_style(theme)),
            ]))
            .style(style)
        })
        .collect();
    f.render_widget(List::new(items), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_prefix_filters() {
        let all = slash_suggestions("");
        assert!(!all.is_empty());
        let help = slash_suggestions("hel");
        assert!(help.iter().any(|c| c.name == "help"));
        assert!(slash_suggestions("zzz_nope").is_empty());
    }

    #[test]
    fn fuzzy_filters_palette() {
        let items = palette_items();
        let theme = fuzzy_filter(&items, "theme");
        assert!(theme.iter().any(|i| i.id == "theme"));
        let doctor = fuzzy_filter(&items, "doct");
        assert!(
            doctor
                .iter()
                .any(|i| i.id == "doctor" || i.label.contains("doctor"))
        );
    }
}
