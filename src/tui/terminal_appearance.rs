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

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use super::theme::TerminalBrightness;

/// How long to wait for the terminal to answer the colour query.
///
/// A terminal that supports `OSC 11` answers in well under a millisecond.
/// One that does not answers never. The bound is short enough that an
/// unsupported terminal is not a visible stall at startup, and long enough
/// that a loaded machine still gets its answer.
const QUERY_TIMEOUT: Duration = Duration::from_millis(120);

/// The `OSC 11` query, followed by a `ST` terminator.
///
/// `OSC` opens an operating-system command, `11` selects the background
/// colour, and `ST` closes it. The reply comes back as
/// `OSC 11;rgb:RRRR/GGGG/BBBB ST`.
const QUERY: &[u8] = b"\x1b]11;?\x1b\\";

/// Ask the terminal for its default background colour.
///
/// Returns the colour as three channel values, or `None` when the
/// terminal did not answer within the bound. Never blocks for longer than
/// the bound.
pub fn query_background() -> Option<(u8, u8, u8)> {
    query_background_with(QUERY_TIMEOUT)
}

/// Ask the terminal for its default background colour, with an explicit
/// bound.
///
/// Split out so a test can drive the query against a pipe it controls.
pub fn query_background_with(timeout: Duration) -> Option<(u8, u8, u8)> {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    stdout.write_all(QUERY).ok()?;
    stdout.flush().ok()?;

    let deadline = Instant::now() + timeout;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 64];
    while Instant::now() < deadline {
        // A short read keeps the wait bounded even when the terminal sends
        // nothing at all.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match stdin.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                buffer.extend_from_slice(&chunk[..count]);
                if let Some(color) = parse_background(&buffer) {
                    return Some(color);
                }
            }
            Err(_) => break,
        }
    }
    parse_background(&buffer)
}

/// Parse a background colour out of an `OSC 11` reply.
///
/// Accepts both the `rgb:` form terminals use (`rgb:1c1c/1c1c/1c1c`) and a
/// plain triple (`1c1c1c`). Returns `None` for anything else, including a
/// reply that is still incomplete.
pub fn parse_background(reply: &[u8]) -> Option<(u8, u8, u8)> {
    let text = String::from_utf8_lossy(reply);
    // The reply is `OSC 11;rgb:RRRR/GGGG/BBBB ST`. Take the part after the
    // `11;` and before the terminator.
    let start = text.find("11;")? + 3;
    let payload = text[start..].split_terminator('\u{1b}').next()?.trim();
    let value = payload.strip_prefix("rgb:").unwrap_or(payload);
    // The `rgb:` form separates the channels with slashes; the plain form
    // is one run of digits. Both are seen in the wild, and a reply in
    // either shape is a colour the terminal meant.
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
    // Scale the reported value into the channel's full range.
    Some(((value * 255) / max) as u8)
}

/// Ask the terminal for its background and classify it.
///
/// A terminal that does not answer is treated as dark, because the dark
/// palette is the one that works on an unknown ground.
pub fn background_brightness() -> TerminalBrightness {
    TerminalBrightness::from_background(query_background())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_four_digit_rgb_reply_is_scaled_into_a_byte() {
        // `1c1c` is 28/255 of the way up the channel.
        let reply = b"\x1b]11;rgb:1c1c/1c1c/1c1c\x1b\\";
        assert_eq!(parse_background(reply), Some((28, 28, 28)));
    }

    #[test]
    fn a_six_digit_reply_is_scaled_per_channel() {
        // `ffffff` is the top of every channel; `000000` is the bottom.
        let white = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
        assert_eq!(parse_background(white), Some((255, 255, 255)));
        let black = b"\x1b]11;rgb:0000/0000/0000\x1b\\";
        assert_eq!(parse_background(black), Some((0, 0, 0)));
    }

    #[test]
    fn a_three_digit_reply_is_scaled_per_channel() {
        // `fff` is 15/15 of the way up; `000` is the bottom.
        let white = b"\x1b]11;rgb:fff/fff/fff\x1b\\";
        assert_eq!(parse_background(white), Some((255, 255, 255)));
        let black = b"\x1b]11;rgb:000/000/000\x1b\\";
        assert_eq!(parse_background(black), Some((0, 0, 0)));
    }

    #[test]
    fn a_plain_triple_without_the_rgb_prefix_is_accepted() {
        assert_eq!(
            parse_background(b"\x1b]11;808080\x1b\\"),
            Some((128, 128, 128))
        );
    }

    #[test]
    fn a_reply_for_another_query_is_not_read_as_a_background() {
        // OSC 10 is the *foreground*. Reading it as the background would
        // invert the decision on a dark terminal.
        assert_eq!(parse_background(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\"), None);
    }

    #[test]
    fn a_reply_that_is_not_a_colour_is_refused() {
        assert_eq!(parse_background(b"\x1b]11;not-a-colour\x1b\\"), None);
        assert_eq!(parse_background(b"\x1b]11;rgb:zzzz/0000/0000\x1b\\"), None);
    }

    #[test]
    fn an_incomplete_reply_is_not_read_as_a_colour() {
        // The channels have not all arrived yet.
        assert_eq!(parse_background(b"\x1b]11;rgb:ffff"), None);
        assert_eq!(parse_background(b"\x1b]11;rgb:ffff/ffff"), None);
    }

    #[test]
    fn an_empty_or_unrelated_reply_is_refused() {
        assert_eq!(parse_background(b""), None);
        assert_eq!(parse_background(b"hello"), None);
        assert_eq!(parse_background(b"\x1b]11;\x1b\\"), None);
    }

    #[test]
    fn a_light_background_is_classified_as_light() {
        // A terminal that answers `ffffff` is a light profile.
        assert_eq!(
            TerminalBrightness::from_background(Some((255, 255, 255))),
            TerminalBrightness::Light
        );
    }

    #[test]
    fn a_dark_background_is_classified_as_dark() {
        assert_eq!(
            TerminalBrightness::from_background(Some((28, 28, 28))),
            TerminalBrightness::Dark
        );
    }

    #[test]
    fn a_terminal_that_does_not_answer_is_dark() {
        // A palette must never be chosen from a value the app could not
        // read, so an absent answer is the dark profile.
        assert_eq!(
            TerminalBrightness::from_background(None),
            TerminalBrightness::Dark
        );
    }
}
