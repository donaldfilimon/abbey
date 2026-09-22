//! Byte stream -> `Key` decoder for a raw-mode terminal (xterm/VT100
//! sequences). Pure: the event loop feeds whatever one read returned.
//!
//! A lone ESC at the end of a read is the Esc key (the loop reads with a
//! timeout, so an escape sequence arrives in one read in practice). Unknown
//! CSI/SS3 sequences are consumed whole and dropped, never typed as text.
const std = @import("std");
const Key = @import("app.zig").Key;

pub const Decoded = struct { key: ?Key, len: usize };

/// Decode one key from the front of `bytes` (non-empty).
pub fn decodeOne(bytes: []const u8) Decoded {
    const b = bytes[0];
    switch (b) {
        0x1b => {
            if (bytes.len == 1) return .{ .key = .esc, .len = 1 };
            if (bytes[1] == '[') return csi(bytes);
            if (bytes[1] == 'O' and bytes.len >= 3) return .{ .key = switch (bytes[2]) {
                'P' => .f1,
                'A' => .up,
                'B' => .down,
                'C' => .right,
                'D' => .left,
                'H' => .home,
                'F' => .end,
                else => null,
            }, .len = 3 };
            // ESC followed by anything else: Esc, then that byte on its own.
            return .{ .key = .esc, .len = 1 };
        },
        '\r', '\n' => return .{ .key = .enter, .len = 1 },
        '\t' => return .{ .key = .tab, .len = 1 },
        0x7f, 0x08 => return .{ .key = .backspace, .len = 1 },
        0x01...0x07, 0x0b, 0x0c, 0x0e...0x1a => return .{ .key = .{ .ctrl = b - 1 + 'a' }, .len = 1 },
        0x00, 0x1c...0x1f => return .{ .key = null, .len = 1 },
        else => {},
    }
    const n = std.unicode.utf8ByteSequenceLength(b) catch return .{ .key = null, .len = 1 }; // std: lib/std/unicode.zig
    if (n > bytes.len) return .{ .key = null, .len = bytes.len };
    const cp = std.unicode.utf8Decode(bytes[0..n]) catch return .{ .key = null, .len = 1 };
    return .{ .key = .{ .char = cp }, .len = n };
}

fn csi(bytes: []const u8) Decoded {
    // ESC [ params... final (0x40..0x7e)
    var i: usize = 2;
    while (i < bytes.len and !(bytes[i] >= 0x40 and bytes[i] <= 0x7e)) i += 1;
    if (i >= bytes.len) return .{ .key = null, .len = bytes.len };
    const params = bytes[2..i];
    const len = i + 1;
    const key: ?Key = switch (bytes[i]) {
        'A' => .up,
        'B' => .down,
        'C' => .right,
        'D' => .left,
        'H' => .home,
        'F' => .end,
        'Z' => .backtab,
        '~' => blk: {
            const eql = std.mem.eql;
            if (eql(u8, params, "1") or eql(u8, params, "7")) break :blk .home;
            if (eql(u8, params, "4") or eql(u8, params, "8")) break :blk .end;
            if (eql(u8, params, "3")) break :blk .delete;
            if (eql(u8, params, "5")) break :blk .page_up;
            if (eql(u8, params, "6")) break :blk .page_down;
            if (eql(u8, params, "11")) break :blk .f1;
            break :blk null;
        },
        else => null,
    };
    return .{ .key = key, .len = len };
}

/// Decode every key in `bytes` into `out`; returns the keys written.
pub fn decodeAll(bytes: []const u8, out: []Key) []Key {
    var n: usize = 0;
    var i: usize = 0;
    while (i < bytes.len and n < out.len) {
        const d = decodeOne(bytes[i..]);
        i += d.len;
        if (d.key) |k| {
            out[n] = k;
            n += 1;
        }
    }
    return out[0..n];
}

test "decoder maps xterm sequences, ctrl chords, and UTF-8" {
    var buf: [32]Key = undefined;
    const keys = decodeAll("a\u{e9}\r\t\x1b[Z\x1b[A\x1b[B\x1b[C\x1b[D\x1b[H\x1b[F\x1b[3~\x1b[5~\x1b[6~\x1bOP\x1b[11~\x02\x0b\x7f\x1b[1;5A\x1b", &buf);
    const want = [_]Key{
        .{ .char = 'a' }, .{ .char = 0xe9 }, .enter,           .tab,       .backtab,
        .up,              .down,             .right,           .left,      .home,
        .end,             .delete,           .page_up,         .page_down, .f1,
        .f1,              .{ .ctrl = 'b' },  .{ .ctrl = 'k' }, .backspace, .up,
        .esc,
    };
    try std.testing.expectEqual(want.len, keys.len);
    for (want, keys) |w, k| try std.testing.expectEqualDeep(w, k);
    // Unknown sequences are swallowed whole; a truncated one yields nothing.
    try std.testing.expectEqual(@as(usize, 0), decodeAll("\x1b[99x\x1b[12", &buf).len);
    try std.testing.expectEqual(@as(usize, 0), decodeAll("\xc3", &buf).len);
    // Ctrl-C / Ctrl-Q arrive as keys in raw mode (ISIG is off).
    try std.testing.expectEqualDeep(Key{ .ctrl = 'c' }, decodeAll("\x03", &buf)[0]);
    try std.testing.expectEqualDeep(Key{ .ctrl = 'q' }, decodeAll("\x11", &buf)[0]);
}
