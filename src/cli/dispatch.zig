//! Command dispatch for the P1 surface. Owns the per-invocation arena.
const std = @import("std");
const Ctx = @import("../ctx.zig").Ctx;
const args_mod = @import("args.zig");
const help = @import("help.zig");
const config = @import("../config/config.zig");
const state_mod = @import("../state/state.zig");
const backend = @import("../agent/backend.zig");
const AgentConfig = @import("../agent/argv.zig").AgentConfig;
const argv_mod = @import("../agent/argv.zig");
const models = @import("../models.zig");
const session = @import("../session.zig");
const actions = @import("../actions.zig");
const capture = @import("../capture.zig");
const learn = @import("../learn.zig");
const doctor = @import("../doctor.zig");
const claims = @import("../claims.zig");
const route_log = @import("../route_log.zig");
const edition = @import("../edition.zig");
const wdbx = @import("../wdbx_bridge.zig");
const memory_cmd = @import("memory_cmd.zig");
const fsx = @import("../util/fsx.zig");
const daemon_cli = @import("../daemon/cli.zig");
const tui = @import("../tui/run.zig");

fn envFlag(ctx: Ctx, name: []const u8, default: bool) bool {
    const v = ctx.getEnv(name) orelse return default;
    if (std.mem.eql(u8, v, "0") or std.ascii.eqlIgnoreCase(v, "false")) return false;
    if (std.mem.eql(u8, v, "1") or std.ascii.eqlIgnoreCase(v, "true")) return true;
    return default;
}

/// Rust `apply_global_flags` for the P1 flag set.
pub fn agentConfig(ctx: Ctx, arena: std.mem.Allocator, g: *const args_mod.Globals, st: *const state_mod.State, be: backend.Backend) state_mod.Error!AgentConfig {
    var a: AgentConfig = .{
        .backend = be,
        .auto_review = envFlag(ctx, "ABBEY_AUTO_REVIEW", true),
        .trust = envFlag(ctx, "ABBEY_TRUST", true),
        .force = envFlag(ctx, "ABBEY_FORCE", false),
        .no_resume = envFlag(ctx, "ABBEY_NO_RESUME", false),
        .transcript_dir = try std.fs.path.join(arena, &.{ st.state_dir, be.transcriptSubdir() }),
    };
    const cli_model = g.model orelse ctx.envNonEmpty("ABBEY_MODEL");
    a.model = switch (be) {
        .ollama => argv_mod.ollamaNormalizeModel(cli_model orelse try state_mod.readModelRaw(ctx, arena, st, "local")),
        .abi => try argv_mod.abiNormalizeModel(arena, cli_model orelse try state_mod.readModelRaw(ctx, arena, st, "local")),
        else => if (cli_model) |m| try models.resolveModel(arena, m) else try state_mod.readModel(ctx, arena, st),
    };
    if (g.force) a.force = true;
    if (g.print) a.print = true;
    if (g.output_format) |f| {
        a.output_format = f;
        a.print = true;
    }
    if (g.plan) a.mode = "plan";
    if (g.ask) a.mode = "ask";
    if (g.mode) |m| a.mode = @tagName(m);
    a.add_dirs = g.add_dirs.items;
    if (g.sandbox) |sb| a.sandbox = if (std.mem.eql(u8, sb, "on") or std.mem.eql(u8, sb, "enable") or std.mem.eql(u8, sb, "enabled")) "enabled" else if (std.mem.eql(u8, sb, "off") or std.mem.eql(u8, sb, "disable") or std.mem.eql(u8, sb, "disabled")) "disabled" else sb;
    var extra: std.ArrayList([]const u8) = .empty;
    if (g.debug) try extra.append(arena, "--debug");
    if (g.approve_mcps) try extra.append(arena, "--approve-mcps");
    if (g.max_turns) |n| try extra.appendSlice(arena, &.{ "--max-turns", try std.fmt.allocPrint(arena, "{d}", .{n}) });
    a.extra_args = extra.items;
    return a;
}

fn usageError(ctx: Ctx, comptime fmt: []const u8, a: anytype) u8 {
    ctx.err.print("error: " ++ fmt ++ "\n\nFor more information, try '--help'.\n", a) catch {};
    return 2;
}

/// Run one invocation (`args` excludes argv[0]); returns the exit code.
pub fn run(ctx: Ctx, args: []const []const u8) u8 {
    var arena_state: std.heap.ArenaAllocator = .init(ctx.gpa);
    defer arena_state.deinit();
    return runArena(ctx, arena_state.allocator(), args) catch |e| {
        ctx.err.print("abbey: error: {t}\n", .{e}) catch {};
        return switch (e) {
            error.Usage => 2,
            else => 1,
        };
    };
}

fn runArena(ctx: Ctx, arena: std.mem.Allocator, args: []const []const u8) !u8 {
    var diag: args_mod.Diagnostic = .{};
    const p = args_mod.parse(arena, args, &diag) catch |e| return switch (e) {
        error.UnknownCommand => usageError(ctx, "unrecognized subcommand '{s}'", .{diag.arg}),
        error.UnknownFlag => usageError(ctx, "unexpected argument '{s}' found", .{diag.arg}),
        error.MissingValue => usageError(ctx, "a value is required for '{s}' but none was supplied", .{diag.arg}),
        error.InvalidValue => usageError(ctx, "invalid value for '{s}'", .{diag.arg}),
        error.OutOfMemory => error.OutOfMemory,
    };
    const cmd = p.command orelse {
        try ctx.out.writeAll(help.top);
        return 0;
    };
    if (p.help) {
        try ctx.out.writeAll(help.forCommand(cmd));
        return 0;
    }
    switch (cmd) {
        .help => {
            if (p.positionals.len == 0) {
                try ctx.out.writeAll(help.top);
                return 0;
            }
            const c = args_mod.Command.parse(p.positionals[0]) orelse return usageError(ctx, "unrecognized subcommand '{s}'", .{p.positionals[0]});
            try ctx.out.writeAll(help.forCommand(c));
            return 0;
        },
        .version => {
            try ctx.out.print("{s} {s}\n", .{ edition.id.binary_name, help.version });
            return 0;
        },
        .claims => {
            if (p.positionals.len == 1 and std.mem.eql(u8, p.positionals[0], "--markdown")) try claims.writeMarkdown(ctx.out) else if (p.positionals.len == 0) try claims.writeTable(ctx.out) else return usageError(ctx, "unexpected argument '{s}' found", .{p.positionals[0]});
            return 0;
        },
        .edition => {
            try edition.identityLines(ctx.out, try edition.stateRoot(ctx, arena));
            return 0;
        },
        // Before config/state load: the daemon verbs must not create state
        // directories, and `serve` resolves its own socket path.
        .daemon => return daemon_cli.run(ctx, arena, p.positionals),
        else => {},
    }

    var cfg = try config.load(ctx);
    defer cfg.deinit();
    const st = try state_mod.load(ctx, arena);
    const sel = backend.select(.{ .ctx = ctx, .arena = arena, .abi_bin = cfg.abi_bin }, cfg.backend);
    var agent = try agentConfig(ctx, arena, &p.globals, &st, sel.backend);
    const s: session.Session = .{ .ctx = ctx, .arena = arena, .cfg = &cfg, .state = &st };
    const le: learn.Env = .{ .ctx = ctx, .arena = arena, .state_dir = st.state_dir, .cwd = st.cwd };
    return switch (cmd) {
        .ask => actions.runAgent(s, &agent, p.positionals, actions.RunSpec.ask()),
        .print => capture.runPrint(ctx, arena, &agent, &st, p.positionals, cfg.abi_bin),
        .commit => capture.runCommit(ctx, arena, &agent, &st, cfg.abi_bin),
        .doctor => doctor.run(ctx, arena, &cfg, &st, &agent, sel),
        .learn => learn.dispatch(le, p.positionals),
        .routes => blk: {
            const n = if (p.positionals.len > 0) std.fmt.parseInt(usize, p.positionals[0], 10) catch return usageError(ctx, "invalid value '{s}' for '[N]'", .{p.positionals[0]}) else 10;
            const recs = try route_log.recent(arena, ctx.io, st.state_dir, n);
            if (recs.len == 0) try ctx.out.writeAll("(no route records yet)\n");
            for (recs) |*r| {
                try route_log.formatLine(ctx.out, r);
                try ctx.out.writeByte('\n');
            }
            break :blk 0;
        },
        .config => blk: {
            if (p.positionals.len == 1 and std.mem.eql(u8, p.positionals[0], "--init")) {
                if (fsx.isFile(ctx.io, cfg.path)) {
                    try ctx.out.print("config exists: {s} (unchanged)\n", .{cfg.path});
                } else {
                    try fsx.writeAll(ctx.io, cfg.path, config.default_toml, .default_file);
                    try ctx.out.print("wrote {s}\n", .{cfg.path});
                }
                break :blk 0;
            }
            if (p.positionals.len != 0) return usageError(ctx, "unexpected argument '{s}' found", .{p.positionals[0]});
            try config.statusLines(&cfg, ctx.out);
            break :blk 0;
        },
        .memory => memory_cmd.run(ctx, arena, st.state_dir, p.positionals),
        .wdbx => wdbx.run(ctx, arena, st.state_dir, cfg.abi_bin, p.positionals),
        // After config and state: the App takes this live `agent` by
        // pointer, and Ctrl-B mutates it for every later run.
        .tui => blk: {
            if (p.positionals.len != 0) return usageError(ctx, "unexpected argument '{s}' found", .{p.positionals[0]});
            break :blk tui.cli(ctx, &cfg, &st, &agent, sel);
        },
        .help, .version, .claims, .edition, .daemon => unreachable, // handled above
    };
}
