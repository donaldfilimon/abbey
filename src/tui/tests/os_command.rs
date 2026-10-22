//! The `!cmd` line: the arguments it runs, and the dialog that shows them.

use super::*;

#[test]
fn a_quoted_os_command_is_one_argument() {
    let mut app = scratch_app("os-quote", AgentBackend::Cursor);
    type_str(&mut app, "!echo \"hello world\"");
    press(&mut app, KeyCode::Enter);
    match &app.overlay {
        Overlay::Confirm { command } => assert_eq!(
            command,
            &vec!["echo".to_string(), "hello world".to_string()],
            "a quoted run is one argument"
        ),
        other => panic!("the confirm dialog must be open, saw {other:?}"),
    }
}

#[test]
fn the_confirm_dialog_shows_the_command_it_will_run() {
    // The dialog has to show the command the gate will execute, quoted the
    // same way, so the user reads what is about to happen.
    let mut app = scratch_app("os-quote-dialog", AgentBackend::Cursor);
    type_str(&mut app, "!echo \"hello world\"");
    press(&mut app, KeyCode::Enter);
    let rendered = screen(&app, 90, 24);
    assert!(
        rendered.contains("echo \"hello world\""),
        "the dialog must show the quoted command: {rendered}"
    );
}
