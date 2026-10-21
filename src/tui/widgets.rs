//! Themed ratatui chrome helpers for Abbey TUI.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use super::theme::Theme;

/// Rounded panel block; uses `border_focus` when `focused`.
pub fn rounded_block(title: &str, theme: &Theme, focused: bool) -> Block<'static> {
    let border = if focused {
        theme.border_focus
    } else {
        theme.border
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(theme.bg).fg(theme.fg))
        .title(title.to_string())
}

fn chip_spans(label: &str, value: &str, theme: &Theme) -> Vec<Span<'static>> {
    let chip_style = Style::default().bg(theme.chip_bg);
    vec![
        Span::styled(format!(" {label} "), chip_style.fg(theme.fg_dim)),
        Span::styled(
            format!("{value} "),
            chip_style.fg(theme.chip_fg).add_modifier(Modifier::BOLD),
        ),
    ]
}

/// Render a horizontal row of KPI chips.
#[allow(dead_code)] // the chat layout does not draw the old KPI strip
pub fn draw_kpi_strip(f: &mut Frame, area: Rect, chips: &[(&str, &str)], theme: &Theme) {
    if chips.is_empty() || area.width == 0 || area.height == 0 {
        return;
    }
    let mut spans = Vec::with_capacity(chips.len() * 3);
    for (i, (label, value)) in chips.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.extend(chip_spans(label, value, theme));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// List row highlight style.
pub fn list_highlight_style(theme: &Theme) -> Style {
    Style::default()
        .bg(theme.selection_bg)
        .fg(theme.selection_fg)
        .add_modifier(Modifier::BOLD)
}

pub fn dim_style(theme: &Theme) -> Style {
    Style::default().fg(theme.fg_dim)
}

pub fn accent_style(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD)
}

/// Themed vertical scrollbar widget and matching state.
pub fn scrollbar_for(
    content_len: usize,
    visible: usize,
    offset: usize,
    theme: &Theme,
) -> (Scrollbar<'static>, ScrollbarState) {
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::default().fg(theme.accent))
        .track_style(Style::default().fg(theme.border))
        .begin_style(dim_style(theme))
        .end_style(dim_style(theme));
    let state = ScrollbarState::new(content_len)
        .position(offset)
        .viewport_content_length(visible);
    (scrollbar, state)
}

/// Draw a vertical scrollbar when content exceeds the viewport.
#[allow(dead_code)] // the chat transcript scrolls by line window, not this widget
pub fn draw_vertical_scrollbar(
    f: &mut Frame,
    area: Rect,
    content_len: usize,
    visible: usize,
    offset: usize,
    theme: &Theme,
) {
    if content_len <= visible || area.width == 0 || area.height == 0 {
        return;
    }
    let (scrollbar, mut state) = scrollbar_for(content_len, visible, offset, theme);
    f.render_stateful_widget(scrollbar, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Theme, ThemeId};

    #[test]
    fn helpers_build_without_panic() {
        let theme = Theme::from_id(ThemeId::Ink);
        let _ = rounded_block("Test", &theme, true);
        let _ = rounded_block("Test", &theme, false);
        assert!(!chip_spans("model", "gpt-4", &theme).is_empty());
        assert_eq!(list_highlight_style(&theme).bg, Some(theme.selection_bg));
        assert_eq!(dim_style(&theme).fg, Some(theme.fg_dim));
        assert_eq!(accent_style(&theme).fg, Some(theme.accent));
        let (_sb, _state) = scrollbar_for(100, 10, 5, &theme);
    }

    #[test]
    fn every_theme_builds_every_helper() {
        // A palette that is added later must not be a palette that panics
        // when a widget is drawn with it.
        for id in ThemeId::all() {
            let theme = Theme::from_id(id);
            let _ = rounded_block("Test", &theme, true);
            let _ = rounded_block("Test", &theme, false);
            assert!(!chip_spans("model", "gpt-4", &theme).is_empty());
            let (_sb, _state) = scrollbar_for(100, 10, 5, &theme);
        }
    }

    #[test]
    fn a_selected_row_is_readable_in_every_theme() {
        // The selection is the one place the app inverts its own colours,
        // so its text and its ground have to differ. A palette that failed
        // this would make the selected row unreadable.
        for id in ThemeId::all() {
            let theme = Theme::from_id(id);
            let style = list_highlight_style(&theme);
            assert_eq!(style.bg, Some(theme.selection_bg));
            assert_eq!(style.fg, Some(theme.selection_fg));
            assert_ne!(
                theme.selection_fg, theme.selection_bg,
                "{id:?} must make the selected row readable"
            );
        }
    }

    #[test]
    fn a_chip_is_readable_in_every_theme() {
        // A chip inverts too: its label and its value sit on the chip's own
        // ground.
        for id in ThemeId::all() {
            let theme = Theme::from_id(id);
            let spans = chip_spans("model", "gpt-4", &theme);
            assert_eq!(spans.len(), 2);
            assert_ne!(
                theme.chip_fg, theme.chip_bg,
                "{id:?} must make the chip's value readable"
            );
            assert_ne!(
                theme.fg_dim, theme.chip_bg,
                "{id:?} must make the chip's label readable"
            );
        }
    }
}
