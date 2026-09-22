//! abbey-zig entry point: wire the process into a `Ctx` and dispatch.
const std = @import("std");
const abbey = @import("abbey");
const Io = std.Io; // std: lib/std/Io/File.zig (Writer.init, stdout/stderr)

/// A panic while the TUI holds the terminal restores termios and leaves the
/// alternate screen first. std: lib/std/debug.zig (FullPanic, defaultPanic).
pub const panic = std.debug.FullPanic(restoreThenPanic);

fn restoreThenPanic(msg: []const u8, first_trace_addr: ?usize) noreturn {
    abbey.tui_term.emergencyRestore();
    std.debug.defaultPanic(msg, first_trace_addr);
}

pub fn main(init: std.process.Init) !void {
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator()); // std: lib/std/process/Args.zig
    const cwd = try std.process.currentPathAlloc(io, init.arena.allocator()); // std: lib/std/process.zig

    var out_buf: [64 * 1024]u8 = undefined;
    var err_buf: [4096]u8 = undefined;
    // Streaming (not positional) writers: with `> log 2>&1` both streams share
    // one file, and positional writes at offset 0 would overwrite each other.
    var out = Io.File.stdout().writerStreaming(io, &out_buf); // std: lib/std/Io/File.zig
    var err = Io.File.stderr().writerStreaming(io, &err_buf);

    const argv = try init.arena.allocator().alloc([]const u8, args.len -| 1);
    for (argv, 1..) |*a, i| a.* = args[i];

    const ctx: abbey.ctx.Ctx = .{
        .gpa = init.gpa,
        .io = io,
        .env = init.environ_map,
        .out = &out.interface,
        .err = &err.interface,
        .cwd = cwd,
    };
    const code = abbey.dispatch.run(ctx, argv);
    out.interface.flush() catch {};
    err.interface.flush() catch {};
    if (code != 0) std.process.exit(code);
}
