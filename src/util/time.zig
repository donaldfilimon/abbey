//! UTC timestamps in the two shapes the Rust tree writes:
//! `%Y-%m-%dT%H:%M:%SZ` (route.jsonl, memory records) and the same with
//! millisecond precision (history.log).
const std = @import("std");
const Io = std.Io; // std: lib/std/Io.zig (Clock.real.now, Timestamp.toSeconds)
const epoch = std.time.epoch; // std: lib/std/time/epoch.zig (EpochSeconds)

pub const seconds_len = "2026-01-01T00:00:00Z".len;
pub const millis_len = "2026-01-01T00:00:00.000Z".len;

/// Current wall-clock time as milliseconds since the Unix epoch.
pub fn nowMillis(io: Io) i64 {
    return Io.Clock.real.now(io).toMilliseconds();
}

fn put(buf: []u8, at: usize, value: u64, width: usize) void {
    var v = value;
    var i = width;
    while (i > 0) {
        i -= 1;
        buf[at + i] = '0' + @as(u8, @intCast(v % 10));
        v /= 10;
    }
}

/// Format `unix_ms` as `YYYY-MM-DDTHH:MM:SSZ` (seconds precision, truncated).
/// Fixed-width digit writes, so no formatting error path exists.
pub fn formatSeconds(buf: *[seconds_len]u8, unix_ms: i64) []const u8 {
    const secs: u64 = @intCast(@max(unix_ms, 0) / 1000);
    const es: epoch.EpochSeconds = .{ .secs = secs };
    const yd = es.getEpochDay().calculateYearDay();
    const md = yd.calculateMonthDay();
    const ds = es.getDaySeconds();
    @memcpy(buf, "0000-00-00T00:00:00Z");
    put(buf, 0, yd.year, 4);
    put(buf, 5, md.month.numeric(), 2);
    put(buf, 8, @as(u64, md.day_index) + 1, 2);
    put(buf, 11, ds.getHoursIntoDay(), 2);
    put(buf, 14, ds.getMinutesIntoHour(), 2);
    put(buf, 17, ds.getSecondsIntoMinute(), 2);
    return buf;
}

/// Format `unix_ms` as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn formatMillis(buf: *[millis_len]u8, unix_ms: i64) []const u8 {
    var head: [seconds_len]u8 = undefined;
    _ = formatSeconds(&head, unix_ms);
    @memcpy(buf[0 .. seconds_len - 1], head[0 .. seconds_len - 1]);
    buf[seconds_len - 1] = '.';
    put(buf, seconds_len, @intCast(@mod(@max(unix_ms, 0), 1000)), 3);
    buf[millis_len - 1] = 'Z';
    return buf;
}

test "timestamps format as UTC RFC 3339" {
    var a: [seconds_len]u8 = undefined;
    try std.testing.expectEqualStrings("1970-01-01T00:00:00Z", formatSeconds(&a, 0));
    // 2026-09-22T03:22:37.976Z, taken from a Rust-written history.log line.
    try std.testing.expectEqualStrings("2026-09-22T03:22:37Z", formatSeconds(&a, 1790047357976));
    var b: [millis_len]u8 = undefined;
    try std.testing.expectEqualStrings("2026-09-22T03:22:37.976Z", formatMillis(&b, 1790047357976));
    try std.testing.expectEqualStrings("2024-02-29T23:59:59Z", formatSeconds(&a, 1709251199000));
}
