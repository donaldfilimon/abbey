use super::app::{App, Overlay};
use crate::agent::{AgentBackend, AgentConfig};
use crate::state::AbbeyState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

pub(super) fn scratch_app(tag: &str, backend: AgentBackend) -> App {
    let dir = std::env::temp_dir().join(format!(
        "abbey-tui-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
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
    App::new(
        state,
        AgentConfig {
            backend,
            ..AgentConfig::default()
        },
    )
    .expect("headless App")
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(key(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(key(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, KeyCode::Char(c));
    }
}

#[test]
fn shift_enter_and_ctrl_j_insert_newlines_enter_submits() {
    let mut app = scratch_app("newline", AgentBackend::Cursor);
    type_str(&mut app, "a");
    app.handle_key(key(KeyCode::Enter, KeyModifiers::SHIFT));
    type_str(&mut app, "b");
    ctrl(&mut app, 'j');
    type_str(&mut app, "c");
    assert_eq!(app.composer.text, "a\nb\nc");
}

#[test]
fn bracketed_paste_inserts_without_submitting() {
    let mut app = scratch_app("paste", AgentBackend::Cursor);
    app.handle_paste("line one\nline two\n");
    assert_eq!(app.composer.text, "line one\nline two\n");
    assert!(app.run.is_none());
    assert!(app.transcript.cells.is_empty());
}

#[test]
fn ctrl_c_clears_then_quits() {
    let mut app = scratch_app("ctrlc", AgentBackend::Cursor);
    type_str(&mut app, "draft");
    ctrl(&mut app, 'c');
    assert!(app.composer.is_empty());
    assert!(!app.should_quit);
    ctrl(&mut app, 'c');
    assert!(app.should_quit);
}

#[test]
fn shift_tab_cycles_claude_permission_mode() {
    let mut app = scratch_app("perm", AgentBackend::Claude);
    app.handle_key(key(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.cfg.permission_mode.as_deref(), Some("acceptEdits"));
}

#[test]
fn bang_command_requires_confirmation_and_n_cancels() {
    let mut app = scratch_app("bang", AgentBackend::Cursor);
    type_str(&mut app, "!whoami");
    press(&mut app, KeyCode::Enter);
    assert!(
        matches!(&app.overlay, Overlay::Confirm { command } if command == &vec!["whoami".to_string()])
    );
    press(&mut app, KeyCode::Char('n'));
    assert!(matches!(app.overlay, Overlay::None));
    assert!(
        app.transcript
            .cells
            .iter()
            .all(|c| !matches!(c, super::transcript::Cell::Assistant(_)))
    );
}

#[test]
fn typing_while_running_queues_and_esc_cancels() {
    let mut app = scratch_app("queue", AgentBackend::Cursor);
    let (ev_tx, events) = std::sync::mpsc::channel();
    let (_done_tx, done) = std::sync::mpsc::channel();
    let cancel = crate::runtime::CancellationToken::new();
    app.run = Some(super::worker::RunHandle {
        events,
        done,
        cancel: cancel.clone(),
        started: std::time::Instant::now(),
        thread: None,
        completion: None,
        local: false,
    });
    drop(ev_tx);
    type_str(&mut app, "next thing");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.queued, vec!["next thing".to_string()]);
    press(&mut app, KeyCode::Esc);
    assert!(cancel.is_cancelled());
}

#[test]
fn esc_esc_on_idle_recalls_last_prompt() {
    let mut app = scratch_app("escesc", AgentBackend::Cursor);
    app.last_submitted = Some("previous".into());
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.composer.text, "previous");
}

#[test]
fn ctrl_k_opens_palette_and_esc_closes_it() {
    let mut app = scratch_app("palette", AgentBackend::Cursor);
    ctrl(&mut app, 'k');
    assert!(matches!(app.overlay, Overlay::Palette { .. }));
    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.overlay, Overlay::None));
}

use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn screen(app: &App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| super::render::draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                + "\n"
        })
        .collect()
}

#[test]
fn transcript_shows_user_markdown_and_tool_cells() {
    let mut app = scratch_app("render", AgentBackend::Claude);
    app.transcript.push_user("fix the bug");
    app.transcript.apply(crate::stream::StreamEvent::TextDelta(
        "**Done** — see `a.rs`".into(),
    ));
    app.transcript.apply(crate::stream::StreamEvent::ToolStart {
        id: "t".into(),
        name: "Bash".into(),
        input: serde_json::json!({"command": "cargo test"}),
    });
    let s = screen(&app, 80, 24);
    assert!(s.contains("› fix the bug"));
    assert!(s.contains("Done — see a.rs"));
    assert!(s.contains("Bash  cargo test"));
    assert!(s.contains("claude"), "header names the backend");
    assert!(s.contains("default"), "header names the permission mode");
}

#[test]
fn narrow_terminal_renders_without_panic_and_keeps_the_composer() {
    let mut app = scratch_app("narrow", AgentBackend::Ollama);
    app.composer.set("hello\nworld".into());
    let s = screen(&app, 40, 12);
    assert!(s.contains("hello"));
    assert!(s.contains("world"));
}

#[test]
fn overlays_render_on_top() {
    let mut app = scratch_app("overlays", AgentBackend::Cursor);
    for o in [
        Overlay::Help,
        Overlay::Palette {
            query: String::new(),
            idx: 0,
        },
        Overlay::Panel(super::app::Panel::Doctor),
        Overlay::ModelPicker { idx: 0 },
        Overlay::ResumePicker { idx: 0 },
        Overlay::Confirm {
            command: vec!["whoami".into()],
        },
        Overlay::Search { query: "x".into() },
    ] {
        app.overlay = o;
        let s = screen(&app, 80, 24);
        assert!(!s.trim().is_empty());
    }
    app.overlay = Overlay::Confirm {
        command: vec!["whoami".into()],
    };
    assert!(screen(&app, 80, 24).contains("whoami"));
}

#[test]
fn scrolling_up_hides_the_newest_line() {
    let mut app = scratch_app("scroll", AgentBackend::Cursor);
    for i in 0..60 {
        app.transcript.push_notice(format!("line-{i}"));
    }
    assert!(screen(&app, 60, 20).contains("line-59"));
    app.scroll_from_bottom = 30;
    assert!(!screen(&app, 60, 20).contains("line-59"));
}

#[test]
fn pump_applies_events_finishes_and_dequeues() {
    let mut app = scratch_app("pump", AgentBackend::Cursor);
    let (ev_tx, events) = std::sync::mpsc::channel();
    let (done_tx, done) = std::sync::mpsc::channel();
    app.run = Some(super::worker::RunHandle {
        events,
        done,
        cancel: crate::runtime::CancellationToken::new(),
        started: std::time::Instant::now(),
        thread: None,
        completion: None,
        local: false,
    });
    ev_tx
        .send(crate::stream::StreamEvent::TextDelta("answer".into()))
        .unwrap();
    ev_tx
        .send(crate::stream::StreamEvent::Done { exit: 0 })
        .unwrap();
    done_tx.send((0, None)).unwrap();
    app.pump();
    assert!(app.run.is_none());
    assert!(
        matches!(app.transcript.cells.last(), Some(super::transcript::Cell::Assistant(s)) if s == "answer")
    );
    assert!(app.status.contains("exit 0"));
}

fn fixture_run(
    app: &mut App,
) -> (
    std::sync::mpsc::Sender<crate::stream::StreamEvent>,
    std::sync::mpsc::Sender<(i32, Option<String>)>,
) {
    let (tx, events) = std::sync::mpsc::channel();
    let (done_tx, done) = std::sync::mpsc::channel();
    app.run = Some(super::worker::RunHandle {
        events,
        done,
        cancel: crate::runtime::CancellationToken::new(),
        started: std::time::Instant::now(),
        thread: None,
        completion: None,
        local: false,
    });
    (tx, done_tx)
}

#[test]
fn pump_is_bounded_and_drains_before_exactly_one_completion() {
    let mut app = scratch_app("bounded-pump", AgentBackend::Ollama);
    let (tx, done) = fixture_run(&mut app);
    for _ in 0..257 {
        tx.send(crate::stream::StreamEvent::TextDelta("x".into()))
            .unwrap();
    }
    tx.send(crate::stream::StreamEvent::Done { exit: 2 })
        .unwrap();
    done.send((2, None)).unwrap();
    app.pump();
    assert!(app.is_running());
    assert!(
        matches!(&app.transcript.cells[0], super::transcript::Cell::Assistant(s) if s.len() == 128)
    );
    app.pump();
    assert!(app.is_running());
    app.pump();
    assert!(!app.is_running());
    app.pump();
    assert_eq!(
        app.transcript
            .cells
            .iter()
            .filter(|c| matches!(c, super::transcript::Cell::Error(_)))
            .count(),
        1
    );
}

#[test]
fn completion_joins_event_producer_before_the_final_drain() {
    let mut app = scratch_app("completion-drain", AgentBackend::Ollama);
    let (tx, done) = fixture_run(&mut app);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    app.run.as_mut().unwrap().thread = Some(std::thread::spawn(move || {
        done.send((0, None)).unwrap();
        ready_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        tx.send(crate::stream::StreamEvent::TextDelta("final event".into()))
            .unwrap();
    }));
    ready_rx.recv().unwrap();
    app.pump();
    assert!(!app.is_running());
    assert!(
        matches!(app.transcript.cells.last(), Some(super::transcript::Cell::Assistant(s)) if s == "final event")
    );
}

#[test]
fn failure_and_interrupt_discard_queue_but_keep_unsent_draft() {
    for code in [1, 130] {
        let mut app = scratch_app("discard-queue", AgentBackend::Ollama);
        let (_tx, done) = fixture_run(&mut app);
        app.queued = vec!["one".into(), "two".into()];
        app.composer.set("unsent draft".into());
        done.send((code, None)).unwrap();
        app.pump();
        assert!(app.queued.is_empty());
        assert_eq!(app.composer.text, "unsent draft");
        assert!(app.status.contains("discarded 2"));
        assert!(!app.is_running());
    }
}

#[test]
fn successful_queue_advance_preserves_draft_and_order() {
    let mut app = scratch_app("queue-success", AgentBackend::Ollama);
    app.cfg.agent_path = app.state.state_dir.join("missing-fixture-agent");
    let (_tx, done) = fixture_run(&mut app);
    app.queued = vec!["next prompt".into(), "last prompt".into()];
    app.composer.set("editing while streaming".into());
    app.composer.cursor = 3;
    done.send((0, None)).unwrap();
    app.pump();
    assert_eq!(app.last_submitted.as_deref(), Some("next prompt"));
    assert_eq!(app.queued, vec!["last prompt"]);
    assert_eq!(app.composer.text, "editing while streaming");
    assert_eq!(app.composer.cursor, 3);
    app.shutdown().unwrap();
}

#[test]
fn worker_panic_and_missing_completion_fail_closed() {
    for panic in [false, true] {
        let mut app = scratch_app("worker-disconnect", AgentBackend::Ollama);
        let (tx, done) = fixture_run(&mut app);
        drop(tx);
        app.run.as_mut().unwrap().thread = Some(std::thread::spawn(move || {
            drop(done);
            assert!(!panic, "fixture worker panic");
        }));
        app.queued.push("must not execute".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while app.is_running() && std::time::Instant::now() < deadline {
            app.pump();
            std::thread::yield_now();
        }
        assert!(!app.is_running());
        assert!(app.queued.is_empty());
        assert!(app.status.contains("exit 1"));
        let wanted = if panic { "panicked" } else { "disconnected" };
        assert!(
            app.transcript
                .cells
                .iter()
                .any(|c| matches!(c, super::transcript::Cell::Error(s) if s.contains(wanted)))
        );
    }
}

#[test]
fn shutdown_cancels_and_joins_before_returning() {
    let mut app = scratch_app("shutdown", AgentBackend::Ollama);
    let (_tx, _done) = fixture_run(&mut app);
    let cancel = app.run.as_ref().unwrap().cancel.clone();
    let (ack_tx, ack_rx) = std::sync::mpsc::channel();
    app.run.as_mut().unwrap().thread = Some(std::thread::spawn(move || {
        while !cancel.is_cancelled() {
            std::thread::yield_now();
        }
        ack_tx.send(()).unwrap();
    }));
    app.queued.push("do not launch".into());
    app.shutdown().unwrap();
    ack_rx
        .try_recv()
        .expect("worker joined after observing cancellation");
    assert!(!app.is_running());
    assert!(app.queued.is_empty());
}

#[test]
fn empty_or_tiny_terminal_area_renders_without_panicking() {
    let app = scratch_app("tiny-terminal", AgentBackend::Ollama);
    for (width, height) in [(0, 0), (1, 1), (2, 2), (8, 4)] {
        let _ = screen(&app, width, height);
    }
}

#[test]
fn pasted_unicode_separator_keeps_a_draft_and_completes_a_file() {
    let mut app = scratch_app("unicode-file-paste", AgentBackend::Cursor);
    app.files = super::completion::FileIndex::from_paths(vec!["src/main.rs".into()]);
    app.handle_paste("hello\u{3000}@src");
    assert_eq!(app.composer.text, "hello\u{3000}@src");
    assert_eq!(app.composer.cursor, app.composer.text.len());
    assert!(matches!(
        &app.completion,
        Some(super::completion::Completion::File { token_start, items })
            if *token_start == "hello\u{3000}".len() && items == &vec!["src/main.rs".to_string()]
    ));
    assert!(app.run.is_none(), "paste submitted a provider run");
    assert!(app.transcript.cells.is_empty());
}

#[test]
fn ctrl_u_then_accept_cannot_apply_a_cached_file_range() {
    for accept_key in [KeyCode::Enter, KeyCode::Tab] {
        let mut app = scratch_app("stale-file-ctrl-u", AgentBackend::Cursor);
        app.files = super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
        type_str(&mut app, "look @app");
        assert!(matches!(
            app.completion,
            Some(super::completion::Completion::File { token_start: 5, .. })
        ));
        ctrl(&mut app, 'u');
        assert_eq!(app.composer.text, "");
        assert_eq!(app.composer.cursor, 0);
        press(&mut app, accept_key);
        assert!(app.composer.is_empty());
        assert!(app.completion.is_none());
        assert!(app.run.is_none());
    }
}

#[test]
fn completion_tracks_word_kill_newline_and_cursor_home() {
    for mutation in [
        key(KeyCode::Char('w'), KeyModifiers::CONTROL),
        key(KeyCode::Char('j'), KeyModifiers::CONTROL),
        key(KeyCode::Home, KeyModifiers::NONE),
    ] {
        let mut app = scratch_app("completion-mutated", AgentBackend::Cursor);
        app.files = super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
        type_str(&mut app, "look @app");
        assert!(matches!(
            app.completion,
            Some(super::completion::Completion::File { .. })
        ));
        app.handle_key(mutation);
        assert!(
            app.completion.is_none(),
            "cached completion survived {mutation:?}"
        );
        assert!(app.run.is_none());
    }
}

#[test]
fn recalled_history_replaces_a_cached_file_completion() {
    let mut app = scratch_app("completion-history", AgentBackend::Cursor);
    app.files = super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
    app.history_log.push("previous plain draft");
    type_str(&mut app, "look @app");
    assert!(matches!(
        app.completion,
        Some(super::completion::Completion::File { .. })
    ));
    // Up navigates an active completion menu. Use the real history-search
    // acceptance path to replace the composer without that unrelated action.
    ctrl(&mut app, 'r');
    type_str(&mut app, "previous");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.composer.text, "previous plain draft");
    assert!(!matches!(
        app.completion,
        Some(super::completion::Completion::File { .. })
    ));
    assert!(app.run.is_none());
}

#[test]
fn natural_language_intent_completion_survives_the_chat_rewrite() {
    let mut app = scratch_app("intent-completion", AgentBackend::Cursor);
    type_str(&mut app, "review the auth diff");
    assert!(matches!(
        &app.completion,
        Some(super::completion::Completion::Slash(predictions))
            if predictions.iter().any(|prediction| prediction.name == "review")
    ));
    press(&mut app, KeyCode::Tab);
    assert!(app.composer.text.starts_with("/review "));
    assert!(app.composer.text.contains("auth diff"));
    assert!(app.run.is_none(), "acceptance submitted the prompt");
}

#[test]
fn retained_panel_rows_remain_accessible_with_existing_navigation() {
    let mut app = scratch_app("panel-navigation", AgentBackend::Cursor);
    app.skill_lines = (0..60).map(|row| format!("panel-row-{row:02}")).collect();
    ctrl(&mut app, 'k');
    type_str(&mut app, "Skills and plugins inventory");
    press(&mut app, KeyCode::Enter);
    assert!(matches!(
        app.overlay,
        Overlay::Panel(super::app::Panel::Skills)
    ));
    assert!(screen(&app, 80, 20).contains("panel-row-00"));
    assert!(!screen(&app, 80, 20).contains("panel-row-59"));
    press(&mut app, KeyCode::End);
    assert!(matches!(
        app.overlay,
        Overlay::Panel(super::app::Panel::Skills)
    ));
    assert!(screen(&app, 80, 20).contains("panel-row-59"));
    press(&mut app, KeyCode::Home);
    assert!(matches!(
        app.overlay,
        Overlay::Panel(super::app::Panel::Skills)
    ));
    assert!(screen(&app, 80, 20).contains("panel-row-00"));
    assert!(app.composer.is_empty());
    assert!(app.run.is_none());
}

#[test]
fn retained_panel_filter_finds_rows_below_the_first_viewport() {
    let mut app = scratch_app("panel-filter", AgentBackend::Cursor);
    app.skill_lines = (0..60).map(|row| format!("panel-row-{row:02}")).collect();
    ctrl(&mut app, 'k');
    type_str(&mut app, "Skills and plugins inventory");
    press(&mut app, KeyCode::Enter);
    assert!(matches!(
        app.overlay,
        Overlay::Panel(super::app::Panel::Skills)
    ));
    press(&mut app, KeyCode::Char('/'));
    assert!(matches!(
        app.overlay,
        Overlay::Panel(super::app::Panel::Skills)
    ));
    type_str(&mut app, "panel-row-59");
    let rendered = screen(&app, 80, 20);
    assert!(rendered.contains("panel-row-59"));
    assert!(!rendered.contains("panel-row-00"));
    assert!(
        app.composer.is_empty(),
        "panel filter leaked into the composer"
    );
    assert!(app.run.is_none());
}

#[test]
fn palette_navigation_cannot_select_past_the_last_filtered_row() {
    let mut app = scratch_app("palette-bound", AgentBackend::Cursor);
    let previous_theme = app.theme_id;
    ctrl(&mut app, 'k');
    type_str(&mut app, "Cycle theme");
    let count =
        super::overlays::fuzzy_filter(&super::overlays::palette_items(), "Cycle theme").len();
    assert_eq!(
        count, 1,
        "fixture must select the one built-in theme action"
    );
    for _ in 0..count + 10 {
        press(&mut app, KeyCode::Down);
    }
    assert!(matches!(&app.overlay, Overlay::Palette { idx, .. } if *idx == count - 1));
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Overlay::None));
    assert_ne!(app.theme_id, previous_theme);
    assert!(app.run.is_none());
}

#[test]
fn model_picker_navigation_matches_the_displayed_last_row() {
    let mut app = scratch_app("model-picker-bound", AgentBackend::Cursor);
    app.live_models = vec!["first-model".into(), "last-model".into()];
    app.overlay = Overlay::ModelPicker { idx: 0 };
    for _ in 0..20 {
        press(&mut app, KeyCode::Down);
    }
    assert!(matches!(app.overlay, Overlay::ModelPicker { idx: 1 }));
    // Actual model command dispatch is covered by the isolated real-binary
    // PTY case; this unit fixture must not spawn its own test executable.
    assert!(app.run.is_none());
}

#[test]
fn resume_picker_accepts_the_displayed_last_row_after_excess_down_keys() {
    // ABI has no server-session ambient Cursor override; read the actual
    // canonical saved identity without inheriting the developer's chat ID.
    let mut app = scratch_app("resume-picker-bound", AgentBackend::Abi);
    app.history = ["first-chat", "last-chat"]
        .into_iter()
        .map(|chat_id| crate::state::HistoryEntry {
            timestamp: "2026-10-03T00:00:00Z".into(),
            chat_id: chat_id.into(),
            cwd: app.state.cwd.display().to_string(),
        })
        .collect();
    app.overlay = Overlay::ResumePicker { idx: 0 };
    for _ in 0..20 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Overlay::None));
    assert_eq!(
        app.state
            .resolve_chat_for(AgentBackend::Abi)
            .unwrap()
            .as_deref(),
        Some("last-chat")
    );
    assert!(app.transcript.cells.iter().any(|cell| {
        matches!(cell, super::transcript::Cell::Notice(text) if text.contains("resumed chat last-chat"))
    }));
    assert!(app.run.is_none());
}

#[test]
fn new_backend_turn_renders_na_until_it_reports_usage() {
    let mut app = scratch_app("usage-backend-turn", AgentBackend::Claude);
    app.transcript.begin_turn();
    app.transcript.apply(crate::stream::StreamEvent::Usage {
        input_tokens: 7,
        output_tokens: 2,
    });
    app.transcript
        .apply(crate::stream::StreamEvent::Done { exit: 0 });
    assert!(screen(&app, 90, 20).contains("tokens 7↑ 2↓"));

    app.cfg.backend = AgentBackend::Ollama;
    app.transcript.begin_turn();
    let active = screen(&app, 90, 20);
    assert!(active.contains("ollama"));
    assert!(active.contains("tokens n/a"));
    assert!(!active.contains("tokens 7↑ 2↓"));
    app.transcript
        .apply(crate::stream::StreamEvent::TextDelta("plain answer".into()));
    app.transcript
        .apply(crate::stream::StreamEvent::Done { exit: 0 });
    assert!(screen(&app, 90, 20).contains("tokens n/a"));

    app.transcript.begin_turn();
    app.transcript.apply(crate::stream::StreamEvent::Usage {
        input_tokens: 19,
        output_tokens: 5,
    });
    assert!(screen(&app, 90, 20).contains("tokens 19↑ 5↓"));
}

#[cfg(unix)]
mod prediction_producer;

#[path = "tests/completion_mutations.rs"]
mod completion_mutations;
