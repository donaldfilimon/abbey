//! POSIX fd-level helpers the daemon transport needs and `std.Io` does not
//! offer: owner/mode inspection without following symlinks, and deadline-
//! bounded socket reads/writes.
//!
//! Why not `std.Io.net.Stream.Reader` with SO_RCVTIMEO: `netReadPosix` maps
//! EAGAIN to `errnoBug`, which panics in debug builds
//! (lib/std/Io/Threaded.zig netReadPosix). Deadlines are therefore enforced
//! with `poll` before each blocking `read`/`write` on a blocking fd.
//! std: lib/std/posix.zig (poll, read, errno), lib/std/c.zig (fstatat, Stat,
//! AT.FDCWD, AT.SYMLINK_NOFOLLOW, S.IF*, geteuid, write, unlink). Darwin
//! always links libSystem, so `std.c` is available there.
const std = @import("std");
const builtin = @import("builtin");
const posix = std.posix;
const c = std.c;
const Io = std.Io;

pub const Kind = enum { dir, socket, file, symlink, other };

pub const Meta = struct { kind: Kind, uid: u32, mode: u32 };

pub const StatError = error{ NameTooLong, AccessDenied, StatFailed };

/// lstat(2): metadata of `path` itself (symlinks are not followed), or null
/// when it does not exist.
pub fn lstat(path: []const u8) StatError!?Meta {
    var buf: [std.fs.max_path_bytes + 1]u8 = undefined;
    if (path.len >= buf.len) return error.NameTooLong;
    @memcpy(buf[0..path.len], path);
    buf[path.len] = 0;
    var st: c.Stat = undefined;
    const rc = c.fstatat(c.AT.FDCWD, buf[0..path.len :0], &st, c.AT.SYMLINK_NOFOLLOW);
    switch (posix.errno(rc)) {
        .SUCCESS => {},
        .NOENT, .NOTDIR => return null,
        .ACCES, .PERM => return error.AccessDenied,
        .NAMETOOLONG => return error.NameTooLong,
        else => return error.StatFailed,
    }
    const mode: u32 = @intCast(st.mode);
    const kind: Kind = switch (mode & c.S.IFMT) {
        c.S.IFDIR => .dir,
        c.S.IFSOCK => .socket,
        c.S.IFREG => .file,
        c.S.IFLNK => .symlink,
        else => .other,
    };
    return .{ .kind = kind, .uid = @intCast(st.uid), .mode = mode & 0o7777 };
}

pub fn euid() u32 {
    return @intCast(c.geteuid());
}

/// unlink(2); a missing path is not an error.
pub fn unlink(path: []const u8) void {
    var buf: [std.fs.max_path_bytes + 1]u8 = undefined;
    if (path.len >= buf.len) return;
    @memcpy(buf[0..path.len], path);
    buf[path.len] = 0;
    _ = c.unlink(buf[0..path.len :0]);
}

/// Longest socket path the platform's `sockaddr_un.sun_path` holds with its
/// NUL terminator. std's `UnixAddress.max_len` is 108 on every POSIX host,
/// but Darwin's `sun_path` is 104 bytes, so the limit is enforced here.
pub const max_socket_path: usize = switch (builtin.os.tag) {
    .macos, .ios, .tvos, .watchos, .visionos, .driverkit, .maccatalyst => 103,
    else => 107,
};

pub const IoError = error{ Timeout, EndOfStream, ConnectionFailed };

fn nowMs(io: Io) i64 {
    return Io.Clock.awake.now(io).toMilliseconds(); // std: lib/std/Io.zig (Clock.awake)
}

fn waitFor(io: Io, fd: posix.fd_t, events: i16, deadline_ms: i64) IoError!void {
    const remaining = deadline_ms - nowMs(io);
    if (remaining <= 0) return error.Timeout;
    var fds = [_]posix.pollfd{.{ .fd = fd, .events = events, .revents = 0 }};
    const n = posix.poll(&fds, @intCast(@min(remaining, std.math.maxInt(i32)))) catch return error.ConnectionFailed;
    if (n == 0) return error.Timeout;
}

/// Fill `buf` completely before `deadline_ms` (awake clock).
pub fn readExact(io: Io, fd: posix.fd_t, buf: []u8, deadline_ms: i64) IoError!void {
    var got: usize = 0;
    while (got < buf.len) {
        try waitFor(io, fd, posix.POLL.IN, deadline_ms);
        const n = posix.read(fd, buf[got..]) catch |e| switch (e) {
            error.WouldBlock => continue,
            else => return error.ConnectionFailed,
        };
        if (n == 0) return error.EndOfStream;
        got += n;
    }
}

/// Write all of `bytes` before `deadline_ms`. A peer that went away yields
/// `ConnectionFailed` (the Threaded Io installs a SIGPIPE handler, so EPIPE
/// is returned rather than the process being killed).
pub fn writeAll(io: Io, fd: posix.fd_t, bytes: []const u8, deadline_ms: i64) IoError!void {
    var sent: usize = 0;
    while (sent < bytes.len) {
        try waitFor(io, fd, posix.POLL.OUT, deadline_ms);
        const rc = c.write(fd, bytes[sent..].ptr, bytes.len - sent);
        switch (posix.errno(rc)) {
            .SUCCESS => sent += @intCast(rc),
            .INTR, .AGAIN => continue,
            else => return error.ConnectionFailed,
        }
    }
}

/// One `u32` big-endian length prefix plus body.
pub fn writeFrame(io: Io, fd: posix.fd_t, body: []const u8, deadline_ms: i64) IoError!void {
    var prefix: [4]u8 = undefined;
    std.mem.writeInt(u32, &prefix, @intCast(body.len), .big);
    try writeAll(io, fd, &prefix, deadline_ms);
    try writeAll(io, fd, body, deadline_ms);
}

pub fn deadline(io: Io, timeout_ms: i64) i64 {
    return nowMs(io) + timeout_ms;
}

test "lstat reports kind, owner, and mode without following symlinks" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    const m = (try lstat(root)).?;
    try std.testing.expectEqual(Kind.dir, m.kind);
    try std.testing.expectEqual(euid(), m.uid);
    const missing = try std.fs.path.join(gpa, &.{ root, "missing" });
    defer gpa.free(missing);
    try std.testing.expect((try lstat(missing)) == null);
    const link = try std.fs.path.join(gpa, &.{ root, "link" });
    defer gpa.free(link);
    try tmp.dir.symLink(io, root, "link", .{});
    try std.testing.expectEqual(Kind.symlink, (try lstat(link)).?.kind);
}
