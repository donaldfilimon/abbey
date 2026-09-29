//! Construct a fresh STM record the way Rust `MemoryRecord::new_stm` does:
//! uuid v4 id, UTC seconds timestamp, `project` = git toplevel of the cwd,
//! else the canonical cwd.
const std = @import("std");
const Ctx = @import("../ctx.zig").Ctx;
const Record = @import("record.zig").Record;
const proc = @import("../proc.zig");
const time = @import("../util/time.zig");
const uuid = @import("../util/uuid.zig");

pub fn currentProject(ctx: Ctx, arena: std.mem.Allocator) error{OutOfMemory}![]const u8 {
    if (proc.capture(ctx.gpa, ctx.io, &.{ "git", "rev-parse", "--show-toplevel" }, .{ .env = ctx.env, .cwd = ctx.cwd, .limits = .{ .timeout_ms = 10_000, .stdout_bytes = 64 * 1024 } })) |r| {
        defer r.deinit(ctx.gpa);
        const root = std.mem.trim(u8, r.stdout, &std.ascii.whitespace);
        if (r.success() and root.len != 0) return arena.dupe(u8, root);
    } else |_| {}
    var buf: [std.fs.max_path_bytes]u8 = undefined;
    const n = std.Io.Dir.cwd().realPathFile(ctx.io, ctx.cwd, &buf) catch return arena.dupe(u8, ctx.cwd);
    return arena.dupe(u8, buf[0..n]);
}

pub fn stm(ctx: Ctx, arena: std.mem.Allocator, summary: []const u8, payload: []const u8) error{OutOfMemory}!Record {
    var ub: [uuid.len]u8 = undefined;
    var tb: [time.seconds_len]u8 = undefined;
    return .{
        .id = try arena.dupe(u8, uuid.v4(ctx.io, &ub)),
        .timestamp = try arena.dupe(u8, time.formatSeconds(&tb, time.nowMillis(ctx.io))),
        .project = try currentProject(ctx, arena),
        .summary = try arena.dupe(u8, summary),
        .payload = try arena.dupe(u8, payload),
        .tags = try arena.dupe([]const u8, &.{"stm"}),
    };
}

test "new stm record carries id, timestamp, project, stm tag" {
    const T = @import("../ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var t = T.init(gpa, root);
    defer t.deinit();
    try t.env.put("PATH", "/usr/bin:/bin:/opt/homebrew/bin");
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const r = try stm(t.ctx(gpa, io), arena.allocator(), "sum", "pay");
    try std.testing.expectEqual(@as(usize, uuid.len), r.id.len);
    try std.testing.expect(std.mem.endsWith(u8, r.timestamp, "Z"));
    // The testing tmp dir sits under .zig-cache inside a checkout, so the
    // project is that checkout's git toplevel, a prefix of the tmp path.
    try std.testing.expect(r.project.len != 0 and std.mem.startsWith(u8, root, r.project));
    try std.testing.expect(r.hasTag("stm"));
    try std.testing.expectEqualStrings("stm", r.retention);
}
