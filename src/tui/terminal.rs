//! Terminal modes are owned by a guard, including partial entry and unwind.
//! The bytes the terminal sends back are owned here too: the keystrokes a
//! startup question took out of the input queue are decoded and given
//! back, a terminal's answer is swallowed rather than typed, and a signal
//! ends the session through the app's own path.

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, KeyCode,
    KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Raw,
    Screen,
    Mouse,
    Paste,
    Cursor,
}

pub(super) trait Control {
    fn set(&mut self, mode: Mode, enabled: bool) -> Result<()>;
}

pub(super) struct NativeControl;
impl Control for NativeControl {
    fn set(&mut self, mode: Mode, enabled: bool) -> Result<()> {
        let mut out = std::io::stdout();
        match (mode, enabled) {
            (Mode::Raw, true) => enable_raw_mode()?,
            (Mode::Raw, false) => disable_raw_mode()?,
            (Mode::Screen, true) => execute!(out, EnterAlternateScreen)?,
            (Mode::Screen, false) => execute!(out, LeaveAlternateScreen)?,
            (Mode::Mouse, true) => execute!(out, EnableMouseCapture)?,
            (Mode::Mouse, false) => execute!(out, DisableMouseCapture)?,
            (Mode::Paste, true) => execute!(out, EnableBracketedPaste)?,
            (Mode::Paste, false) => execute!(out, DisableBracketedPaste)?,
            (Mode::Cursor, _) => execute!(out, Show)?,
        }
        Ok(())
    }
}

pub(super) struct TerminalGuard<C: Control = NativeControl> {
    control: C,
    armed: bool,
}

impl<C: Control> TerminalGuard<C> {
    pub(super) fn new(control: C) -> Self {
        Self {
            control,
            armed: false,
        }
    }

    pub(super) fn enter(&mut self) -> Result<()> {
        // Arm before the first side effect: even a failed mode command may
        // have partially changed the terminal.
        self.armed = true;
        for mode in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            self.control.set(mode, true)?;
        }
        Ok(())
    }

    pub(super) fn restore(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let mut first = None;
        for mode in [
            Mode::Raw,
            Mode::Mouse,
            Mode::Paste,
            Mode::Screen,
            Mode::Cursor,
        ] {
            if let Err(e) = self.control.set(mode, false)
                && first.is_none()
            {
                first = Some(e);
            }
        }
        // Retry cleanup in Drop if any step failed, without masking the
        // original error from initialization, drawing, or suspension.
        self.armed = first.is_some();
        first.map_or(Ok(()), Err)
    }
}

impl TerminalGuard<NativeControl> {
    pub(super) fn native() -> Self {
        Self::new(NativeControl)
    }
}

impl<C: Control> Drop for TerminalGuard<C> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Whether a termination signal has arrived.
///
/// This flag is the whole of the signal handling. A handler runs on the
/// interrupted thread, where nothing else may be called, so the handler
/// does one thing — set this — and the event loop, which is ordinary code
/// doing ordinary work, acts on it.
static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The handler a termination signal runs.
///
/// It takes no lock, allocates nothing, and touches no terminal: it only
/// records that the session should end.
#[cfg(unix)]
extern "C" fn note_termination(_signal: libc::c_int) {
    TERMINATION_REQUESTED.store(true, Ordering::SeqCst);
}

/// Install the handlers that let the session end through its own path.
///
/// Without them an external `kill -INT` — or `-TERM`, or a hangup — takes
/// the default disposition: the process dies where it stands, the guard's
/// `Drop` never runs, and the user is left with echo off, raw mode, and
/// the alternate screen still up (the shell needs `reset`). With them the
/// signal is a flag the event loop reads, and the app leaves the way it
/// leaves for any other reason: the terminal is restored on the way out.
///
/// Ctrl-C is not one of these: raw mode turns it into a key event, and
/// the keymap owns it.
#[cfg(unix)]
#[allow(unsafe_code)]
pub(super) fn install_termination_handlers() -> Result<()> {
    use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

    let action = SigAction::new(
        SigHandler::Handler(note_termination),
        // A restartable syscall keeps a signal from turning into an
        // error: the loop waits at most a hundred milliseconds anyway.
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
        // SAFETY: `action` names an `extern "C"` handler that only stores
        // into an atomic, and it stays valid for the whole call.
        unsafe { sigaction(signal, &action) }
            .map_err(|error| anyhow::anyhow!("signal {signal:?} could not be handled: {error}"))?;
    }
    Ok(())
}

/// Install the handlers that let the session end through its own path.
///
/// Windows has no POSIX signal here: the console delivers Ctrl-C as a key
/// event, and the session ends the ordinary way.
#[cfg(not(unix))]
pub(super) fn install_termination_handlers() -> Result<()> {
    Ok(())
}

/// Whether a termination signal has arrived.
pub(super) fn termination_requested() -> bool {
    TERMINATION_REQUESTED.load(Ordering::SeqCst)
}

/// Decode the bytes a startup question took back into key events.
///
/// The question is asked on the terminal's *input* stream, so the bytes it
/// takes are the user's own. They are given back here as the key events
/// they would have been, and the app receives them through the same path
/// the event loop uses.
///
/// The decoder is deliberately small. It understands printable text, Enter,
/// Backspace, Tab, Escape, `Ctrl+<letter>` and the arrow keys. Every other
/// escape sequence is dropped whole rather than decoded as its own
/// characters: a sequence the decoder does not understand is not something
/// the user typed, and putting its characters in the composer would be
/// typing text nobody asked for.
pub(super) fn decode_keystrokes(bytes: &[u8]) -> Vec<KeyEvent> {
    let mut events = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            0x1b => {
                let (event, next) = decode_escape(bytes, at);
                events.extend(event);
                at = next;
            }
            b'\r' | b'\n' => {
                events.push(key(KeyCode::Enter));
                at += 1;
            }
            b'\t' => {
                events.push(key(KeyCode::Tab));
                at += 1;
            }
            0x08 | 0x7f => {
                events.push(key(KeyCode::Backspace));
                at += 1;
            }
            // Ctrl+<letter>: the terminal sends the control code itself.
            0x01..=0x1a => {
                let letter = (bytes[at] - 0x01 + b'a') as char;
                events.push(key_with(KeyCode::Char(letter), KeyModifiers::CONTROL));
                at += 1;
            }
            // NUL is not a key.
            0x00 => at += 1,
            _ => match next_char(bytes, at) {
                Some((character, width)) => {
                    events.push(key_with(KeyCode::Char(character), shift_for(character)));
                    at += width;
                }
                // A byte that does not start a character is not something
                // the user can have typed: it is skipped, not guessed at.
                None => at += 1,
            },
        }
    }
    events
}

/// Decode one escape sequence.
///
/// The sequence is consumed whole. A sequence the decoder does not
/// understand is dropped: its characters are not the user's, and typing
/// them would put text in the composer that nobody typed.
fn decode_escape(bytes: &[u8], at: usize) -> (Option<KeyEvent>, usize) {
    let rest = &bytes[at + 1..];
    match rest.first() {
        // A lone Escape.
        None => (Some(key(KeyCode::Esc)), at + 1),
        // A control sequence: `ESC [ <parameters> <final>`.
        Some(b'[') => decode_csi(bytes, at),
        // An operating-system command: the terminal answering a question.
        Some(b']') => (None, osc_end(bytes, at).unwrap_or(bytes.len())),
        // A single shift: `ESC O <final>`.
        Some(b'O') => (None, (at + 3).min(bytes.len())),
        // Two escapes in a row are one Escape, as crossterm reads them.
        Some(b'\x1b') => (Some(key(KeyCode::Esc)), at + 2),
        // The escape prefix is how the terminal says "alt".
        Some(&byte) if byte < 0x80 => (
            Some(key_with(KeyCode::Char(byte as char), KeyModifiers::ALT)),
            at + 2,
        ),
        Some(_) => match next_char(bytes, at + 1) {
            Some((character, width)) => (
                Some(key_with(
                    KeyCode::Char(character),
                    KeyModifiers::ALT | shift_for(character),
                )),
                at + 1 + width,
            ),
            None => (None, at + 2),
        },
    }
}

/// Decode a control sequence.
///
/// The final byte of a control sequence is in `0x40..=0x7e`. The ones the
/// app uses are the arrows and the home/end keys; every other one is
/// dropped whole.
fn decode_csi(bytes: &[u8], at: usize) -> (Option<KeyEvent>, usize) {
    let mut index = at + 2;
    while index < bytes.len() && !(0x40..=0x7e).contains(&bytes[index]) {
        index += 1;
    }
    if index >= bytes.len() {
        // The sequence is not all here yet, so the rest of it is not ours.
        return (None, bytes.len());
    }
    let code = match bytes[index] {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        _ => return (None, index + 1),
    };
    (Some(key(code)), index + 1)
}

/// The end of an operating-system command, if it is complete.
///
/// An `OSC` sequence is `ESC ] <body> ST`, where `ST` is `ESC \` or `BEL`.
/// The body is the terminal answering, not the user typing.
fn osc_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut index = at + 2;
    while index < bytes.len() {
        match bytes[index] {
            // `BEL` ends the command.
            0x07 => return Some(index + 1),
            // `ESC \` ends the command.
            0x1b if bytes.get(index + 1) == Some(&b'\\') => return Some(index + 2),
            _ => index += 1,
        }
    }
    None
}

/// The next printable character, and how many bytes it took.
///
/// `None` means the byte does not start a character: it is skipped. A
/// character that is still arriving is not a character yet, and half of
/// one is not something the user typed.
fn next_char(bytes: &[u8], at: usize) -> Option<(char, usize)> {
    let rest = bytes.get(at..)?;
    let first = *rest.first()?;
    if first < 0x80 {
        return Some((first as char, 1));
    }
    // A multi-byte character: the lead byte says how many bytes follow.
    let width = match first {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return None,
    };
    if rest.len() < width {
        return None;
    }
    match std::str::from_utf8(&rest[..width]) {
        Ok(text) => text.chars().next().map(|character| (character, width)),
        Err(_) => None,
    }
}

/// The modifiers a typed character carries.
///
/// The terminal reports an upper-case letter with Shift, and crossterm
/// reports it the same way, so the replay matches what the event loop
/// would have delivered.
fn shift_for(character: char) -> KeyModifiers {
    if character.is_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    }
}

/// A key event of the kind the terminal reports.
fn key(code: KeyCode) -> KeyEvent {
    key_with(code, KeyModifiers::NONE)
}

/// A key event with the modifiers the terminal reported.
fn key_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// Split pending input into the terminal's answers and the user's keys.
///
/// An operating-system command is the terminal answering a question. It is
/// not typing, and it is swallowed: a reply that reached the composer
/// would put the terminal's own text in front of the user. Everything else
/// is the user's, and is kept for replay.
#[cfg_attr(not(unix), allow(dead_code))]
pub(super) fn split_terminal_replies(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut replies = Vec::new();
    let mut keystrokes = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == 0x1b && bytes.get(at + 1) == Some(&b']') {
            // A command that is not complete yet is swallowed whole as
            // well: it is still the terminal's answer, and half of it
            // typed into the composer is worse than none of it.
            let end = osc_end(bytes, at).unwrap_or(bytes.len());
            replies.extend_from_slice(&bytes[at..end]);
            at = end;
            continue;
        }
        keystrokes.push(bytes[at]);
        at += 1;
    }
    (replies, keystrokes)
}

/// Take what the terminal's input queue holds right now.
///
/// The read never waits: the queue is read as it stands. The terminal's
/// answers are swallowed and the user's keystrokes are kept, for the app
/// to be given back.
#[cfg(unix)]
pub(super) fn drain_input() -> Vec<u8> {
    let stdin = std::io::stdin();
    let Some(bytes) = with_nonblocking(&stdin, || read_available(&stdin)) else {
        return Vec::new();
    };
    let (_replies, keystrokes) = split_terminal_replies(&bytes);
    keystrokes
}

/// Run a read with the descriptor in non-blocking mode.
///
/// The descriptor is found as it was: the flags are read before they are
/// changed and put back afterwards, so the question asks and changes
/// nothing. A restore that fails is reported rather than swallowed — the
/// question is given up, and the user is told why.
#[cfg(unix)]
pub(super) fn with_nonblocking<F: std::os::fd::AsFd, T>(
    fd: &F,
    read: impl FnOnce() -> T,
) -> Option<T> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};

    let original = fcntl(fd, FcntlArg::F_GETFL).ok()?;
    let nonblocking = OFlag::from_bits_truncate(original) | OFlag::O_NONBLOCK;
    if fcntl(fd, FcntlArg::F_SETFL(nonblocking)).is_err() {
        return None;
    }
    let value = read();
    if let Err(error) = fcntl(fd, FcntlArg::F_SETFL(OFlag::from_bits_truncate(original))) {
        eprintln!("abbey: the terminal's input mode could not be restored ({error})");
        return None;
    }
    Some(value)
}

/// Read what a descriptor has ready, without waiting for more.
#[cfg(unix)]
fn read_available<F: std::os::fd::AsFd>(fd: &F) -> Vec<u8> {
    use nix::errno::Errno;

    /// The most the drain takes at once.
    const MAX_DRAIN: usize = 64 * 1024;

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while buffer.len() < MAX_DRAIN {
        match nix::unistd::read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            // A signal interrupted the read: it was not answered, not
            // refused, so it goes on.
            Err(Errno::EINTR) => continue,
            // Nothing more is ready.
            Err(Errno::EAGAIN) => break,
            Err(_) => break,
        }
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Fake {
        calls: Arc<Mutex<Vec<(Mode, bool)>>>,
        fail: Option<(Mode, bool)>,
    }
    impl Control for Fake {
        fn set(&mut self, mode: Mode, enabled: bool) -> Result<()> {
            self.calls.lock().unwrap().push((mode, enabled));
            if self.fail == Some((mode, enabled)) {
                anyhow::bail!("injected terminal error");
            }
            Ok(())
        }
    }

    #[test]
    fn partial_entry_restores_every_mode() {
        for failed in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            {
                let mut guard = TerminalGuard::new(Fake {
                    calls: calls.clone(),
                    fail: Some((failed, true)),
                });
                assert!(guard.enter().is_err());
            }
            let calls = calls.lock().unwrap();
            for mode in [
                Mode::Raw,
                Mode::Mouse,
                Mode::Paste,
                Mode::Screen,
                Mode::Cursor,
            ] {
                assert!(calls.contains(&(mode, false)));
            }
        }
    }

    #[test]
    fn cleanup_continues_after_failure_and_drop_retries() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        {
            let mut guard = TerminalGuard::new(Fake {
                calls: calls.clone(),
                fail: Some((Mode::Raw, false)),
            });
            guard.enter().unwrap();
            assert!(guard.restore().is_err());
        }
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| **v == (Mode::Cursor, false))
                .count(),
            2
        );
    }

    #[test]
    fn unwind_restores_and_successful_suspend_can_reenter() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let _ = std::panic::catch_unwind({
            let calls = calls.clone();
            move || {
                let mut guard = TerminalGuard::new(Fake { calls, fail: None });
                guard.enter().unwrap();
                guard.restore().unwrap();
                guard.enter().unwrap();
                panic!("fixture panic");
            }
        });
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| **v == (Mode::Raw, false))
                .count(),
            2
        );
    }

    /// The bytes a startup question took are given back as key events.
    ///
    /// The question is read on the terminal's input stream, so the bytes
    /// it takes are the user's own. They have to come back as the key
    /// events they would have been.
    #[test]
    fn typed_bytes_are_decoded_back_into_the_keys_they_were() {
        let events = decode_keystrokes(b"hi");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].code, crossterm::event::KeyCode::Char('h'));
        assert_eq!(events[1].code, crossterm::event::KeyCode::Char('i'));
        assert!(events.iter().all(|event| event.modifiers.is_empty()));
    }

    /// The keys that are not text are the keys they were.
    #[test]
    fn the_control_keys_are_decoded_back_into_the_keys_they_were() {
        let events = decode_keystrokes(b"\r\t\x7f\x03");
        assert_eq!(events[0].code, crossterm::event::KeyCode::Enter);
        assert_eq!(events[1].code, crossterm::event::KeyCode::Tab);
        assert_eq!(events[2].code, crossterm::event::KeyCode::Backspace);
        assert_eq!(events[3].code, crossterm::event::KeyCode::Char('c'));
        assert!(
            events[3]
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL),
            "Ctrl-C must come back as a control key"
        );
    }

    /// A sequence the decoder does not know is dropped whole.
    ///
    /// Its characters are not the user's: a bracketed paste marker typed
    /// into the composer would be text nobody typed.
    #[test]
    fn an_unknown_escape_sequence_is_dropped_whole() {
        let events = decode_keystrokes(b"\x1b[200~pasted");
        let text: String = events
            .iter()
            .filter_map(|event| match event.code {
                crossterm::event::KeyCode::Char(character) => Some(character),
                _ => None,
            })
            .collect();
        assert_eq!(text, "pasted");
        assert_eq!(events.len(), 6);
    }

    /// A terminal's answer is not typing.
    ///
    /// The reply to the background-colour question is an operating-system
    /// command: it is swallowed, so it can never reach the composer as
    /// text.
    #[test]
    fn a_terminal_reply_is_swallowed_rather_than_kept_as_typing() {
        let (replies, keystrokes) = split_terminal_replies(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
        assert_eq!(replies, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
        assert!(
            keystrokes.is_empty(),
            "a reply must not be given back as typing: {keystrokes:?}"
        );
    }

    /// A signal is a flag the loop reads, not a death.
    ///
    /// The handler does nothing but set the flag, so the app can leave
    /// through its own path and restore the terminal.
    #[cfg(unix)]
    #[test]
    fn a_termination_signal_is_recorded_for_the_loop_to_find() {
        assert!(!termination_requested(), "the flag starts clear");
        super::note_termination(libc::SIGINT);
        assert!(termination_requested(), "the signal must be recorded");
        // The flag is global: it is put back so the rest of the suite
        // sees the session it expects.
        super::TERMINATION_REQUESTED.store(false, Ordering::SeqCst);
    }
}
