//! `wdbx` subcommand: subprocess bridge to `abi wdbx` (vectors live in abi's
//! WDBX, reached only as a subprocess). Port of Rust `wdbx_bridge.rs`.
//! Abbey's store directory `<state>/wdbx/` is the BASE path
//! `<state>/wdbx/wdbx` to abi, which splits its argument into dir + base
//! name; passing the bare directory would read one level up.
const std = @import("std");
const Ctx = @import("ctx.zig").Ctx;
const backend = @import("agent/backend.zig");

pub fn storeBase(arena: std.mem.Allocator, state_dir: []const u8) error{OutOfMemory}![]const u8 {
    return std.fs.path.join(arena, &.{ state_dir, "wdbx", "wdbx" });
}

const value_flags = [_][]const u8{ "--limit", "--text", "--persona" };

/// Whether `args` holds a positional (for `query`, the store path).
pub fn hasPositional(args: []const []const u8) bool {
    var skip = false;
    for (args) |a| {
        if (skip) {
            skip = false;
            continue;
        }
        for (value_flags) |f| if (std.mem.eql(u8, a, f)) {
            skip = true;
            break;
        };
        if (skip) continue;
        if (std.mem.startsWith(u8, a, "-")) continue;
        return true;
    }
    return false;
}

/// argv for `abi` (without argv[0]): `query` gains `--json`, and a bare
/// query targets Abbey's own store base path.
pub fn buildArgv(arena: std.mem.Allocator, args: []const []const u8, default_base: []const u8) error{OutOfMemory}![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    try out.append(arena, "wdbx");
    try out.appendSlice(arena, args);
    if (args.len > 0 and std.mem.eql(u8, args[0], "query")) {
        if (!hasPositional(args[1..])) try out.insert(arena, 2, default_base);
        for (args) |a| {
            if (std.mem.eql(u8, a, "--json")) break;
        } else try out.append(arena, "--json");
    }
    return out.items;
}

pub const Error = error{ OutOfMemory, AbiUnavailable, SpawnFailed } || std.Io.Writer.Error;

pub fn run(ctx: Ctx, arena: std.mem.Allocator, state_dir: []const u8, abi_bin: ?[]const u8, args: []const []const u8) Error!u8 {
    if (args.len == 0 or std.mem.eql(u8, args[0], "help") or std.mem.eql(u8, args[0], "--help")) {
        try ctx.out.writeAll(@import("cli/help.zig").forCommand(.wdbx));
        return 0;
    }
    if (std.mem.eql(u8, args[0], "stats") or std.mem.eql(u8, args[0], "checkpoint")) {
        try ctx.err.print("abbey: `wdbx {s}` reads an in-process WDBX store; this rewrite never links WDBX (Out of scope). Use `wdbx db ...` through abi.\n", .{args[0]});
        return 2;
    }
    const exe = backend.resolveFor(.{ .ctx = ctx, .arena = arena, .abi_bin = abi_bin }, .abi) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        else => {
            try ctx.err.writeAll("abbey: `abi` is not on PATH: the WDBX CLI bridge is unavailable. Set `abi_bin` in config.toml or ABBEY_ABI_BIN.\n");
            return error.AbiUnavailable;
        },
    };
    const argv = try buildArgv(arena, args, try storeBase(arena, state_dir));
    const full = try std.mem.concat(arena, []const u8, &.{ &.{exe}, argv });
    try ctx.out.flush();
    try ctx.err.flush();
    var child = std.process.spawn(ctx.io, .{ .argv = full, .environ_map = ctx.env }) catch return error.SpawnFailed;
    const term = child.wait(ctx.io) catch return error.SpawnFailed;
    return switch (term) {
        .exited => |c| c,
        else => 1,
    };
}

fn expectArgv(expected: []const []const u8, got: []const []const u8) !void {
    try std.testing.expectEqual(expected.len, got.len);
    for (expected, got) |e, g| try std.testing.expectEqualStrings(e, g);
}

test "wdbx bridge argv matches the Rust bridge" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const base = "/state/wdbx/wdbx";
    try std.testing.expectEqualStrings(base, try storeBase(a, "/state"));
    try expectArgv(&.{ "wdbx", "query", "/tmp/store", "--json" }, try buildArgv(a, &.{ "query", "/tmp/store" }, base));
    try expectArgv(&.{ "wdbx", "query", "/tmp/store", "--json" }, try buildArgv(a, &.{ "query", "/tmp/store", "--json" }, base));
    try expectArgv(&.{ "wdbx", "query", base, "--json" }, try buildArgv(a, &.{"query"}, base));
    try expectArgv(&.{ "wdbx", "query", base, "--limit", "5", "--json" }, try buildArgv(a, &.{ "query", "--limit", "5" }, base));
    for ([_][]const []const u8{ &.{ "query", "/elsewhere", "--limit", "5" }, &.{ "query", "--limit", "5", "/elsewhere" }, &.{ "query", "--limit=5", "/elsewhere" } }) |form| {
        const argv = try buildArgv(a, form, base);
        for (argv) |x| try std.testing.expect(std.mem.find(u8, x, "/state/wdbx") == null);
    }
    try std.testing.expect(!hasPositional(&.{ "--limit", "5" }));
    try std.testing.expect(!hasPositional(&.{ "--limit=5", "--json" }));
    try std.testing.expect(hasPositional(&.{ "--limit", "5", "/p" }));
    try expectArgv(&.{ "wdbx", "db", "verify", "/tmp/store" }, try buildArgv(a, &.{ "db", "verify", "/tmp/store" }, base));
}
