//! The overlay keys: a chord is not a letter, and a palette describes the
//! cycle the app really walks.

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

fn scratch(tag: &str) -> App {
    scratch_app(tag, AgentBackend::Cursor)
}

#[test]
fn a_modified_y_does_not_run_the_confirmed_command() {
    // The dialog says "y = run". A chord is not that gesture: Ctrl-Y must
    // not run an allowlisted OS command behind the user's back.
    let mut app = scratch("overlay-confirm-chord");
    app.overlay = Overlay::Confirm {
        command: vec!["whoami".into()],
    };
    app.handle_key(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert!(
        app.run.is_none(),
        "Ctrl-Y must not run the confirmed command"
    );
    assert!(
        !app
            .transcript
            .cells
            .iter()
            .any(|cell| matches!(cell, super::super::transcript::Cell::User(text) if text.contains("whoami"))),
        "Ctrl-Y must not confirm the command: {:?}",
        app.transcript.cells
    );
    assert!(
        app.status.contains("not run"),
        "a chord must cancel the dialog, said {:?}",
        app.status
    );
}

#[test]
fn a_modified_y_is_not_a_yes_but_a_plain_y_is() {
    // The other half of the same rule: a plain `y` still confirms. The
    // fixture run is already active, so the confirmed command is not
    // launched a second time.
    let mut app = scratch("overlay-confirm-plain");
    let (_events, _done) = fixture_run(&mut app);
    app.overlay = Overlay::Confirm {
        command: vec!["whoami".into()],
    };
    app.handle_key(key(KeyCode::Char('y'), KeyModifiers::NONE));
    assert!(
        app.transcript
            .cells
            .iter()
            .any(|cell| matches!(cell, super::super::transcript::Cell::User(text) if text.contains("whoami"))),
        "a plain y must confirm the command: {:?}",
        app.transcript.cells
    );
}

#[test]
fn ctrl_c_does_not_type_into_the_history_search() {
    let mut app = scratch("overlay-search-chord");
    app.overlay = Overlay::Search {
        query: String::new(),
    };
    app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    match &app.overlay {
        Overlay::Search { query } => assert!(
            query.is_empty(),
            "Ctrl-C must not type a `c` into the search query, saw {query:?}"
        ),
        other => panic!("the search overlay must stay open, saw {other:?}"),
    }
}

#[test]
fn a_chord_does_not_type_into_the_palette_or_fire_its_binding() {
    let mut app = scratch("overlay-palette-chord");
    let theme = app.theme_id;
    ctrl(&mut app, 'k');
    app.handle_key(key(KeyCode::Char('t'), KeyModifiers::CONTROL));
    match &app.overlay {
        Overlay::Palette { query, .. } => assert!(
            query.is_empty(),
            "a chord must not type into the palette query, saw {query:?}"
        ),
        other => panic!("the palette must stay open, saw {other:?}"),
    }
    assert_eq!(
        app.theme_id, theme,
        "a chord must not fire a binding behind the palette"
    );
}

#[test]
fn the_palette_describes_the_theme_cycle_the_app_actually_visits() {
    // The description is a promise about the cycle. It has to list the
    // palettes the cycle really visits, in the order it visits them.
    let described = super::super::overlays::palette_items()
        .into_iter()
        .find(|item| item.id == "theme")
        .expect("the palette has a theme item")
        .detail;
    let mut last = 0;
    for id in ThemeId::all() {
        let at = described.find(id.as_str()).unwrap_or_else(|| {
            panic!("the description {described:?} must mention {}", id.as_str())
        });
        assert!(
            at >= last,
            "the description {described:?} must list the cycle in order"
        );
        last = at;
    }
}
