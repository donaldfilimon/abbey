//! The TUI's key bindings.
//!
//! A wrong binding is a silent failure: the feature is still there, the
//! keystroke just does not reach it, and nothing says why. These tests pin
//! the bindings that carry a feature, and the ones whose *absence* is a
//! safety property.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl(code: KeyCode) -> KeyEvent {
    key(code, KeyModifiers::CONTROL)
}

fn scratch(tag: &str) -> App {
    scratch_app(tag, crate::agent::AgentBackend::Ollama)
}

#[test]
fn ctrl_t_cycles_the_theme() {
    let mut app = scratch("keymap-theme");
    let before = app.theme_id;
    app.handle_key(ctrl(KeyCode::Char('t')));
    assert_ne!(app.theme_id, before, "Ctrl-T must cycle the theme");
}

#[test]
fn ctrl_y_asks_to_copy_the_last_reply() {
    let mut app = scratch("keymap-copy");
    // With no reply there is nothing to copy, and the status says so
    // rather than reporting a copy that did not happen.
    app.handle_key(ctrl(KeyCode::Char('y')));
    assert!(
        app.status.contains("nothing to copy"),
        "an empty reply must not report a copy, said {:?}",
        app.status
    );
}

#[test]
fn ctrl_k_opens_the_palette() {
    let mut app = scratch("keymap-palette");
    app.handle_key(ctrl(KeyCode::Char('k')));
    assert!(
        matches!(app.overlay, Overlay::Palette { .. }),
        "Ctrl-K must open the palette, saw {:?}",
        app.overlay
    );
}

#[test]
fn f1_and_question_mark_open_the_help() {
    let mut app = scratch("keymap-help-f1");
    app.handle_key(key(KeyCode::F(1), KeyModifiers::NONE));
    assert!(
        matches!(app.overlay, Overlay::Help),
        "F1 must open the help"
    );

    let mut app = scratch("keymap-help-question");
    // The question mark only opens the help when the composer is empty —
    // otherwise it is a character the user is typing.
    app.composer.set("".into());
    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(matches!(app.overlay, Overlay::Help));
}

#[test]
fn a_question_mark_in_a_draft_is_a_character_not_the_help() {
    let mut app = scratch("keymap-question-draft");
    app.composer.set("what?".into());
    app.handle_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(
        !matches!(app.overlay, Overlay::Help),
        "a question mark in a draft must not open the help"
    );
}

#[test]
fn ctrl_c_arms_before_it_quits() {
    // The first Ctrl-C clears; a second one quits. A single Ctrl-C must
    // never quit, because a stray keystroke would lose the session.
    let mut app = scratch("keymap-ctrl-c");
    app.handle_key(ctrl(KeyCode::Char('c')));
    assert!(
        app.status.contains("quit"),
        "the first Ctrl-C must arm the quit, said {:?}",
        app.status
    );
    assert!(!app.should_quit, "one Ctrl-C must not quit");
}

#[test]
fn a_key_release_is_ignored() {
    // Terminals report a key release as well as a press. Acting on the
    // release would fire every binding twice.
    let mut app = scratch("keymap-release");
    let before = app.theme_id;
    app.handle_key(KeyEvent {
        code: KeyCode::Char('t'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    });
    assert_eq!(app.theme_id, before, "a key release must do nothing");
}

#[test]
fn a_binding_does_not_fire_while_an_overlay_is_open() {
    // The overlay owns the keyboard while it is up, so a stray Ctrl-T
    // behind the help does not change the theme underneath it.
    let mut app = scratch("keymap-overlay");
    app.overlay = Overlay::Help;
    let before = app.theme_id;
    app.handle_key(ctrl(KeyCode::Char('t')));
    assert_eq!(
        app.theme_id, before,
        "a binding must not fire while an overlay is open"
    );
}
