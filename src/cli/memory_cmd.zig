//! `memory search|similar <query>` over the JSONL store.
const std = @import("std");
const Ctx = @import("../ctx.zig").Ctx;
const mem = @import("../memory/store.zig");
const similarity = @import("../memory/similarity.zig");

pub const Error = mem.Error || std.Io.Writer.Error;

pub fn run(ctx: Ctx, arena: std.mem.Allocator, state_dir: []const u8, args: []const []const u8) Error!u8 {
    if (args.len < 2 or !(std.mem.eql(u8, args[0], "search") or std.mem.eql(u8, args[0], "similar"))) {
        try ctx.err.writeAll("usage: memory <search|similar> <query>...\n");
        return 2;
    }
    const query = try std.mem.join(arena, " ", args[1..]);
    const s = try mem.open(ctx.gpa, ctx.io, arena, state_dir);
    if (std.mem.eql(u8, args[0], "search")) {
        const rows = try s.searchKeyword(arena, query, 20);
        if (rows.len == 0) try ctx.out.writeAll("(no matches)\n");
        for (rows) |r| try ctx.out.print("{s}\t{s}\t{s}\n", .{ r.id, r.retention, r.summary });
        return 0;
    }
    const q = similarity.embedBytes(query);
    const hits = try similarity.rank(arena, try s.filterWith(arena, .{}, 1000), &q, 10);
    if (hits.len == 0) try ctx.out.writeAll("(no records)\n");
    for (hits) |h| try ctx.out.print("{d:.3}\t{s}\t{s}\n", .{ h.score, h.record.id, h.record.summary });
    return 0;
}
