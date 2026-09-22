//! Compile-time edition identity. Editions separate identity and on-disk
//! namespaces only; neither edition implements an unrestricted runtime.
//!
//! Every root is NEW and scoped to this Zig rewrite: the Rust tree's
//! `ABBEY_STATE_DIR` / `~/.local/state/abbey` are never read, so one exported
//! Rust variable cannot make the Zig binary adopt the Rust store.
const std = @import("std");
const build_options = @import("build_options");
const Ctx = @import("ctx.zig").Ctx;

pub const Edition = enum { safe, personal };

pub const Identity = struct {
    slug: []const u8,
    product_name: []const u8,
    binary_name: []const u8,
    state_dir_env: []const u8,
    config_path_env: []const u8,
    chat_file_env: []const u8,
    model_file_env: []const u8,
    history_file_env: []const u8,
    daemon_socket_env: []const u8,
    daemon_socket_name: []const u8,
    daemon_bearer_env: []const u8,
    daemon_bearer_file_env: []const u8,
};

pub const safe_identity: Identity = .{
    .slug = "abbey-zig",
    .product_name = "Abbey (Zig)",
    .binary_name = "abbey-zig",
    .state_dir_env = "ABBEY_ZIG_STATE_DIR",
    .config_path_env = "ABBEY_ZIG_CONFIG",
    .chat_file_env = "ABBEY_ZIG_CHAT_FILE",
    .model_file_env = "ABBEY_ZIG_MODEL_FILE",
    .history_file_env = "ABBEY_ZIG_HISTORY_FILE",
    .daemon_socket_env = "ABBEY_ZIG_DAEMON_SOCKET_PATH",
    .daemon_socket_name = "abbey-zig-daemon.sock",
    .daemon_bearer_env = "ABBEY_ZIG_DAEMON_BEARER_TOKEN",
    .daemon_bearer_file_env = "ABBEY_ZIG_DAEMON_BEARER_TOKEN_FILE",
};

pub const personal_identity: Identity = .{
    .slug = "abbey-zig-personal",
    .product_name = "Abbey Personal (Zig)",
    .binary_name = "abbey-zig-personal",
    .state_dir_env = "ABBEY_ZIG_PERSONAL_STATE_DIR",
    .config_path_env = "ABBEY_ZIG_PERSONAL_CONFIG",
    .chat_file_env = "ABBEY_ZIG_PERSONAL_CHAT_FILE",
    .model_file_env = "ABBEY_ZIG_PERSONAL_MODEL_FILE",
    .history_file_env = "ABBEY_ZIG_PERSONAL_HISTORY_FILE",
    .daemon_socket_env = "ABBEY_ZIG_PERSONAL_DAEMON_SOCKET_PATH",
    .daemon_socket_name = "abbey-zig-personal-daemon.sock",
    .daemon_bearer_env = "ABBEY_ZIG_PERSONAL_DAEMON_BEARER_TOKEN",
    .daemon_bearer_file_env = "ABBEY_ZIG_PERSONAL_DAEMON_BEARER_TOKEN_FILE",
};

pub const active: Edition = if (build_options.personal) .personal else .safe;

pub fn identity(e: Edition) *const Identity {
    return switch (e) {
        .safe => &safe_identity,
        .personal => &personal_identity,
    };
}

pub const id = identity(active);

pub const PathError = error{ NoHome, OutOfMemory };

/// State root: the edition override variable, else `$XDG_STATE_HOME/<slug>`
/// (non-macOS, absolute only), else `$HOME/.local/state/<slug>`.
pub fn stateRoot(ctx: Ctx, arena: std.mem.Allocator) PathError![]const u8 {
    if (ctx.envNonEmpty(id.state_dir_env)) |p| return arena.dupe(u8, p);
    if (@import("builtin").os.tag != .macos) {
        if (ctx.envNonEmpty("XDG_STATE_HOME")) |x| {
            if (std.fs.path.isAbsolute(x)) return std.fs.path.join(arena, &.{ x, id.slug });
        }
    }
    const home = ctx.envNonEmpty("HOME") orelse return error.NoHome;
    return std.fs.path.join(arena, &.{ home, ".local", "state", id.slug });
}

/// Config file: the edition override variable, else the platform config dir
/// (macOS `~/Library/Application Support`, else `$XDG_CONFIG_HOME` or
/// `~/.config`) joined with `<slug>/config.toml`, like the Rust `dirs` crate.
pub fn configPath(ctx: Ctx, arena: std.mem.Allocator) PathError![]const u8 {
    if (ctx.envNonEmpty(id.config_path_env)) |p| return arena.dupe(u8, p);
    const home = ctx.envNonEmpty("HOME") orelse return error.NoHome;
    if (@import("builtin").os.tag == .macos) {
        return std.fs.path.join(arena, &.{ home, "Library", "Application Support", id.slug, "config.toml" });
    }
    if (ctx.envNonEmpty("XDG_CONFIG_HOME")) |x| {
        if (std.fs.path.isAbsolute(x)) return std.fs.path.join(arena, &.{ x, id.slug, "config.toml" });
    }
    return std.fs.path.join(arena, &.{ home, ".config", id.slug, "config.toml" });
}

pub fn identityLines(w: *std.Io.Writer, state_dir: []const u8) std.Io.Writer.Error!void {
    try w.print("edition:   {s} ({s})\n", .{ id.product_name, switch (active) {
        .safe => "safe public edition, default build",
        .personal => "personal edition, built with -Dpersonal=true",
    } });
    try w.print("binary:    {s}\n", .{id.binary_name});
    try w.print("state env: {s} (root {s})\n", .{ id.state_dir_env, state_dir });
    try w.print("config env: {s}\n", .{id.config_path_env});
    try w.writeAll("unrestricted runtime implemented: false\n");
}

test "edition namespaces never reuse the Rust variables" {
    for ([_]Edition{ .safe, .personal }) |e| {
        const i = identity(e);
        try std.testing.expect(!std.mem.eql(u8, i.state_dir_env, "ABBEY_STATE_DIR"));
        try std.testing.expect(!std.mem.eql(u8, i.state_dir_env, "ABBEY_PERSONAL_STATE_DIR"));
        try std.testing.expect(!std.mem.eql(u8, i.config_path_env, "ABBEY_CONFIG"));
        try std.testing.expect(std.mem.startsWith(u8, i.slug, "abbey-zig"));
        // The Rust daemon variables (both editions) are never read here.
        for ([_][]const u8{ i.daemon_socket_env, i.daemon_bearer_env, i.daemon_bearer_file_env }) |v| {
            try std.testing.expect(std.mem.startsWith(u8, v, "ABBEY_ZIG_"));
            for ([_][]const u8{ "ABBEYD_SOCKET_PATH", "ABBEYD_BEARER_TOKEN", "ABBEYD_BEARER_TOKEN_FILE", "ABBEY_PERSONAL_DAEMON_SOCKET_PATH", "ABBEY_PERSONAL_DAEMON_BEARER_TOKEN", "ABBEY_PERSONAL_DAEMON_BEARER_TOKEN_FILE" }) |rust| {
                try std.testing.expect(!std.mem.eql(u8, v, rust));
            }
        }
    }
    try std.testing.expect(!std.mem.eql(u8, safe_identity.daemon_bearer_env, personal_identity.daemon_bearer_env));
    try std.testing.expect(!std.mem.eql(u8, safe_identity.daemon_socket_name, personal_identity.daemon_socket_name));
    try std.testing.expect(!std.mem.eql(u8, safe_identity.state_dir_env, personal_identity.state_dir_env));
    try std.testing.expect(!std.mem.eql(u8, safe_identity.slug, personal_identity.slug));
}

test "state root honors only the edition variable" {
    const T = @import("ctx.zig").TestCtx;
    var t = T.init(std.testing.allocator, "/");
    defer t.deinit();
    try t.env.put("HOME", "/home/u");
    try t.env.put("ABBEY_STATE_DIR", "/rust/state");
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const c = t.ctx(std.testing.allocator, std.testing.io);
    const root = try stateRoot(c, arena.allocator());
    try std.testing.expect(std.mem.find(u8, root, "/rust/state") == null);
    try std.testing.expect(std.mem.endsWith(u8, root, id.slug));
    try t.env.put(id.state_dir_env, "/zig/state");
    try std.testing.expectEqualStrings("/zig/state", try stateRoot(t.ctx(std.testing.allocator, std.testing.io), arena.allocator()));
}
