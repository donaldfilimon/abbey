//! Actual TUI completion mutations with private fixtures; no provider spawned.
use super::*;

#[test]
fn completion_navigation_accepts_the_highlighted_last_file_row() {
    let mut app = scratch_app("completion-last-row", AgentBackend::Cursor);
    app.files = super::super::completion::FileIndex::from_paths(vec![
        "apple.rs".into(),
        "apricot.rs".into(),
    ]);
    type_str(&mut app, "@a");
    let items = match &app.completion {
        Some(super::super::completion::Completion::File { items, .. }) => items.clone(),
        other => panic!("control: expected file choices, got {other:?}"),
    };
    assert_eq!(items.len(), 2);
    let last = items.last().unwrap().clone();
    assert!(screen(&app, 90, 20).contains(&last));
    for _ in 0..20 {
        press(&mut app, KeyCode::Down);
    }
    assert_eq!(
        app.completion_idx,
        items.len() - 1,
        "rendered selection and stored selection diverged"
    );
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.composer.text, format!("@{last} "));
    assert!(app.completion.is_none());
    assert!(app.run.is_none());
}

#[test]
fn stale_same_range_completion_cannot_overwrite_a_replaced_draft() {
    let mut app = scratch_app("completion-stale-basis", AgentBackend::Cursor);
    app.files = super::super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
    type_str(&mut app, "look @app");
    assert!(matches!(
        &app.completion,
        Some(super::super::completion::Completion::File { token_start: 5, .. })
    ));
    // Defensive cache injection: the byte range is still in bounds, UTF-8,
    // and begins with @. Range checks alone cannot detect the old draft.
    // Real producer/editor/palette mutations have separate path tests.
    app.composer.set("look @ban".into());
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.composer.text, "look @ban");
    assert_eq!(app.composer.cursor, "look @ban".len());
    assert!(app.completion.is_none());
    assert!(app.run.is_none());
}

#[test]
fn palette_slash_fill_invalidates_the_prior_file_completion() {
    let mut app = scratch_app("completion-palette-fill", AgentBackend::Cursor);
    app.files = super::super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
    type_str(&mut app, "look @app");
    assert!(app.completion.is_some());
    ctrl(&mut app, 'k');
    type_str(&mut app, "help");
    let items =
        super::super::overlays::fuzzy_filter(&super::super::overlays::palette_items(), "help");
    let idx = items
        .iter()
        .position(|item| {
            matches!(
                item.action,
                super::super::overlays::PaletteAction::Slash("help")
            )
        })
        .expect("control: current slash help action is registered");
    app.overlay = Overlay::Palette {
        query: "help".into(),
        idx,
    };
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.composer.text, "/help ");
    assert!(
        !matches!(
            app.completion,
            Some(super::super::completion::Completion::File { .. })
        ),
        "old file menu survived palette draft replacement"
    );
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.composer.text, "/help ");
    assert!(app.run.is_none(), "palette fill submitted a provider run");
}

#[test]
fn queued_submit_clears_the_draft_and_its_cached_completion() {
    let mut app = scratch_app("completion-queued-submit", AgentBackend::Cursor);
    let (_events_tx, events) = std::sync::mpsc::channel();
    let (_done_tx, done) = std::sync::mpsc::channel();
    app.run = Some(super::super::worker::RunHandle {
        events,
        done,
        cancel: crate::runtime::CancellationToken::new(),
        started: std::time::Instant::now(),
        thread: None,
        completion: None,
        local: false,
    });
    app.files = super::super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
    type_str(&mut app, "look @app");
    assert!(app.completion.is_some());
    // Exercise public App submission directly. Ordinary Enter first accepts
    // a menu, so this does not claim that one Enter queues an active menu.
    app.submit();
    assert_eq!(app.queued, vec!["look @app"]);
    assert!(app.composer.is_empty());
    assert!(
        app.completion.is_none(),
        "queued draft left its old byte-range cache"
    );
    press(&mut app, KeyCode::Tab);
    assert!(app.composer.is_empty());
    assert_eq!(app.composer.cursor, 0);
}
