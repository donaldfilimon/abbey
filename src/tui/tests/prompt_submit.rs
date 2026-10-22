//! Enter sends the draft the user typed.
//!
//! The composer's suggestion menu used to open on ordinary English — the
//! ranker matched *any* word of a command's help text — and Enter then
//! rewrote the draft into that slash command. "next thing" became
//! "/clear next thing" (the /reset alias points at /clear, whose help
//! reads "… fresh session next run") and was never sent. These tests pin
//! the prompt the run actually receives, and the rule that decides when
//! Enter may accept a suggestion at all.

use super::*;

fn rows(app: &App) -> Vec<super::super::predict::Prediction> {
    match &app.completion {
        Some(super::super::completion::Completion::Slash(rows)) => rows.clone(),
        other => panic!("expected a suggestion menu, got {other:?}"),
    }
}

#[test]
fn ordinary_prose_reaches_the_run_exactly_as_typed() {
    let mut app = scratch_app("prompt-submit-prose", AgentBackend::Cursor);
    // A missing executor: the run fails fast instead of contacting a
    // provider. The prompt is recorded before the run starts.
    app.cfg.agent_path = app.state.state_dir.join("missing-fixture-agent");
    type_str(&mut app, "next thing");
    assert!(
        app.completion.is_none(),
        "ordinary prose opened a suggestion menu: {:?}",
        app.completion
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.last_submitted.as_deref(),
        Some("next thing"),
        "the run received a rewritten prompt"
    );
    assert!(
        app.transcript.cells.iter().any(|cell| matches!(
            cell,
            super::super::transcript::Cell::User(text) if text == "next thing"
        )),
        "the transcript does not carry the typed prompt: {:?}",
        app.transcript.cells
    );
    let user_cells: Vec<&str> = app
        .transcript
        .cells
        .iter()
        .filter_map(|cell| match cell {
            super::super::transcript::Cell::User(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        user_cells,
        vec!["next thing"],
        "the typed prompt was rewritten into a slash command"
    );
    app.shutdown().unwrap();
}

#[test]
fn ordinary_prose_is_queued_unchanged_while_a_run_is_active() {
    let mut app = scratch_app("prompt-submit-queue", AgentBackend::Cursor);
    let (_events, _done) = fixture_run(&mut app);
    type_str(&mut app, "next thing");
    assert!(
        app.completion.is_none(),
        "ordinary prose opened a suggestion menu: {:?}",
        app.completion
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.queued,
        vec!["next thing".to_string()],
        "the draft was mangled instead of queued"
    );
    assert!(
        app.composer.is_empty(),
        "the queued draft stayed in the composer"
    );
}

#[test]
fn an_unchosen_menu_sends_the_prose_the_user_typed() {
    // The intent tier opens a menu on "review the auth diff". A menu the
    // user did not choose is a suggestion, not an instruction: Enter sends
    // the sentence, and Tab takes the suggestion.
    let mut app = scratch_app("prompt-submit-intent", AgentBackend::Cursor);
    app.cfg.agent_path = app.state.state_dir.join("missing-fixture-agent");
    type_str(&mut app, "review the auth diff");
    assert!(
        app.completion.is_some(),
        "the intent tier opens a menu on this draft"
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.last_submitted.as_deref(),
        Some("review the auth diff"),
        "an unchosen menu rewrote the prompt"
    );
    assert!(
        app.composer.is_empty(),
        "the sent draft stayed in the composer"
    );
    app.shutdown().unwrap();
}

#[test]
fn an_unchosen_menu_is_queued_unchanged_while_a_run_is_active() {
    // The same menu while a run is active: the draft is queued as typed.
    // It used to be rewritten into a slash command and not queued at all.
    let mut app = scratch_app("prompt-submit-intent-queue", AgentBackend::Cursor);
    let (_events, _done) = fixture_run(&mut app);
    type_str(&mut app, "review the auth diff");
    assert!(
        app.completion.is_some(),
        "the intent tier opens a menu on this draft"
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.queued,
        vec!["review the auth diff".to_string()],
        "an unchosen menu rewrote the queued draft"
    );
}

#[test]
fn a_menu_the_user_navigated_is_accepted_by_enter() {
    let mut app = scratch_app("prompt-submit-navigated", AgentBackend::Cursor);
    type_str(&mut app, "commit the review notes");
    let offered = rows(&app);
    assert!(
        offered.len() > 1,
        "the fixture needs a menu with a second row: {offered:?}"
    );
    // The user moves the selection: the menu is theirs now.
    press(&mut app, KeyCode::Down);
    let chosen = rows(&app)[app.completion_idx].name;
    press(&mut app, KeyCode::Enter);
    assert!(
        app.composer.text.starts_with(&format!("/{chosen}")),
        "the chosen suggestion was not accepted, said {:?}",
        app.composer.text
    );
    assert!(
        app.last_submitted.is_none(),
        "the accepted suggestion submitted a turn"
    );
    assert!(app.run.is_none(), "the accepted suggestion started a run");
}

#[test]
fn a_command_name_the_user_is_typing_is_accepted_by_enter() {
    let mut app = scratch_app("prompt-submit-slash", AgentBackend::Cursor);
    type_str(&mut app, "/re");
    let top = rows(&app)[0].name;
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.composer.text, format!("/{top} "));
    assert!(
        app.last_submitted.is_none(),
        "the accepted suggestion submitted a turn"
    );
    assert!(app.run.is_none(), "the accepted suggestion started a run");
}

#[test]
fn tab_still_accepts_a_menu_the_ranker_opened() {
    // Tab is the explicit "take the suggestion" key. It keeps accepting a
    // menu the ranker opened, because Tab is never a send.
    let mut app = scratch_app("prompt-submit-tab", AgentBackend::Cursor);
    type_str(&mut app, "review the auth diff");
    let top = rows(&app)[0].name;
    press(&mut app, KeyCode::Tab);
    assert!(
        app.composer.text.starts_with(&format!("/{top}")),
        "Tab did not accept the suggestion, said {:?}",
        app.composer.text
    );
    assert!(app.run.is_none(), "Tab submitted the prompt");
}
