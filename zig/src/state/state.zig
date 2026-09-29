//! Edition-scoped state: chat ids (global + per-cwd mirror), model file,
//! history log. Port of the Rust `state.rs` file layout; the Rust
//! runtime.sqlite conversation-identity journal is Proposed here, so the
//! chat-id files themselves are authoritative in this rewrite.
const std = @import("std");
const Io = std.Io;
const Ctx = @import("../ctx.zig").Ctx;
const edition = @import("../edition.zig");
const fsx = @import("../util/fsx.zig");
const time = @import("../util/time.zig");
const Backend = @import("../agent/backend.zig").Backend;
const models = @import("../models.zig");

pub const Error = edition.PathError || fsx.ReadError || fsx.WriteError;

pub const State = struct {
    state_dir: []const u8,
    chat_file: []const u8,
    model_file: []const u8,
    history_file: []const u8,
    cwd_dir: []const u8,
    per_cwd: bool,
    cwd: []const u8,
};

/// Resolve (and create) the state layout. Strings live in `arena`.
pub fn load(ctx: Ctx, arena: std.mem.Allocator) Error!State {
    const root = try edition.stateRoot(ctx, arena);
    const J = std.fs.path.join;
    const s: State = .{
        .state_dir = root,
        .chat_file = if (ctx.envNonEmpty(edition.id.chat_file_env)) |p| p else try J(arena, &.{ root, "chat-id" }),
        .model_file = if (ctx.envNonEmpty(edition.id.model_file_env)) |p| p else try J(arena, &.{ root, "model" }),
        .history_file = if (ctx.envNonEmpty(edition.id.history_file_env)) |p| p else try J(arena, &.{ root, "history.log" }),
        .cwd_dir = try J(arena, &.{ root, "by-cwd" }),
        .per_cwd = if (ctx.getEnv("ABBEY_PER_CWD")) |v| !std.mem.eql(u8, v, "0") else true,
        .cwd = ctx.cwd,
    };
    try fsx.makePath(ctx.io, s.state_dir);
    try fsx.makePath(ctx.io, s.cwd_dir);
    return s;
}

/// Rust `cwd_key`: ASCII alnum and `.-_` kept, everything else `_`, <= 180.
pub fn cwdKey(buf: *[180]u8, cwd: []const u8) []const u8 {
    var n: usize = 0;
    for (cwd) |c| {
        if (n == buf.len) break;
        buf[n] = if (std.ascii.isAlphanumeric(c) or c == '.' or c == '-' or c == '_') c else '_';
        n += 1;
    }
    return buf[0..n];
}

pub fn activeChatFile(s: *const State, arena: std.mem.Allocator) error{OutOfMemory}![]const u8 {
    if (!s.per_cwd) return s.chat_file;
    var buf: [180]u8 = undefined;
    return std.fs.path.join(arena, &.{ s.cwd_dir, cwdKey(&buf, s.cwd) });
}

fn firstLine(io: Io, arena: std.mem.Allocator, p: []const u8) fsx.ReadError!?[]const u8 {
    const text = (try fsx.readOptional(io, arena, p, 64 * 1024)) orelse return null;
    var it = std.mem.splitScalar(u8, text, '\n');
    const line = std.mem.trim(u8, it.first(), &std.ascii.whitespace);
    return if (line.len == 0) null else line;
}

/// Chat id as seen by `backend` (the live backend, never a cached one):
/// CURSOR_AGENT_CHAT_ID is adopted only by backends with server sessions,
/// otherwise it would hijack a local transcript.
pub fn resolveChatFor(ctx: Ctx, arena: std.mem.Allocator, s: *const State, backend: Backend) Error!?[]const u8 {
    if (backend.hasServerSessions()) {
        if (ctx.envNonEmpty("CURSOR_AGENT_CHAT_ID")) |id| return id;
    }
    if (try firstLine(ctx.io, arena, try activeChatFile(s, arena))) |id| return id;
    if (s.per_cwd) return firstLine(ctx.io, arena, s.chat_file);
    return null;
}

pub const IdError = error{InvalidChatId};

fn validId(id: []const u8) bool {
    if (id.len == 0 or id.len > 256) return false;
    for (id) |c| if (!(std.ascii.isAlphanumeric(c) or c == '-' or c == '_' or c == '.')) return false;
    return !std.mem.eql(u8, id, ".") and !std.mem.eql(u8, id, "..");
}

/// Persist a chat id to the global file and the per-cwd mirror (owner-only),
/// and append a history line `<ts ms>\t<id>\t<cwd>`.
pub fn saveChat(ctx: Ctx, arena: std.mem.Allocator, s: *const State, id: []const u8) (Error || IdError)!void {
    if (!validId(id)) return error.InvalidChatId;
    const line = try std.fmt.allocPrint(arena, "{s}\n", .{id});
    try fsx.writeAll(ctx.io, s.chat_file, line, fsx.owner_only);
    if (s.per_cwd) try fsx.writeAll(ctx.io, try activeChatFile(s, arena), line, fsx.owner_only);
    var tb: [time.millis_len]u8 = undefined;
    const ts = time.formatMillis(&tb, time.nowMillis(ctx.io));
    try fsx.appendLocked(ctx.io, s.history_file, try std.fmt.allocPrint(arena, "{s}\t{s}\t{s}\n", .{ ts, id, s.cwd }), fsx.owner_only);
}

/// ABBEY_MODEL or the model file, resolved through cursor aliases; "auto".
pub fn readModel(ctx: Ctx, arena: std.mem.Allocator, s: *const State) Error![]const u8 {
    return models.resolveModel(arena, try readModelRaw(ctx, arena, s, "auto"));
}

/// Raw ABBEY_MODEL / model-file text without alias expansion (abi/ollama).
pub fn readModelRaw(ctx: Ctx, arena: std.mem.Allocator, s: *const State, fallback: []const u8) Error![]const u8 {
    if (ctx.envNonEmpty("ABBEY_MODEL")) |m| return m;
    if (try firstLine(ctx.io, arena, s.model_file)) |m| return m;
    return fallback;
}

test "cwd key sanitizes like Rust" {
    var buf: [180]u8 = undefined;
    try std.testing.expectEqualStrings("_private_tmp_abbey-zig-p1_work", cwdKey(&buf, "/private/tmp/abbey-zig-p1/work"));
}

test "save then resolve, per-cwd mirror, and CURSOR_AGENT_CHAT_ID only for server backends" {
    const T = @import("../ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var t = T.init(gpa, "/work/dir");
    defer t.deinit();
    try t.env.put("HOME", root);
    try t.env.put(edition.id.state_dir_env, root);
    try t.env.put("CURSOR_AGENT_CHAT_ID", "cursor-session");
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const c = t.ctx(gpa, io);
    const s = try load(c, a);
    try std.testing.expect((try resolveChatFor(c, a, &s, .abi)) == null);
    try saveChat(c, a, &s, "local-abi-session");
    try std.testing.expectEqualStrings("local-abi-session", (try resolveChatFor(c, a, &s, .abi)).?);
    try std.testing.expectEqualStrings("cursor-session", (try resolveChatFor(c, a, &s, .cursor)).?);
    try std.testing.expectError(error.InvalidChatId, saveChat(c, a, &s, "../escape"));
    const hist = (try fsx.readOptional(io, a, s.history_file, 4096)).?;
    try std.testing.expect(std.mem.endsWith(u8, hist, "\tlocal-abi-session\t/work/dir\n"));
}
