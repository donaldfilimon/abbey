//! Bounded protocol-v1 client for the local daemon socket.
//!
//! This client speaks v1 only and never downgrades or retries: there is no
//! v2 command to try. It checks the response's version and request_id before
//! looking at the payload (the Rust client's order), and re-validates a
//! route-audit page so an unsanitized page from a differently built peer is
//! rejected here too, not just promised by the producer.
//! std: lib/std/Io/net.zig (UnixAddress.connect, Stream.close).
const std = @import("std");
const Io = std.Io;
const net = Io.net;
const protocol = @import("protocol.zig");
const route_audit = @import("route_audit.zig");
const sys = @import("sys.zig");
const json = @import("../util/json.zig");
const uuid = @import("../util/uuid.zig");

pub const Error = error{
    SocketPathTooLong,
    Connect,
    Transport,
    EmptyResponse,
    ResponseTooLarge,
    MalformedResponse,
    ProtocolMismatch,
    RequestIdMismatch,
    UnexpectedEvent,
    InvalidRouteAudit,
    Daemon,
    OutOfMemory,
};

pub const Response = struct {
    /// The event object (`{"type":..,"payload":..}`) on success.
    event: std.json.Value,
    event_type: []const u8,
};

pub const DaemonError = struct { code: []const u8, message: []const u8 };

/// Send one frame and read one frame back.
pub fn roundTrip(arena: std.mem.Allocator, io: Io, socket_path: []const u8, body: []const u8, timeout_ms: i64) Error![]u8 {
    if (socket_path.len > sys.max_socket_path) return error.SocketPathTooLong;
    const addr = net.UnixAddress.init(socket_path) catch return error.SocketPathTooLong;
    const stream = addr.connect(io) catch return error.Connect;
    defer stream.close(io);
    const fd = stream.socket.handle;
    sys.writeFrame(io, fd, body, sys.deadline(io, timeout_ms)) catch return error.Transport;
    const read_deadline = sys.deadline(io, timeout_ms);
    var prefix: [4]u8 = undefined;
    sys.readExact(io, fd, &prefix, read_deadline) catch return error.Transport;
    const len = std.mem.readInt(u32, &prefix, .big);
    if (len == 0) return error.EmptyResponse;
    if (len > protocol.max_frame_len) return error.ResponseTooLarge;
    const buf = try arena.alloc(u8, len);
    sys.readExact(io, fd, buf, read_deadline) catch return error.Transport;
    return buf;
}

/// Build `{"version":1,"request_id":..,"bearer":..,"command":<command_json>}`.
fn envelope(arena: std.mem.Allocator, request_id: []const u8, bearer: []const u8, command_json: []const u8) error{OutOfMemory}![]u8 {
    var aw: Io.Writer.Allocating = .init(arena);
    const w = &aw.writer;
    w.print("{{\"version\":{d},\"request_id\":", .{protocol.version_v1}) catch return error.OutOfMemory;
    json.writeString(w, request_id) catch return error.OutOfMemory;
    w.writeAll(",\"bearer\":") catch return error.OutOfMemory;
    json.writeString(w, bearer) catch return error.OutOfMemory;
    w.writeAll(",\"command\":") catch return error.OutOfMemory;
    w.writeAll(command_json) catch return error.OutOfMemory;
    w.writeAll("}") catch return error.OutOfMemory;
    return aw.written();
}

/// One request/response exchange. `last_error` receives the daemon's code and
/// message when the daemon answers `error.Daemon`.
pub fn request(
    arena: std.mem.Allocator,
    io: Io,
    socket_path: []const u8,
    bearer: []const u8,
    command_json: []const u8,
    expect_type: []const u8,
    last_error: *DaemonError,
) Error!Response {
    const rid_buf = try arena.create([uuid.len]u8);
    const rid = uuid.v4(io, rid_buf);
    const frame = try envelope(arena, rid, bearer, command_json);
    const bytes = try roundTrip(arena, io, socket_path, frame, 5000);
    const v = std.json.parseFromSliceLeaky(std.json.Value, arena, bytes, .{}) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return error.MalformedResponse,
    };
    if (v != .object) return error.MalformedResponse;
    const got_rid = switch (v.object.get("request_id") orelse return error.MalformedResponse) {
        .string => |s| s,
        else => return error.MalformedResponse,
    };
    if (!std.mem.eql(u8, got_rid, rid)) return error.RequestIdMismatch;
    const payload = switch (v.object.get("payload") orelse return error.MalformedResponse) {
        .object => |o| o,
        else => return error.MalformedResponse,
    };
    const outcome = switch (payload.get("outcome") orelse return error.MalformedResponse) {
        .string => |s| s,
        else => return error.MalformedResponse,
    };
    if (std.mem.eql(u8, outcome, "error")) {
        last_error.* = .{
            .code = if (payload.get("code")) |x| (if (x == .string) x.string else "") else "",
            .message = if (payload.get("message")) |x| (if (x == .string) x.string else "") else "",
        };
        return error.Daemon;
    }
    const version = switch (v.object.get("version") orelse return error.MalformedResponse) {
        .integer => |i| i,
        else => return error.MalformedResponse,
    };
    if (version != protocol.version_v1) return error.ProtocolMismatch;
    const event = switch (payload.get("event") orelse return error.MalformedResponse) {
        .object => |o| o,
        else => return error.MalformedResponse,
    };
    const ty = switch (event.get("type") orelse return error.MalformedResponse) {
        .string => |s| s,
        else => return error.MalformedResponse,
    };
    if (!std.mem.eql(u8, ty, expect_type)) return error.UnexpectedEvent;
    if (std.mem.eql(u8, ty, "route_audit")) try validateRouteAudit(arena, event.get("payload") orelse return error.MalformedResponse);
    return .{ .event = payload.get("event").?, .event_type = ty };
}

/// Re-check the sanitization invariants on a page we did not build.
fn validateRouteAudit(arena: std.mem.Allocator, payload: std.json.Value) Error!void {
    if (payload != .object) return error.MalformedResponse;
    const limit = switch (payload.object.get("limit") orelse return error.MalformedResponse) {
        .integer => |i| std.math.cast(u16, i) orelse return error.InvalidRouteAudit,
        else => return error.MalformedResponse,
    };
    const returned = switch (payload.object.get("returned") orelse return error.MalformedResponse) {
        .integer => |i| std.math.cast(u16, i) orelse return error.InvalidRouteAudit,
        else => return error.MalformedResponse,
    };
    const entries = switch (payload.object.get("entries") orelse return error.MalformedResponse) {
        .array => |a| a,
        else => return error.MalformedResponse,
    };
    if (entries.items.len != returned) return error.InvalidRouteAudit;
    var list: std.ArrayList(route_audit.Entry) = .empty;
    for (entries.items) |item| {
        if (item != .object) return error.MalformedResponse;
        const o = item.object;
        var e: route_audit.Entry = .{
            .recorded_at = strField(o, "recorded_at") orelse return error.MalformedResponse,
            .workspace = strField(o, "workspace"),
            .persona = strField(o, "persona") orelse return error.MalformedResponse,
            .role = strField(o, "role") orelse return error.MalformedResponse,
            .model = strField(o, "model") orelse return error.MalformedResponse,
            .confidence_percent = switch (o.get("confidence_percent") orelse return error.MalformedResponse) {
                .integer => |i| std.math.cast(u8, i) orelse return error.InvalidRouteAudit,
                else => return error.MalformedResponse,
            },
            .reason = strField(o, "reason") orelse return error.MalformedResponse,
            .stage = strField(o, "stage"),
            .correlation = strField(o, "correlation"),
            .alternate = strField(o, "alternate"),
            .fallback = strField(o, "fallback"),
        };
        if (o.get("tools")) |t| {
            if (t != .array) return error.MalformedResponse;
            var tools: std.ArrayList([]const u8) = .empty;
            for (t.array.items) |x| {
                if (x != .string) return error.MalformedResponse;
                try tools.append(arena, x.string);
            }
            e.tools = tools.items;
        }
        try list.append(arena, e);
    }
    const page: route_audit.Page = .{ .entries = list.items, .limit = limit };
    if (!route_audit.validPage(&page)) return error.InvalidRouteAudit;
}

fn strField(o: std.json.ObjectMap, name: []const u8) ?[]const u8 {
    const v = o.get(name) orelse return null;
    return switch (v) {
        .string => |s| s,
        else => null,
    };
}
