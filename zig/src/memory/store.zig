//! Zig-native append-only JSONL memory store (Decision 2, option b).
//!
//! `<state>/memory/memory.jsonl` holds one full `Record` snapshot per line.
//! Nothing is rewritten or deleted: store/update/promote/invalidate append a
//! new snapshot, and readers fold by `id`, last line wins. Obsolete records
//! stay in the log as provenance (the Rust no-silent-deletes rule).
//! Ordering matches the Rust SQLite queries: reads are `timestamp` DESC,
//! `all` is ASC; equal timestamps order by first append (newest first for
//! DESC reads).
const std = @import("std");
const Io = std.Io;
const rec = @import("record.zig");
const fsx = @import("../util/fsx.zig");

pub const Record = rec.Record;

pub const Error = fsx.ReadError || fsx.WriteError || error{ OutOfMemory, NotFound, DuplicateId, MissingProvenance };

pub const Filter = struct {
    retention: ?[]const u8 = null,
    tag: ?[]const u8 = null,
    source_type: ?[]const u8 = null,
    source_ref: ?[]const u8 = null,
    project: ?[]const u8 = null,

    pub fn matches(f: Filter, r: *const Record) bool {
        if (f.retention) |v| if (!std.mem.eql(u8, r.retention, v)) return false;
        if (f.tag) |v| if (!r.hasTag(v)) return false;
        if (f.source_type) |v| if (!std.mem.eql(u8, r.source_type, v)) return false;
        if (f.source_ref) |v| if (!std.mem.eql(u8, r.source_ref, v)) return false;
        if (f.project) |v| if (!std.mem.eql(u8, r.project, v)) return false;
        return true;
    }
};

pub const Reflect = struct {
    duplicate_summaries: []const [2][]const u8 = &.{},
    low_confidence: []const []const u8 = &.{},
    superseded: []const []const u8 = &.{},
};

pub fn pathFor(arena: std.mem.Allocator, state_dir: []const u8) error{OutOfMemory}![]const u8 {
    return std.fs.path.join(arena, &.{ state_dir, "memory", "memory.jsonl" });
}

pub const Store = struct {
    io: Io,
    gpa: std.mem.Allocator,
    path: []const u8,

    /// Rust `validate_train`: train_candidate requires non-empty provenance.
    fn validate(r: *const Record) Error!void {
        if (std.mem.eql(u8, r.retention, "train_candidate") and std.mem.trim(u8, r.provenance, &std.ascii.whitespace).len == 0) return error.MissingProvenance;
    }

    fn appendSnapshot(self: Store, r: *const Record) Error!void {
        try validate(r);
        var aw: Io.Writer.Allocating = .init(self.gpa);
        defer aw.deinit();
        rec.writeJson(&aw.writer, r) catch return error.OutOfMemory;
        aw.writer.writeByte('\n') catch return error.OutOfMemory;
        try fsx.appendLocked(self.io, self.path, aw.written(), .default_file);
    }

    const Entry = struct { r: Record, first: usize };

    /// Fold the log into the latest snapshot per id, in first-append order.
    fn fold(self: Store, arena: std.mem.Allocator) Error![]Entry {
        const text = (try fsx.readOptional(self.io, arena, self.path, 1024 * 1024 * 1024)) orelse return &.{};
        var list: std.ArrayList(Entry) = .empty;
        var index: std.StringHashMapUnmanaged(usize) = .empty;
        var lines = std.mem.splitScalar(u8, text, '\n');
        while (lines.next()) |line| {
            if (std.mem.trim(u8, line, &std.ascii.whitespace).len == 0) continue;
            const r = rec.parse(arena, line) orelse continue;
            if (index.get(r.id)) |i| {
                list.items[i].r = r;
            } else {
                try index.put(arena, r.id, list.items.len);
                try list.append(arena, .{ .r = r, .first = list.items.len });
            }
        }
        return list.items;
    }

    fn descLess(_: void, a: Entry, b: Entry) bool {
        const o = std.mem.order(u8, a.r.timestamp, b.r.timestamp);
        return if (o == .eq) a.first > b.first else o == .gt;
    }

    fn ascLess(_: void, a: Entry, b: Entry) bool {
        const o = std.mem.order(u8, a.r.timestamp, b.r.timestamp);
        return if (o == .eq) a.first < b.first else o == .lt;
    }

    pub fn get(self: Store, arena: std.mem.Allocator, id: []const u8) Error!?Record {
        for (try self.fold(arena)) |e| if (std.mem.eql(u8, e.r.id, id)) return e.r;
        return null;
    }

    /// Insert a new record; an existing id is refused (SQLite PRIMARY KEY).
    pub fn store(self: Store, arena: std.mem.Allocator, r: Record) Error!void {
        if (try self.get(arena, r.id) != null) return error.DuplicateId;
        try self.appendSnapshot(&r);
    }

    pub fn update(self: Store, arena: std.mem.Allocator, r: Record) Error!void {
        if (try self.get(arena, r.id) == null) return error.NotFound;
        try self.appendSnapshot(&r);
    }

    /// Mark obsolete; never deletes.
    pub fn invalidate(self: Store, arena: std.mem.Allocator, id: []const u8) Error!void {
        var r = (try self.get(arena, id)) orelse return error.NotFound;
        r.obsolete = true;
        try self.appendSnapshot(&r);
    }

    pub fn promote(self: Store, arena: std.mem.Allocator, id: []const u8, retention: []const u8) Error!void {
        var r = (try self.get(arena, id)) orelse return error.NotFound;
        r.retention = retention;
        if (!r.hasTag(retention)) r.tags = try std.mem.concat(arena, []const u8, &.{ r.tags, &.{retention} });
        try self.update(arena, r);
    }

    pub fn supersede(self: Store, arena: std.mem.Allocator, old_id: []const u8, new_rec: Record) Error!void {
        var n = new_rec;
        n.supersedes = old_id;
        try self.store(arena, n);
        try self.invalidate(arena, old_id);
    }

    /// Non-obsolete records matching `f`, newest first, at most `limit`.
    pub fn filterWith(self: Store, arena: std.mem.Allocator, f: Filter, limit: usize) Error![]Record {
        if (limit == 0) return &.{};
        const entries = try self.fold(arena);
        std.mem.sort(Entry, entries, {}, descLess);
        var out: std.ArrayList(Record) = .empty;
        for (entries) |e| {
            if (e.r.obsolete or !f.matches(&e.r)) continue;
            try out.append(arena, e.r);
            if (out.items.len >= limit) break;
        }
        return out.items;
    }

    pub fn filter(self: Store, arena: std.mem.Allocator, retention: ?[]const u8, tag: ?[]const u8, limit: usize) Error![]Record {
        return self.filterWith(arena, .{ .retention = retention, .tag = tag }, limit);
    }

    /// Every record including obsolete ones, oldest first (migration view).
    pub fn allIncludingObsolete(self: Store, arena: std.mem.Allocator) Error![]Record {
        const entries = try self.fold(arena);
        std.mem.sort(Entry, entries, {}, ascLess);
        const out = try arena.alloc(Record, entries.len);
        for (entries, out) |e, *o| o.* = e.r;
        return out;
    }

    /// Case-insensitive substring over summary/payload/provenance, newest first.
    pub fn searchKeyword(self: Store, arena: std.mem.Allocator, query: []const u8, limit: usize) Error![]Record {
        const needle = try std.ascii.allocLowerString(arena, query);
        var out: std.ArrayList(Record) = .empty;
        for (try self.filterWith(arena, .{}, std.math.maxInt(usize))) |r| {
            if (out.items.len >= limit) break;
            for ([_][]const u8{ r.summary, r.payload, r.provenance }) |field| {
                if (std.ascii.findIgnoreCase(field, needle) != null) {
                    try out.append(arena, r);
                    break;
                }
            }
        }
        return out.items;
    }

    pub fn reflect(self: Store, arena: std.mem.Allocator) Error!Reflect {
        return reflectOver(arena, try self.filter(arena, null, null, 500));
    }
};

pub fn open(gpa: std.mem.Allocator, io: Io, arena: std.mem.Allocator, state_dir: []const u8) error{OutOfMemory}!Store {
    return .{ .io = io, .gpa = gpa, .path = try pathFor(arena, state_dir) };
}

fn charPrefix(s: []const u8, n: usize) []const u8 {
    var count: usize = 0;
    var i: usize = 0;
    while (i < s.len) {
        if (count == n) return s[0..i];
        const len = std.unicode.utf8ByteSequenceLength(s[i]) catch 1;
        i += @min(len, s.len - i);
        count += 1;
    }
    return s;
}

fn charCount(s: []const u8) usize {
    return std.unicode.utf8CountCodepoints(s) catch s.len;
}

/// Shared reflection pass (Rust `reflect_over`): reports, never deletes.
pub fn reflectOver(arena: std.mem.Allocator, all: []const Record) error{OutOfMemory}!Reflect {
    var low: std.ArrayList([]const u8) = .empty;
    var sup: std.ArrayList([]const u8) = .empty;
    var dups: std.ArrayList([2][]const u8) = .empty;
    for (all) |r| {
        if (r.confidence < 0.4) try low.append(arena, r.id);
        if (r.supersedes != null or r.obsolete) try sup.append(arena, r.id);
    }
    for (all, 0..) |a, i| {
        for (all[i + 1 ..]) |b| {
            if (std.mem.eql(u8, a.source_type, "route") or std.mem.eql(u8, b.source_type, "route")) continue;
            const pa = charPrefix(a.summary, 24);
            const pb = charPrefix(b.summary, 24);
            if (charCount(pa) >= 12 and std.mem.eql(u8, pa, pb) and std.mem.eql(u8, a.payload, b.payload)) {
                try dups.append(arena, .{ a.id, b.id });
            }
        }
    }
    return .{ .duplicate_summaries = dups.items, .low_confidence = low.items, .superseded = sup.items };
}

fn testRec(id: []const u8, ts: []const u8, retention: []const u8) Record {
    return .{ .id = id, .timestamp = ts, .payload = "p", .summary = "s", .retention = retention };
}

test "append-only fold: last snapshot wins, obsolete is retained not deleted" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const s = try open(gpa, io, a, root);
    try s.store(a, testRec("a", "2026-01-01T00:00:00Z", "activity"));
    try s.store(a, testRec("b", "2026-01-02T00:00:00Z", "stm"));
    try s.store(a, testRec("c", "2026-01-02T00:00:00Z", "stm"));
    try std.testing.expectError(error.DuplicateId, s.store(a, testRec("a", "t", "stm")));
    try s.promote(a, "a", "ltm");
    try s.invalidate(a, "b");
    const live = try s.filterWith(a, .{}, 10);
    try std.testing.expectEqual(@as(usize, 2), live.len);
    try std.testing.expectEqualStrings("c", live[0].id); // newest first
    try std.testing.expectEqualStrings("ltm", live[1].retention);
    try std.testing.expect(live[1].hasTag("ltm"));
    const all = try s.allIncludingObsolete(a);
    try std.testing.expectEqual(@as(usize, 3), all.len);
    try std.testing.expectEqualStrings("a", all[0].id);
    try std.testing.expect(all[1].obsolete);
    const text = (try fsx.readOptional(io, a, s.path, 1 << 20)).?;
    try std.testing.expectEqual(@as(usize, 5), std.mem.count(u8, text, "\n"));
    try std.testing.expectError(error.NotFound, s.invalidate(a, "zzz"));
    var t = testRec("t", "t", "train_candidate");
    t.provenance = " ";
    try std.testing.expectError(error.MissingProvenance, s.store(a, t));
}

test "keyword search is case-insensitive over summary, payload, provenance" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const s = try open(gpa, io, a, root);
    var x = testRec("x", "2026-01-01T00:00:00Z", "ltm");
    x.summary = "WDBX checkpoint";
    try s.store(a, x);
    var y = testRec("y", "2026-01-02T00:00:00Z", "ltm");
    y.provenance = "user correction @ /Proj";
    try s.store(a, y);
    try std.testing.expectEqualStrings("x", (try s.searchKeyword(a, "wdbx", 10))[0].id);
    try std.testing.expectEqualStrings("y", (try s.searchKeyword(a, "proj", 10))[0].id);
    try std.testing.expectEqual(@as(usize, 0), (try s.searchKeyword(a, "absent", 10)).len);
    try s.invalidate(a, "x");
    try std.testing.expectEqual(@as(usize, 0), (try s.searchKeyword(a, "wdbx", 10)).len);
}

test "reflect ignores route duplicates and requires payload equality" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var r1 = testRec("1", "t", "activity");
    r1.summary = "route abbey/max -> gemma4";
    r1.source_type = "route";
    var r2 = r1;
    r2.id = "2";
    var r3 = r1;
    r3.id = "3";
    r3.confidence = 0.3;
    const rep = try reflectOver(a, &.{ r1, r2, r3 });
    try std.testing.expectEqual(@as(usize, 0), rep.duplicate_summaries.len);
    try std.testing.expectEqual(@as(usize, 1), rep.low_confidence.len);
    var s1 = testRec("s1", "t", "activity");
    s1.summary = "prefer small diffs";
    var s2 = s1;
    s2.id = "s2";
    try std.testing.expectEqual(@as(usize, 1), (try reflectOver(a, &.{ s1, s2 })).duplicate_summaries.len);
    s2.payload = "different";
    try std.testing.expectEqual(@as(usize, 0), (try reflectOver(a, &.{ s1, s2 })).duplicate_summaries.len);
}
