//! The live `Host`: panel data from this tree's own modules, backend
//! resolution, and persistence (port of Rust `tui/refresh.rs`).
//!
//! Every read keys off the App's LIVE `agent` (backend, model), never a
//! value captured at startup: the doctor panel, the active chat, and the
//! model listing all follow a Ctrl-B switch.
const std = @import("std");
const Io = std.Io;
const Ctx = @import("../ctx.zig").Ctx;
const app_mod = @import("app.zig");
const App = app_mod.App;
const Panel = app_mod.Panel;
const theme = @import("theme.zig");
const config = @import("../config/config.zig");
const state_mod = @import("../state/state.zig");
const backend = @import("../agent/backend.zig");
const Backend = backend.Backend;
const doctor = @import("../doctor.zig");
const route_log = @import("../route_log.zig");
const mem = @import("../memory/store.zig");
const persona = @import("../persona/router.zig");
const models = @import("../models.zig");
const fsx = @import("../util/fsx.zig");

pub const Live = struct {
    ctx: Ctx,
    cfg: *const config.Config,
    st: *const state_mod.State,
    /// Backend and source chosen at startup, for the doctor's "from" note.
    initial: backend.Selection,
    /// Resolved executor paths and model ids; outlives the App.
    long: std.heap.ArenaAllocator,
    /// Restrict resolution to HOME candidates and PATH (the Rust
    /// ABBEY_TEST_HOME_AGENTS_ONLY hook); tests set it.
    home_only: bool = false,

    pub fn init(ctx: Ctx, cfg: *const config.Config, st: *const state_mod.State, initial: backend.Selection) Live {
        return .{ .ctx = ctx, .cfg = cfg, .st = st, .initial = initial, .long = .init(ctx.gpa) };
    }

    pub fn deinit(self: *Live) void {
        self.long.deinit();
    }

    pub fn host(self: *Live) app_mod.Host {
        return .{ .ud = self, .resolve = resolve, .refresh = refresh, .save_chat = saveChat, .select_model = selectModel, .save_theme = saveTheme };
    }

    fn cast(ud: ?*anyopaque) *Live {
        return @ptrCast(@alignCast(ud.?));
    }

    fn resolve(ud: ?*anyopaque, b: Backend) ?[]const u8 {
        const self = cast(ud);
        return backend.resolveFor(.{ .ctx = self.ctx, .arena = self.long.allocator(), .abi_bin = self.cfg.abi_bin, .home_only = self.home_only }, b) catch null;
    }

    fn saveChat(ud: ?*anyopaque, _: *App, id: []const u8) bool {
        const self = cast(ud);
        var arena: std.heap.ArenaAllocator = .init(self.ctx.gpa);
        defer arena.deinit();
        state_mod.saveChat(self.ctx, arena.allocator(), self.st, id) catch return false;
        return true;
    }

    fn selectModel(ud: ?*anyopaque, _: *App, raw: []const u8) ?[]const u8 {
        const self = cast(ud);
        const a = self.long.allocator();
        const id = models.resolveModel(a, raw) catch return null;
        const line = std.fmt.allocPrint(a, "{s}\n", .{id}) catch return null;
        fsx.writeAll(self.ctx.io, self.st.model_file, line, fsx.owner_only) catch return null;
        return id;
    }

    fn saveTheme(ud: ?*anyopaque, id: theme.Id) void {
        const self = cast(ud);
        var buf: [std.fs.max_path_bytes]u8 = undefined;
        const path = std.fmt.bufPrint(&buf, "{s}/{s}", .{ self.st.state_dir, theme.file_name }) catch return;
        var line: [16]u8 = undefined;
        fsx.writeAll(self.ctx.io, path, std.fmt.bufPrint(&line, "{s}\n", .{id.asStr()}) catch return, .default_file) catch {};
    }

    fn refresh(ud: ?*anyopaque, app: *App, p: Panel) void {
        const self = cast(ud);
        switch (p) {
            .doctor => self.refreshDoctor(app) catch {
                const a = app.doctor.reset();
                app.doctor.items = a.dupe([]const u8, &.{"doctor: refresh failed"}) catch &.{};
                app.chat = null;
            },
            .personas => fill(&app.personas, self.personaLines(app.personas.reset())),
            .memory => fill(&app.memory, self.memoryLines(app.memory.reset())),
            .skills => fill(&app.skills, skillLines(app.skills.reset())),
            .models => fill(&app.models, modelLines(app.models.reset(), app.agent.backend)),
        }
    }

    fn fill(l: *app_mod.Lines, r: error{OutOfMemory}![]const []const u8) void {
        l.items = r catch &.{};
    }

    fn splitLines(a: std.mem.Allocator, text: []const u8) error{OutOfMemory}![]const []const u8 {
        var out: std.ArrayList([]const u8) = .empty;
        var it = std.mem.splitScalar(u8, std.mem.trimEnd(u8, text, "\n"), '\n');
        while (it.next()) |l| try out.append(a, l);
        return out.items;
    }

    fn refreshDoctor(self: *Live, app: *App) !void {
        const a = app.doctor.reset();
        var out: Io.Writer.Allocating = .init(a);
        var sub = self.ctx;
        sub.out = &out.writer;
        const source = if (app.agent.backend == self.initial.backend) self.initial.source else "tui Ctrl-B";
        _ = try doctor.run(sub, a, self.cfg, self.st, app.agent, .{ .backend = app.agent.backend, .source = source });
        app.doctor.items = try splitLines(a, out.written());
        app.chat = try state_mod.resolveChatFor(self.ctx, a, self.st, app.agent.backend);

        // History and the route tail ride the doctor refresh, as in Rust.
        const ha = app.history.reset();
        var rows: std.ArrayList([]const u8) = .empty;
        if (fsx.readOptional(self.ctx.io, ha, self.st.history_file, 4 << 20) catch null) |text| {
            var lines: std.ArrayList([]const u8) = .empty;
            var it = std.mem.splitScalar(u8, text, '\n');
            while (it.next()) |l| if (l.len != 0) try lines.append(ha, l);
            const start = lines.items.len -| 40;
            for (lines.items[start..]) |l| {
                var f = std.mem.splitScalar(u8, l, '\t');
                const ts = f.next() orelse continue;
                const id = f.next() orelse continue;
                try rows.append(ha, try std.fmt.allocPrint(ha, "{s}  {s}  {s}", .{ ts, id, f.rest() }));
            }
        }
        app.history.items = rows.items;

        const ra = app.routes.reset();
        const recs = route_log.recent(ra, self.ctx.io, self.st.state_dir, 8) catch &.{};
        var compact: std.ArrayList([]const u8) = .empty;
        var i = recs.len;
        while (i > 0) {
            i -= 1;
            try compact.append(ra, try compactRoute(ra, &recs[i]));
        }
        app.routes.items = compact.items;
    }

    fn personaLines(self: *Live, a: std.mem.Allocator) error{OutOfMemory}![]const []const u8 {
        const selected = persona.select(self.ctx.getEnv("ABBEY_PERSONA"), "");
        const w = persona.Weights.prior;
        return a.dupe([]const u8, &.{
            try std.fmt.allocPrint(a, "persona: {s} (router/env/explicit)", .{selected.label()}),
            try std.fmt.allocPrint(a, "prior weights abbey={d:.2} aviva={d:.2} abi={d:.2}", .{ w.abbey, w.aviva, w.abi }),
            "personas: Abbey (default) . Aviva (direct) . Abi (orchestrator)",
            try std.fmt.allocPrint(a, "role.max -> {s} (executor model binding)", .{self.cfg.role_max}),
            try std.fmt.allocPrint(a, "role.gemma -> {s} (executor model binding)", .{self.cfg.role_gemma}),
            try std.fmt.allocPrint(a, "default_role: {s}", .{self.cfg.default_role}),
            "routing: route decision -> route.jsonl (alt/fb audit only; no auto second agent)",
            "Tip: ABBEY_PERSONA=aviva . ABBEY_ROLE=max . abbey-zig routes",
        });
    }

    fn memoryLines(self: *Live, a: std.mem.Allocator) error{OutOfMemory}![]const []const u8 {
        var lines: std.ArrayList([]const u8) = .empty;
        const path = try mem.pathFor(a, self.st.state_dir);
        try lines.append(a, try std.fmt.allocPrint(a, "jsonl {s}", .{path}));
        try lines.append(a, "semantic: Proposed (lexical `memory similar` is available)");
        const store = try mem.open(self.ctx.gpa, self.ctx.io, a, self.st.state_dir);
        for ([_][]const u8{ "stm", "ltm", "activity", "train_candidate" }) |layer| {
            const n = if (store.filter(a, layer, null, 500)) |v| v.len else |_| 0;
            try lines.append(a, try std.fmt.allocPrint(a, "{s: <16} {d}", .{ layer, n }));
        }
        if (store.reflect(a)) |r| {
            try lines.append(a, try std.fmt.allocPrint(a, "reflect low={d} dups={d} superseded={d}", .{ r.low_confidence.len, r.duplicate_summaries.len, r.superseded.len }));
        } else |e| try lines.append(a, try std.fmt.allocPrint(a, "unavailable: {t}", .{e}));
        if (store.filter(a, "ltm", "preference", 10)) |prefs| {
            for (prefs[0..@min(prefs.len, 5)]) |p| try lines.append(a, try std.fmt.allocPrint(a, "pref: {s}", .{p.summary}));
        } else |_| {}
        if (store.filter(a, null, null, 12)) |recent| {
            if (recent.len == 0) {
                try lines.append(a, "recent: empty (teach with `abbey-zig learn preference ...`)");
            } else {
                try lines.append(a, "recent (retention, summary); map coordinates: Proposed");
                for (recent) |r| try lines.append(a, try std.fmt.allocPrint(a, "  {s: <16} {s}", .{ r.retention, r.summary }));
            }
        } else |_| {}
        try lines.append(a, "CLI: abbey-zig learn correction|preference|digest|review|stats");
        return lines.items;
    }
};

pub fn skillLines(a: std.mem.Allocator) error{OutOfMemory}![]const []const u8 {
    return a.dupe([]const u8, &.{"skills / plugins / agent-tools inventory: Proposed in abbey-zig (see `claims`)"});
}

/// Rust `AgentConfig::list_models_text` for the backends that answer
/// statically; cursor and grok would exec `<agent> models` (Proposed), so
/// they return nothing and the Models tab shows the alias table.
pub fn modelLines(a: std.mem.Allocator, b: Backend) error{OutOfMemory}![]const []const u8 {
    const text: []const u8 = switch (b) {
        .fm => "system  on-device Apple Foundation Model\npcc     Apple Foundation Model on Private Cloud Compute",
        .abi => "local     deterministic persona-template completion (abi-ai, no network)\nclaude-*  Anthropic via `abi complete --live` (needs abi credentials)\nlive      Anthropic live transport with abi's default model",
        .claude => "opus      Claude Opus (Abbey's default Max binding)\nsonnet    Claude Sonnet (Abbey's Gemma binding)\nhaiku     Claude Haiku\nfable     Claude Fable (plan-gated)\nclaude-*  full Claude catalog id, passed through",
        .ollama => models.ollama_default_model ++ "  default Ollama tag (alias gemma:27b-mlx)\ngemma4:12b-mlx  smaller Ollama tag\n<tag>           any tag `ollama list` reports; passed through",
        .cursor, .grok => return &.{},
    };
    return Live.splitLines(a, text);
}

/// Rust `compact_route_line`: `HH:MM persona/role model conf stage`.
pub fn compactRoute(a: std.mem.Allocator, r: *const route_log.Record) error{OutOfMemory}![]const u8 {
    const clock = if (r.ts.len >= 16) r.ts[11..16] else r.ts;
    return std.fmt.allocPrint(a, "{s} {s}/{s} {s} {d:.2} {s}", .{ clock, r.persona, r.role, r.model, r.confidence, r.stage orelse "-" });
}

test "model listing follows the live backend and route lines compact like Rust" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expectEqualStrings("local     deterministic persona-template completion (abi-ai, no network)", (try modelLines(a, .abi))[0]);
    try std.testing.expectEqual(@as(usize, 5), (try modelLines(a, .claude)).len);
    try std.testing.expectEqual(@as(usize, 0), (try modelLines(a, .cursor)).len);
    const r: route_log.Record = .{ .ts = "2026-09-22T03:22:37Z", .cwd = "/w", .persona = "abbey", .role = "gemma", .model = "local", .reason = "x", .confidence = 0.95 };
    try std.testing.expectEqualStrings("03:22 abbey/gemma local 0.95 -", try compactRoute(a, &r));
}
