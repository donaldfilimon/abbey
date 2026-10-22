//! Asking the terminal what it looks like.
//!
//! A TUI draws with its own palette, and the only way it can know whether
//! the terminal is light or dark is to ask. The terminal answers the
//! `OSC 11` query with its default background colour, as `rgb:RRRR/GGGG/BBBB`.
//!
//! # Boundaries
//!
//! * **Read-only.** The query asks; it changes nothing.
//! * **Bounded.** The query is written once, and the wait is bounded. A
//!   terminal that does not answer must not stall the app, so a missing
//!   answer is `None`, not a hang.
//! * **The answer is parsed, never executed.** The reply is a colour
//!   specification; it is turned into three numbers and nothing else.
//! * **A terminal that cannot answer keeps the dark palette.** The caller
//!   decides what to do with `None`, and the answer is always the dark one:
//!   a palette must never be chosen from a value the app could not read.
//! * **The query window is before the app accepts input.** The answer comes
//!   back on the terminal's *input* stream, so a byte the app reads during
//!   the query is a byte the user can no longer type. The query is asked at
//!   startup, before the first frame and before the event loop starts, so
//!   the app has not yet acted on any input.
//! * **A keystroke is never spent on learning a colour.** The bytes the
//!   query reads are the user's own, and they are given back: the query
//!   returns them with the colour, and the app replays them as key events
//!   before its first draw. A terminal that already holds bytes is not
//!   asked at all — a person who is typing is not made to wait for a
//!   colour.
//! * **A late answer is swallowed, not typed.** A terminal that answers
//!   after the window would have its answer read as keystrokes by the
//!   event loop, so the query waits a little longer for the slow case,
//!   and what is still queued afterwards is drained: the answers in it are
//!   dropped, because a reply is not typing.
//!
//! # Platform boundary
//!
//! The reply is read from the terminal's own input stream, and only a Unix
//! terminal can be read that way without blocking: a Windows console hands
//! the answer to the application as console input events rather than as a
//! byte stream, so the question is not asked there at all and the palette
//! stays the dark one. The reply's *grammar* is platform-independent, and
//! its tests run everywhere.

// On a platform that never asks the question, the parsing helpers are still
// exercised by this module's own tests; they are not dead code there.
#![cfg_attr(not(unix), allow(dead_code))]

use std::io::Write;
use std::time::{Duration, Instant};

use super::theme::TerminalBrightness;

/// How long to wait for the terminal to answer the colour query.
///
/// A terminal that supports `OSC 11` answers in well under a millisecond.
/// One that does not answers never. The bound is short enough that an
/// unsupported terminal is not a visible stall at startup, and long enough
/// that a loaded machine still gets its answer.
const QUERY_TIMEOUT: Duration = Duration::from_millis(120);

/// How long a terminal that has not answered yet is given to answer late.
///
/// A terminal that answers after the main window would have its answer read
/// as user input by the event loop, so the query waits a little longer for
/// the slow case and takes the answer. The grace is bounded: a terminal
/// that does not support the query still only costs a fixed, short wait.
const LATE_REPLY_GRACE: Duration = Duration::from_millis(200);

/// The `OSC 11` query, followed by a `ST` terminator.
///
/// `OSC` opens an operating-system command, `11` selects the background
/// colour, and `ST` closes it. The reply comes back as
/// `OSC 11;rgb:RRRR/GGGG/BBBB ST`.
const QUERY: &[u8] = b"\x1b]11;?\x1b\\";

/// The introducer of an `OSC 11` reply.
///
/// The reply is `ESC ] 11 ; <colour> ST`. The introducer is the reply's
/// own opening: everything before it in the input queue belongs to
/// whoever typed it.
const INTRODUCER: &[u8] = b"\x1b]11;";

/// The terminal's appearance, and the keystrokes the question took.
pub struct Appearance {
    /// The brightness the terminal reported.
    ///
    /// A terminal that did not answer is dark: a palette must never be
    /// chosen from a value the app could not read.
    pub brightness: TerminalBrightness,
    /// The user's keystrokes that were read out of the input queue.
    ///
    /// The question is asked on the terminal's *input* stream, so the
    /// bytes it reads are the user's own. They are given back to the app
    /// as key events: a keystroke is never spent on learning a colour.
    pub keystrokes: Vec<u8>,
}

/// Ask the terminal what it looks like, and give back what the ask took.
///
/// The answer is the terminal's background colour; the keystrokes are the
/// user's, because the question is asked on the terminal's input stream.
#[cfg(unix)]
pub fn appearance() -> Appearance {
    // What the terminal already holds is taken first. A terminal with
    // bytes waiting has a person in front of it, and the question is not
    // worth a keystroke: it is not asked, and the palette stays the one
    // the caller already has.
    let mut keystrokes = super::terminal::drain_input();
    let mut brightness = TerminalBrightness::Dark;
    if keystrokes.is_empty()
        && ask_the_terminal()
        && let Some(read) = read_reply_nonblocking(QUERY_TIMEOUT, LATE_REPLY_GRACE)
    {
        keystrokes.extend(read.leftover);
        if let Some(color) = parse_background(&read.reply) {
            brightness = TerminalBrightness::from_background(Some(color));
        }
    }
    // A terminal that answered late left its answer in the input queue.
    // The answer is not typing: it is taken and dropped, and the
    // keystrokes that came with it are kept.
    keystrokes.extend(super::terminal::drain_input());
    Appearance {
        brightness,
        keystrokes,
    }
}

/// Ask the terminal for its background colour.
///
/// The question goes out on the terminal's output; the answer comes back
/// on its input. A question that cannot be written is not asked: the
/// palette stays the one the caller already has.
#[cfg(unix)]
fn ask_the_terminal() -> bool {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(QUERY).is_ok() && out.flush().is_ok()
}

/// Ask the terminal what it looks like, and give back what the ask took.
///
/// The answer arrives on the terminal's *input* stream, and only a Unix
/// terminal can be read that way without risking a block: a Windows console
/// hands the `OSC 11` reply to the application as console input events
/// rather than as a byte stream, and a blocking read there waits for a
/// keystroke that may never come. The honest answer on that platform is
/// "unknown", which keeps the dark palette — a palette must never be
/// chosen from a value the app could not read.
#[cfg(not(unix))]
pub fn appearance() -> Appearance {
    Appearance {
        brightness: TerminalBrightness::Dark,
        keystrokes: Vec::new(),
    }
}

/// Read the terminal's reply without ever blocking past the bound.
///
/// A blocking read on the terminal's input would wait for a byte that a
/// terminal which does not support `OSC 11` will never send, so the bound
/// would never be reached and the app would hang before its first frame.
/// The descriptor is therefore put into non-blocking mode for the duration
/// of the read, and the terminal's own flags are restored afterwards — this
/// is a query, not a change to the terminal.
#[cfg(unix)]
fn read_reply_nonblocking(timeout: Duration, grace: Duration) -> Option<ReplyRead> {
    use std::os::fd::AsFd;
    let stdin = std::io::stdin();
    read_reply_from_fd(stdin.as_fd(), timeout, grace)
}

/// Read the reply from a descriptor, and leave the descriptor as found.
///
/// The flags are read before they are changed and restored afterwards, so
/// the terminal's input mode is exactly as it was found: the query asks,
/// and it changes nothing. A restore that fails is reported rather than
/// swallowed: the query is given up (the palette stays dark), and the user
/// is told why.
#[cfg(unix)]
fn read_reply_from_fd<F: std::os::fd::AsFd>(
    fd: F,
    timeout: Duration,
    grace: Duration,
) -> Option<ReplyRead> {
    super::terminal::with_nonblocking(&fd, || read_reply(&mut || read_byte(&fd), timeout, grace))
}

/// Read one byte from a descriptor.
///
/// The descriptor is in non-blocking mode, so "nothing yet" is an answer
/// rather than a wait: it comes back as `WouldBlock`, and the caller
/// decides whether to keep waiting.
#[cfg(unix)]
fn read_byte<F: std::os::fd::AsFd>(fd: &F) -> std::io::Result<u8> {
    use nix::errno::Errno;
    let mut byte = [0u8; 1];
    loop {
        match nix::unistd::read(fd, &mut byte) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the terminal closed its input",
                ));
            }
            Ok(_) => return Ok(byte[0]),
            // A signal interrupted the read — a window resize, most often.
            // The read was not answered, not refused, so it goes on.
            Err(Errno::EINTR) => continue,
            Err(Errno::EAGAIN) => return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)),
            Err(error) => return Err(std::io::Error::from_raw_os_error(error as i32)),
        }
    }
}

/// Read the reply, one byte at a time, within the bound.
///
/// A byte at a time is deliberate: a chunked read would take the bytes that
/// follow the reply in the same chunk, and those bytes may be the user's
/// keystrokes. The read stops at the reply's terminator, so a keystroke
/// typed after the answer is left in the queue for the event loop.
///
/// A terminal that has not answered when the window ends is given a short
/// grace, so that a slow answer is taken rather than read as user input.
/// The grace is given whenever the reply is *incomplete* — not only when
/// nothing arrived at all: a reply that started and was cut short is
/// exactly the case the grace exists for.
fn read_reply(
    read_byte: &mut dyn FnMut() -> std::io::Result<u8>,
    window: Duration,
    grace: Duration,
) -> ReplyRead {
    let mut buffer = Vec::new();
    let deadline = Instant::now() + window;
    let mut grace_deadline = None;
    loop {
        let now = Instant::now();
        let limit = grace_deadline.unwrap_or(deadline);
        if now >= limit {
            if grace_deadline.is_none() && !reply_is_complete(&buffer) {
                // The window is over and the reply is not here. The
                // terminal may be slow rather than silent, so it gets the
                // grace.
                grace_deadline = Some(now + grace);
                continue;
            }
            break;
        }
        match read_byte() {
            Ok(byte) => {
                buffer.push(byte);
                if reply_is_complete(&buffer) {
                    break;
                }
            }
            // The input has nothing ready yet. The bound is what decides
            // when to stop waiting, so the wait continues rather than
            // giving up on the first empty poll. An interrupted read is
            // the same: it was not answered, not refused.
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::Interrupted =>
            {
                std::thread::sleep((limit - now).min(Duration::from_millis(2)));
            }
            Err(_) => break,
        }
    }

    let (reply, leftover) = split_reply(&buffer);
    ReplyRead { reply, leftover }
}

/// What the reader found.
struct ReplyRead {
    /// The reply itself: from its introducer to its terminator, if it
    /// arrived at all.
    reply: Vec<u8>,
    /// The bytes that were not the reply: the user's keystrokes.
    ///
    /// The query reads the terminal's input queue, so the bytes it takes
    /// are the user's own. They are given back to the app, not dropped:
    /// a keystroke is never spent on learning a colour.
    leftover: Vec<u8>,
}

/// Split what was read into the reply and the user's keystrokes.
///
/// The reply starts at its introducer (`ESC ] 11;`). Everything before it
/// is the user's: those bytes were read out of the input queue and have
/// to be given back. Everything from the introducer onwards is the
/// terminal's — a reply that was cut short is still the terminal's, and
/// typing it would put the terminal's text in the composer.
fn split_reply(buffer: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let Some(start) = find_introducer_start(buffer) else {
        // No reply at all: everything that was read is the user's.
        return (Vec::new(), buffer.to_vec());
    };
    let payload = &buffer[start..];
    let end = payload
        .windows(2)
        .position(|pair| pair == b"\x1b\\")
        .map(|at| at + 2)
        .or_else(|| {
            payload
                .iter()
                .position(|byte| *byte == 0x07)
                .map(|at| at + 1)
        });
    match end {
        Some(end) => {
            let mut leftover = buffer[..start].to_vec();
            leftover.extend_from_slice(&payload[end..]);
            (payload[..end].to_vec(), leftover)
        }
        // A reply that was cut short is still the terminal's.
        None => (payload.to_vec(), buffer[..start].to_vec()),
    }
}

/// Whether a reply has arrived in full.
///
/// An `OSC` reply ends with `ST` (`ESC \`) or `BEL`. The terminator is
/// what tells the reader to stop, so that a keystroke typed after the reply
/// is not swallowed with it.
///
/// The terminator only counts when it belongs to *this* reply: the buffer
/// also carries whatever else the terminal's input queue held, and a `BEL`
/// or an `ESC \` typed by the user would otherwise end the read before the
/// answer arrived. So the reply's own introducer (`ESC ] 11 ;`) has to be
/// there first, and only what follows it can end the reply.
fn reply_is_complete(buffer: &[u8]) -> bool {
    let Some(start) = find_introducer(buffer) else {
        return false;
    };
    let payload = &buffer[start..];
    payload.windows(2).any(|pair| pair == b"\x1b\\") || payload.contains(&0x07)
}

/// Where the `OSC 11` reply starts in the buffer.
///
/// `None` when the reply has not started yet.
fn find_introducer(buffer: &[u8]) -> Option<usize> {
    find_introducer_start(buffer).map(|at| at + INTRODUCER.len())
}

/// Where the `OSC 11` reply starts in the buffer.
///
/// The introducer is the whole of the reply's own opening: everything
/// before it belongs to whoever typed it.
fn find_introducer_start(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(INTRODUCER.len())
        .position(|window| window == INTRODUCER)
}

/// Ask the terminal through the given streams.
///
/// The streams are parameters so a test can drive the query against a pipe
/// it controls: a real query writes to the terminal and reads the reply,
/// and only the *shape* of the exchange matters to the caller.
///
/// The bound here is the *test* bound: the reader is a pipe the test owns,
/// so it ends rather than blocking. The real terminal read is
/// [`read_reply_nonblocking`], which is the one that has to cope with a
/// terminal that simply never answers.
#[cfg(test)]
pub fn query_background_from(
    input: &mut impl std::io::Read,
    output: &mut impl Write,
    timeout: Duration,
) -> Option<(u8, u8, u8)> {
    output.write_all(QUERY).ok()?;
    output.flush().ok()?;

    let read = read_reply(&mut || read_byte_from(input), timeout, Duration::ZERO);
    parse_background(&read.reply)
}

/// Read one byte from a stream.
///
/// The stream is at its end rather than slow: the reader a test owns
/// returns EOF instead of waiting.
#[cfg(test)]
fn read_byte_from(input: &mut impl std::io::Read) -> std::io::Result<u8> {
    let mut byte = [0u8; 1];
    loop {
        match input.read(&mut byte) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the input ended",
                ));
            }
            Ok(_) => return Ok(byte[0]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Parse a background colour out of an `OSC 11` reply.
///
/// Accepts both the `rgb:` form terminals use (`rgb:1c1c/1c1c/1c1c`) and a
/// plain triple (`1c1c1c`). Returns `None` for anything else, including a
/// reply that has not been terminated: a reply still on its way is not a
/// colour yet, and reading it as one would take a truncated answer for a
/// whole one.
pub fn parse_background(reply: &[u8]) -> Option<(u8, u8, u8)> {
    // The reply is `OSC 11;rgb:RRRR/GGGG/BBBB ST`. The search is anchored
    // on the *introducer* — `ESC ] 11;` — and not on the bare substring
    // `11;`, because the same buffer may hold the user's own keystrokes or
    // the answer to a different query, and a bare `11;` anywhere in it
    // would latch the parse onto the wrong payload for good.
    let start = find_introducer(reply)?;
    let payload = &reply[start..];
    // The reply ends with `ST` (`ESC \`) or `BEL`. Both are legal, and
    // the terminator is what says the answer is whole: what comes before
    // it is the colour, and the terminator itself is not part of it.
    let terminator = payload
        .windows(2)
        .position(|window| window == b"\x1b\\")
        .or_else(|| payload.iter().position(|byte| *byte == 0x07))?;
    let text = std::str::from_utf8(&payload[..terminator])
        .ok()?
        .trim_ascii();
    let value = text.strip_prefix("rgb:").unwrap_or(text);
    // The `rgb:` form separates the channels with slashes; the plain form
    // is one run of digits. Both are seen in the wild, and a reply in
    // either shape is a colour the terminal meant.
    if !value.is_ascii() {
        return None;
    }
    if value.contains('/') {
        let mut channels = value.split('/');
        let r = parse_channel(channels.next()?)?;
        let g = parse_channel(channels.next()?)?;
        let b = parse_channel(channels.next()?)?;
        Some((r, g, b))
    } else {
        // A plain triple is `RRGGBB` or `RGB`, with each pair or digit
        // giving one channel.
        match value.len() {
            6 => {
                let r = parse_channel(&value[0..2])?;
                let g = parse_channel(&value[2..4])?;
                let b = parse_channel(&value[4..6])?;
                Some((r, g, b))
            }
            3 => {
                let r = parse_channel(&value[0..1])?;
                let g = parse_channel(&value[1..2])?;
                let b = parse_channel(&value[2..3])?;
                Some((r, g, b))
            }
            _ => None,
        }
    }
}

/// Parse one `rgb:` channel.
///
/// A terminal reports each channel as one to four hex digits, scaled to the
/// channel's full range: `1c1c` is 28/255 of the way up, and `c1c` is
/// 12/15 of the way up. Both are accepted, and the value is scaled into a
/// byte.
fn parse_channel(raw: &str) -> Option<u8> {
    if raw.is_empty() || raw.len() > 4 {
        return None;
    }
    if !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(raw, 16).ok()?;
    let max = (1u32 << (4 * raw.len())) - 1;
    if max == 0 {
        return None;
    }
    // Scale the reported value into the channel's full range. The division
    // rounds rather than floors: `8000` is exactly the middle of the
    // four-digit range, and flooring it would report the channel as 127 —
    // under the middle — instead of the 128 it is.
    Some((((value * 255) + (max / 2)) / max) as u8)
}

#[cfg(test)]
mod tests;
