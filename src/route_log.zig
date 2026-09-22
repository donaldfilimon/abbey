//! Append-only JSONL routing audit log, byte-compatible with the Rust
//! `route_log.rs` writer and readable by the Rust reader. Field order is the
//! serde declaration order; `correlation`/`stage`/`alternate`/`fallback` are
//! omitted when null (`skip_serializing_if = "Option::is_none"`).
const std = @import("std");
const Io = std.Io;
const json = @import("util/json.zig");
const fsx = @import("util/fsx.zig");

pub const Record = struct {
    ts: []const u8,
    cwd: []const u8,
    persona: []const u8,
    role: []const u8,
    model: []const u8,
    reason: []const u8,
    confidence: f32,
    tools: []const []const u8 = &.{},
    correlation: ?[]const u8 = null,
    stage: ?[]const u8 = null,
    alternate: ?[]const u8 = null,
    fallback: ?[]const u8 = null,
};

pub const file_name = "route.jsonl";

pub fn path(arena: std.mem.Allocator, state_dir: []const u8) error{OutOfMemory}![]const u8 {
    return std.fs.path.join(arena, &.{ state_dir, file_name });
}

/// Serialize one record as a JSON object (no trailing newline).
pub fn writeJson(w: *Io.Writer, r: *const Record) Io.Writer.Error!void {
    try w.writeAll("{\"ts\":");
    try json.writeString(w, r.ts);
    try w.writeAll(",\"cwd\":");
    try json.writeString(w, r.cwd);
    try w.writeAll(",\"persona\":");
    try json.writeString(w, r.persona);
    try w.writeAll(",\"role\":");
    try json.writeString(w, r.role);
    try w.writeAll(",\"model\":");
    try json.writeString(w, r.model);
    try w.writeAll(",\"reason\":");
    try json.writeString(w, r.reason);
    try w.writeAll(",\"confidence\":");
    try json.writeF32(w, r.confidence);
    try w.writeAll(",\"tools\":[");
    for (r.tools, 0..) |t, i| {
        if (i != 0) try w.writeByte(',');
        try json.writeString(w, t);
    }
    try w.writeByte(']');
    inline for (.{ "correlation", "stage", "alternate", "fallback" }) |name| {
        if (@field(r, name)) |v| {
            try w.writeAll(",\"" ++ name ++ "\":");
            try json.writeString(w, v);
        }
    }
    try w.writeByte('}');
}

pub const AppendError = fsx.WriteError || error{OutOfMemory};

pub fn append(gpa: std.mem.Allocator, io: Io, state_dir: []const u8, r: *const Record) AppendError!void {
    var aw: Io.Writer.Allocating = .init(gpa);
    defer aw.deinit();
    writeJson(&aw.writer, r) catch return error.OutOfMemory;
    aw.writer.writeByte('\n') catch return error.OutOfMemory;
    const p = try path(gpa, state_dir);
    defer gpa.free(p);
    try fsx.appendLocked(io, p, aw.written(), .default_file);
}

pub const ReadError = fsx.ReadError;

/// The newest `n` parseable records, oldest first. Unparseable lines are
/// skipped, as in Rust (`filter_map(serde_json::from_str(..).ok())`).
/// Everything is allocated in `arena`.
pub fn recent(arena: std.mem.Allocator, io: Io, state_dir: []const u8, n: usize) ReadError![]Record {
    const p = try path(arena, state_dir);
    const text = (try fsx.readOptional(io, arena, p, 256 * 1024 * 1024)) orelse return &.{};
    var out: std.ArrayList(Record) = .empty;
    var lines = std.mem.splitScalar(u8, text, '\n');
    while (lines.next()) |line| {
        if (std.mem.trim(u8, line, &std.ascii.whitespace).len == 0) continue;
        // std: lib/std/json/static.zig (parseFromSliceLeaky, ParseOptions)
        const rec = std.json.parseFromSliceLeaky(Record, arena, line, .{ .ignore_unknown_fields = true, .allocate = .alloc_always }) catch continue;
        try out.append(arena, rec);
    }
    const items = out.items;
    return if (items.len > n) items[items.len - n ..] else items;
}

fn orDash(v: ?[]const u8) []const u8 {
    return v orelse "-";
}

/// One-line display shared by CLI surfaces (Rust `format_route_line`).
pub fn formatLine(w: *Io.Writer, r: *const Record) Io.Writer.Error!void {
    try w.print("{s}\t{s}\t{s}\t{s}\t{d:.2}\t{s}\talt={s}\tfb={s}\t{s}", .{
        r.ts, r.persona, r.role, r.model, r.confidence, orDash(r.stage), orDash(r.alternate), orDash(r.fallback), r.reason,
    });
}

test "record JSON is byte-identical to a Rust-written route.jsonl line" {
    // Captured from `~/.local/bin/abbey ask` (Rust abbey 2.6.0) on 2026-09-21.
    const rust_line = "{\"ts\":\"2026-09-22T03:22:37Z\",\"cwd\":\"/private/tmp/abbey-zig-p1/work\",\"persona\":\"abbey\",\"role\":\"gemma\",\"model\":\"local\",\"reason\":\"persona=abbey role=gemma class=Code\",\"confidence\":0.95,\"tools\":[]}";
    const r: Record = .{
        .ts = "2026-09-22T03:22:37Z",
        .cwd = "/private/tmp/abbey-zig-p1/work",
        .persona = "abbey",
        .role = "gemma",
        .model = "local",
        .reason = "persona=abbey role=gemma class=Code",
        .confidence = 0.95,
    };
    var buf: [512]u8 = undefined;
    var w: Io.Writer = .fixed(&buf);
    try writeJson(&w, &r);
    try std.testing.expectEqualStrings(rust_line, w.buffered());
}

test "append and read back, optional fields round-trip, bad lines skipped" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    const a: Record = .{ .ts = "t1", .cwd = ".", .persona = "abbey", .role = "max", .model = "fable", .reason = "code heuristic", .confidence = 0.8 };
    try append(gpa, io, root, &a);
    const p = try path(gpa, root);
    defer gpa.free(p);
    try fsx.appendLocked(io, p, "not json\n\n", .default_file);
    const b: Record = .{ .ts = "t2", .cwd = ".", .persona = "abbey", .role = "max", .model = "fable", .reason = "hybrid", .confidence = 0.7, .tools = &.{"media"}, .alternate = "gemma", .fallback = "prefer hybrid-loop" };
    try append(gpa, io, root, &b);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const got = try recent(arena.allocator(), io, root, 5);
    try std.testing.expectEqual(@as(usize, 2), got.len);
    try std.testing.expectEqualStrings("max", got[0].role);
    try std.testing.expectEqualStrings("gemma", got[1].alternate.?);
    try std.testing.expectEqualStrings("media", got[1].tools[0]);
    const last = try recent(arena.allocator(), io, root, 1);
    try std.testing.expectEqualStrings("t2", last[0].ts);
    var buf: [512]u8 = undefined;
    var w: Io.Writer = .fixed(&buf);
    try formatLine(&w, &got[1]);
    try std.testing.expectEqualStrings("t2\tabbey\tmax\tfable\t0.70\t-\talt=gemma\tfb=prefer hybrid-loop\thybrid", w.buffered());
}
