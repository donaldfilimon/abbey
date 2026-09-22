//! abbeyd protocol v1: the read-only Status / Claims / ReadRoutes command
//! family, byte-compatible with the Rust `daemon/protocol.rs` +
//! `app_core/contracts.rs` wire shapes (serde field order, adjacently tagged
//! `{"type","payload"}` commands and events, `deny_unknown_fields`).
//!
//! `respond` is the whole per-frame decision, transport-free so it is unit
//! tested without sockets. Its check order is the Rust server's:
//! JSON parse -> bearer -> rate limit -> strict envelope decode ->
//! request_id grammar -> version -> command minimum version -> payload
//! validation -> handler. This daemon serves only v1. A v2 envelope gets
//! `unsupported_version` at version 2 with its request_id echoed, which is
//! exactly what makes the Rust `DaemonClient` (v2 first) retry at v1. The v2
//! run commands answer `unsupported_command`; v3 and federation frames are
//! never decoded (Proposed) and fall out as `malformed_request` or
//! `unauthorized`.
const std = @import("std");
const Io = std.Io;
const json = @import("../util/json.zig");
const text = @import("text.zig");
const route_audit = @import("route_audit.zig");
const claims = @import("../claims.zig");
const Value = std.json.Value; // std: lib/std/json/dynamic.zig (Value, ObjectMap)

pub const version_v1: u16 = 1;
/// The Rust tree's CURRENT_PROTOCOL_VERSION; used for `unsupported_version`.
pub const version_current: u16 = 2;
pub const schema_v1: u16 = 1;
pub const max_frame_len: usize = 1024 * 1024;
const max_request_id_len = 128;
const max_claims_filter = 256;

pub const ClaimFilter = enum { current, partial, proposed, blocked, out_of_scope, failed, revoked, superseded, expired };

pub const ClaimsQuery = struct { status: ?ClaimFilter = null, contains: ?[]const u8 = null };

pub const Command = union(enum) {
    status,
    claims: ClaimsQuery,
    read_routes: u16,
    /// submit_run / get_run / cancel_run / run_events: known names whose
    /// minimum protocol is 2. Their payloads are not decoded here.
    v2_only,
};

pub const Envelope = struct {
    version: u16,
    request_id: []const u8,
    bearer: []const u8,
    command: Command,
};

fn onlyKeys(obj: std.json.ObjectMap, allowed: []const []const u8) bool {
    for (obj.keys()) |k| {
        for (allowed) |a| {
            if (std.mem.eql(u8, k, a)) break;
        } else return false;
    }
    return true;
}

fn asU16(v: Value) ?u16 {
    return switch (v) {
        .integer => |i| std.math.cast(u16, i),
        else => null,
    };
}

fn optString(v: ?Value) error{Malformed}!?[]const u8 {
    const x = v orelse return null;
    return switch (x) {
        .null => null,
        .string => |s| s,
        else => error.Malformed,
    };
}

fn decodeCommand(v: Value) error{Malformed}!Command {
    if (v != .object) return error.Malformed;
    const obj = v.object;
    if (!onlyKeys(obj, &.{ "type", "payload" })) return error.Malformed;
    const ty = switch (obj.get("type") orelse return error.Malformed) {
        .string => |s| s,
        else => return error.Malformed,
    };
    const payload = obj.get("payload");
    if (std.mem.eql(u8, ty, "status")) {
        if (payload) |p| if (p != .null) return error.Malformed;
        return .status;
    }
    if (std.mem.eql(u8, ty, "claims")) {
        const p = payload orelse return error.Malformed;
        if (p != .object or !onlyKeys(p.object, &.{ "status", "contains" })) return error.Malformed;
        var q: ClaimsQuery = .{ .contains = try optString(p.object.get("contains")) };
        if (try optString(p.object.get("status"))) |s| q.status = std.meta.stringToEnum(ClaimFilter, s) orelse return error.Malformed;
        return .{ .claims = q };
    }
    if (std.mem.eql(u8, ty, "read_routes")) {
        const p = payload orelse return error.Malformed;
        if (p != .object or !onlyKeys(p.object, &.{"limit"})) return error.Malformed;
        const limit = if (p.object.get("limit")) |l| asU16(l) orelse return error.Malformed else route_audit.max_page;
        return .{ .read_routes = limit };
    }
    for ([_][]const u8{ "submit_run", "get_run", "cancel_run", "run_events" }) |n| {
        if (std.mem.eql(u8, ty, n)) return .v2_only;
    }
    return error.Malformed;
}

/// Strict serde-equivalent decode of the legacy envelope.
pub fn decodeEnvelope(v: Value) error{Malformed}!Envelope {
    if (v != .object) return error.Malformed;
    const obj = v.object;
    if (!onlyKeys(obj, &.{ "version", "request_id", "bearer", "command" })) return error.Malformed;
    const version = asU16(obj.get("version") orelse return error.Malformed) orelse return error.Malformed;
    const rid = switch (obj.get("request_id") orelse return error.Malformed) {
        .string => |s| s,
        else => return error.Malformed,
    };
    const bearer = switch (obj.get("bearer") orelse return error.Malformed) {
        .string => |s| s,
        else => return error.Malformed,
    };
    return .{ .version = version, .request_id = rid, .bearer = bearer, .command = try decodeCommand(obj.get("command") orelse return error.Malformed) };
}

/// Rust `valid_request_id`: 1..=128 bytes of ASCII alnum or `._:-`.
pub fn validRequestId(s: []const u8) bool {
    if (s.len == 0 or s.len > max_request_id_len) return false;
    for (s) |c| if (!(std.ascii.isAlphanumeric(c) or c == '.' or c == '_' or c == ':' or c == '-')) return false;
    return true;
}

/// Rust `ClaimsQuery::validate` + `RouteAuditQuery::validate`.
pub fn validCommand(c: Command) bool {
    return switch (c) {
        .status, .v2_only => true,
        .claims => |q| blk: {
            const f = text.trim(q.contains orelse break :blk true);
            break :blk f.len != 0 and f.len <= max_claims_filter and !text.hasControl(f);
        },
        .read_routes => |n| n >= 1 and n <= route_audit.max_page,
    };
}

/// Compare every byte when the lengths match (the Rust `BearerSecret::matches`).
pub fn bearerMatches(secret: []const u8, candidate: []const u8) bool {
    if (secret.len != candidate.len) return false;
    var diff: u8 = 0;
    for (secret, candidate) |a, b| diff |= a ^ b;
    return diff == 0;
}

/// Fixed-memory limit applied only after authentication (Rust default 64/s).
pub const RateLimiter = struct {
    requests: u16 = 64,
    window_ms: i64 = 1000,
    window_start_ms: ?i64 = null,
    accepted: u16 = 0,

    pub fn admit(l: *RateLimiter, now_ms: i64) bool {
        if (l.window_start_ms == null or now_ms - l.window_start_ms.? >= l.window_ms) {
            l.window_start_ms = now_ms;
            l.accepted = 0;
        }
        if (l.accepted >= l.requests) return false;
        l.accepted += 1;
        return true;
    }
};

/// Everything a v1 handler needs; no environment access at request time.
pub const Service = struct {
    io: Io,
    personal: bool,
    version: []const u8,
    build_git: []const u8,
    build_target: []const u8,
    /// Edition state root, resolved without creating it; null reads as empty.
    state_dir: ?[]const u8,
    home_marker: ?[]const u8,
};

fn wireStatus(s: claims.Status) []const u8 {
    return switch (s) {
        .current => "current",
        .partial => "partial",
        .proposed => "proposed",
        .out_of_scope => "out_of_scope",
    };
}

fn claimMatches(c: *const claims.Claim, q: ClaimsQuery, needle: ?[]const u8) bool {
    if (q.status) |st| if (!std.mem.eql(u8, @tagName(st), wireStatus(c.status))) return false;
    const n = needle orelse return true;
    for ([_][]const u8{ c.id, c.capability, c.evidence }) |hay| {
        if (std.ascii.findIgnoreCase(hay, n) != null) return true;
    }
    return false;
}

pub const HandlerError = error{ OutOfMemory, WriteFailed };

/// Write one v1 event (`{"type":..,"payload":..}`).
pub fn writeEvent(arena: std.mem.Allocator, svc: *const Service, w: *Io.Writer, cmd: Command) HandlerError!void {
    switch (cmd) {
        .status => {
            try w.print("{{\"type\":\"status\",\"payload\":{{\"protocol_version\":{d},\"schema_version\":{d},\"edition\":\"{s}\",\"state\":\"ready\",\"version\":", .{ version_v1, schema_v1, if (svc.personal) "personal" else "standard" });
            try json.writeString(w, svc.version);
            try w.writeAll(",\"build_git\":");
            try json.writeString(w, svc.build_git);
            try w.writeAll(",\"build_target\":");
            try json.writeString(w, svc.build_target);
            try w.writeAll(",\"capabilities\":{\"capabilities\":[\"read_status\",\"read_claims\",\"read_routes\"]}}}");
        },
        .claims => |q| {
            const needle: ?[]const u8 = if (q.contains) |c| text.trim(c) else null;
            try w.writeAll("{\"type\":\"claims\",\"payload\":{\"claims\":[");
            var n: usize = 0;
            for (&claims.all) |*c| {
                if (!claimMatches(c, q, needle)) continue;
                if (n != 0) try w.writeByte(',');
                n += 1;
                try w.writeAll("{\"name\":");
                try json.writeString(w, c.id);
                try w.print(",\"status\":\"{s}\",\"note\":", .{wireStatus(c.status)});
                try json.writeString(w, try std.fmt.allocPrint(arena, "{s}. {s}", .{ c.capability, c.evidence }));
                try w.writeAll(",\"instead\":null}");
            }
            try w.print("],\"matched\":{d}}}}}", .{n});
        },
        .read_routes => |limit| {
            const page = try route_audit.readPage(arena, svc.io, svc.state_dir, limit, svc.home_marker);
            try w.writeAll("{\"type\":\"route_audit\",\"payload\":");
            try route_audit.writePage(w, &page);
            try w.writeByte('}');
        },
        // `respond` refuses these with `unsupported_command` before any
        // handler runs; reaching here is a caller bug, reported as a failure.
        .v2_only => return error.WriteFailed,
    }
}

fn writeError(w: *Io.Writer, version: u16, rid: []const u8, code: []const u8, message: []const u8) Io.Writer.Error!void {
    try w.print("{{\"version\":{d},\"request_id\":", .{version});
    try json.writeString(w, rid);
    try w.writeAll(",\"payload\":{\"outcome\":\"error\",\"code\":");
    try json.writeString(w, code);
    try w.writeAll(",\"message\":");
    try json.writeString(w, message);
    try w.writeAll("}}");
}

pub const FrameIssue = enum { empty, oversize };

/// The response to a frame the transport could not deliver whole.
pub fn respondIssue(w: *Io.Writer, issue: FrameIssue) Io.Writer.Error!void {
    return switch (issue) {
        .empty => writeError(w, version_v1, "", "malformed_request", "frame must not be empty"),
        .oversize => writeError(w, version_v1, "", "frame_too_large", "frame exceeds configured limit"),
    };
}

/// Decide and serialize the response to one complete frame into `out`.
/// `out` is reset on a too-large response and replaced with the Rust
/// `response_too_large` error at the same version and request_id.
pub fn respond(arena: std.mem.Allocator, frame: []const u8, bearer: []const u8, limiter: *RateLimiter, now_ms: i64, svc: *const Service, out: *Io.Writer.Allocating) HandlerError!void {
    const w = &out.writer;
    // std: lib/std/json/static.zig (parseFromSliceLeaky; duplicate keys are an error by default)
    const v = std.json.parseFromSliceLeaky(Value, arena, frame, .{}) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => return writeError(w, version_v1, "", "malformed_request", "request is not valid JSON"),
    };
    var candidate: []const u8 = "";
    var echo_version: u16 = version_v1;
    var echo_rid: []const u8 = "";
    if (v == .object) {
        if (v.object.get("bearer")) |b| if (b == .string) {
            candidate = b.string;
        };
        if (v.object.get("version")) |x| if (x == .integer and x.integer >= 0) {
            echo_version = std.math.cast(u16, x.integer) orelse version_v1;
        };
        if (v.object.get("request_id")) |r| if (r == .string and validRequestId(r.string)) {
            echo_rid = r.string;
        };
    }
    if (!bearerMatches(bearer, candidate)) return writeError(w, echo_version, echo_rid, "unauthorized", "authentication failed");
    const auth_version: u16 = if (echo_version >= 1 and echo_version <= 3) echo_version else version_current;
    if (!limiter.admit(now_ms)) return writeError(w, auth_version, echo_rid, "rate_limited", "authenticated request rate limit exceeded");
    const env = decodeEnvelope(v) catch return writeError(w, version_v1, "", "malformed_request", "request is not valid JSON");
    if (!validRequestId(env.request_id)) return writeError(w, version_v1, "", "invalid_request_id", "request_id is invalid");
    if (env.version != version_v1) return writeError(w, version_current, env.request_id, "unsupported_version", "protocol version is unsupported");
    if (env.command == .v2_only) return writeError(w, env.version, env.request_id, "unsupported_command", "command is unavailable in this protocol version");
    if (!validCommand(env.command)) return writeError(w, env.version, env.request_id, "invalid_command", "command payload is invalid");

    try w.print("{{\"version\":{d},\"request_id\":", .{env.version});
    try json.writeString(w, env.request_id);
    try w.writeAll(",\"payload\":{\"outcome\":\"ok\",\"event\":");
    writeEvent(arena, svc, w, env.command) catch |e| switch (e) {
        error.OutOfMemory => {
            out.clearRetainingCapacity();
            return writeError(w, env.version, env.request_id, "handler_failed", "request handling failed");
        },
        else => return e,
    };
    try w.writeAll("}}");
    if (out.written().len > max_frame_len) {
        out.clearRetainingCapacity();
        return writeError(w, env.version, env.request_id, "response_too_large", "handler response exceeds configured limit");
    }
}
