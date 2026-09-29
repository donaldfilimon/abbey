//! Transport-free tests for `protocol.respond`: exact v1 wire fixtures and
//! the Rust server's error-code order.
const std = @import("std");
const protocol = @import("protocol.zig");

const bearer = "0123456789abcdef0123456789abcdef";

const Harness = struct {
    arena: std.heap.ArenaAllocator,
    out: std.Io.Writer.Allocating,
    limiter: protocol.RateLimiter = .{},
    svc: protocol.Service,

    fn init(state_dir: ?[]const u8) Harness {
        return .{
            .arena = .init(std.testing.allocator),
            .out = .init(std.testing.allocator),
            .svc = .{ .io = std.testing.io, .personal = false, .version = "0.2.0-p2", .build_git = "abc123", .build_target = "aarch64-macos", .state_dir = state_dir, .home_marker = null },
        };
    }

    fn deinit(h: *Harness) void {
        h.arena.deinit();
        h.out.deinit();
    }

    fn send(h: *Harness, frame: []const u8) ![]const u8 {
        h.out.clearRetainingCapacity();
        try protocol.respond(h.arena.allocator(), frame, bearer, &h.limiter, 0, &h.svc, &h.out);
        return h.out.written();
    }

    fn code(h: *Harness, frame: []const u8) ![]const u8 {
        const bytes = try h.send(frame);
        const v = try std.json.parseFromSliceLeaky(std.json.Value, h.arena.allocator(), bytes, .{});
        const payload = v.object.get("payload").?.object;
        return if (payload.get("code")) |c| c.string else "ok";
    }
};

fn req(buf: []u8, version: u16, rid: []const u8, bearer_s: []const u8, command: []const u8) []const u8 {
    return std.fmt.bufPrint(buf, "{{\"version\":{d},\"request_id\":\"{s}\",\"bearer\":\"{s}\",\"command\":{s}}}", .{ version, rid, bearer_s, command }) catch unreachable;
}

test "protocol v1 status round trip is the exact Rust wire fixture" {
    var h = Harness.init(null);
    defer h.deinit();
    var buf: [512]u8 = undefined;
    try std.testing.expectEqualStrings(
        "{\"version\":1,\"request_id\":\"r1\",\"payload\":{\"outcome\":\"ok\",\"event\":{\"type\":\"status\",\"payload\":{\"protocol_version\":1,\"schema_version\":1,\"edition\":\"standard\",\"state\":\"ready\",\"version\":\"0.2.0-p2\",\"build_git\":\"abc123\",\"build_target\":\"aarch64-macos\",\"capabilities\":{\"capabilities\":[\"read_status\",\"read_claims\",\"read_routes\"]}}}}}",
        try h.send(req(&buf, 1, "r1", bearer, "{\"type\":\"status\"}")),
    );
}

test "wrong bearer echoes the caller's version; v2 gets unsupported_version at v2 with its id" {
    var h = Harness.init(null);
    defer h.deinit();
    var buf: [512]u8 = undefined;
    const wrong: [32]u8 = @splat('a');
    try std.testing.expectEqualStrings(
        "{\"version\":99,\"request_id\":\"r1\",\"payload\":{\"outcome\":\"error\",\"code\":\"unauthorized\",\"message\":\"authentication failed\"}}",
        try h.send(req(&buf, 99, "r1", &wrong, "{\"type\":\"status\"}")),
    );
    try std.testing.expectEqualStrings("unauthorized", try h.code("[1,2]"));
    // The Rust DaemonClient sends v2 first and downgrades only on exactly this.
    try std.testing.expectEqualStrings(
        "{\"version\":2,\"request_id\":\"abc-1\",\"payload\":{\"outcome\":\"error\",\"code\":\"unsupported_version\",\"message\":\"protocol version is unsupported\"}}",
        try h.send(req(&buf, 2, "abc-1", bearer, "{\"type\":\"status\"}")),
    );
    try std.testing.expectEqualStrings("unsupported_command", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"get_run\",\"payload\":{}}")));
}

test "request ids, unknown fields, and payload bounds fail closed in the Rust order" {
    var h = Harness.init(null);
    defer h.deinit();
    var buf: [1024]u8 = undefined;
    const long_id: [129]u8 = @splat('x');
    for ([_][]const u8{ "has space", "unicode-\u{3bb}", &long_id }) |rid| {
        const bytes = try h.send(req(&buf, 1, rid, bearer, "{\"type\":\"status\"}"));
        try std.testing.expect(std.mem.startsWith(u8, bytes, "{\"version\":1,\"request_id\":\"\",\"payload\":{\"outcome\":\"error\",\"code\":\"invalid_request_id\""));
    }
    try std.testing.expectEqualStrings("malformed_request", try h.code("not-json"));
    try std.testing.expectEqualStrings("malformed_request", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"status\",\"extra\":true}")));
    try std.testing.expectEqualStrings("malformed_request", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"read_routes\",\"payload\":{\"limit\":7,\"cwd\":\"/Users/x\"}}")));
    try std.testing.expectEqualStrings("malformed_request", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"shell\"}")));
    try std.testing.expectEqualStrings("invalid_command", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"claims\",\"payload\":{\"contains\":\"\\n\"}}")));
    try std.testing.expectEqualStrings("invalid_command", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"claims\",\"payload\":{\"contains\":\"   \"}}")));
    try std.testing.expectEqualStrings("invalid_command", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"read_routes\",\"payload\":{\"limit\":0}}")));
    try std.testing.expectEqualStrings("invalid_command", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"read_routes\",\"payload\":{\"limit\":51}}")));
    // An omitted limit is the cap, not zero.
    const bytes = try h.send(req(&buf, 1, "r", bearer, "{\"type\":\"read_routes\",\"payload\":{}}"));
    try std.testing.expect(std.mem.endsWith(u8, bytes, "{\"type\":\"route_audit\",\"payload\":{\"entries\":[],\"returned\":0,\"limit\":50}}}}"));
    var issue: std.Io.Writer.Allocating = .init(std.testing.allocator);
    defer issue.deinit();
    try protocol.respondIssue(&issue.writer, .oversize);
    try std.testing.expect(std.mem.find(u8, issue.written(), "\"frame_too_large\"") != null);
}

test "claims read the canonical ledger with typed filters" {
    var h = Harness.init(null);
    defer h.deinit();
    var buf: [512]u8 = undefined;
    const a = h.arena.allocator();
    const bytes = try h.send(req(&buf, 1, "c", bearer, "{\"type\":\"claims\",\"payload\":{\"status\":\"out_of_scope\",\"contains\":\" LINKED \"}}"));
    const v = try std.json.parseFromSliceLeaky(std.json.Value, a, bytes, .{});
    const snap = v.object.get("payload").?.object.get("event").?.object.get("payload").?.object;
    try std.testing.expectEqual(@as(i64, 1), snap.get("matched").?.integer);
    const rec = snap.get("claims").?.array.items[0].object;
    try std.testing.expectEqualStrings("linked-abi-wdbx", rec.get("name").?.string);
    try std.testing.expect(rec.get("instead").? == .null);
    // Statuses this ledger never uses are valid filters that match nothing.
    const none = try h.send(req(&buf, 1, "c", bearer, "{\"type\":\"claims\",\"payload\":{\"status\":\"blocked\"}}"));
    try std.testing.expect(std.mem.endsWith(u8, none, "\"claims\":[],\"matched\":0}}}}"));
    try std.testing.expectEqualStrings("malformed_request", try h.code(req(&buf, 1, "c", bearer, "{\"type\":\"claims\",\"payload\":{\"status\":\"nope\"}}")));
}

test "only authenticated requests consume the bounded rate limit" {
    var h = Harness.init(null);
    defer h.deinit();
    h.limiter.requests = 1;
    var buf: [512]u8 = undefined;
    const other: [32]u8 = @splat('b');
    for (0..3) |_| try std.testing.expectEqualStrings("unauthorized", try h.code(req(&buf, 1, "r", &other, "{\"type\":\"status\"}")));
    try std.testing.expectEqualStrings("ok", try h.code(req(&buf, 1, "r", bearer, "{\"type\":\"status\"}")));
    const limited = try h.send(req(&buf, 2, "r", bearer, "{\"type\":\"status\"}"));
    try std.testing.expect(std.mem.startsWith(u8, limited, "{\"version\":2,\"request_id\":\"r\",\"payload\":{\"outcome\":\"error\",\"code\":\"rate_limited\""));
    try std.testing.expect(h.limiter.admit(1000));
}
