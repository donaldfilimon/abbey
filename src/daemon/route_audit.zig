//! Sanitized read-only projection of route.jsonl for the daemon, ported from
//! the Rust `app_core/routes.rs`. The on-disk record carries a raw absolute
//! `cwd` and free-text `reason`; neither may cross the socket.
//!
//! * `cwd` becomes `ws-<12 lowercase hex>`: the first 6 bytes of
//!   SHA-256(`"abbey:route-audit-workspace:v1\0"` ++ trimmed cwd).
//! * every free-text field is split on Unicode whitespace, a token that looks
//!   like a filesystem path (or contains the daemon's HOME) becomes `[path]`,
//!   other tokens lose Cc code points, and the result is truncated on a UTF-8
//!   boundary (64 bytes per field, 240 for the reason).
//! * `confidence` is quantized to a whole percent, clamped, NaN/inf -> 0.
//! * a page carries at most 50 entries; a record whose sanitized entry fails
//!   validation is dropped, never emitted (the Rust client re-validates every
//!   page and rejects the whole page on one bad entry).
const std = @import("std");
const Io = std.Io;
const text = @import("text.zig");
const json = @import("../util/json.zig");
const route_log = @import("../route_log.zig");
const fsx = @import("../util/fsx.zig");
const Sha256 = std.crypto.hash.sha2.Sha256; // std: lib/std/crypto/sha2.zig

pub const max_page: u16 = 50;
const max_field = 64;
const max_reason = 240;
const max_timestamp = 64;
const max_tools = 8;
const digest_hex = 12;
const workspace_prefix = "ws-";
const workspace_domain = "abbey:route-audit-workspace:v1\x00";

pub const Entry = struct {
    recorded_at: []const u8,
    workspace: ?[]const u8 = null,
    persona: []const u8,
    role: []const u8,
    model: []const u8,
    confidence_percent: u8,
    reason: []const u8,
    stage: ?[]const u8 = null,
    correlation: ?[]const u8 = null,
    alternate: ?[]const u8 = null,
    fallback: ?[]const u8 = null,
    tools: []const []const u8 = &.{},
};

pub const Page = struct {
    entries: []const Entry,
    limit: u16,

    pub fn returned(p: Page) u16 {
        return @intCast(p.entries.len);
    }
};

/// `ws-` + 12 hex of the domain-separated digest, or null for a blank cwd.
pub fn workspaceDigest(buf: *[workspace_prefix.len + digest_hex]u8, cwd: []const u8) ?[]const u8 {
    const trimmed = text.trim(cwd);
    if (trimmed.len == 0) return null;
    var h = Sha256.init(.{});
    h.update(workspace_domain);
    h.update(trimmed);
    var out: [Sha256.digest_length]u8 = undefined;
    h.final(&out);
    @memcpy(buf[0..workspace_prefix.len], workspace_prefix);
    const hex = "0123456789abcdef";
    for (out[0 .. digest_hex / 2], 0..) |b, i| {
        buf[workspace_prefix.len + 2 * i] = hex[b >> 4];
        buf[workspace_prefix.len + 2 * i + 1] = hex[b & 0x0f];
    }
    return buf;
}

/// Rust `(confidence * 100.0).round().clamp(0.0, 100.0) as u8`, NaN/inf -> 0.
pub fn quantize(confidence: f32) u8 {
    if (!std.math.isFinite(confidence)) return 0;
    const v = std.math.clamp(@round(confidence * 100.0), 0.0, 100.0);
    return @intFromFloat(v);
}

fn segmentIsPath(raw: []const u8) bool {
    const seg = std.mem.trim(u8, raw, "\"'()[],;");
    if (std.mem.startsWith(u8, seg, "/") or std.mem.startsWith(u8, seg, "~") or std.mem.startsWith(u8, seg, "\\\\")) return true;
    return seg.len >= 3 and std.ascii.isAlphabetic(seg[0]) and seg[1] == ':' and (seg[2] == '/' or seg[2] == '\\');
}

/// A token every consumer must reject: tested whole first, then per `=`/`:`
/// segment (Abbey reasons are `key=value` shaped).
pub fn isStructuralPath(token: []const u8) bool {
    if (segmentIsPath(token)) return true;
    var it = std.mem.tokenizeAny(u8, token, "=:");
    // Rust `split` keeps empty segments; an empty segment is never a path.
    while (it.next()) |seg| if (segmentIsPath(seg)) return true;
    return false;
}

/// The daemon's HOME (or USERPROFILE) with trailing separators trimmed, when
/// longer than 3 bytes; a token containing it is redacted on the producer side.
pub fn homeMarker(home: ?[]const u8, userprofile: ?[]const u8) ?[]const u8 {
    for ([_]?[]const u8{ home, userprofile }) |v| {
        const t = std.mem.trimEnd(u8, v orelse continue, "/\\");
        if (t.len > 3) return t;
    }
    return null;
}

/// Collapse whitespace, redact paths, drop controls, truncate on a boundary.
pub fn sanitizeText(arena: std.mem.Allocator, raw: []const u8, max: usize, home: ?[]const u8) error{OutOfMemory}![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    var toks = text.tokens(raw);
    while (toks.next()) |tok| {
        const redact = isStructuralPath(tok) or (if (home) |h| std.mem.find(u8, tok, h) != null else false);
        const start = out.items.len;
        if (out.items.len != 0) try out.append(arena, ' ');
        const before = out.items.len;
        if (redact) {
            try out.appendSlice(arena, "[path]");
        } else {
            const view = std.unicode.Utf8View.init(tok) catch {
                out.shrinkRetainingCapacity(start);
                continue;
            };
            var it = view.iterator();
            while (it.nextCodepointSlice()) |cs| {
                const cp = std.unicode.utf8Decode(cs) catch continue;
                if (!text.isControl(cp)) try out.appendSlice(arena, cs);
            }
        }
        if (out.items.len == before) out.shrinkRetainingCapacity(start);
    }
    return text.truncateOnBoundary(out.items, max);
}

fn sanitizeOptional(arena: std.mem.Allocator, v: ?[]const u8, home: ?[]const u8) error{OutOfMemory}!?[]const u8 {
    const s = try sanitizeText(arena, v orelse return null, max_field, home);
    return if (s.len == 0) null else s;
}

fn validField(v: []const u8, max: usize) bool {
    if (v.len == 0 or v.len > max or text.hasControl(v)) return false;
    var toks = text.tokens(v);
    while (toks.next()) |tok| if (isStructuralPath(tok)) return false;
    return true;
}

fn validWorkspace(v: []const u8) bool {
    if (!std.mem.startsWith(u8, v, workspace_prefix) or v.len != workspace_prefix.len + digest_hex) return false;
    for (v[workspace_prefix.len..]) |c| if (!(std.ascii.isDigit(c) or (c >= 'a' and c <= 'f'))) return false;
    return true;
}

/// Rust `RouteAuditEntry::validate`, the consumer-side wire invariant.
pub fn validEntry(e: *const Entry) bool {
    if (e.recorded_at.len == 0 or e.recorded_at.len > max_timestamp or !text.isRfc3339(e.recorded_at)) return false;
    if (e.workspace) |w| if (!validWorkspace(w)) return false;
    if (!validField(e.persona, max_field) or !validField(e.role, max_field) or !validField(e.model, max_field)) return false;
    if (!validField(e.reason, max_reason)) return false;
    for ([_]?[]const u8{ e.stage, e.correlation, e.alternate, e.fallback }) |o| {
        if (o) |v| if (!validField(v, max_field)) return false;
    }
    if (e.confidence_percent > 100 or e.tools.len > max_tools) return false;
    for (e.tools) |t| if (!validField(t, max_field)) return false;
    return true;
}

/// Rust `RouteAuditPage::validate`.
pub fn validPage(p: *const Page) bool {
    if (p.limit == 0 or p.limit > max_page or p.entries.len > p.limit) return false;
    for (p.entries) |*e| if (!validEntry(e)) return false;
    return true;
}

/// Project one record, or null when it cannot be represented honestly.
pub fn sanitizeRecord(arena: std.mem.Allocator, r: *const route_log.Record, home: ?[]const u8) error{OutOfMemory}!?Entry {
    const recorded_at = text.trim(r.ts);
    if (recorded_at.len == 0 or recorded_at.len > max_timestamp or !text.isRfc3339(recorded_at)) return null;
    const persona = try sanitizeText(arena, r.persona, max_field, home);
    const role = try sanitizeText(arena, r.role, max_field, home);
    const model = try sanitizeText(arena, r.model, max_field, home);
    if (persona.len == 0 or role.len == 0 or model.len == 0) return null;
    const reason_s = try sanitizeText(arena, r.reason, max_reason, home);
    var tools: std.ArrayList([]const u8) = .empty;
    for (r.tools) |t| {
        if (tools.items.len == max_tools) break;
        if (try sanitizeOptional(arena, t, home)) |s| try tools.append(arena, s);
    }
    const ws_buf = try arena.create([workspace_prefix.len + digest_hex]u8);
    const e: Entry = .{
        .recorded_at = try arena.dupe(u8, recorded_at),
        .workspace = workspaceDigest(ws_buf, r.cwd),
        .persona = persona,
        .role = role,
        .model = model,
        .confidence_percent = quantize(r.confidence),
        .reason = if (reason_s.len == 0) "(no reason recorded)" else reason_s,
        .stage = try sanitizeOptional(arena, r.stage, home),
        .correlation = try sanitizeOptional(arena, r.correlation, home),
        .alternate = try sanitizeOptional(arena, r.alternate, home),
        .fallback = try sanitizeOptional(arena, r.fallback, home),
        .tools = tools.items,
    };
    return if (validEntry(&e)) e else null;
}

/// Read the newest `limit` records under `state_dir` and sanitize them. Fails
/// soft to an empty page like Rust: a missing or unreadable log, or a log that
/// is not valid UTF-8 (Rust `read_to_string` fails), is "nothing audited".
/// Dropped records are not refilled from older lines.
pub fn readPage(arena: std.mem.Allocator, io: Io, state_dir: ?[]const u8, limit: u16, home: ?[]const u8) error{OutOfMemory}!Page {
    const root = state_dir orelse return .{ .entries = &.{}, .limit = limit };
    const p = try route_log.path(arena, root);
    const bytes = fsx.readOptional(io, arena, p, 256 * 1024 * 1024) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return .{ .entries = &.{}, .limit = limit },
    };
    if (bytes) |b| if (!std.unicode.utf8ValidateSlice(b)) return .{ .entries = &.{}, .limit = limit };
    const recs = route_log.recent(arena, io, root, limit) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return .{ .entries = &.{}, .limit = limit },
    };
    var out: std.ArrayList(Entry) = .empty;
    for (recs) |*r| if (try sanitizeRecord(arena, r, home)) |e| try out.append(arena, e);
    return .{ .entries = out.items, .limit = limit };
}

/// serde field order; `None` optionals and an empty `tools` are omitted.
pub fn writeEntry(w: *Io.Writer, e: *const Entry) Io.Writer.Error!void {
    try w.writeAll("{\"recorded_at\":");
    try json.writeString(w, e.recorded_at);
    if (e.workspace) |v| {
        try w.writeAll(",\"workspace\":");
        try json.writeString(w, v);
    }
    inline for (.{ "persona", "role", "model" }) |name| {
        try w.writeAll(",\"" ++ name ++ "\":");
        try json.writeString(w, @field(e, name));
    }
    try w.print(",\"confidence_percent\":{d},\"reason\":", .{e.confidence_percent});
    try json.writeString(w, e.reason);
    inline for (.{ "stage", "correlation", "alternate", "fallback" }) |name| {
        if (@field(e, name)) |v| {
            try w.writeAll(",\"" ++ name ++ "\":");
            try json.writeString(w, v);
        }
    }
    if (e.tools.len != 0) {
        try w.writeAll(",\"tools\":[");
        for (e.tools, 0..) |t, i| {
            if (i != 0) try w.writeByte(',');
            try json.writeString(w, t);
        }
        try w.writeByte(']');
    }
    try w.writeByte('}');
}

pub fn writePage(w: *Io.Writer, p: *const Page) Io.Writer.Error!void {
    try w.writeAll("{\"entries\":[");
    for (p.entries, 0..) |*e, i| {
        if (i != 0) try w.writeByte(',');
        try writeEntry(w, e);
    }
    try w.print("],\"returned\":{d},\"limit\":{d}}}", .{ p.returned(), p.limit });
}

fn testRecord(cwd: []const u8, reason: []const u8) route_log.Record {
    return .{ .ts = "2026-08-08T12:00:00Z", .cwd = cwd, .persona = "Abbey", .role = "max", .model = "fable", .reason = reason, .confidence = 0.82 };
}

test "the working directory becomes a pinned opaque digest and never a path" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // Pinned against Python: sha256(b"abbey:route-audit-workspace:v1\x00" + cwd)[:12].
    const e = (try sanitizeRecord(a, &testRecord("/tmp/project", "persona=Abbey"), null)).?;
    try std.testing.expectEqualStrings("ws-dd101579a2a2", e.workspace.?);
    const f = (try sanitizeRecord(a, &testRecord("/Users/someone/code/abbey", "x"), null)).?;
    try std.testing.expectEqualStrings("ws-15ae1bd8de88", f.workspace.?);
    try std.testing.expect((try sanitizeRecord(a, &testRecord("   ", "x"), null)).?.workspace == null);
}

test "free text is bounded, control-stripped, and path-redacted with Unicode whitespace" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const e = (try sanitizeRecord(a, &testRecord("/tmp/project", "persona=Abbey\x07\n role=max wrote /etc/passwd and C:\\Windows\\System32 and ~/.ssh/id_rsa"), null)).?;
    try std.testing.expect(!text.hasControl(e.reason));
    try std.testing.expectEqual(@as(usize, 3), std.mem.count(u8, e.reason, "[path]"));
    try std.testing.expect(std.mem.find(u8, e.reason, "persona=Abbey") != null);
    const keyed = (try sanitizeRecord(a, &testRecord("/tmp/p", "stage=gate log=/var/log/abbey.jsonl"), null)).?;
    // The whole token is replaced, as in Rust.
    try std.testing.expectEqualStrings("stage=gate [path]", keyed.reason);
    // NBSP and NEL separate tokens exactly as Rust split_whitespace does.
    const nbsp = (try sanitizeRecord(a, &testRecord("/tmp/p", "wrote\u{a0}/etc/passwd\u{85}ok"), null)).?;
    try std.testing.expectEqualStrings("wrote [path] ok", nbsp.reason);
    const home = (try sanitizeRecord(a, &testRecord("/tmp/p", "in=Users/me/secret"), "Users/me")).?;
    try std.testing.expectEqualStrings("[path]", home.reason);
    const xs: [4000]u8 = @splat('x');
    const long = (try sanitizeRecord(a, &testRecord("/tmp/p", &xs), null)).?;
    try std.testing.expectEqual(@as(usize, 240), long.reason.len);
    var wide_src: std.ArrayList(u8) = .empty;
    for (0..400) |_| try wide_src.appendSlice(a, "\u{65e5}");
    const wide = (try sanitizeRecord(a, &testRecord("/tmp/p", wide_src.items), null)).?;
    try std.testing.expectEqual(@as(usize, 240), wide.reason.len);
    try std.testing.expect(std.unicode.utf8ValidateSlice(wide.reason));
}

test "confidence is quantized and clamps instead of wrapping" {
    try std.testing.expectEqual(@as(u8, 82), quantize(0.82));
    try std.testing.expectEqual(@as(u8, 100), quantize(1.0));
    try std.testing.expectEqual(@as(u8, 0), quantize(std.math.nan(f32)));
    try std.testing.expectEqual(@as(u8, 0), quantize(std.math.inf(f32)));
    try std.testing.expectEqual(@as(u8, 100), quantize(1e30));
    try std.testing.expectEqual(@as(u8, 0), quantize(-5.0));
}

test "validation rejects an unsanitized entry and a record that cannot be represented is dropped" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const good = (try sanitizeRecord(a, &testRecord("/tmp/project", "persona=Abbey"), null)).?;
    try std.testing.expect(validEntry(&good));
    var bad = good;
    bad.reason = "routed in /Users/someone/secret";
    try std.testing.expect(!validEntry(&bad));
    bad = good;
    bad.reason = "cwd:/Users/x";
    try std.testing.expect(!validEntry(&bad));
    bad = good;
    bad.workspace = "ws-NOTHEXAAAAA";
    try std.testing.expect(!validEntry(&bad));
    bad = good;
    bad.recorded_at = "yesterday";
    try std.testing.expect(!validEntry(&bad));
    bad = good;
    bad.confidence_percent = 101;
    try std.testing.expect(!validEntry(&bad));
    var r = testRecord("/tmp/p", "x");
    r.ts = "not-a-timestamp";
    try std.testing.expect((try sanitizeRecord(a, &r, null)) == null);
    r = testRecord("/tmp/p", "x");
    r.persona = "  \x07 ";
    try std.testing.expect((try sanitizeRecord(a, &r, null)) == null);
    const pathy = (try sanitizeRecord(a, &testRecord("/tmp/p", "/Users/someone/only-a-path"), null)).?;
    try std.testing.expectEqualStrings("[path]", pathy.reason);
    const none = (try sanitizeRecord(a, &testRecord("/tmp/p", "\x07"), null)).?;
    try std.testing.expectEqualStrings("(no reason recorded)", none.reason);
    const p: Page = .{ .entries = &.{ good, good }, .limit = 1 };
    try std.testing.expect(!validPage(&p));
}

test "route audit page caps at the limit, keeps the newest, and skips malformed lines" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expectEqual(@as(usize, 0), (try readPage(a, io, root, 50, null)).entries.len);
    var i: usize = 0;
    while (i < 80) : (i += 1) {
        var r = testRecord("/tmp/project", try std.fmt.allocPrint(a, "decision-{d}", .{i}));
        r.ts = try std.fmt.allocPrint(a, "2026-08-08T12:{d:0>2}:00Z", .{i % 60});
        try route_log.append(gpa, io, root, &r);
    }
    try fsx.appendLocked(io, try route_log.path(a, root), "{ not json\nnull\n", .default_file);
    const page = try readPage(a, io, root, 50, null);
    try std.testing.expectEqual(@as(usize, 50), page.entries.len);
    try std.testing.expectEqualStrings("decision-79", page.entries[49].reason);
    try std.testing.expectEqualStrings("decision-30", page.entries[0].reason);
    try std.testing.expect(validPage(&page));
    try std.testing.expectEqual(@as(usize, 3), (try readPage(a, io, root, 3, null)).entries.len);
    var aw: Io.Writer.Allocating = .init(gpa);
    defer aw.deinit();
    try writePage(&aw.writer, &.{ .entries = page.entries[0..1], .limit = 50 });
    try std.testing.expectEqualStrings(
        "{\"entries\":[{\"recorded_at\":\"2026-08-08T12:30:00Z\",\"workspace\":\"ws-dd101579a2a2\",\"persona\":\"Abbey\",\"role\":\"max\",\"model\":\"fable\",\"confidence_percent\":82,\"reason\":\"decision-30\"}],\"returned\":1,\"limit\":50}",
        aw.written(),
    );
}
