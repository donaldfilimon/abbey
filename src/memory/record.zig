//! Memory record: the Rust `MemoryRecord` serde shape, one JSON object per
//! JSONL line. Field order and null handling match serde_json exactly, so
//! `learn export` output is interchangeable with the Rust tree's.
//!
//! Mapping from the Rust SQLite `memory` table (src/memory/sqlite.rs):
//! every column maps 1:1 to a JSON field of the same name, except
//! `tags_json TEXT` -> `tags` (a JSON array, not a string) and
//! `obsolete INTEGER` -> `obsolete` (a JSON bool). The `memory_embeddings`
//! table (learned vectors) has no counterpart: learned embeddings are
//! Proposed. The `migration_N_*.sql` files in the Rust tree migrate
//! `runtime.sqlite` (conversations, runs, tool approvals, model operations),
//! not memory; that runtime store is Proposed here.
const std = @import("std");
const Io = std.Io;
const json = @import("../util/json.zig");

pub const Record = struct {
    id: []const u8,
    source_type: []const u8 = "session",
    source_ref: []const u8 = "",
    project: []const u8 = "",
    timestamp: []const u8,
    origin: []const u8 = "system",
    payload: []const u8,
    summary: []const u8,
    tags: []const []const u8 = &.{},
    embedding_ref: ?[]const u8 = null,
    confidence: f32 = 0.7,
    provenance: []const u8 = "abbey session",
    /// stm | ltm | activity | train_candidate
    retention: []const u8 = "stm",
    supersedes: ?[]const u8 = null,
    classification: []const u8 = "internal",
    obsolete: bool = false,

    pub fn hasTag(self: *const Record, tag: []const u8) bool {
        for (self.tags) |t| if (std.mem.eql(u8, t, tag)) return true;
        return false;
    }
};

fn optString(w: *Io.Writer, v: ?[]const u8) Io.Writer.Error!void {
    if (v) |s| try json.writeString(w, s) else try w.writeAll("null");
}

pub fn writeJson(w: *Io.Writer, r: *const Record) Io.Writer.Error!void {
    try w.writeAll("{\"id\":");
    try json.writeString(w, r.id);
    try w.writeAll(",\"source_type\":");
    try json.writeString(w, r.source_type);
    try w.writeAll(",\"source_ref\":");
    try json.writeString(w, r.source_ref);
    try w.writeAll(",\"project\":");
    try json.writeString(w, r.project);
    try w.writeAll(",\"timestamp\":");
    try json.writeString(w, r.timestamp);
    try w.writeAll(",\"origin\":");
    try json.writeString(w, r.origin);
    try w.writeAll(",\"payload\":");
    try json.writeString(w, r.payload);
    try w.writeAll(",\"summary\":");
    try json.writeString(w, r.summary);
    try w.writeAll(",\"tags\":[");
    for (r.tags, 0..) |t, i| {
        if (i != 0) try w.writeByte(',');
        try json.writeString(w, t);
    }
    try w.writeAll("],\"embedding_ref\":");
    try optString(w, r.embedding_ref);
    try w.writeAll(",\"confidence\":");
    try json.writeF32(w, r.confidence);
    try w.writeAll(",\"provenance\":");
    try json.writeString(w, r.provenance);
    try w.writeAll(",\"retention\":");
    try json.writeString(w, r.retention);
    try w.writeAll(",\"supersedes\":");
    try optString(w, r.supersedes);
    try w.writeAll(",\"classification\":");
    try json.writeString(w, r.classification);
    try w.writeAll(if (r.obsolete) ",\"obsolete\":true}" else ",\"obsolete\":false}");
}

/// Parse one line. Required fields match serde: everything except
/// `project`, `classification`, and `obsolete`, which carry serde defaults.
/// std: lib/std/json/static.zig (parseFromSliceLeaky)
pub fn parse(arena: std.mem.Allocator, line: []const u8) ?Record {
    const Wire = struct {
        id: []const u8,
        source_type: []const u8,
        source_ref: []const u8,
        project: []const u8 = "",
        timestamp: []const u8,
        origin: []const u8,
        payload: []const u8,
        summary: []const u8,
        tags: []const []const u8,
        embedding_ref: ?[]const u8,
        confidence: f32,
        provenance: []const u8,
        retention: []const u8,
        supersedes: ?[]const u8,
        classification: []const u8 = "internal",
        obsolete: bool = false,
    };
    const w = std.json.parseFromSliceLeaky(Wire, arena, line, .{ .ignore_unknown_fields = true, .allocate = .alloc_always }) catch return null;
    return .{
        .id = w.id,
        .source_type = w.source_type,
        .source_ref = w.source_ref,
        .project = w.project,
        .timestamp = w.timestamp,
        .origin = w.origin,
        .payload = w.payload,
        .summary = w.summary,
        .tags = w.tags,
        .embedding_ref = w.embedding_ref,
        .confidence = w.confidence,
        .provenance = w.provenance,
        .retention = w.retention,
        .supersedes = w.supersedes,
        .classification = w.classification,
        .obsolete = w.obsolete,
    };
}

test "record JSON has serde field order, nulls, and round-trips" {
    const r: Record = .{
        .id = "11111111-2222-4333-8444-555555555555",
        .timestamp = "2026-09-21T10:00:00Z",
        .payload = "p\n1",
        .summary = "s",
        .tags = &.{ "stm", "activity" },
        .project = "/proj",
    };
    var buf: [1024]u8 = undefined;
    var w: Io.Writer = .fixed(&buf);
    try writeJson(&w, &r);
    try std.testing.expectEqualStrings(
        "{\"id\":\"11111111-2222-4333-8444-555555555555\",\"source_type\":\"session\",\"source_ref\":\"\",\"project\":\"/proj\",\"timestamp\":\"2026-09-21T10:00:00Z\",\"origin\":\"system\",\"payload\":\"p\\n1\",\"summary\":\"s\",\"tags\":[\"stm\",\"activity\"],\"embedding_ref\":null,\"confidence\":0.7,\"provenance\":\"abbey session\",\"retention\":\"stm\",\"supersedes\":null,\"classification\":\"internal\",\"obsolete\":false}",
        w.buffered(),
    );
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const back = parse(arena.allocator(), w.buffered()).?;
    try std.testing.expectEqualStrings("p\n1", back.payload);
    try std.testing.expectEqual(@as(f32, 0.7), back.confidence);
    try std.testing.expect(back.hasTag("activity"));
    // A pre-`project` Rust export line (serde default) still parses.
    const legacy = "{\"id\":\"x\",\"source_type\":\"session\",\"source_ref\":\"\",\"timestamp\":\"t\",\"origin\":\"system\",\"payload\":\"p\",\"summary\":\"s\",\"tags\":[],\"embedding_ref\":null,\"confidence\":0.5,\"provenance\":\"\",\"retention\":\"stm\",\"supersedes\":null}";
    const l = parse(arena.allocator(), legacy).?;
    try std.testing.expectEqualStrings("", l.project);
    try std.testing.expectEqualStrings("internal", l.classification);
    try std.testing.expect(parse(arena.allocator(), "{\"id\":\"x\"}") == null);
}
