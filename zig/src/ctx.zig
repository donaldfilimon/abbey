//! Per-invocation context threaded through every command.
//!
//! Library code never reads the process environment, the working directory,
//! or stdio directly: everything arrives here, so tests can hand in a
//! synthetic environment and capture output in memory.
const std = @import("std");
const Io = std.Io; // std: lib/std/Io.zig
const Environ = std.process.Environ; // std: lib/std/process/Environ.zig (Map.get)

pub const Ctx = struct {
    gpa: std.mem.Allocator,
    io: Io,
    env: *const Environ.Map,
    out: *Io.Writer,
    err: *Io.Writer,
    /// Absolute working directory.
    cwd: []const u8,

    pub fn getEnv(self: Ctx, name: []const u8) ?[]const u8 {
        return self.env.get(name);
    }

    /// The variable's value trimmed of ASCII whitespace, or null when unset
    /// or blank (the Rust tree treats a blank override as unset).
    pub fn envNonEmpty(self: Ctx, name: []const u8) ?[]const u8 {
        const v = self.env.get(name) orelse return null;
        const t = std.mem.trim(u8, v, &std.ascii.whitespace);
        return if (t.len == 0) null else t;
    }
};

/// A test context whose output lands in allocating writers.
pub const TestCtx = struct {
    env: Environ.Map,
    out: Io.Writer.Allocating,
    err: Io.Writer.Allocating,
    cwd: []const u8,

    pub fn init(gpa: std.mem.Allocator, cwd: []const u8) TestCtx {
        return .{
            .env = Environ.Map.init(gpa),
            .out = .init(gpa),
            .err = .init(gpa),
            .cwd = cwd,
        };
    }

    pub fn deinit(self: *TestCtx) void {
        self.env.deinit();
        self.out.deinit();
        self.err.deinit();
    }

    pub fn ctx(self: *TestCtx, gpa: std.mem.Allocator, io: Io) Ctx {
        return .{
            .gpa = gpa,
            .io = io,
            .env = &self.env,
            .out = &self.out.writer,
            .err = &self.err.writer,
            .cwd = self.cwd,
        };
    }

    pub fn outText(self: *TestCtx) []const u8 {
        return self.out.written();
    }

    pub fn errText(self: *TestCtx) []const u8 {
        return self.err.written();
    }
};

/// Absolute path of a testing tmp dir. Caller frees.
pub fn tmpPath(gpa: std.mem.Allocator, io: Io, dir: Io.Dir) ![]u8 {
    var buf: [std.fs.max_path_bytes]u8 = undefined;
    const n = try dir.realPath(io, &buf);
    return gpa.dupe(u8, buf[0..n]);
}

test "envNonEmpty trims and treats blank as unset" {
    var t = TestCtx.init(std.testing.allocator, "/");
    defer t.deinit();
    try t.env.put("A", "  x ");
    try t.env.put("B", "   ");
    const c = t.ctx(std.testing.allocator, std.testing.io);
    try std.testing.expectEqualStrings("x", c.envNonEmpty("A").?);
    try std.testing.expect(c.envNonEmpty("B") == null);
    try std.testing.expect(c.envNonEmpty("C") == null);
}
