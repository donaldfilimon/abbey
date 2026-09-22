//! The one canonical execution path: `actions.runAgent(RunSpec)` ->
//! `hybridRun` (persona x role wrap, preference context, route audit,
//! activity memory) -> `run.runResilient`. Port of Rust `session.rs`.
const std = @import("std");
const Ctx = @import("ctx.zig").Ctx;
const config = @import("config/config.zig");
const argv_mod = @import("agent/argv.zig");
const AgentConfig = argv_mod.AgentConfig;
const run = @import("agent/run.zig");
const persona = @import("persona/router.zig");
const roles = @import("roles.zig");
const route_log = @import("route_log.zig");
const state_mod = @import("state/state.zig");
const learn = @import("learn.zig");
const mem = @import("memory/store.zig");
const newrec = @import("memory/new.zig");
const models = @import("models.zig");
const time = @import("util/time.zig");

pub const Session = struct {
    ctx: Ctx,
    arena: std.mem.Allocator,
    cfg: *const config.Config,
    state: *const state_mod.State,
};

pub const Error = run.Error || learn.Error || route_log.AppendError;

/// `note \n\n [prefs \n] wrapped` exactly as Rust `assemble_prompt`.
pub fn assemblePrompt(arena: std.mem.Allocator, p: persona.Profile, role: roles.Role, user_body: []const u8, prefs: []const u8) error{OutOfMemory}![]const u8 {
    const wrapped = try persona.wrap(arena, p, user_body);
    const note = roles.systemNote(role);
    if (prefs.len == 0) return std.mem.concat(arena, u8, &.{ note, "\n\n", wrapped });
    return std.mem.concat(arena, u8, &.{ note, "\n\n", prefs, "\n", wrapped });
}

fn maybeInjectRoleModel(s: Session, agent: *AgentConfig, role: roles.Role, alias: []const u8) Error!void {
    // Never under fm (system|pcc vocabulary) or abi (a cursor id would read
    // as an explicit live-transport request).
    if (agent.backend == .fm or agent.backend == .abi) return;
    if (s.ctx.getEnv("ABBEY_MODEL") != null) return;
    if (!std.mem.eql(u8, agent.model, try state_mod.readModel(s.ctx, s.arena, s.state))) return;
    if (agent.backend == .ollama) {
        agent.model = argv_mod.ollamaNormalizeModel(alias);
        return;
    }
    const portable = if (std.mem.eql(u8, alias, models.ollama_default_model)) roles.defaultModelForRole(role) else alias;
    agent.model = try models.resolveModel(s.arena, portable);
}

fn recordActivity(s: Session, p: persona.Profile, role: roles.Role, joined: []const u8, model: []const u8) void {
    const store = mem.open(s.ctx.gpa, s.ctx.io, s.arena, s.state.state_dir) catch return;
    const summary = std.fmt.allocPrint(s.arena, "route {s}/{s}", .{ p.label(), role.label() }) catch return;
    const payload = std.fmt.allocPrint(s.arena, "{s}\n\u{2192} model {s}", .{ joined, model }) catch return;
    var r = newrec.stm(s.ctx, s.arena, summary, payload) catch return;
    r.tags = &.{ "stm", "activity" };
    r.retention = "activity";
    store.store(s.arena, r) catch {};
}

pub fn hybridRun(s: Session, agent: *AgentConfig, fresh: bool, prompt: []const []const u8, role_override: ?roles.Role) Error!u8 {
    const joined = try std.mem.join(s.arena, " ", prompt);
    const p = persona.select(s.ctx.getEnv("ABBEY_PERSONA"), joined);
    const override = role_override orelse roles.Role.parse(s.cfg.default_role);
    const d = try roles.decide(s.arena, joined, override, s.ctx.getEnv("ABBEY_ROLE"));
    const alias = switch (d.primary) {
        .gemma => s.cfg.role_gemma,
        .max, .auto => if (s.cfg.role_max.len == 0) roles.defaultModelForRole(d.primary) else s.cfg.role_max,
    };
    try maybeInjectRoleModel(s, agent, d.primary, alias);

    const le: learn.Env = .{ .ctx = s.ctx, .arena = s.arena, .state_dir = s.state.state_dir, .cwd = s.state.cwd };
    const prefs = try learn.preferenceContext(le, 8);
    const final = try assemblePrompt(s.arena, p, d.primary, joined, prefs);

    var tb: [time.seconds_len]u8 = undefined;
    var tools: std.ArrayList([]const u8) = .empty;
    for (agent.extra_args) |a| if (std.mem.eql(u8, a, "--approve-mcps")) {
        try tools.append(s.arena, "mcp");
        break;
    };
    const rec: route_log.Record = .{
        .ts = time.formatSeconds(&tb, time.nowMillis(s.ctx.io)),
        .cwd = s.state.cwd,
        .persona = p.label(),
        .role = d.primary.label(),
        .model = agent.model,
        .reason = try std.fmt.allocPrint(s.arena, "persona={s} role={s} class={s}", .{ p.label(), d.primary.label(), d.class.debugName() }),
        .confidence = d.confidence,
        .tools = tools.items,
        .alternate = if (d.alternate) |r| r.label() else null,
        .fallback = d.fallback,
    };
    // Best effort, as in Rust (`let _ = append_route_record(..)`), but loud.
    route_log.append(s.ctx.gpa, s.ctx.io, s.state.state_dir, &rec) catch |e| {
        try s.ctx.err.print("abbey: route log append failed: {t}\n", .{e});
    };
    recordActivity(s, p, d.primary, joined, agent.model);
    return run.runResilient(s.ctx, s.arena, agent, s.state, fresh, &.{final}, s.cfg.abi_bin);
}

test "assembled ask prompt matches the Rust-built abi argv element" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    // Captured from Rust abbey 2.6.0 `ask "fix the compile error in main"`
    // under ABBEY_BACKEND=abi with an argv-echo stub (2026-09-21).
    const expected = "Worker role: Gemma (visual/conversational). Prioritize clear human-facing interpretation, tone, and multimodal description when relevant.\n\nAbbey: fix the compile error in main\n\nI\u{2019}ll approach this with warmth, creativity, and technical care while keeping uncertainty explicit.";
    const got = try assemblePrompt(arena.allocator(), persona.select(null, "fix the compile error in main"), .gemma, "fix the compile error in main", "");
    try std.testing.expectEqualStrings(expected, got);
    const with_prefs = try assemblePrompt(arena.allocator(), .abbey, .max, "x", "Standing user preferences (from Abbey self-learn LTM):\n- p\n");
    try std.testing.expect(std.mem.find(u8, with_prefs, "Max (technical)") != null);
    try std.testing.expect(std.mem.find(u8, with_prefs, "- p\n\nAbbey: x") != null);
}
