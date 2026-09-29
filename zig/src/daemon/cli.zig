//! The `abbey-zig daemon` verb: `serve` (the abbeyd role) plus the read-only
//! `status`, `claims`, and `routes` client commands.
//!
//! Text output reproduces the Rust `format_daemon_event` rendering so an
//! operator reading either tree's output sees the same shape; `--json` emits
//! the typed event.
const std = @import("std");
const Io = std.Io;
const build_options = @import("build_options");
const Ctx = @import("../ctx.zig").Ctx;
const edition = @import("../edition.zig");
const help = @import("../cli/help.zig");
const config = @import("config.zig");
const protocol = @import("protocol.zig");
const server = @import("server.zig");
const client = @import("client.zig");
const route_audit = @import("route_audit.zig");

/// Set by SIGINT/SIGTERM while `serve` is running; the accept loop returns
/// on the next poll tick and unlinks the socket.
var stop_flag: std.atomic.Value(bool) = .init(false);

fn onSignal(_: std.posix.SIG) callconv(.c) void {
    stop_flag.store(true, .release);
}

fn installSignals() void {
    // std: lib/std/posix.zig (Sigaction, sigaction, sigemptyset); the
    // handler only stores to an atomic, which is async-signal-safe.
    const act: std.posix.Sigaction = .{ .handler = .{ .handler = onSignal }, .mask = std.posix.sigemptyset(), .flags = 0 };
    std.posix.sigaction(.INT, &act, null);
    std.posix.sigaction(.TERM, &act, null);
}

fn usage(ctx: Ctx) u8 {
    ctx.err.writeAll("error: expected one of serve, status, claims, routes\n\nFor more information, try '--help'.\n") catch {};
    return 2;
}

fn fail(ctx: Ctx, comptime fmt: []const u8, args: anytype) u8 {
    ctx.err.print("abbey-zig daemon: " ++ fmt ++ "\n", args) catch {};
    return 1;
}

fn service(ctx: Ctx, arena: std.mem.Allocator) protocol.Service {
    // Resolved without creating anything: a read-only command must not make
    // directories (the Rust `audit_state_root` has the same rule).
    const state_dir = edition.stateRoot(ctx, arena) catch null;
    return .{
        .io = ctx.io,
        .personal = edition.active == .personal,
        .version = help.version,
        .build_git = build_options.build_git,
        .build_target = build_options.build_target,
        .state_dir = state_dir,
        .home_marker = route_audit.homeMarker(ctx.envNonEmpty("HOME"), ctx.envNonEmpty("USERPROFILE")),
    };
}

fn writeJson(ctx: Ctx, v: std.json.Value) !void {
    // std: lib/std/json/Stringify.zig (value, Options.whitespace)
    try std.json.Stringify.value(v, .{ .whitespace = .indent_2 }, ctx.out);
    try ctx.out.writeByte('\n');
}

// The renderers read a response from a peer this build did not produce, so
// every field access tolerates a missing or mistyped value instead of
// asserting (a wrong shape prints "-" or 0; it never panics).
fn str(o: std.json.ObjectMap, name: []const u8) []const u8 {
    const v = o.get(name) orelse return "-";
    return switch (v) {
        .string => |s| s,
        else => "-",
    };
}

fn int(o: std.json.ObjectMap, name: []const u8) i64 {
    const v = o.get(name) orelse return 0;
    return switch (v) {
        .integer => |i| i,
        else => 0,
    };
}

fn items(o: std.json.ObjectMap, name: []const u8) []const std.json.Value {
    const v = o.get(name) orelse return &.{};
    return switch (v) {
        .array => |a| a.items,
        else => &.{},
    };
}

fn obj(v: std.json.Value) ?std.json.ObjectMap {
    return switch (v) {
        .object => |o| o,
        else => null,
    };
}

fn statusText(ctx: Ctx, payload: std.json.ObjectMap) !void {
    try ctx.out.print("abbeyd: {s} ({s} edition)\n", .{ str(payload, "state"), str(payload, "edition") });
    try ctx.out.print("protocol: {d} \u{b7} schema: {d}\n", .{ int(payload, "protocol_version"), int(payload, "schema_version") });
    try ctx.out.print("build: {s} \u{b7} {s} \u{b7} {s}\n", .{ str(payload, "version"), str(payload, "build_git"), str(payload, "build_target") });
    try ctx.out.writeAll("capabilities: ");
    const caps = if (payload.get("capabilities")) |c| (if (obj(c)) |co| items(co, "capabilities") else &.{}) else &.{};
    for (caps, 0..) |cap, i| {
        if (i != 0) try ctx.out.writeAll(", ");
        try ctx.out.writeAll(if (cap == .string) cap.string else "-");
    }
    try ctx.out.writeByte('\n');
}

fn claimsText(ctx: Ctx, payload: std.json.ObjectMap) !void {
    try ctx.out.print("abbeyd claims: {d} match(es)\n", .{int(payload, "matched")});
    for (items(payload, "claims")) |item| {
        const o = obj(item) orelse continue;
        const label = if (std.meta.stringToEnum(protocol.ClaimFilter, str(o, "status"))) |st| switch (st) {
            .current => "Current",
            .partial => "Partial",
            .proposed => "Proposed",
            .out_of_scope => "Out of scope",
            else => @tagName(st),
        } else "-";
        try ctx.out.print("  [{s}] {s}\n      {s}\n", .{ label, str(o, "name"), str(o, "note") });
    }
}

fn routesText(ctx: Ctx, payload: std.json.ObjectMap) !void {
    try ctx.out.print("abbeyd routes: {d} of at most {d} decision(s)\n", .{ int(payload, "returned"), int(payload, "limit") });
    for (items(payload, "entries")) |item| {
        const e = obj(item) orelse continue;
        try ctx.out.print("  {s} {s}/{s} {s} {d}% {s} [{s}]\n      alt={s} fb={s} {s}\n", .{
            str(e, "recorded_at"),        str(e, "persona"), str(e, "role"),      str(e, "model"),
            int(e, "confidence_percent"), str(e, "stage"),   str(e, "workspace"), str(e, "alternate"),
            str(e, "fallback"),           str(e, "reason"),
        });
    }
}

fn runClient(ctx: Ctx, arena: std.mem.Allocator, cmd_json: []const u8, expect: []const u8, as_json: bool) u8 {
    const cfg = config.fromEnv(ctx, arena) catch |e| return fail(ctx, "{s}", .{config.describe(e)});
    var derr: client.DaemonError = .{ .code = "", .message = "" };
    const res = client.request(arena, ctx.io, cfg.socket_path, cfg.bearer, cmd_json, expect, &derr) catch |e| return switch (e) {
        error.Daemon => fail(ctx, "daemon rejected the request ({s}): {s}", .{ derr.code, derr.message }),
        error.Connect => fail(ctx, "cannot connect to the daemon at {s}", .{cfg.socket_path}),
        else => fail(ctx, "{t}", .{e}),
    };
    const payload = (if (res.event.object.get("payload")) |p| obj(p) else null) orelse return fail(ctx, "daemon returned an event without a payload object", .{});
    if (as_json) {
        writeJson(ctx, res.event) catch return 1;
        return 0;
    }
    (if (std.mem.eql(u8, expect, "status")) statusText(ctx, payload) else if (std.mem.eql(u8, expect, "claims")) claimsText(ctx, payload) else routesText(ctx, payload)) catch return 1;
    return 0;
}

/// `serve`: bind the edition's socket and answer v1 frames until signalled.
pub fn serve(ctx: Ctx, arena: std.mem.Allocator) u8 {
    const cfg = config.fromEnv(ctx, arena) catch |e| return fail(ctx, "{s}", .{config.describe(e)});
    const svc = service(ctx, arena);
    installSignals();
    stop_flag.store(false, .release);
    ctx.err.print("abbeyd (zig): protocol v1 read-only on {s}\n", .{cfg.socket_path}) catch {};
    ctx.err.flush() catch {};
    server.serve(ctx.gpa, ctx.io, &cfg, &svc, &stop_flag, .{}) catch |e| return fail(ctx, "{s}", .{server.describe(e)});
    return 0;
}

/// `abbey-zig daemon <sub> [flags]`.
pub fn run(ctx: Ctx, arena: std.mem.Allocator, args: []const []const u8) u8 {
    if (args.len == 0) return usage(ctx);
    var as_json = false;
    var limit: ?u16 = null;
    var status_filter: ?[]const u8 = null;
    var contains: ?[]const u8 = null;
    var i: usize = 1;
    while (i < args.len) : (i += 1) {
        const a = args[i];
        if (std.mem.eql(u8, a, "--json")) {
            as_json = true;
        } else if (std.mem.eql(u8, a, "--limit") and i + 1 < args.len) {
            i += 1;
            limit = std.fmt.parseInt(u16, args[i], 10) catch return fail(ctx, "invalid --limit value '{s}'", .{args[i]});
        } else if (std.mem.eql(u8, a, "--status") and i + 1 < args.len) {
            i += 1;
            status_filter = args[i];
        } else if (std.mem.eql(u8, a, "--contains") and i + 1 < args.len) {
            i += 1;
            contains = args[i];
        } else return fail(ctx, "unexpected argument '{s}'", .{a});
    }
    const sub = args[0];
    if (std.mem.eql(u8, sub, "serve")) return serve(ctx, arena);
    if (std.mem.eql(u8, sub, "status")) return runClient(ctx, arena, "{\"type\":\"status\"}", "status", as_json);
    if (std.mem.eql(u8, sub, "claims")) {
        var aw: Io.Writer.Allocating = .init(arena);
        const w = &aw.writer;
        w.writeAll("{\"type\":\"claims\",\"payload\":{") catch return 1;
        if (status_filter) |s| {
            if (std.meta.stringToEnum(protocol.ClaimFilter, s) == null) return fail(ctx, "invalid --status value '{s}'", .{s});
            w.print("\"status\":\"{s}\"", .{s}) catch return 1;
        }
        if (contains) |c| {
            if (status_filter != null) w.writeByte(',') catch return 1;
            w.writeAll("\"contains\":") catch return 1;
            @import("../util/json.zig").writeString(w, c) catch return 1;
        }
        w.writeAll("}}") catch return 1;
        return runClient(ctx, arena, aw.written(), "claims", as_json);
    }
    if (std.mem.eql(u8, sub, "routes")) {
        const n = limit orelse route_audit.max_page;
        const cmd = std.fmt.allocPrint(arena, "{{\"type\":\"read_routes\",\"payload\":{{\"limit\":{d}}}}}", .{n}) catch return 1;
        return runClient(ctx, arena, cmd, "route_audit", as_json);
    }
    return usage(ctx);
}
