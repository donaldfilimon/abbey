//! serde_json-compatible JSON emission for the on-disk formats Abbey shares
//! with the Rust tree (route.jsonl, memory JSONL).
//!
//! `std.json.Stringify` is not used for emission because two details must be
//! byte-identical to serde_json: string escaping (serde escapes only `"`,
//! `\\`, and C0 controls, using `\b \f \n \r \t` and lowercase `\u00xx`) and
//! f32 formatting (serde uses ryu's shortest f32 digits with its own
//! decimal/scientific thresholds). Reading uses `std.json` (see record
//! parsers), which accepts everything written here.
const std = @import("std");
const Writer = std.Io.Writer; // std: lib/std/Io/Writer.zig (print, writeAll, writeByte)

/// Write `s` as a JSON string literal exactly as serde_json does.
pub fn writeString(w: *Writer, s: []const u8) Writer.Error!void {
    try w.writeByte('"');
    var start: usize = 0;
    for (s, 0..) |c, i| {
        const esc: ?[]const u8 = switch (c) {
            '"' => "\\\"",
            '\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            else => null,
        };
        if (esc == null and c >= 0x20) continue;
        try w.writeAll(s[start..i]);
        if (esc) |e| {
            try w.writeAll(e);
        } else {
            const hex = "0123456789abcdef";
            try w.writeAll("\\u00");
            try w.writeByte(hex[c >> 4]);
            try w.writeByte(hex[c & 0xf]);
        }
        start = i + 1;
    }
    try w.writeAll(s[start..]);
    try w.writeByte('"');
}

/// Write an f32 the way serde_json does: ryu shortest digits, `null` for
/// non-finite values, decimal notation when the decimal point position `kk`
/// satisfies -6 < kk <= 13, scientific otherwise, and a trailing `.0` for
/// integral decimal output. Thresholds are ryu's `format32`.
pub fn writeF32(w: *Writer, v: f32) Writer.Error!void {
    if (!std.math.isFinite(v)) return w.writeAll("null");
    if (v == 0) return w.writeAll(if (std.math.signbit(v)) "-0.0" else "0.0");
    // Zig's `{e}` prints the shortest round-trip f32 digits as d.ddde[-]x.
    var buf: [64]u8 = undefined;
    const sci = std.fmt.bufPrint(&buf, "{e}", .{v}) catch return error.WriteFailed;
    var rest = sci;
    const negative = rest[0] == '-';
    if (negative) rest = rest[1..];
    const e_at = std.mem.indexOfScalar(u8, rest, 'e') orelse return error.WriteFailed;
    const mant = rest[0..e_at];
    const exp = std.fmt.parseInt(i32, rest[e_at + 1 ..], 10) catch return error.WriteFailed;
    var digits_buf: [32]u8 = undefined;
    var n: usize = 0;
    for (mant) |c| {
        if (c == '.') continue;
        digits_buf[n] = c;
        n += 1;
    }
    const digits = digits_buf[0..n];
    const len: i32 = @intCast(n);
    const kk: i32 = exp + 1; // position of the decimal point relative to the digits
    if (negative) try w.writeByte('-');
    if (kk >= len and kk <= 13) {
        try w.writeAll(digits);
        var z = kk - len;
        while (z > 0) : (z -= 1) try w.writeByte('0');
        try w.writeAll(".0");
    } else if (kk > 0 and kk <= 13) {
        const k: usize = @intCast(kk);
        try w.writeAll(digits[0..k]);
        try w.writeByte('.');
        try w.writeAll(digits[k..]);
    } else if (kk > -6 and kk <= 0) {
        try w.writeAll("0.");
        var z = -kk;
        while (z > 0) : (z -= 1) try w.writeByte('0');
        try w.writeAll(digits);
    } else {
        try w.writeByte(digits[0]);
        if (n > 1) {
            try w.writeByte('.');
            try w.writeAll(digits[1..]);
        }
        const e10 = kk - 1;
        // serde_json writes an explicit `+` on positive exponents (`1e+13`).
        if (e10 >= 0) try w.writeAll("e+") else try w.writeAll("e-");
        try w.print("{d}", .{@abs(e10)});
    }
}

fn expectF32(expected: []const u8, v: f32) !void {
    var buf: [64]u8 = undefined;
    var w: Writer = .fixed(&buf);
    try writeF32(&w, v);
    try std.testing.expectEqualStrings(expected, w.buffered());
}

test "writeF32 matches serde_json f32 output" {
    // Expected strings were produced by serde_json 1.x (`to_string(&v)` on f32)
    // in a scratch crate on 2026-09-21; see docs/claims.md provenance.
    try expectF32("0.8", 0.8);
    try expectF32("0.95", 0.95);
    try expectF32("0.55", 0.55);
    try expectF32("0.85", 0.85);
    try expectF32("1.0", 1.0);
    try expectF32("0.0", 0.0);
    try expectF32("-0.5", -0.5);
    try expectF32("123456.7", 123456.7);
    try expectF32("0.00001", 0.00001);
    try expectF32("0.000001", 0.000001);
    try expectF32("1e-7", 1e-7);
    try expectF32("1.5e-7", 1.5e-7);
    try expectF32("1000000000000.0", 1e12);
    try expectF32("1e+13", 1e13);
    try expectF32("null", std.math.nan(f32));
}

fn expectString(expected: []const u8, s: []const u8) !void {
    var buf: [128]u8 = undefined;
    var w: Writer = .fixed(&buf);
    try writeString(&w, s);
    try std.testing.expectEqualStrings(expected, w.buffered());
}

test "writeString matches serde_json escaping" {
    try expectString("\"plain\"", "plain");
    try expectString("\"a\\\"b\\\\c\"", "a\"b\\c");
    try expectString("\"\\n\\t\\r\\b\\f\"", "\n\t\r\x08\x0c");
    try expectString("\"\\u0001\\u001f\"", "\x01\x1f");
    // serde_json leaves DEL, `/`, and non-ASCII UTF-8 unescaped.
    try expectString("\"\x7f/\u{2019}\"", "\x7f/\u{2019}");
}
