//! Hand-written clap-equivalent parser for the P1 surface.
//!
//! Global flags may appear before or after the subcommand, up to the first
//! positional word; from there on every word (including `-`-leading ones)
//! is positional, like clap's `trailing_var_arg + allow_hyphen_values`.
//! `--` ends flag parsing explicitly.
const std = @import("std");

pub const Command = enum {
    ask,
    print,
    commit,
    doctor,
    learn,
    routes,
    config,
    claims,
    memory,
    wdbx,
    edition,
    version,
    help,

    pub fn parse(s: []const u8) ?Command {
        const aliases = [_]struct { []const u8, Command }{
            .{ "p", .print },      .{ "e", .print },     .{ "exec", .print },
            .{ "which", .doctor }, .{ "info", .doctor }, .{ "roadmap", .claims },
            .{ "scope", .claims }, .{ "-V", .version },  .{ "--version", .version },
            .{ "-h", .help },      .{ "--help", .help },
        };
        for (aliases) |a| if (std.mem.eql(u8, s, a[0])) return a[1];
        return std.meta.stringToEnum(Command, s);
    }
};

pub const Mode = enum { ask, plan };

pub const Globals = struct {
    model: ?[]const u8 = null,
    mode: ?Mode = null,
    plan: bool = false,
    ask: bool = false,
    print: bool = false,
    output_format: ?[]const u8 = null,
    force: bool = false,
    add_dirs: std.ArrayList([]const u8) = .empty,
    sandbox: ?[]const u8 = null,
    debug: bool = false,
    approve_mcps: bool = false,
    max_turns: ?u32 = null,
};

pub const Parsed = struct {
    globals: Globals = .{},
    command: ?Command = null,
    help: bool = false,
    positionals: []const []const u8 = &.{},
};

pub const Error = error{ MissingValue, InvalidValue, UnknownFlag, UnknownCommand, OutOfMemory };

pub const Diagnostic = struct { arg: []const u8 = "" };

/// Flags that take a value; `--flag=value` is accepted for each.
const value_flags = [_][]const u8{ "-m", "--model", "--mode", "--output-format", "--add-dir", "--sandbox", "--max-turns" };

fn isValueFlag(name: []const u8) bool {
    for (value_flags) |f| if (std.mem.eql(u8, f, name)) return true;
    return false;
}

fn applyFlag(arena: std.mem.Allocator, g: *Globals, name: []const u8, value: ?[]const u8) Error!bool {
    const eq = std.mem.eql;
    if (eq(u8, name, "-m") or eq(u8, name, "--model")) g.model = value.? else if (eq(u8, name, "--mode")) {
        g.mode = std.meta.stringToEnum(Mode, value.?) orelse return error.InvalidValue;
    } else if (eq(u8, name, "--output-format")) g.output_format = value.? else if (eq(u8, name, "--add-dir")) {
        try g.add_dirs.append(arena, value.?);
    } else if (eq(u8, name, "--sandbox")) g.sandbox = value.? else if (eq(u8, name, "--max-turns")) {
        g.max_turns = std.fmt.parseInt(u32, value.?, 10) catch return error.InvalidValue;
    } else if (eq(u8, name, "--plan")) g.plan = true else if (eq(u8, name, "--ask")) g.ask = true else if (eq(u8, name, "-p") or eq(u8, name, "--print")) {
        g.print = true;
    } else if (eq(u8, name, "-f") or eq(u8, name, "--force") or eq(u8, name, "--yolo") or eq(u8, name, "--always-approve")) {
        g.force = true;
    } else if (eq(u8, name, "--debug")) g.debug = true else if (eq(u8, name, "--approve-mcps")) g.approve_mcps = true else return false;
    return true;
}

pub fn parse(arena: std.mem.Allocator, args: []const []const u8, diag: *Diagnostic) Error!Parsed {
    var p: Parsed = .{};
    var pos: std.ArrayList([]const u8) = .empty;
    var i: usize = 0;
    var flags_done = false;
    while (i < args.len) : (i += 1) {
        const a = args[i];
        diag.arg = a;
        if (!flags_done and pos.items.len == 0) {
            if (std.mem.eql(u8, a, "--")) {
                flags_done = true;
                continue;
            }
            if (p.command != .help and (std.mem.eql(u8, a, "-h") or std.mem.eql(u8, a, "--help"))) {
                if (p.command == null) p.command = .help else p.help = true;
                continue;
            }
            if (p.command == null and (std.mem.eql(u8, a, "-V") or std.mem.eql(u8, a, "--version"))) {
                p.command = .version;
                continue;
            }
            if (a.len > 1 and a[0] == '-') {
                var name = a;
                var value: ?[]const u8 = null;
                if (std.mem.findScalar(u8, a, '=')) |eqi| if (std.mem.startsWith(u8, a, "--")) {
                    name = a[0..eqi];
                    value = a[eqi + 1 ..];
                };
                if (isValueFlag(name) and value == null) {
                    if (i + 1 >= args.len) return error.MissingValue;
                    i += 1;
                    value = args[i];
                }
                if (try applyFlag(arena, &p.globals, name, value)) continue;
                // After a subcommand, an unknown flag is the verb's own
                // (`config --init`, `claims --markdown`); the verb validates it.
                if (p.command == null) return error.UnknownFlag;
                try pos.append(arena, a);
                continue;
            }
            if (p.command == null) {
                p.command = Command.parse(a) orelse return error.UnknownCommand;
                continue;
            }
        }
        try pos.append(arena, a);
    }
    p.positionals = pos.items;
    return p;
}

test "globals before and after the subcommand, trailing prompt keeps dashes" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    var d: Diagnostic = .{};
    const p = try parse(arena.allocator(), &.{ "-m", "local", "ask", "--force", "hello", "--not-a-flag", "-x" }, &d);
    try std.testing.expectEqual(Command.ask, p.command.?);
    try std.testing.expectEqualStrings("local", p.globals.model.?);
    try std.testing.expect(p.globals.force);
    try std.testing.expectEqual(@as(usize, 3), p.positionals.len);
    try std.testing.expectEqualStrings("--not-a-flag", p.positionals[1]);
    const q = try parse(arena.allocator(), &.{ "print", "--", "--literal" }, &d);
    try std.testing.expectEqualStrings("--literal", q.positionals[0]);
    const r = try parse(arena.allocator(), &.{ "--model=x", "--mode=plan", "p", "hi" }, &d);
    try std.testing.expectEqual(Command.print, r.command.?);
    try std.testing.expectEqual(Mode.plan, r.globals.mode.?);
}

test "help, version, aliases, and errors" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var d: Diagnostic = .{};
    try std.testing.expectEqual(Command.help, (try parse(a, &.{"--help"}, &d)).command.?);
    const h = try parse(a, &.{ "learn", "--help" }, &d);
    try std.testing.expect(h.help and h.command.? == .learn);
    // `learn review --help` is a positional word, handled by the learn verb.
    try std.testing.expect(!(try parse(a, &.{ "learn", "review", "--help" }, &d)).help);
    try std.testing.expectEqual(Command.version, (try parse(a, &.{"-V"}, &d)).command.?);
    try std.testing.expectEqual(Command.doctor, (try parse(a, &.{"which"}, &d)).command.?);
    try std.testing.expectError(error.UnknownCommand, parse(a, &.{"frobnicate"}, &d));
    try std.testing.expectError(error.UnknownFlag, parse(a, &.{"--frob"}, &d));
    const v = try parse(a, &.{ "config", "--init" }, &d);
    try std.testing.expectEqualStrings("--init", v.positionals[0]);
    try std.testing.expectError(error.MissingValue, parse(a, &.{"-m"}, &d));
    try std.testing.expectError(error.InvalidValue, parse(a, &.{ "--mode", "yolo" }, &d));
}
