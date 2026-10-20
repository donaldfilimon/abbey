use super::*;
use crate::agent::AgentBackend;
use crate::tui::tests::scratch_app;
use crate::tui::worker::RunHandle;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::mpsc;
use std::time::Instant;

fn screen(app: &App, width: u16, height: u16) -> (String, Option<(u16, u16)>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let frame = terminal.draw(|f| draw(f, app)).unwrap();
    let mut text = String::new();
    for y in 0..height {
        for x in 0..width {
            text.push_str(frame.buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    let backend = terminal.backend();
    let cursor = backend.cursor_visible().then(|| {
        let p = backend.cursor_position();
        (p.x, p.y)
    });
    (text, cursor)
}

#[test]
fn soft_wrap_tracks_long_line_in_middle_and_at_end() {
    let text = "abcdefghijklmnopqrstuvwxyz";
    let middle = ComposerLayout::new(text, 13, 8);
    assert_eq!((middle.row, middle.col), (1, 5));
    let end = ComposerLayout::new(text, text.len(), 8);
    assert_eq!((end.row, end.col), (3, 2));
    assert_eq!(end.lines[3].to_string(), "yz");
    let exact = ComposerLayout::new("abcdefgh", 8, 8);
    assert_eq!((exact.row, exact.col), (1, 0));
    assert_eq!(exact.lines.len(), 2);
}

#[test]
fn unicode_graphemes_use_display_cells_not_chars_or_bytes() {
    let text = "ab界e\u{301}👩‍💻z";
    let middle = ComposerLayout::new(text, "ab界".len(), 6);
    assert_eq!((middle.row, middle.col), (0, 4));
    let end = ComposerLayout::new(text, text.len(), 6);
    assert_eq!((end.row, end.col), (1, 3));
    assert_eq!(end.lines[0].to_string(), "ab界e\u{301}");
    assert_eq!(end.lines[1].to_string(), "👩‍💻z");
    let narrow = ComposerLayout::new("界x", "界x".len(), 1);
    assert!(narrow.lines.iter().all(|l| l.width() <= 1));
    assert_eq!((narrow.row, narrow.col), (2, 0));
}

#[test]
fn hard_lines_empty_lines_and_pasted_controls_keep_cursor_mapping() {
    let empty = ComposerLayout::new("", 0, 0);
    assert_eq!((empty.row, empty.col), (0, 0));
    let hard = ComposerLayout::new("first\n\nlast", 7, 20);
    assert_eq!((hard.row, hard.col), (2, 0));
    let control = ComposerLayout::new("a\tb\u{1b}c", 4, 20);
    assert_eq!((control.row, control.col), (0, 7));
    assert_eq!(control.lines[0].to_string(), "a    b�c");
}

#[test]
fn full_width_hard_newlines_do_not_insert_cursor_dependent_blank_rows() {
    for (text, width, before, after) in [
        ("abcdefgh\nX", 8, 8, 9),
        ("abcd\n界z", 4, 4, 5),
        ("ab\n界z", 2, 2, 3),
    ] {
        let expected: Vec<String> = ComposerLayout::new(text, after, width)
            .lines
            .into_iter()
            .map(|line| line.to_string())
            .collect();
        let layout = ComposerLayout::new(text, before, width);
        let actual: Vec<String> = layout.lines.iter().map(ToString::to_string).collect();
        assert_eq!(actual, expected, "{text:?}, width={width}");
        assert_eq!((layout.row, layout.col), (1, 0));
    }
}

#[test]
fn welcome_exposes_provider_context_hints_and_honest_boundaries() {
    let app = scratch_app("render-welcome", AgentBackend::Cursor);
    let (text, cursor) = screen(&app, 110, 25);
    for expected in [
        "abbey",
        "What would you like",
        "Ctrl-K",
        "Ctrl-S",
        "F1",
        "Apple Foundation Models",
        "not a tool host",
        "Ask anything",
        "Enter send",
        "permission:",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(cursor.is_some());
}

#[test]
fn tiny_and_narrow_terminals_never_place_cursor_outside_frame() {
    let mut app = scratch_app("render-tiny", AgentBackend::Cursor);
    for draft in ["", "界界界\n👩‍💻\nlong long long long long"] {
        app.composer.set(draft.to_string());
        for width in [0, 1, 2, 3, 4, 8, 20] {
            for height in [0, 1, 2, 3, 4, 5, 8, 12] {
                let (_, cursor) = screen(&app, width, height);
                if let Some((x, y)) = cursor {
                    assert!(x < width && y < height, "{width}x{height}: {x},{y}");
                }
            }
        }
    }
}

#[test]
fn long_draft_viewport_follows_cursor_instead_of_clipping_end() {
    let mut app = scratch_app("render-long", AgentBackend::Cursor);
    app.composer.set(format!("{}THE-END", "x".repeat(300)));
    let (text, cursor) = screen(&app, 24, 12);
    assert!(text.contains("THE-END"), "{text}");
    assert!(cursor.is_some());
    app.composer.cursor = 29;
    let (_, cursor) = screen(&app, 24, 12);
    assert_eq!(cursor.unwrap().0, 8); // border + 29 modulo 22 display cells
}

#[test]
fn running_queue_cancel_and_terminal_outcome_stay_visible() {
    let mut app = scratch_app("render-running", AgentBackend::Cursor);
    app.run = Some(RunHandle {
        events: mpsc::channel().1,
        done: mpsc::channel().1,
        cancel: crate::runtime::CancellationToken::new(),
        started: Instant::now(),
        thread: None,
        completion: None,
        local: false,
    });
    app.queued.push("follow up".into());
    app.status = "queued (1)".into();
    let (text, _) = screen(&app, 100, 20);
    for expected in [
        "Running",
        "Esc cancel",
        "queued (1)",
        "Queue message",
        "Add a follow-up",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    app.run = None;
    app.status = "done · exit 0".into();
    let (text, _) = screen(&app, 20, 5);
    assert!(text.contains("done · exit 0"));
}
