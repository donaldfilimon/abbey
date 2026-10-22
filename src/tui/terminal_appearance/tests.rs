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

#[test]
fn the_query_is_written_and_the_reply_is_read() {
    // The real exchange: the query goes out, the terminal's reply comes
    // back, and the reply is the colour. This is the shape a terminal
    // performs, driven through pipes the test owns.
    let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
    let mut input = std::io::Cursor::new(reply.to_vec());
    let mut output: Vec<u8> = Vec::new();
    let color = query_background_from(&mut input, &mut output, Duration::from_millis(500));
    assert_eq!(color, Some((255, 255, 255)));
    // The query that was written is the one a terminal answers.
    assert_eq!(output, b"\x1b]11;?\x1b\\");
}

#[test]
fn a_terminal_that_says_nothing_is_dark() {
    // The bound is what makes this safe: a terminal that does not
    // support the query never answers, and the app must not wait
    // forever for it.
    let mut input = std::io::Cursor::new(Vec::new());
    let mut output: Vec<u8> = Vec::new();
    let color = query_background_from(&mut input, &mut output, Duration::from_millis(50));
    assert_eq!(color, None);
    // The query was still sent: the terminal simply did not answer.
    assert_eq!(output, b"\x1b]11;?\x1b\\");
}

#[test]
fn a_reply_that_arrives_in_pieces_is_still_read() {
    // A terminal may answer in more than one write, and a partial
    // answer must not be mistaken for a complete one.
    let mut input = std::io::Cursor::new(b"\x1b]11;rgb:".to_vec());
    let mut output: Vec<u8> = Vec::new();
    let color = query_background_from(&mut input, &mut output, Duration::from_millis(50));
    // The first piece is incomplete, so no colour is claimed.
    assert_eq!(color, None);
}

#[test]
fn a_bel_terminated_reply_is_read() {
    // `BEL` is the other legal terminator. A terminal that uses it must
    // be answered, not refused because the reply did not end in `ST`.
    let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x07";
    assert_eq!(parse_background(reply), Some((255, 255, 255)));
}

#[test]
fn text_typed_before_the_reply_does_not_latch_the_parse() {
    // The input queue carries the user's keystrokes as well as the
    // reply. A pasted `11;rgb:ffff/ffff/ffff` must not be read as the
    // terminal's answer: the parse is anchored on the introducer, so
    // the real reply is what is read.
    let mut buffer = b"11;rgb:ffff/ffff/ffff".to_vec();
    buffer.extend_from_slice(b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    assert_eq!(parse_background(&buffer), Some((0, 0, 0)));
}

#[test]
fn a_channel_is_rounded_rather_than_floored() {
    // `8000` is exactly half of `ffff`, and half of the channel's range
    // is 127.5. Flooring would report the channel as 127 — under the
    // middle — while the rounding reports the 128 it is.
    assert_eq!(
        parse_background(b"\x1b]11;rgb:8000/8000/8000\x1b\\"),
        Some((128, 128, 128))
    );
    // `8080` is 128/256 of the way up, and lands on 128 exactly.
    assert_eq!(
        parse_background(b"\x1b]11;rgb:8080/8080/8080\x1b\\"),
        Some((128, 128, 128))
    );
    // `8101` is past the middle, and rounds up rather than down.
    assert_eq!(
        parse_background(b"\x1b]11;rgb:8101/8101/8101\x1b\\"),
        Some((129, 129, 129))
    );
}

#[test]
fn a_reply_is_complete_only_at_its_terminator() {
    // The terminator is what tells the reader to stop, so a keystroke
    // typed after the reply is not swallowed with it.
    assert!(!reply_is_complete(b"\x1b]11;rgb:ffff/ffff/ffff"));
    assert!(reply_is_complete(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\"));
    assert!(reply_is_complete(b"\x1b]11;rgb:ffff/ffff/ffff\x07"));
}

#[test]
fn a_terminator_before_the_reply_does_not_end_the_reply() {
    // The input queue carries the user's keystrokes as well as the
    // answer. A `BEL` or an `ESC \` typed before the terminal answers
    // belongs to the user, not to the reply: ending the read there would
    // throw the answer away and leave the palette dark.
    assert!(!reply_is_complete(b"\x07"));
    assert!(!reply_is_complete(b"\x1b\\"));
    assert!(!reply_is_complete(b"hello\x07"));
    assert!(!reply_is_complete(b"\x07\x1b]11;rgb:ffff/ffff/ffff"));
    assert!(reply_is_complete(b"\x07\x1b]11;rgb:ffff/ffff/ffff\x1b\\"));
}

#[test]
fn a_saturated_green_background_is_classified_as_light() {
    // The classification is by perceived brightness, not by the average
    // of the channels: a channel average calls this green dark (85),
    // while the eye reads it as bright. The rule that decides the
    // palette has to be the rule the palette was checked against.
    assert_eq!(
        TerminalBrightness::from_background(Some((0, 255, 0))),
        TerminalBrightness::Light
    );
}

#[test]
fn a_reply_without_its_terminator_is_not_read_as_a_colour() {
    // The reply is still on its way. Reading it as a colour would take
    // a truncated answer for a whole one — a reply cut short at
    // `ffff/ffff/ff` would be read as white.
    assert_eq!(parse_background(b"\x1b]11;rgb:ffff/ffff/ffff"), None);
    assert_eq!(parse_background(b"\x1b]11;rgb:ffff/ffff/ff"), None);
}

#[cfg(unix)]
#[test]
fn a_terminal_that_never_answers_is_waited_on_only_for_the_bound() {
    // The bound is the whole safety of the query: a terminal that does
    // not support `OSC 11` never answers, and the app must reach its
    // first frame anyway. The reader here is a pipe that stays open and
    // stays silent, which is exactly that terminal.
    use nix::fcntl::{FcntlArg, OFlag, fcntl};

    let (read_end, _write_end) = nix::unistd::pipe().expect("a pipe");
    fcntl(&read_end, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).expect("a non-blocking pipe");

    let bound = Duration::from_millis(120);
    let started = Instant::now();
    let reply = read_reply(
        &mut || read_byte(&read_end),
        bound,
        Duration::from_millis(60),
    );
    let elapsed = started.elapsed();

    assert!(
        reply.reply.is_empty(),
        "a terminal that never answers has no colour"
    );
    assert!(
        elapsed >= bound,
        "the read gave up before the bound had passed: {elapsed:?}"
    );
    assert!(
        elapsed < bound + Duration::from_secs(5),
        "the read outlasted the bound: {elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_terminals_own_flags_are_restored_after_the_query() {
    // The query asks; it does not change the terminal. The flags are
    // read before they are changed and put back afterwards, so a
    // terminal that is found blocking is found blocking.
    use nix::fcntl::{FcntlArg, OFlag, fcntl};

    let (read_end, _write_end) = nix::unistd::pipe().expect("a pipe");
    let before = fcntl(&read_end, FcntlArg::F_GETFL).expect("the pipe's flags");
    let reply = read_reply_from_fd(&read_end, Duration::from_millis(50), Duration::ZERO);
    let after = fcntl(&read_end, FcntlArg::F_GETFL).expect("the pipe's flags");

    assert!(
        reply.as_ref().is_none_or(|read| read.reply.is_empty()),
        "the pipe is silent"
    );
    assert_eq!(
        OFlag::from_bits_truncate(after),
        OFlag::from_bits_truncate(before),
        "the query left the descriptor's flags changed"
    );
}

#[cfg(unix)]
#[test]
fn a_reply_that_arrives_after_the_window_is_taken_by_the_grace() {
    // A terminal that answers late is still worth waiting for: the
    // answer would otherwise be read as keystrokes by the event loop.
    use nix::fcntl::{FcntlArg, OFlag, fcntl};

    let (read_end, write_end) = nix::unistd::pipe().expect("a pipe");
    fcntl(&read_end, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).expect("a non-blocking pipe");
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        nix::unistd::write(&write_end, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\")
            .expect("the reply is written");
    });

    let reply = read_reply(
        &mut || read_byte(&read_end),
        Duration::from_millis(10),
        Duration::from_millis(500),
    );
    writer.join().expect("the writer");

    assert_eq!(parse_background(&reply.reply), Some((255, 255, 255)));
}

#[cfg(unix)]
#[test]
fn the_read_stops_at_the_terminator_and_leaves_the_keystrokes_after_it() {
    // The input queue carries the user's keystrokes as well as the
    // answer. A read that took more than the reply would swallow them,
    // so the read is a byte at a time and stops at the terminator.
    use nix::fcntl::{FcntlArg, OFlag, fcntl};

    let (read_end, write_end) = nix::unistd::pipe().expect("a pipe");
    fcntl(&read_end, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).expect("a non-blocking pipe");
    nix::unistd::write(&write_end, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\hi")
        .expect("the reply is written");

    let reply = read_reply(
        &mut || read_byte(&read_end),
        Duration::from_millis(500),
        Duration::ZERO,
    );
    assert_eq!(reply.reply, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");

    // The keystrokes are still in the queue: the read stopped at the
    // terminator instead of taking the whole chunk.
    let mut rest = [0u8; 2];
    nix::unistd::read(&read_end, &mut rest).expect("the rest is still there");
    assert_eq!(&rest, b"hi");
}

/// The keystrokes the query read are the user's, and they are kept.
///
/// The input queue carries the user's typing as well as the answer.
/// A read that dropped the typing would spend a keystroke on learning
/// a colour, so what came before the reply is given back.
#[test]
fn the_keystrokes_that_came_before_the_reply_are_kept() {
    let mut buffer = b"hi".to_vec();
    buffer.extend_from_slice(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
    let (reply, leftover) = split_reply(&buffer);
    assert_eq!(reply, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
    assert_eq!(leftover, b"hi");
}

/// A reply that never came leaves the typing alone.
///
/// Nothing was read but the user's own bytes, so all of them are the
/// user's and all of them are given back.
#[test]
fn the_typing_is_kept_when_no_reply_arrives() {
    let (reply, leftover) = split_reply(b"hi there");
    assert!(reply.is_empty(), "there was no reply to read");
    assert_eq!(leftover, b"hi there");
}

/// A reply that was cut short is still the terminal's.
///
/// Half an answer is not something the user typed, and it must not
/// reach the composer as text.
#[test]
fn a_reply_that_was_cut_short_is_not_given_back_as_typing() {
    let mut buffer = b"hi".to_vec();
    buffer.extend_from_slice(b"\x1b]11;rgb:ffff");
    let (reply, leftover) = split_reply(&buffer);
    assert_eq!(reply, b"\x1b]11;rgb:ffff");
    assert_eq!(leftover, b"hi");
}
