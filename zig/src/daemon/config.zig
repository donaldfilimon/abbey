//! Fail-closed daemon configuration, ported from the Rust `daemon/config.rs`.
//!
//! Socket: the edition's `*_DAEMON_SOCKET_PATH`, else
//! `<edition state root>/daemon/<edition socket name>`. Bearer: exactly one of
//! the inline `*_DAEMON_BEARER_TOKEN` or `*_DAEMON_BEARER_TOKEN_FILE`; the
//! secret is 32..=4096 bytes of UTF-8 without control characters after one
//! trailing `\n` and one `\r` are trimmed. A token file must be a regular
//! file (not a symlink) owned by the effective user with no group/other
//! permission bits. The Rust `ABBEYD_*` variables are never read.
const std = @import("std");
const Ctx = @import("../ctx.zig").Ctx;
const edition = @import("../edition.zig");
const fsx = @import("../util/fsx.zig");
const text = @import("text.zig");
const sys = @import("sys.zig");

pub const Error = error{
    MissingBearer,
    ConflictingBearerSources,
    BearerLength,
    BearerNotUtf8,
    BearerControlCharacter,
    BearerFileType,
    BearerFileOwner,
    BearerFilePermissions,
    BearerFileUnreadable,
    NoHome,
    OutOfMemory,
};

pub const Config = struct {
    socket_path: []const u8,
    bearer: []const u8,
    read_timeout_ms: i64 = 5000,
    write_timeout_ms: i64 = 5000,
    accept_poll_ms: i32 = 25,
};

/// Rust `BearerSecret::parse`. Returns a slice of `raw`.
pub fn parseBearer(raw: []const u8) Error![]const u8 {
    var v = raw;
    if (std.mem.endsWith(u8, v, "\n")) v = v[0 .. v.len - 1];
    if (std.mem.endsWith(u8, v, "\r")) v = v[0 .. v.len - 1];
    if (v.len < 32 or v.len > 4096) return error.BearerLength;
    if (!std.unicode.utf8ValidateSlice(v)) return error.BearerNotUtf8;
    if (text.hasControl(v)) return error.BearerControlCharacter;
    return v;
}

/// Rust `load_bearer_file`: owner-only regular file, then `parseBearer`.
pub fn loadBearerFile(ctx: Ctx, arena: std.mem.Allocator, path: []const u8) Error![]const u8 {
    const m = (sys.lstat(path) catch return error.BearerFileUnreadable) orelse return error.BearerFileUnreadable;
    if (m.kind != .file) return error.BearerFileType;
    if (m.uid != sys.euid()) return error.BearerFileOwner;
    if (m.mode & 0o077 != 0) return error.BearerFilePermissions;
    const bytes = (fsx.readOptional(ctx.io, arena, path, 4096 + 2) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return error.BearerFileUnreadable,
    }) orelse return error.BearerFileUnreadable;
    return parseBearer(bytes);
}

pub fn defaultSocketPath(ctx: Ctx, arena: std.mem.Allocator) Error![]const u8 {
    const root = edition.stateRoot(ctx, arena) catch |e| return switch (e) {
        error.NoHome => error.NoHome,
        error.OutOfMemory => error.OutOfMemory,
    };
    return std.fs.path.join(arena, &.{ root, "daemon", edition.id.daemon_socket_name });
}

/// Load the edition's daemon configuration from `ctx.env`.
pub fn fromEnv(ctx: Ctx, arena: std.mem.Allocator) Error!Config {
    const socket_path = if (ctx.getEnv(edition.id.daemon_socket_env)) |p| try arena.dupe(u8, p) else try defaultSocketPath(ctx, arena);
    const inline_secret = ctx.getEnv(edition.id.daemon_bearer_env);
    const file = ctx.getEnv(edition.id.daemon_bearer_file_env);
    if (inline_secret != null and file != null) return error.ConflictingBearerSources;
    const bearer = if (inline_secret) |v| try parseBearer(v) else if (file) |f| try loadBearerFile(ctx, arena, f) else return error.MissingBearer;
    return .{ .socket_path = socket_path, .bearer = bearer };
}

pub fn describe(e: Error) []const u8 {
    return switch (e) {
        error.MissingBearer => "set exactly one of " ++ edition.id.daemon_bearer_env ++ " or " ++ edition.id.daemon_bearer_file_env,
        error.ConflictingBearerSources => edition.id.daemon_bearer_env ++ " and " ++ edition.id.daemon_bearer_file_env ++ " cannot both be set",
        error.BearerLength => "daemon bearer token must contain 32 through 4096 bytes",
        error.BearerNotUtf8 => "daemon bearer token must be valid UTF-8",
        error.BearerControlCharacter => "daemon bearer token must not contain control characters",
        error.BearerFileType => "bearer file must be a regular file, not a symlink",
        error.BearerFileOwner => "bearer file must be owned by the current user",
        error.BearerFilePermissions => "bearer file must not grant group or other permissions",
        error.BearerFileUnreadable => "bearer file cannot be inspected or read",
        error.NoHome => "HOME is unset and no socket path was given",
        error.OutOfMemory => "out of memory",
    };
}

test "bearer rules: length, line ending, controls, and exclusive sources" {
    const good = "0123456789abcdef0123456789abcdef";
    try std.testing.expectEqualStrings(good, try parseBearer(good ++ "\r\n"));
    try std.testing.expectError(error.BearerLength, parseBearer("short"));
    try std.testing.expectError(error.BearerControlCharacter, parseBearer(good ++ "\x07"));
    try std.testing.expectError(error.BearerNotUtf8, parseBearer(good ++ "\xff"));
    const T = @import("../ctx.zig").TestCtx;
    var t = T.init(std.testing.allocator, "/");
    defer t.deinit();
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    try t.env.put("HOME", "/home/u");
    try t.env.put("ABBEYD_BEARER_TOKEN", good); // the Rust variable is ignored
    try std.testing.expectError(error.MissingBearer, fromEnv(t.ctx(std.testing.allocator, std.testing.io), arena.allocator()));
    try t.env.put(edition.id.daemon_bearer_env, good);
    const cfg = try fromEnv(t.ctx(std.testing.allocator, std.testing.io), arena.allocator());
    try std.testing.expect(std.mem.endsWith(u8, cfg.socket_path, "/daemon/" ++ edition.id.daemon_socket_name));
    try t.env.put(edition.id.daemon_bearer_file_env, "/nonexistent");
    try std.testing.expectError(error.ConflictingBearerSources, fromEnv(t.ctx(std.testing.allocator, std.testing.io), arena.allocator()));
}

test "bearer file must be owner-only and regular" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const T = @import("../ctx.zig").TestCtx;
    var t = T.init(gpa, root);
    defer t.deinit();
    const c = t.ctx(gpa, io);
    const p = try std.fs.path.join(a, &.{ root, "bearer" });
    try fsx.writeAll(io, p, "0123456789abcdef0123456789abcdef\n", .fromMode(0o640));
    try std.Io.Dir.cwd().setFilePermissions(io, p, .fromMode(0o640), .{});
    try std.testing.expectError(error.BearerFilePermissions, loadBearerFile(c, a, p));
    try std.Io.Dir.cwd().setFilePermissions(io, p, .fromMode(0o600), .{}); // std: lib/std/Io/Dir.zig (setFilePermissions)
    try std.testing.expectEqualStrings("0123456789abcdef0123456789abcdef", try loadBearerFile(c, a, p));
    try tmp.dir.symLink(io, p, "link", .{});
    try std.testing.expectError(error.BearerFileType, loadBearerFile(c, a, try std.fs.path.join(a, &.{ root, "link" })));
}
