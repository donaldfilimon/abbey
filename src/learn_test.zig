//! Integration tests for the learn pipeline against a temp state dir.
const std = @import("std");
const learn = @import("learn.zig");
const route_log = @import("route_log.zig");
const mem = @import("memory/store.zig");
const T = @import("ctx.zig").TestCtx;

const Fixture = struct {
    tmp: std.testing.TmpDir,
    root: []u8,
    t: T,
    arena: std.heap.ArenaAllocator,

    fn init() !*Fixture {
        const gpa = std.testing.allocator;
        const f = try gpa.create(Fixture);
        f.tmp = std.testing.tmpDir(.{});
        f.root = try @import("ctx.zig").tmpPath(gpa, std.testing.io, f.tmp.dir);
        f.t = T.init(gpa, f.root);
        try f.t.env.put("PATH", "/usr/bin:/bin:/opt/homebrew/bin");
        f.arena = .init(gpa);
        return f;
    }

    fn deinit(f: *Fixture) void {
        const gpa = std.testing.allocator;
        f.arena.deinit();
        f.t.deinit();
        gpa.free(f.root);
        f.tmp.cleanup();
        gpa.destroy(f);
    }

    fn env(f: *Fixture) learn.Env {
        return .{ .ctx = f.t.ctx(std.testing.allocator, std.testing.io), .arena = f.arena.allocator(), .state_dir = f.root, .cwd = f.root };
    }
};

test "learn routes keeps alternate/fallback and is idempotent over the same tail" {
    const f = try Fixture.init();
    defer f.deinit();
    const e = f.env();
    const gpa = std.testing.allocator;
    const r: route_log.Record = .{ .ts = "t1", .cwd = ".", .persona = "abbey", .role = "max", .model = "fable", .reason = "hybrid", .confidence = 0.7, .alternate = "gemma", .fallback = "prefer hybrid-loop" };
    try route_log.append(gpa, std.testing.io, f.root, &r);
    for ([_][]const u8{ "t2", "t3" }) |ts| {
        var x = r;
        x.ts = ts;
        try route_log.append(gpa, std.testing.io, f.root, &x);
    }
    try std.testing.expectEqual(@as(usize, 3), try learn.learnFromRoutes(e, 5));
    try std.testing.expectEqual(@as(usize, 0), try learn.learnFromRoutes(e, 5));
    const s = try mem.open(gpa, std.testing.io, e.arena, f.root);
    const rows = try s.filter(e.arena, "activity", "self-learn", 100);
    try std.testing.expectEqual(@as(usize, 3), rows.len);
    try std.testing.expect(std.mem.find(u8, rows[0].payload, "alternate=gemma") != null);
    try std.testing.expect(std.mem.find(u8, rows[0].payload, "fallback=prefer hybrid-loop") != null);
    try std.testing.expect(std.mem.find(u8, rows[0].payload, "confidence=0.70") != null);
}

test "learn correction, train, preference, stats, review, export, status" {
    const f = try Fixture.init();
    defer f.deinit();
    const e = f.env();
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{}));
    try std.testing.expect(std.mem.find(u8, f.t.outText(), "(empty: capture corrections") != null);
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{ "correction", "prefer", "small", "diffs" }));
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{ "train", "curated", "example" }));
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{ "preference", "answer", "tersely" }));
    try std.testing.expectError(error.Usage, learn.dispatch(e, &.{"train"}));
    try std.testing.expectEqual(@as(u8, 2), try learn.dispatch(e, &.{"lora"}));
    const c = try learn.curation(e);
    try std.testing.expectEqual(@as(usize, 1), c.total);
    try std.testing.expectEqual(@as(usize, 1), c.ready);
    const ctx_text = try learn.preferenceContext(e, 8);
    try std.testing.expect(std.mem.find(u8, ctx_text, "- answer tersely\n") != null);
    f.t.out.clearRetainingCapacity();
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{"status"}));
    const out = f.t.outText();
    try std.testing.expect(std.mem.find(u8, out, "train_candidate: total=1 prov_ok=1 missing=0 high_conf=1 ready=1") != null);
    try std.testing.expect(std.mem.find(u8, out, "ltm              self-learn=2") != null);
    f.t.out.clearRetainingCapacity();
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{ "export", "train_candidate" }));
    try std.testing.expect(std.mem.find(u8, f.t.outText(), "\"retention\":\"train_candidate\"") != null);
    f.t.out.clearRetainingCapacity();
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{"review"}));
    try std.testing.expect(std.mem.find(u8, f.t.outText(), "review: 1 candidate(s); 0 missing provenance; 1 ready") != null);
    f.t.out.clearRetainingCapacity();
    try std.testing.expectEqual(@as(u8, 0), try learn.dispatch(e, &.{"digest"}));
    try std.testing.expect(std.mem.startsWith(u8, f.t.outText(), "digest: promoted=0"));
}
