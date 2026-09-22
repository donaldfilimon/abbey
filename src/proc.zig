//! Bounded subprocess capture. Every executor (abi, ollama, claude, fm,
//! grok, cursor-agent, git) is a subprocess with its own argv grammar; none
//! is linked. std: lib/std/process.zig (run, RunOptions, RunError),
//! lib/std/process/Child.zig (Term).
const std = @import("std");
const Io = std.Io;
const Environ = std.process.Environ;

pub const Error = std.process.RunError;

pub const Limits = struct {
    pub const stdout_bytes_default = 4 * 1024 * 1024;
    stdout_bytes: usize = stdout_bytes_default,
    stderr_bytes: usize = 4 * 1024 * 1024,
    timeout_ms: i64 = 30 * 60 * 1000,
};

pub const Result = struct {
    term: std.process.Child.Term,
    stdout: []u8,
    stderr: []u8,

    pub fn deinit(self: Result, gpa: std.mem.Allocator) void {
        gpa.free(self.stdout);
        gpa.free(self.stderr);
    }

    pub fn success(self: Result) bool {
        return self.term.success();
    }

    /// Exit code, or 1 for a signal/stop/unknown termination (the Rust
    /// tree's `status.code().unwrap_or(1)`).
    pub fn code(self: Result) u8 {
        return switch (self.term) {
            .exited => |c| c,
            else => 1,
        };
    }
};

pub const Options = struct {
    env: ?*const Environ.Map = null,
    cwd: ?[]const u8 = null,
    limits: Limits = .{},
};

/// Run `argv` to completion with stdin closed, capturing both streams under
/// byte ceilings and a deadline. Caller frees via `Result.deinit`.
pub fn capture(gpa: std.mem.Allocator, io: Io, argv: []const []const u8, opts: Options) Error!Result {
    const r = try std.process.run(gpa, io, .{
        .argv = argv,
        .environ_map = opts.env,
        .cwd = if (opts.cwd) |c| .{ .path = c } else .inherit,
        .stdout_limit = .limited(opts.limits.stdout_bytes),
        .stderr_limit = .limited(opts.limits.stderr_bytes),
        .timeout = .{ .duration = .{ .raw = .fromMilliseconds(opts.limits.timeout_ms), .clock = .awake } },
    });
    return .{ .term = r.term, .stdout = r.stdout, .stderr = r.stderr };
}

test "capture collects stdout, stderr, and the exit code" {
    const gpa = std.testing.allocator;
    const r = try capture(gpa, std.testing.io, &.{ "/bin/sh", "-c", "printf out; printf err >&2; exit 3" }, .{});
    defer r.deinit(gpa);
    try std.testing.expectEqualStrings("out", r.stdout);
    try std.testing.expectEqualStrings("err", r.stderr);
    try std.testing.expectEqual(@as(u8, 3), r.code());
}

test "capture enforces the stdout ceiling and the deadline" {
    const gpa = std.testing.allocator;
    try std.testing.expectError(error.StreamTooLong, capture(gpa, std.testing.io, &.{ "/bin/sh", "-c", "yes | head -c 100000" }, .{ .limits = .{ .stdout_bytes = 1024 } }));
    try std.testing.expectError(error.Timeout, capture(gpa, std.testing.io, &.{ "/bin/sh", "-c", "sleep 5" }, .{ .limits = .{ .timeout_ms = 100 } }));
}
