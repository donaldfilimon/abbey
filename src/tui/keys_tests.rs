//! Unit tests for the TUI input spine (`keys.rs`).
//!
//! Everything here drives `App::handle_key` / `handle_mouse` with synthetic
//! crossterm events against a scratch state dir — no terminal, no backend
//! process. The assertions pin the event→state contracts the TUI promises:
//! editor correctness on multibyte input, slash suggest/accept, submit
//! semantics, focus/tab switching, palette open/close, and Esc ordering.

use super::super::app::App;
use super::super::tabs::{Focus, OverlayKind, PendingAction, Tab};
use crate::agent::{AgentBackend, AgentConfig};
use crate::state::AbbeyState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

fn scratch_app(tag: &str) -> App {
    let dir = std::env::temp_dir().join(format!(
        "abbey-keys-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("by-cwd")).unwrap();
    let state = AbbeyState {
        state_dir: dir.clone(),
        chat_file: dir.join("chat-id"),
        model_file: dir.join("model"),
        history_file: dir.join("history.log"),
        cwd_dir: dir.join("by-cwd"),
        per_cwd: false,
        cwd: dir,
    };
    // Cursor backend with an empty agent path: nothing in these tests may
    // spawn a process, and account-surface prefetch stays off.
    let cfg = AgentConfig {
        backend: AgentBackend::Cursor,
        ..AgentConfig::default()
    };
    App::new(state, cfg).expect("headless App")
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn type_str(app: &mut App, text: &str) {
    for c in text.chars() {
        app.handle_key(press(KeyCode::Char(c)));
    }
}

#[test]
fn editor_is_utf8_safe_at_every_cursor_move() {
    let mut app = scratch_app("utf8");
    // 'é' is 2 bytes; a byte-indexed cursor bug panics on the boundary.
    type_str(&mut app, "aéz");
    assert_eq!(app.input, "aéz");
    assert_eq!(app.cursor, app.input.len());

    app.handle_key(press(KeyCode::Left)); // before 'z'
    app.handle_key(press(KeyCode::Left)); // before 'é'
    app.handle_key(press(KeyCode::Backspace)); // deletes 'a'
    assert_eq!(app.input, "éz");
    app.handle_key(press(KeyCode::Delete)); // deletes 'é' under cursor
    assert_eq!(app.input, "z");
    app.handle_key(press(KeyCode::End));
    assert_eq!(app.cursor, app.input.len());
    app.handle_key(press(KeyCode::Home));
    assert_eq!(app.cursor, 0);
}

#[test]
fn typing_a_slash_opens_suggest_and_tab_accepts_it() {
    let mut app = scratch_app("slash");
    type_str(&mut app, "/he");
    assert_eq!(app.overlay, OverlayKind::SlashSuggest);

    app.handle_key(press(KeyCode::Tab));
    assert_eq!(app.overlay, OverlayKind::None);
    assert!(
        app.input.starts_with('/') && app.input.ends_with(' '),
        "accepted suggestion should be a complete '/cmd ': {:?}",
        app.input
    );

    // A slash with arguments is no longer a bare prefix: suggest closes.
    app.input.clear();
    app.cursor = 0;
    type_str(&mut app, "/model auto");
    assert_eq!(app.overlay, OverlayKind::None);
}

#[test]
fn natural_language_predicts_review_and_tab_accepts() {
    let mut app = scratch_app("predict-nl");
    type_str(&mut app, "review the auth diff");
    assert_eq!(app.overlay, OverlayKind::SlashSuggest);
    assert!(
        app.predictions.iter().any(|p| p.name == "review"),
        "{:?}",
        app.predictions
    );
    app.handle_key(press(KeyCode::Tab));
    assert!(
        app.input.starts_with("/review "),
        "accepted NL prediction: {:?}",
        app.input
    );
}

#[cfg(unix)]
#[test]
fn unchanged_input_attempts_the_llm_rerank_once() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::Duration;

    let mut app = scratch_app("predict-once");
    let script = app.state.state_dir.join("ollama");
    let calls = app.state.state_dir.join("ollama-calls");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = list ]; then printf 'NAME ID SIZE\\n{} digest 1GB\\n'; exit 0; fi\necho call >> '{}'\necho review\n",
            crate::tui::predict::PREDICT_MODEL,
            calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    app.cfg.backend = AgentBackend::Ollama;
    app.cfg.agent_path = script;
    type_str(&mut app, "review this change");
    app.predict_idle = 7;
    app.poll_command_prediction();
    let rx = app.predict_rx.take().expect("rerank receiver");
    let hint = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("rerank result");
    assert_eq!(hint.name, Some("review"));
    app.poll_command_prediction();
    app.poll_command_prediction();
    assert!(app.predict_rx.is_none(), "unchanged input reranked twice");
    assert_eq!(std::fs::read_to_string(calls).unwrap().lines().count(), 1);
}

#[test]
fn enter_routes_slash_input_and_plain_prompts_differently() {
    let mut app = scratch_app("submit");
    type_str(&mut app, "hello world");
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.pending, PendingAction::RunSession { fresh: false });
    assert_eq!(
        app.input_history.last().map(String::as_str),
        Some("hello world")
    );

    // The main loop clears the editor when it consumes `pending`; mirror that.
    app.pending = PendingAction::None;
    app.input.clear();
    app.cursor = 0;
    type_str(&mut app, "/doctor now");
    app.handle_key(press(KeyCode::Enter));
    assert_eq!(app.pending, PendingAction::Slash("/doctor now".into()));
}

#[test]
fn prompt_history_recalls_and_dedupes() {
    let mut app = scratch_app("history");
    for text in ["first", "second", "second"] {
        type_str(&mut app, text);
        app.handle_key(press(KeyCode::Enter));
        app.input.clear();
        app.cursor = 0;
    }
    assert_eq!(app.input_history, vec!["first", "second"], "dedup adjacent");

    app.handle_key(press(KeyCode::Up));
    assert_eq!(app.input, "second");
    app.handle_key(press(KeyCode::Up));
    assert_eq!(app.input, "first");
    app.handle_key(press(KeyCode::Down));
    assert_eq!(app.input, "second");
    app.handle_key(press(KeyCode::Down));
    assert_eq!(app.input, "", "walking past the newest entry clears");
}

#[test]
fn esc_clears_input_before_it_quits() {
    let mut app = scratch_app("esc");
    type_str(&mut app, "draft");
    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.input, "");
    assert!(!app.should_quit, "first Esc only clears the draft");
    app.handle_key(press(KeyCode::Esc));
    assert!(app.should_quit, "Esc on an empty prompt quits");
}

#[test]
fn focus_toggle_and_panel_keys() {
    let mut app = scratch_app("focus");
    assert_eq!(app.focus, Focus::Prompt);
    app.handle_key(press(KeyCode::Char('`')));
    assert_eq!(app.focus, Focus::Panel, "backtick toggles on empty input");

    // Digits address tabs directly in panel focus.
    app.handle_key(press(KeyCode::Char('2')));
    assert_eq!(app.tab, Tab::Chats);
    app.handle_key(press(KeyCode::Char('7')));
    assert_eq!(app.tab, Tab::Doctor);

    // Tab cycles; the filter state resets on entry.
    app.filter = "stale".into();
    let before = app.tab;
    app.handle_key(press(KeyCode::Tab));
    assert_eq!(app.tab, before.next());
    assert!(app.filter.is_empty(), "tab entry clears the panel filter");

    // 'q' quits from panel focus (no editor to type into).
    app.handle_key(press(KeyCode::Char('q')));
    assert!(app.should_quit);
}

#[test]
fn backtick_types_into_a_non_empty_prompt() {
    let mut app = scratch_app("backtick");
    type_str(&mut app, "cargo ");
    app.handle_key(press(KeyCode::Char('`')));
    assert_eq!(app.focus, Focus::Prompt, "focus must not steal mid-edit");
    assert_eq!(app.input, "cargo `");
}

#[test]
fn ctrl_k_palette_filters_and_escapes() {
    let mut app = scratch_app("palette");
    app.handle_key(ctrl('k'));
    assert_eq!(app.overlay, OverlayKind::Palette);

    type_str(&mut app, "the");
    assert_eq!(app.overlay_query, "the");
    assert!(
        app.input.is_empty(),
        "palette input must not leak into prompt"
    );

    app.handle_key(press(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayKind::None);
    assert!(app.overlay_query.is_empty());
}

#[test]
fn ctrl_q_quits_from_any_focus() {
    for focus in [Focus::Prompt, Focus::Panel] {
        let mut app = scratch_app("quit");
        app.focus = focus;
        app.handle_key(ctrl('q'));
        assert!(app.should_quit, "{focus:?}");
    }
}

#[test]
fn key_release_events_are_ignored() {
    let mut app = scratch_app("release");
    let mut release = press(KeyCode::Char('x'));
    release.kind = KeyEventKind::Release;
    app.handle_key(release);
    assert_eq!(app.input, "", "only Press events reach the editor");
}

#[test]
fn mouse_scroll_saturates_at_list_bounds() {
    let mut app = scratch_app("scroll");
    app.handle_mouse(MouseEventKind::ScrollUp);
    assert_eq!(app.list_idx, 0, "scrolling up at the top stays put");
    let len = app.list_len();
    for _ in 0..len + 5 {
        app.handle_mouse(MouseEventKind::ScrollDown);
    }
    assert!(
        app.list_idx <= len.saturating_sub(1),
        "scrolling down clamps to the last row"
    );
}
