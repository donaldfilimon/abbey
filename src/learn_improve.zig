//! `learn improve`: propose from routes + reflection + curation, apply only
//! the additive route promotion (port of Rust `learn::improve`).
const std = @import("std");
const learn = @import("learn.zig");
const mem = @import("memory/store.zig");
const route_log = @import("route_log.zig");
const fsx = @import("util/fsx.zig");
const bin = @import("edition.zig").id.binary_name;

pub const low_route_confidence: f32 = 0.6;

pub const Proposal = struct { kind: []const u8, detail: []const u8, applies: bool };

/// Union-find clusters over duplicate pairs, in first-seen order.
fn duplicateClusters(arena: std.mem.Allocator, pairs: []const [2][]const u8) error{OutOfMemory}![]const []const []const u8 {
    var ids: std.ArrayList([]const u8) = .empty;
    var parent: std.ArrayList(usize) = .empty;
    var index: std.StringHashMapUnmanaged(usize) = .empty;
    const root = struct {
        fn f(p: []usize, start: usize) usize {
            var i = start;
            while (p[i] != i) {
                p[i] = p[p[i]];
                i = p[i];
            }
            return i;
        }
    }.f;
    for (pairs) |pair| {
        var slots: [2]usize = undefined;
        for (pair, 0..) |id, k| {
            if (index.get(id)) |i| {
                slots[k] = i;
            } else {
                const i = ids.items.len;
                try ids.append(arena, id);
                try parent.append(arena, i);
                try index.put(arena, id, i);
                slots[k] = i;
            }
        }
        const ra = root(parent.items, slots[0]);
        const rb = root(parent.items, slots[1]);
        if (ra != rb) parent.items[@max(ra, rb)] = @min(ra, rb);
    }
    var roots: std.ArrayList(usize) = .empty;
    var clusters: std.ArrayList(std.ArrayList([]const u8)) = .empty;
    for (ids.items, 0..) |id, i| {
        const r = root(parent.items, i);
        const at = std.mem.findScalar(usize, roots.items, r) orelse blk: {
            try roots.append(arena, r);
            try clusters.append(arena, .empty);
            break :blk roots.items.len - 1;
        };
        try clusters.items[at].append(arena, id);
    }
    const out = try arena.alloc([]const []const u8, clusters.items.len);
    for (clusters.items, out) |c, *o| o.* = c.items;
    return out;
}

pub fn plan(arena: std.mem.Allocator, reflect: mem.Reflect, routes: []const route_log.Record, cur: ?learn.Curation) error{OutOfMemory}![]Proposal {
    var out: std.ArrayList(Proposal) = .empty;
    if (routes.len != 0) try out.append(arena, .{ .kind = "promote-routes", .detail = try std.fmt.allocPrint(arena, "promote {d} recent route record(s) into activity", .{routes.len}), .applies = true });
    const Group = struct { key: []const u8, count: usize, first: []const u8, last: []const u8 };
    var groups: std.ArrayList(Group) = .empty;
    for (routes) |r| {
        if (!(r.confidence < low_route_confidence)) continue;
        const key = try std.fmt.allocPrint(arena, "{s}/{s} -> {s} at conf {d:.2} ({s})", .{ r.persona, r.role, r.model, r.confidence, r.reason });
        for (groups.items) |*g| {
            if (std.mem.eql(u8, g.key, key)) {
                g.count += 1;
                g.last = r.ts;
                break;
            }
        } else try groups.append(arena, .{ .key = key, .count = 1, .first = r.ts, .last = r.ts });
    }
    for (groups.items) |g| {
        const when = if (g.count == 1) g.first else try std.fmt.allocPrint(arena, "x{d} {s}..{s}", .{ g.count, g.first, g.last });
        try out.append(arena, .{ .kind = "review-route", .detail = try std.fmt.allocPrint(arena, "{s} {s}; consider `{s} learn preference`", .{ when, g.key, bin }), .applies = false });
    }
    for (try duplicateClusters(arena, reflect.duplicate_summaries)) |cluster| {
        const shown = cluster[0..@min(3, cluster.len)];
        const more = cluster.len - shown.len;
        const tail = if (more > 0) try std.fmt.allocPrint(arena, " (+{d} more)", .{more}) else "";
        try out.append(arena, .{ .kind = "dedupe", .detail = try std.fmt.allocPrint(arena, "{d} memories share one summary: {s}{s}", .{ cluster.len, try std.mem.join(arena, ", ", shown), tail }), .applies = false });
    }
    for (reflect.low_confidence) |id| try out.append(arena, .{ .kind = "low-confidence", .detail = try std.fmt.allocPrint(arena, "memory {s} is low confidence; confirm or correct it", .{id}), .applies = false });
    for (reflect.superseded) |id| try out.append(arena, .{ .kind = "superseded", .detail = try std.fmt.allocPrint(arena, "memory {s} is superseded; retire it by hand if stale", .{id}), .applies = false });
    if (cur) |c| if (c.total > c.with_provenance) {
        try out.append(arena, .{ .kind = "provenance", .detail = try std.fmt.allocPrint(arena, "{d} train_candidate record(s) lack provenance; see `{s} learn review`", .{ c.total - c.with_provenance, bin }), .applies = false });
    };
    return out.items;
}

pub fn run(e: learn.Env, n: usize, apply: bool) learn.Error!usize {
    const w = e.ctx.out;
    try w.print("{s} learn improve ({s}): propose, then apply only additive steps\n\n", .{ bin, if (apply) "apply" else "dry-run" });
    const routes = try route_log.recent(e.arena, e.ctx.io, e.state_dir, n);
    var reflect: mem.Reflect = .{};
    var cur: ?learn.Curation = null;
    if (fsx.exists(e.ctx.io, try mem.pathFor(e.arena, e.state_dir))) {
        const s = try mem.open(e.ctx.gpa, e.ctx.io, e.arena, e.state_dir);
        cur = try learn.curation(e);
        reflect = try s.reflect(e.arena);
    }
    const proposals = try plan(e.arena, reflect, routes, cur);
    if (proposals.len == 0) {
        try w.writeAll("(no proposals: no routes, reflect issues, or curation gaps)\n");
        return 0;
    }
    for (proposals) |p| try w.print("[{s}] {s:<15} {s}\n", .{ if (p.applies) "auto" else "review", p.kind, p.detail });
    var applied: usize = 0;
    if (apply) {
        for (proposals) |p| if (std.mem.eql(u8, p.kind, "promote-routes")) {
            applied += try learn.learnFromRoutes(e, n);
            break;
        };
        try w.print("\napplied: {d} record(s) promoted; review items unchanged\n", .{applied});
    } else {
        try w.print("\nnext: `{s} learn improve {d} --apply` runs the [auto] steps only\n", .{ bin, n });
    }
    return applied;
}

test "plan groups low-confidence routes and clusters duplicates" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const r = route_log.Record{ .ts = "t1", .cwd = ".", .persona = "abbey", .role = "max", .model = "m", .reason = "other", .confidence = 0.55 };
    var r2 = r;
    r2.ts = "t2";
    const reflect: mem.Reflect = .{ .duplicate_summaries = &.{ .{ "a", "b" }, .{ "b", "c" }, .{ "x", "y" } }, .low_confidence = &.{"lc"} };
    const p = try plan(a, reflect, &.{ r, r2 }, .{ .total = 2, .with_provenance = 1, .high_confidence = 0, .ready = 0 });
    try std.testing.expectEqualStrings("promote-routes", p[0].kind);
    try std.testing.expect(p[0].applies);
    try std.testing.expectEqualStrings("review-route", p[1].kind);
    try std.testing.expect(std.mem.startsWith(u8, p[1].detail, "x2 t1..t2 "));
    try std.testing.expectEqualStrings("dedupe", p[2].kind);
    try std.testing.expect(std.mem.startsWith(u8, p[2].detail, "3 memories share one summary: a, b, c"));
    try std.testing.expectEqualStrings("dedupe", p[3].kind);
    try std.testing.expectEqualStrings("low-confidence", p[4].kind);
    try std.testing.expectEqualStrings("provenance", p[5].kind);
    try std.testing.expectEqual(@as(usize, 6), p.len);
}
