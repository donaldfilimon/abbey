//! The terminal's input, and the keystrokes the startup question took.
//!
//! The question about the terminal's colour is asked on the terminal's
//! *input* stream, so it takes the user's keystrokes with it. These tests
//! pin what happens to them: they are given back to the app, and the
//! terminal's own answer is swallowed rather than typed.

use super::super::terminal::{decode_keystrokes, split_terminal_replies};
use super::*;

/// Give the bytes back to the app, the way the event loop does.
fn replay(app: &mut App, bytes: &[u8]) {
    for key in decode_keystrokes(bytes) {
        app.handle_key(key);
    }
}

/// The keystrokes the question read are the user's, and they are given
/// back.
///
/// A keystroke spent on learning a colour is a keystroke the user never
/// gets to type.
#[test]
fn the_keystrokes_the_question_took_are_given_back_to_the_composer() {
    let mut app = scratch_app("terminal-input-replay", crate::agent::AgentBackend::Ollama);
    replay(&mut app, b"hello");
    assert_eq!(app.composer.text, "hello");
}

/// The keystrokes come back as the keys they were.
///
/// A backspace that came back as a letter would be worse than a lost
/// keystroke.
#[test]
fn the_keystrokes_come_back_as_the_keys_they_were() {
    let mut app = scratch_app("terminal-input-keys", crate::agent::AgentBackend::Ollama);
    replay(&mut app, b"abc\x7f");
    assert_eq!(app.composer.text, "ab");
}

/// A late terminal reply is swallowed, and the composer sees nothing.
///
/// The reply to the background-colour question is an operating-system
/// command. crossterm has no branch for one, so it would be read as the
/// characters `]11;rgb:ffff/ffff/ffff\`. Those characters are not the
/// user's, and they must never reach the composer.
#[test]
fn a_late_terminal_reply_reaches_the_composer_as_nothing_at_all() {
    let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
    let (answers, keystrokes) = split_terminal_replies(reply);
    assert_eq!(answers, reply.to_vec(), "the reply is swallowed whole");
    assert!(
        keystrokes.is_empty(),
        "a reply must not be given back as typing: {keystrokes:?}"
    );

    let mut app = scratch_app(
        "terminal-input-late-reply",
        crate::agent::AgentBackend::Ollama,
    );
    replay(&mut app, &keystrokes);
    assert_eq!(
        app.composer.text, "",
        "a terminal's own answer must never be typed into the composer"
    );
}

/// A reply that ends in `BEL` is swallowed as well.
///
/// `BEL` is the other legal terminator, and a terminal that uses it must
/// not have its answer typed either.
#[test]
fn a_bel_terminated_reply_reaches_the_composer_as_nothing_at_all() {
    let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x07";
    let (answers, keystrokes) = split_terminal_replies(reply);
    assert_eq!(answers, reply.to_vec());
    assert!(keystrokes.is_empty());

    let mut app = scratch_app(
        "terminal-input-bel-reply",
        crate::agent::AgentBackend::Ollama,
    );
    replay(&mut app, &keystrokes);
    assert_eq!(app.composer.text, "");
}

/// The typing around a reply is the user's, and it is kept.
#[test]
fn the_typing_around_a_reply_is_kept() {
    let mut bytes = b"hi".to_vec();
    bytes.extend_from_slice(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
    bytes.extend_from_slice(b"!");

    let (answers, keystrokes) = split_terminal_replies(&bytes);
    assert_eq!(answers, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\".to_vec());
    assert_eq!(keystrokes, b"hi!");

    let mut app = scratch_app(
        "terminal-input-around-reply",
        crate::agent::AgentBackend::Ollama,
    );
    replay(&mut app, &keystrokes);
    assert_eq!(app.composer.text, "hi!");
}
