//! abbeyd transport: an owner-only Unix socket serving protocol v1 frames,
//! ported from the Rust `daemon/server/unix.rs`.
//!
//! * the socket's parent directory is created 0700 when absent, and must be a
//!   real directory (not a symlink) owned by the effective user with no
//!   group/other bits; otherwise the daemon refuses to start.
//! * an existing path is removed only when it is a socket we own that nobody
//!   answers on (a stale socket); anything else is a conflict.
//! * the socket is chmod 0600 right after bind and unlinked on shutdown.
//! * one connection at a time; each carries one `u32` big-endian length
//!   prefix and at most 1 MiB of JSON, under a 5 s read and 5 s write
//!   deadline; an empty or oversize frame gets its error frame back.
//! * shutdown is cooperative: the accept loop polls the listener every
//!   25 ms and returns when `stop` is set (SIGINT/SIGTERM in `daemon serve`).
//! std: lib/std/Io/net.zig (UnixAddress.listen/connect, Server.accept,
//! Stream.close), lib/std/Io/Dir.zig (createDirPath, setFilePermissions),
//! lib/std/atomic.zig (Value).
const std = @import("std");
const Io = std.Io;
const net = Io.net;
const posix = std.posix;
const protocol = @import("protocol.zig");
const Config = @import("config.zig").Config;
const sys = @import("sys.zig");

pub const ServeError = error{
    MissingSocketParent,
    SocketPathTooLong,
    SocketDirectoryType,
    SocketDirectoryOwner,
    SocketDirectoryPermissions,
    SocketPathConflict,
    SocketSetupFailed,
    AcceptFailed,
};

pub fn describe(e: ServeError) []const u8 {
    return switch (e) {
        error.MissingSocketParent => "socket path has no parent directory",
        error.SocketPathTooLong => "socket path does not fit in sockaddr_un",
        error.SocketDirectoryType => "socket directory must be a real directory, not a symlink",
        error.SocketDirectoryOwner => "socket directory must be owned by the current user",
        error.SocketDirectoryPermissions => "socket directory must not grant group or other permissions",
        error.SocketPathConflict => "socket path already exists or is not a stale Abbey-owned socket",
        error.SocketSetupFailed => "cannot create, bind, or secure the socket",
        error.AcceptFailed => "accepting a connection failed",
    };
}

fn prepareDirectory(io: Io, socket_path: []const u8) ServeError!void {
    const parent = std.fs.path.dirname(socket_path) orelse return error.MissingSocketParent;
    if (parent.len == 0) return error.MissingSocketParent;
    const existing = sys.lstat(parent) catch return error.SocketSetupFailed;
    if (existing == null) {
        Io.Dir.cwd().createDirPath(io, parent) catch return error.SocketSetupFailed;
        Io.Dir.cwd().setFilePermissions(io, parent, .fromMode(0o700), .{}) catch return error.SocketSetupFailed;
    }
    const m = (sys.lstat(parent) catch return error.SocketSetupFailed) orelse return error.SocketSetupFailed;
    if (m.kind != .dir) return error.SocketDirectoryType;
    if (m.uid != sys.euid()) return error.SocketDirectoryOwner;
    if (m.mode & 0o077 != 0) return error.SocketDirectoryPermissions;
}

fn removeStale(io: Io, socket_path: []const u8) ServeError!void {
    const m = (sys.lstat(socket_path) catch return error.SocketSetupFailed) orelse return;
    if (m.kind != .socket or m.uid != sys.euid()) return error.SocketPathConflict;
    const addr = net.UnixAddress.init(socket_path) catch return error.SocketPathTooLong;
    if (addr.connect(io)) |s| {
        s.close(io);
        return error.SocketPathConflict; // a live daemon owns it
    } else |_| {}
    sys.unlink(socket_path);
}

fn handleConnection(gpa: std.mem.Allocator, io: Io, cfg: *const Config, svc: *const protocol.Service, limiter: *protocol.RateLimiter, fd: posix.fd_t) void {
    var arena_state: std.heap.ArenaAllocator = .init(gpa);
    defer arena_state.deinit();
    const arena = arena_state.allocator();
    var out: Io.Writer.Allocating = .init(arena);
    const read_deadline = sys.deadline(io, cfg.read_timeout_ms);
    var prefix: [4]u8 = undefined;
    sys.readExact(io, fd, &prefix, read_deadline) catch return;
    const len = std.mem.readInt(u32, &prefix, .big);
    if (len == 0) {
        protocol.respondIssue(&out.writer, .empty) catch return;
    } else if (len > protocol.max_frame_len) {
        protocol.respondIssue(&out.writer, .oversize) catch return;
    } else {
        const body = arena.alloc(u8, len) catch return;
        sys.readExact(io, fd, body, read_deadline) catch return;
        const now = Io.Clock.awake.now(io).toMilliseconds();
        protocol.respond(arena, body, cfg.bearer, limiter, now, svc, &out) catch return;
    }
    sys.writeFrame(io, fd, out.written(), sys.deadline(io, cfg.write_timeout_ms)) catch {};
}

/// Bind, secure, and serve until `stop` is set. The socket file is removed
/// on every return path after a successful bind.
pub fn serve(gpa: std.mem.Allocator, io: Io, cfg: *const Config, svc: *const protocol.Service, stop: *const std.atomic.Value(bool), limiter_init: protocol.RateLimiter) ServeError!void {
    if (cfg.socket_path.len > sys.max_socket_path) return error.SocketPathTooLong;
    try prepareDirectory(io, cfg.socket_path);
    try removeStale(io, cfg.socket_path);
    const addr = net.UnixAddress.init(cfg.socket_path) catch return error.SocketPathTooLong;
    var server = addr.listen(io, .{}) catch return error.SocketSetupFailed;
    defer sys.unlink(cfg.socket_path);
    defer server.deinit(io);
    Io.Dir.cwd().setFilePermissions(io, cfg.socket_path, .fromMode(0o600), .{}) catch return error.SocketSetupFailed;

    var limiter = limiter_init;
    while (!stop.load(.acquire)) {
        var fds = [_]posix.pollfd{.{ .fd = server.socket.handle, .events = posix.POLL.IN, .revents = 0 }};
        const ready = posix.poll(&fds, cfg.accept_poll_ms) catch return error.AcceptFailed;
        if (ready == 0) continue;
        const stream = server.accept(io) catch |e| switch (e) {
            error.ConnectionAborted, error.WouldBlock => continue,
            else => return error.AcceptFailed,
        };
        defer stream.close(io);
        handleConnection(gpa, io, cfg, svc, &limiter, stream.socket.handle);
    }
}
