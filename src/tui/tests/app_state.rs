//! The app's own state: the theme the user chose, and the status line.

use super::*;
use crate::tui::theme::ThemeId;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// The theme the state directory holds, if any.
///
/// The file name belongs to the theme module, so the test looks for the
/// content instead: a file whose trimmed content is a palette name. The
/// `ABBEY_TUI_THEME` override is deliberately not consulted — it is the
/// developer's own choice, and it would mask the persisted file.
fn persisted_theme(state_dir: &std::path::Path) -> Option<ThemeId> {
    for entry in std::fs::read_dir(state_dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(id) = ThemeId::parse(text.trim()) {
            return Some(id);
        }
    }
    None
}

#[test]
fn cycling_the_theme_previews_it_without_choosing_it() {
    // A persisted palette wins over the terminal's own brightness for
    // every later session. One stray cycle key must not decide that for
    // the user.
    let mut app = scratch_app("theme-cycle", AgentBackend::Cursor);
    let before = app.theme_id;
    app.handle_key(key(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert_ne!(app.theme_id, before, "Ctrl-T must cycle the theme");
    assert_eq!(
        persisted_theme(&app.state.state_dir),
        None,
        "cycling must not persist a palette"
    );
    assert!(
        app.status.contains("not saved"),
        "the status must say the palette was not saved, said {:?}",
        app.status
    );
}

#[test]
fn choosing_a_palette_in_the_overlay_saves_it() {
    // Choosing is the deliberate act, and it is remembered.
    let mut app = scratch_app("theme-choose", AgentBackend::Cursor);
    ctrl(&mut app, 'k');
    type_str(&mut app, "Theme: daylight");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.theme_id, ThemeId::Daylight);
    assert_eq!(
        persisted_theme(&app.state.state_dir),
        Some(ThemeId::Daylight),
        "an explicitly chosen palette must be persisted"
    );
    assert!(
        app.status.contains("saved"),
        "the status must say the palette was saved, said {:?}",
        app.status
    );
}

#[test]
fn the_status_line_stays_bounded() {
    // The status is one row of the terminal. Every discarded queue adds a
    // segment to it, and the line has to stay readable.
    let mut app = scratch_app("status-bound", AgentBackend::Cursor);
    for _ in 0..50 {
        app.queued.push("pending prompt".into());
        app.discard_queue();
    }
    assert!(
        app.status.chars().count() <= 200,
        "the status grew without bound: {} characters",
        app.status.chars().count()
    );
    assert!(
        app.status.contains("discarded"),
        "the newest notice must survive the trim, said {:?}",
        app.status
    );
}
