//! `doctor`: an honest snapshot of what a run would use. Local verbs need no
//! executor, so a missing one is reported, not treated as a failure.
const std = @import("std");
const builtin = @import("builtin");
const Ctx = @import("ctx.zig").Ctx;
const config = @import("config/config.zig");
const edition = @import("edition.zig");
const state_mod = @import("state/state.zig");
const backend = @import("agent/backend.zig");
const AgentConfig = @import("agent/argv.zig").AgentConfig;
const persona = @import("persona/router.zig");
const mem = @import("memory/store.zig");
const fsx = @import("util/fsx.zig");
const help = @import("cli/help.zig");
const wdbx = @import("wdbx_bridge.zig");

pub const Error = state_mod.Error || std.Io.Writer.Error;

pub fn run(ctx: Ctx, arena: std.mem.Allocator, cfg: *const config.Config, st: *const state_mod.State, agent: *const AgentConfig, sel: backend.Selection) Error!u8 {
    const w = ctx.out;
    try w.print("{s} {s} (zig {s}, {t}-{t})\n", .{ edition.id.binary_name, help.version, builtin.zig_version_string, builtin.cpu.arch, builtin.os.tag });
    try edition.identityLines(w, st.state_dir);
    const probe: backend.Probe = .{ .ctx = ctx, .arena = arena, .abi_bin = cfg.abi_bin };
    const agent_path: []const u8 = if (agent.agent_path.len != 0) agent.agent_path else if (backend.resolveFor(probe, agent.backend)) |p| p else |_| "(none found: generation needs ollama, grok, fm, abi, claude, or cursor-agent)";
    const chat = try state_mod.resolveChatFor(ctx, arena, st, agent.backend);
    try w.print("agent:     {s}\n", .{agent_path});
    try w.print("model:     {s}\n", .{agent.model});
    try w.print("chat:      {s}\n", .{chat orelse "(none)"});
    try w.print("chat file: {s}\n", .{try state_mod.activeChatFile(st, arena)});
    try w.print("per-cwd:   {}\n", .{st.per_cwd});
    try w.print("cwd:       {s}\n", .{st.cwd});
    try w.print("state:     {s}\n", .{st.state_dir});
    try w.print("auto-review: {}\ntrust:     {}\nforce:     {}\nno-resume: {}\n", .{ agent.auto_review, agent.trust, agent.force, agent.no_resume });
    try w.print("backend:   {s} (from {s})\n", .{ agent.backend.label(), sel.source });
    const selected = persona.select(ctx.getEnv("ABBEY_PERSONA"), "");
    try w.print("persona:     {s} (from router/env/explicit)\n", .{selected.label()});
    try w.print("prior weights abbey={d:.2} aviva={d:.2} abi={d:.2}\n", .{ persona.Weights.prior.abbey, persona.Weights.prior.aviva, persona.Weights.prior.abi });
    try w.writeAll("personas:    Abbey (default) . Aviva (direct) . Abi (orchestrator)\n");
    try w.writeAll("source:      abi-ai router + frozen contracts, ported (abi is a subprocess, never linked)\n");
    try w.print("role.max ->   {s} (executor model binding)\nrole.gemma -> {s} (executor model binding)\n", .{ cfg.role_max, cfg.role_gemma });
    try w.writeAll("note:        Max/Gemma are roles, not bundled Abbey weights\n");
    try w.writeAll("routing:    confidence/alternate/fallback on route.jsonl (audit only, no auto second agent)\n");
    try w.writeAll("learn:      correction|train|preference|routes|digest|export|review|stats|improve (LoRA Proposed)\n");
    const mpath = try mem.pathFor(arena, st.state_dir);
    const present = if (fsx.exists(ctx.io, mpath)) "present" else "will create on first write";
    const requested_note: []const u8 = if (std.ascii.eqlIgnoreCase(cfg.memory_backend, "jsonl")) "" else try std.fmt.allocPrint(arena, " [requested {s}: this rewrite stores memory as JSONL only]", .{cfg.memory_backend});
    try w.print("memory:     jsonl {s} ({s}){s}\n", .{ mpath, present, requested_note });
    try w.writeAll("similarity: lexical feature hash (32-d wyhash n-grams); learned embeddings Proposed\n");
    if (backend.resolveFor(probe, .abi)) |p| {
        try w.print("abi:        {s} (wdbx via `abi wdbx` when invoked; base {s})\n", .{ p, try wdbx.storeBase(arena, st.state_dir) });
    } else |_| try w.writeAll("abi:        (not found: the WDBX CLI bridge is unavailable)\n");
    try config.statusLines(cfg, w);
    try w.writeAll("later:      TUI, daemon, MCP server, OS control, voice are Proposed (see `claims`)\n");
    return 0;
}

test "doctor reports the env-selected abi backend and edition paths" {
    const T = @import("ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var t = T.init(gpa, root);
    defer t.deinit();
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const abi = try std.fs.path.join(a, &.{ root, "abi" });
    try fsx.writeAll(io, abi, "#!/bin/sh\n", .executable_file);
    try t.env.put("HOME", root);
    try t.env.put("PATH", root);
    try t.env.put("ABBEY_BACKEND", "abi");
    try t.env.put("ABBEY_ABI_BIN", abi);
    try t.env.put("ABBEY_MEMORY_BACKEND", "sqlite");
    try t.env.put(edition.id.state_dir_env, try std.fs.path.join(a, &.{ root, "state" }));
    try t.env.put(edition.id.config_path_env, try std.fs.path.join(a, &.{ root, "none.toml" }));
    const c = t.ctx(gpa, io);
    var cfg = try config.load(c);
    defer cfg.deinit();
    const st = try state_mod.load(c, a);
    const sel = backend.select(.{ .ctx = c, .arena = a, .abi_bin = cfg.abi_bin }, cfg.backend);
    const agent: AgentConfig = .{ .backend = sel.backend, .model = "local" };
    try std.testing.expectEqual(@as(u8, 0), try run(c, a, &cfg, &st, &agent, sel));
    const out = t.outText();
    try std.testing.expect(std.mem.find(u8, out, "backend:   abi (from env)") != null);
    try std.testing.expect(std.mem.find(u8, out, try std.fmt.allocPrint(a, "agent:     {s}\n", .{abi})) != null);
    try std.testing.expect(std.mem.find(u8, out, "[requested sqlite: this rewrite stores memory as JSONL only]") != null);
    try std.testing.expect(std.mem.find(u8, out, "unrestricted runtime implemented: false") != null);
    try std.testing.expect(std.mem.find(u8, out, "/state/wdbx/wdbx") != null);
}
