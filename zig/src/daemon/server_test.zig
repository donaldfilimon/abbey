//! Socket-level tests: a real listener on a temporary owner-only directory,
//! driven by the in-repo client on a second thread.
const std = @import("std");
const Io = std.Io;
const protocol = @import("protocol.zig");
const server = @import("server.zig");
const client = @import("client.zig");
const sys = @import("sys.zig");
const route_log = @import("../route_log.zig");
const Config = @import("config.zig").Config;

const bearer = "0123456789abcdef0123456789abcdef";

const Daemon = struct {
    gpa: std.mem.Allocator,
    io: Io,
    cfg: Config,
    svc: protocol.Service,
    stop: std.atomic.Value(bool) = .init(false), // std: lib/std/atomic.zig (Value)
    failure: ?server.ServeError = null,

    fn run(d: *Daemon) void {
        server.serve(d.gpa, d.io, &d.cfg, &d.svc, &d.stop, .{}) catch |e| {
            d.failure = e;
        };
    }
};

const Harness = struct {
    tmp: std.testing.TmpDir,
    root: []u8,
    socket: []u8,
    daemon: *Daemon,
    thread: std.Thread,
    joined: bool = false,

    fn start(gpa: std.mem.Allocator, io: Io, state_dir: ?[]const u8) !Harness {
        const tmp = std.testing.tmpDir(.{});
        const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
        const socket = try std.fs.path.join(gpa, &.{ root, "daemon", "abz.sock" });
        const d = try gpa.create(Daemon);
        d.* = .{
            .gpa = gpa,
            .io = io,
            .cfg = .{ .socket_path = socket, .bearer = bearer, .read_timeout_ms = 2000, .write_timeout_ms = 2000, .accept_poll_ms = 5 },
            .svc = .{ .io = io, .personal = false, .version = "test", .build_git = "test", .build_target = "test", .state_dir = state_dir, .home_marker = null },
        };
        const thread = try std.Thread.spawn(.{}, Daemon.run, .{d}); // std: lib/std/Thread.zig (spawn)
        var waited: usize = 0;
        while (waited < 400) : (waited += 1) {
            if (try sys.lstat(socket)) |m| if (m.kind == .socket and m.mode & 0o077 == 0) break;
            if (d.failure != null) break;
            try Io.sleep(io, .fromMilliseconds(5), .awake); // std: lib/std/Io.zig (sleep)
        }
        return .{ .tmp = tmp, .root = root, .socket = socket, .daemon = d, .thread = thread };
    }

    /// Ask the accept loop to return and wait for it; idempotent.
    fn shutdown(h: *Harness) void {
        if (h.joined) return;
        h.daemon.stop.store(true, .release);
        h.thread.join();
        h.joined = true;
    }

    fn stop(h: *Harness, gpa: std.mem.Allocator) void {
        h.shutdown();
        gpa.destroy(h.daemon);
        gpa.free(h.socket);
        gpa.free(h.root);
        h.tmp.cleanup();
    }
};

/// Send a frame whose declared length is not the body length.
fn rawFrame(gpa: std.mem.Allocator, io: Io, socket_path: []const u8, declared: u32, body: []const u8) ![]u8 {
    const addr = try Io.net.UnixAddress.init(socket_path);
    const stream = try addr.connect(io);
    defer stream.close(io);
    const fd = stream.socket.handle;
    var prefix: [4]u8 = undefined;
    std.mem.writeInt(u32, &prefix, declared, .big);
    const dl = sys.deadline(io, 2000);
    try sys.writeAll(io, fd, &prefix, dl);
    if (body.len != 0) try sys.writeAll(io, fd, body, dl);
    try sys.readExact(io, fd, &prefix, dl);
    const len = std.mem.readInt(u32, &prefix, .big);
    const buf = try gpa.alloc(u8, len);
    errdefer gpa.free(buf);
    try sys.readExact(io, fd, buf, dl);
    return buf;
}

test "the daemon serves an authenticated status request and removes its socket on stop" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var h = try Harness.start(gpa, io, null);
    defer h.stop(gpa);
    try std.testing.expect(h.daemon.failure == null);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    var err: client.DaemonError = .{ .code = "", .message = "" };
    const res = try client.request(arena.allocator(), io, h.socket, bearer, "{\"type\":\"status\"}", "status", &err);
    const payload = res.event.object.get("payload").?.object;
    try std.testing.expectEqual(@as(i64, 1), payload.get("protocol_version").?.integer);
    try std.testing.expectEqualStrings("standard", payload.get("edition").?.string);
    try std.testing.expectEqual(@as(usize, 3), payload.get("capabilities").?.object.get("capabilities").?.array.items.len);
    // Two requests on two connections: the accept loop is not one-shot.
    _ = try client.request(arena.allocator(), io, h.socket, bearer, "{\"type\":\"claims\",\"payload\":{}}", "claims", &err);
    h.shutdown();
    try std.testing.expect((try sys.lstat(h.socket)) == null);
}

test "over the socket: a wrong bearer, an empty frame, and an oversize frame all fail closed" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var h = try Harness.start(gpa, io, null);
    defer h.stop(gpa);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    var err: client.DaemonError = .{ .code = "", .message = "" };
    const wrong: [32]u8 = @splat('a');
    try std.testing.expectError(error.Daemon, client.request(arena.allocator(), io, h.socket, &wrong, "{\"type\":\"status\"}", "status", &err));
    try std.testing.expectEqualStrings("unauthorized", err.code);

    const empty = try rawFrame(gpa, io, h.socket, 0, "");
    defer gpa.free(empty);
    try std.testing.expect(std.mem.find(u8, empty, "\"malformed_request\"") != null);
    try std.testing.expect(std.mem.find(u8, empty, "frame must not be empty") != null);

    const oversize = try rawFrame(gpa, io, h.socket, protocol.max_frame_len + 1, "");
    defer gpa.free(oversize);
    try std.testing.expect(std.mem.find(u8, oversize, "\"frame_too_large\"") != null);

    const junk = try rawFrame(gpa, io, h.socket, 8, "not-json");
    defer gpa.free(junk);
    try std.testing.expect(std.mem.find(u8, junk, "\"malformed_request\"") != null);
}

test "a sanitized route audit page crosses the socket and the client re-validates it" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var state = std.testing.tmpDir(.{});
    defer state.cleanup();
    const state_dir = try @import("../ctx.zig").tmpPath(gpa, io, state.dir);
    defer gpa.free(state_dir);
    var r: route_log.Record = .{
        .ts = "2026-09-21T10:11:12Z",
        .cwd = state_dir,
        .persona = "Abbey",
        .role = "max",
        .model = "fable",
        .reason = "persona=Abbey class=Code log=/var/log/abbey.jsonl\x07",
        .confidence = 0.735,
        .stage = "gate",
    };
    try route_log.append(gpa, io, state_dir, &r);
    var h = try Harness.start(gpa, io, state_dir);
    defer h.stop(gpa);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    var err: client.DaemonError = .{ .code = "", .message = "" };
    const res = try client.request(arena.allocator(), io, h.socket, bearer, "{\"type\":\"read_routes\",\"payload\":{\"limit\":5}}", "route_audit", &err);
    const page = res.event.object.get("payload").?.object;
    try std.testing.expectEqual(@as(i64, 1), page.get("returned").?.integer);
    const entry = page.get("entries").?.array.items[0].object;
    try std.testing.expectEqual(@as(i64, 74), entry.get("confidence_percent").?.integer);
    try std.testing.expectEqualStrings("persona=Abbey class=Code [path]", entry.get("reason").?.string);
    try std.testing.expect(std.mem.startsWith(u8, entry.get("workspace").?.string, "ws-"));
    try std.testing.expect(entry.get("cwd") == null);
}

test "the socket directory must be owner-only, and a foreign path is a conflict" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    try Io.Dir.cwd().setFilePermissions(io, root, .fromMode(0o755), .{});
    const open = try std.fs.path.join(gpa, &.{ root, "open.sock" });
    defer gpa.free(open);
    var stop: std.atomic.Value(bool) = .init(true);
    const cfg: Config = .{ .socket_path = open, .bearer = bearer };
    const svc: protocol.Service = .{ .io = io, .personal = false, .version = "t", .build_git = "t", .build_target = "t", .state_dir = null, .home_marker = null };
    try std.testing.expectError(error.SocketDirectoryPermissions, server.serve(gpa, io, &cfg, &svc, &stop, .{}));

    try Io.Dir.cwd().setFilePermissions(io, root, .fromMode(0o700), .{});
    const taken = try std.fs.path.join(gpa, &.{ root, "taken.sock" });
    defer gpa.free(taken);
    try @import("../util/fsx.zig").writeAll(io, taken, "not a socket\n", .default_file);
    const cfg2: Config = .{ .socket_path = taken, .bearer = bearer };
    try std.testing.expectError(error.SocketPathConflict, server.serve(gpa, io, &cfg2, &svc, &stop, .{}));

    var long: [sys.max_socket_path + 1]u8 = @splat('x');
    const cfg3: Config = .{ .socket_path = &long, .bearer = bearer };
    try std.testing.expectError(error.SocketPathTooLong, server.serve(gpa, io, &cfg3, &svc, &stop, .{}));
}
