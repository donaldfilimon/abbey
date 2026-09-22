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

/// Format `unix_ms` as `YYYY-MM-DDTHH:MM:SSZ` (seconds precision, truncated).
pub fn formatSeconds(buf: *[seconds_len]u8, unix_ms: i64) []const u8 {
    const secs: u64 = @intCast(@max(unix_ms, 0) / 1000);
    const es: epoch.EpochSeconds = .{ .secs = secs };
    const yd = es.getEpochDay().calculateYearDay();
    const md = yd.calculateMonthDay();
    const ds = es.getDaySeconds();
    return std.fmt.bufPrint(buf, "{d:0>4}-{d:0>2}-{d:0>2}T{d:0>2}:{d:0>2}:{d:0>2}Z", .{
        yd.year,              md.month.numeric(),      @as(u32, md.day_index) + 1,
        ds.getHoursIntoDay(), ds.getMinutesIntoHour(), ds.getSecondsIntoMinute(),
    }) catch unreachable; // fixed width: the buffer is exactly seconds_len
}

/// Format `unix_ms` as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn formatMillis(buf: *[millis_len]u8, unix_ms: i64) []const u8 {
    var head: [seconds_len]u8 = undefined;
    const s = formatSeconds(&head, unix_ms);
    const ms: u64 = @intCast(@mod(@max(unix_ms, 0), 1000));
    return std.fmt.bufPrint(buf, "{s}.{d:0>3}Z", .{ s[0 .. s.len - 1], ms }) catch unreachable; // fixed width
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
