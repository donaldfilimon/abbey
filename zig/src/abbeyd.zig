//! abbeyd-zig entry point: the daemon role, equivalent to
//! `abbey-zig daemon serve`. Protocol v1, read-only, owner-only Unix socket.
const std = @import("std");
const abbey = @import("abbey");
const Io = std.Io; // std: lib/std/Io/File.zig (stderr writerStreaming)

pub fn main(init: std.process.Init) !void {
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator()); // std: lib/std/process/Args.zig
    var err_buf: [4096]u8 = undefined;
    var out_buf: [4096]u8 = undefined;
    var err = Io.File.stderr().writerStreaming(io, &err_buf);
    var out = Io.File.stdout().writerStreaming(io, &out_buf);
    const ctx: abbey.ctx.Ctx = .{
        .gpa = init.gpa,
        .io = io,
        .env = init.environ_map,
        .out = &out.interface,
        .err = &err.interface,
        .cwd = "/",
    };
    const code: u8 = blk: {
        if (args.len > 1 and (std.mem.eql(u8, args[1], "-h") or std.mem.eql(u8, args[1], "--help"))) {
            out.interface.writeAll(abbey.help.forCommand(.daemon)) catch {};
            break :blk 0;
        }
        if (args.len > 1) {
            err.interface.print("abbeyd: unexpected argument '{s}'\n", .{args[1]}) catch {};
            break :blk 2;
        }
        var arena: std.heap.ArenaAllocator = .init(init.gpa);
        defer arena.deinit();
        break :blk abbey.daemon_cli.serve(ctx, arena.allocator());
    };
    out.interface.flush() catch {};
    err.interface.flush() catch {};
    if (code != 0) std.process.exit(code);
}
