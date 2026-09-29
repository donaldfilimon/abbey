//! Unicode-exact text predicates the daemon's sanitizer shares with the Rust
//! consumer, plus a strict RFC 3339 checker.
//!
//! The Rust client re-validates every route-audit page with `str::split_whitespace`
//! and `char::is_control`, which are Unicode (White_Space, and general
//! category Cc). An ASCII-only port would let a `U+00A0` or `U+0085` separated
//! path survive our redaction and then fail the Rust client's check, so both
//! predicates here decode UTF-8. std: lib/std/unicode.zig (Utf8View,
//! utf8ValidateSlice, Utf8Iterator.nextCodepoint / nextCodepointSlice).
const std = @import("std");
const unicode = std.unicode;

/// Rust `char::is_whitespace` (the Unicode White_Space property).
pub fn isWhitespace(cp: u21) bool {
    return switch (cp) {
        0x09...0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x2000...0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000 => true,
        else => false,
    };
}

/// Rust `char::is_control` (general category Cc).
pub fn isControl(cp: u21) bool {
    return cp <= 0x1f or (cp >= 0x7f and cp <= 0x9f);
}

/// True when `s` is not valid UTF-8 or contains any Cc code point.
pub fn hasControl(s: []const u8) bool {
    const view = unicode.Utf8View.init(s) catch return true;
    var it = view.iterator();
    while (it.nextCodepoint()) |cp| if (isControl(cp)) return true;
    return false;
}

/// Rust `str::trim`: strip Unicode White_Space from both ends. Invalid UTF-8
/// is returned unchanged (callers validate separately).
pub fn trim(s: []const u8) []const u8 {
    if (!unicode.utf8ValidateSlice(s)) return s;
    var start: usize = 0;
    var it = unicode.Utf8View.initUnchecked(s).iterator();
    while (it.nextCodepointSlice()) |cs| {
        const cp = unicode.utf8Decode(cs) catch break;
        if (!isWhitespace(cp)) break;
        start += cs.len;
    }
    var end: usize = s.len;
    while (end > start) {
        var b = end - 1;
        while (b > start and (s[b] & 0xc0) == 0x80) b -= 1;
        const cp = unicode.utf8Decode(s[b..end]) catch break;
        if (!isWhitespace(cp)) break;
        end = b;
    }
    return s[start..end];
}

/// Rust `str::split_whitespace` over valid UTF-8: non-empty runs between
/// Unicode White_Space code points.
pub const Tokens = struct {
    s: []const u8,
    i: usize = 0,

    pub fn next(t: *Tokens) ?[]const u8 {
        // Skip leading whitespace.
        while (t.i < t.s.len) {
            const n = unicode.utf8ByteSequenceLength(t.s[t.i]) catch 1;
            const end = @min(t.i + n, t.s.len);
            const cp = unicode.utf8Decode(t.s[t.i..end]) catch break;
            if (!isWhitespace(cp)) break;
            t.i = end;
        }
        if (t.i >= t.s.len) return null;
        const start = t.i;
        while (t.i < t.s.len) {
            const n = unicode.utf8ByteSequenceLength(t.s[t.i]) catch 1;
            const end = @min(t.i + n, t.s.len);
            const cp = unicode.utf8Decode(t.s[t.i..end]) catch {
                t.i = end;
                continue;
            };
            if (isWhitespace(cp)) break;
            t.i = end;
        }
        return t.s[start..t.i];
    }
};

pub fn tokens(s: []const u8) Tokens {
    return .{ .s = s };
}

/// Longest prefix of `s` that is at most `max` bytes and ends on a UTF-8
/// boundary (Rust `String::truncate` after backing off `is_char_boundary`).
pub fn truncateOnBoundary(s: []const u8, max: usize) []const u8 {
    if (s.len <= max) return s;
    var end = max;
    while (end > 0 and (s[end] & 0xc0) == 0x80) end -= 1;
    return s[0..end];
}

fn digits(s: []const u8, at: usize, n: usize) ?u32 {
    if (at + n > s.len) return null;
    var v: u32 = 0;
    for (s[at .. at + n]) |c| {
        if (c < '0' or c > '9') return null;
        v = v * 10 + (c - '0');
    }
    return v;
}

fn daysIn(year: u32, month: u32) u32 {
    return switch (month) {
        1, 3, 5, 7, 8, 10, 12 => 31,
        4, 6, 9, 11 => 30,
        2 => if ((year % 4 == 0 and year % 100 != 0) or year % 400 == 0) 29 else 28,
        else => 0,
    };
}

/// Strict RFC 3339 `date-time`: `YYYY-MM-DDTHH:MM:SS(.d+)?(Z|(+|-)HH:MM)` with
/// calendar-valid dates, hours < 24, minutes and seconds < 60, offsets
/// < 24:00. Hand-written stand-in for the Rust side's
/// `chrono::DateTime::parse_from_rfc3339`, deliberately narrower (no
/// lowercase `t`/`z`, space separator, or leap second) so it accepts no shape
/// the Rust consumer could reject; a narrower check only drops a record.
pub fn isRfc3339(s: []const u8) bool {
    const year = digits(s, 0, 4) orelse return false;
    if (s.len < 20 or s[4] != '-' or s[7] != '-') return false;
    const month = digits(s, 5, 2) orelse return false;
    const day = digits(s, 8, 2) orelse return false;
    if (month < 1 or month > 12 or day < 1 or day > daysIn(year, month)) return false;
    if (s[10] != 'T') return false;
    const hour = digits(s, 11, 2) orelse return false;
    const minute = digits(s, 14, 2) orelse return false;
    const second = digits(s, 17, 2) orelse return false;
    if (s[13] != ':' or s[16] != ':' or hour > 23 or minute > 59 or second > 59) return false;
    var i: usize = 19;
    if (i < s.len and s[i] == '.') {
        i += 1;
        const start = i;
        while (i < s.len and s[i] >= '0' and s[i] <= '9') i += 1;
        if (i == start) return false;
    }
    if (i >= s.len) return false;
    if (s[i] == 'Z') return i + 1 == s.len;
    if (s[i] != '+' and s[i] != '-') return false;
    if (i + 6 != s.len or s[i + 3] != ':') return false;
    const oh = digits(s, i + 1, 2) orelse return false;
    const om = digits(s, i + 4, 2) orelse return false;
    return oh <= 23 and om <= 59;
}

test "unicode whitespace and control predicates match Rust char semantics" {
    try std.testing.expect(isWhitespace(0x85) and isWhitespace(0xa0) and isWhitespace(0x3000));
    try std.testing.expect(!isWhitespace('x') and !isWhitespace(0x200b));
    try std.testing.expect(isControl(0x07) and isControl(0x7f) and isControl(0x85) and !isControl(0xa0));
    try std.testing.expect(hasControl("a\u{85}b") and hasControl("a\x07") and !hasControl("caf\u{e9}"));
    try std.testing.expectEqualStrings("x y", trim("\u{a0} x y\u{3000}\n"));
    var t = tokens("a\u{a0}/etc/passwd\u{85}b  c");
    try std.testing.expectEqualStrings("a", t.next().?);
    try std.testing.expectEqualStrings("/etc/passwd", t.next().?);
    try std.testing.expectEqualStrings("b", t.next().?);
    try std.testing.expectEqualStrings("c", t.next().?);
    try std.testing.expect(t.next() == null);
    try std.testing.expectEqualStrings("\u{65e5}", truncateOnBoundary("\u{65e5}\u{65e5}", 4));
}

test "rfc3339 checker accepts chrono shapes and rejects the rest" {
    for ([_][]const u8{ "2026-08-08T12:00:00Z", "2026-08-08T12:00:00.123Z", "2024-02-29T23:59:59+05:30", "2026-01-01T00:00:00-00:00" }) |ok| {
        try std.testing.expect(isRfc3339(ok));
    }
    for ([_][]const u8{ "yesterday", "not-a-timestamp", "2026-02-29T00:00:00Z", "2026-13-01T00:00:00Z", "2026-08-08T24:00:00Z", "2026-08-08T12:00:00", "2026-08-08T12:00:00.Z", "2026-08-08T12:00:00+24:00", "2026-08-08T12:00:00Zx", "2026-08-08t12:00:00z", "2026-08-08 12:00:00Z" }) |bad| {
        try std.testing.expect(!isRfc3339(bad));
    }
}
