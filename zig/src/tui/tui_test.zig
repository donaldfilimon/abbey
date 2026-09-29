//! TUI tests with no real terminal: the key state machine (ported from Rust
//! `keys_tests.rs`), golden frames per tab, the raw-mode restore paths
//! through a recording fake terminal, and the Ctrl-B continuity regression
//! through the real event loop and the canonical run path.
const std = @import("std");
const Io = std.Io;
const app_mod = @import("app.zig");
const App = app_mod.App;
const Key = app_mod.Key;
const Backend = @import("../agent/backend.zig").Backend;
const AgentConfig = @import("../agent/argv.zig").AgentConfig;
const frame = @import("frame.zig");
const ui = @import("ui.zig");
const term = @import("term.zig");
const run = @import("run.zig");
const host_mod = @import("host.zig");
const theme = @import("theme.zig");
const T = @import("../ctx.zig").TestCtx;
const tmpPath = @import("../ctx.zig").tmpPath;
const config = @import("../config/config.zig");
const state_mod = @import("../state/state.zig");
const route_log = @import("../route_log.zig");
const fsx = @import("../util/fsx.zig");
const edition = @import("../edition.zig");

// ---- a fake host: resolution from a table, recorded side effects ----

const FakeHost = struct {
    resolvable: []const Backend = &.{},
    refreshes: std.ArrayList(struct { app_mod.Panel, Backend }) = .empty,
    saved_chat: ?[]const u8 = null,
    saved_theme: ?theme.Id = null,

    fn host(self: *FakeHost) app_mod.Host {
        return .{ .ud = self, .resolve = resolve, .refresh = refresh, .save_chat = saveChat, .select_model = selectModel, .save_theme = saveTheme };
    }
    fn cast(ud: ?*anyopaque) *FakeHost {
        return @ptrCast(@alignCast(ud.?));
    }
    fn resolve(ud: ?*anyopaque, b: Backend) ?[]const u8 {
        const self = cast(ud);
        if (std.mem.findScalar(Backend, self.resolvable, b) == null) return null;
        return switch (b) {
            inline else => |tag| "/fake/bin/" ++ @tagName(tag),
        };
    }
    fn refresh(ud: ?*anyopaque, app: *App, p: app_mod.Panel) void {
        const self = cast(ud);
        self.refreshes.append(std.testing.allocator, .{ p, app.agent.backend }) catch {};
    }
    fn saveChat(ud: ?*anyopaque, _: *App, id: []const u8) bool {
        cast(ud).saved_chat = id;
        return true;
    }
    fn selectModel(_: ?*anyopaque, _: *App, raw: []const u8) ?[]const u8 {
        return if (std.mem.eql(u8, raw, "opus")) "claude-opus-5-thinking-high" else null;
    }
    fn saveTheme(ud: ?*anyopaque, id: theme.Id) void {
        cast(ud).saved_theme = id;
    }
};

const Harness = struct {
    fake: FakeHost = .{},
    agent: AgentConfig = .{ .backend = .cursor, .model = "auto" },
    app: App = undefined,

    fn init(self: *Harness) !void {
        self.app = try App.init(std.testing.allocator, self.fake.host(), &self.agent, "/state", .ink);
    }
    fn deinit(self: *Harness) void {
        self.app.deinit();
        self.fake.refreshes.deinit(std.testing.allocator);
    }
    fn typeStr(self: *Harness, s: []const u8) void {
        var it = (std.unicode.Utf8View.init(s) catch unreachable).iterator();
        while (it.nextCodepoint()) |cp| self.app.handleKey(.{ .char = cp });
    }
    fn setLines(l: *app_mod.Lines, rows: []const []const u8) !void {
        const a = l.reset();
        l.items = try a.dupe([]const u8, rows);
    }
};

test "editor is UTF-8 safe at every cursor move" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    h.typeStr("a\u{e9}z");
    try std.testing.expectEqualStrings("a\u{e9}z", h.app.input.items);
    try std.testing.expectEqual(h.app.input.items.len, h.app.cursor);
    h.app.handleKey(.left);
    h.app.handleKey(.left);
    h.app.handleKey(.backspace);
    try std.testing.expectEqualStrings("\u{e9}z", h.app.input.items);
    h.app.handleKey(.delete);
    try std.testing.expectEqualStrings("z", h.app.input.items);
    h.app.handleKey(.end);
    try std.testing.expectEqual(@as(usize, 1), h.app.cursor);
    h.app.handleKey(.home);
    try std.testing.expectEqual(@as(usize, 0), h.app.cursor);
}

test "enter runs plain prompts and refuses slash commands as Proposed" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    h.typeStr("hello world");
    h.app.handleKey(.enter);
    try std.testing.expectEqual(app_mod.Pending.run_resume, h.app.pending);
    try std.testing.expectEqualStrings("hello world", h.app.input_history.items[0]);
    h.app.pending = .none;
    h.app.clearInput();
    h.typeStr("/doctor now");
    h.app.handleKey(.enter);
    try std.testing.expectEqual(app_mod.Pending.none, h.app.pending);
    try std.testing.expect(std.mem.find(u8, h.app.status(), "Proposed") != null);
    try std.testing.expectEqualStrings("/doctor now", h.app.input.items);
    h.app.clearInput();
    h.app.handleKey(.{ .ctrl = 'n' });
    try std.testing.expectEqual(app_mod.Pending.run_fresh, h.app.pending);
}

test "prompt history recalls and dedupes" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    for ([_][]const u8{ "first", "second", "second" }) |s| {
        h.typeStr(s);
        h.app.handleKey(.enter);
        h.app.clearInput();
    }
    try std.testing.expectEqual(@as(usize, 2), h.app.input_history.items.len);
    h.app.handleKey(.up);
    try std.testing.expectEqualStrings("second", h.app.input.items);
    h.app.handleKey(.up);
    try std.testing.expectEqualStrings("first", h.app.input.items);
    h.app.handleKey(.down);
    try std.testing.expectEqualStrings("second", h.app.input.items);
    h.app.handleKey(.down);
    try std.testing.expectEqualStrings("", h.app.input.items);
}

test "esc clears input before it quits; ctrl-q quits from any focus" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    h.typeStr("draft");
    h.app.handleKey(.esc);
    try std.testing.expectEqualStrings("", h.app.input.items);
    try std.testing.expect(!h.app.should_quit);
    h.app.handleKey(.esc);
    try std.testing.expect(h.app.should_quit);
    for ([_]app_mod.Focus{ .prompt, .panel }) |focus| {
        var g: Harness = .{};
        try g.init();
        defer g.deinit();
        g.app.focus = focus;
        g.app.handleKey(.{ .ctrl = 'q' });
        try std.testing.expect(g.app.should_quit);
    }
}

test "focus toggle, tab keys, filter reset, and backtick inside a prompt" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    h.app.handleKey(.{ .char = '`' });
    try std.testing.expectEqual(app_mod.Focus.panel, h.app.focus);
    h.app.handleKey(.{ .char = '2' });
    try std.testing.expectEqual(app_mod.Tab.chats, h.app.tab);
    h.app.handleKey(.{ .char = '7' });
    try std.testing.expectEqual(app_mod.Tab.doctor, h.app.tab);
    try h.app.filter.appendSlice(std.testing.allocator, "stale");
    h.app.handleKey(.tab);
    try std.testing.expectEqual(app_mod.Tab.home, h.app.tab);
    try std.testing.expectEqual(@as(usize, 0), h.app.filter.items.len);
    h.app.handleKey(.backtab);
    try std.testing.expectEqual(app_mod.Tab.doctor, h.app.tab);
    h.app.handleKey(.{ .char = 'q' });
    try std.testing.expect(h.app.should_quit);

    var g: Harness = .{};
    try g.init();
    defer g.deinit();
    g.typeStr("cargo ");
    g.app.handleKey(.{ .char = '`' });
    try std.testing.expectEqual(app_mod.Focus.prompt, g.app.focus);
    try std.testing.expectEqualStrings("cargo `", g.app.input.items);
}

test "palette filters, runs an action, and never leaks into the prompt" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    h.app.handleKey(.{ .ctrl = 'k' });
    try std.testing.expectEqual(app_mod.Overlay.palette, h.app.overlay);
    h.typeStr("the");
    try std.testing.expectEqualStrings("the", h.app.overlay_query.items);
    try std.testing.expectEqual(@as(usize, 0), h.app.input.items.len);
    h.app.handleKey(.enter); // "Cycle theme" is the only match
    try std.testing.expectEqual(app_mod.Overlay.none, h.app.overlay);
    try std.testing.expectEqual(theme.Id.violet, h.app.theme_id);
    try std.testing.expectEqual(theme.Id.violet, h.fake.saved_theme.?);
    h.app.handleKey(.{ .ctrl = 'k' });
    h.app.handleKey(.esc);
    try std.testing.expectEqual(app_mod.Overlay.none, h.app.overlay);
    try std.testing.expectEqual(@as(usize, 0), h.app.overlay_query.items.len);
}

test "list keys saturate, filter narrows, enter activates a chat and a model" {
    var h: Harness = .{};
    try h.init();
    defer h.deinit();
    try Harness.setLines(&h.app.history, &.{ "2026-09-22T01:00:00.000Z  chat-one  /w/a", "2026-09-22T02:00:00.000Z  chat-two  /w/b" });
    h.app.focus = .panel;
    h.app.tab = .chats;
    h.app.handleKey(.up);
    try std.testing.expectEqual(@as(usize, 0), h.app.list_idx);
    for (0..5) |_| h.app.handleKey(.down);
    try std.testing.expectEqual(@as(usize, 1), h.app.list_idx);
    h.app.handleKey(.enter);
    try std.testing.expectEqualStrings("chat-two", h.fake.saved_chat.?);
    h.app.handleKey(.{ .char = '/' });
    h.typeStr("/a");
    h.app.handleKey(.enter);
    try std.testing.expectEqual(@as(usize, 1), h.app.listLen());
    try std.testing.expectEqualStrings("2026-09-22T01:00:00.000Z  chat-one  /w/a", h.app.filteredAt(0).?);
    h.app.enterTab(.models);
    h.app.handleKey(.{ .char = '4' - 1 + 1 }); // '4' = memory; back to models below
    h.app.enterTab(.models);
    try std.testing.expectEqualStrings("auto         auto", h.app.filteredAt(0).?);
    h.app.list_idx = 3; // "opus"
    h.app.handleKey(.enter);
    try std.testing.expectEqualStrings("claude-opus-5-thinking-high", h.agent.model);
}

test "ctrl-b skips unresolvable backends and moves path and transcript dir with the live value" {
    var h: Harness = .{};
    h.fake.resolvable = &.{ .abi, .claude };
    h.agent = .{ .backend = .ollama, .model = "gemma4:26b-mlx", .agent_path = "/fake/bin/ollama", .transcript_dir = "/state/ollama" };
    try h.init();
    defer h.deinit();
    h.app.handleKey(.{ .ctrl = 'b' }); // grok and fm do not resolve
    try std.testing.expectEqual(Backend.abi, h.agent.backend);
    try std.testing.expectEqualStrings("/fake/bin/abi", h.agent.agent_path);
    try std.testing.expectEqualStrings("/state/abi", h.agent.transcript_dir.?);
    try std.testing.expectEqualStrings("gemma4:26b-mlx", h.agent.model); // carried over, as in Rust
    try std.testing.expectEqualStrings("backend -> abi", h.app.status());
    // The refreshes that followed saw the new backend, not the startup one.
    for (h.fake.refreshes.items) |r| try std.testing.expectEqual(Backend.abi, r[1]);
    h.app.handleKey(.{ .ctrl = 'b' });
    try std.testing.expectEqual(Backend.claude, h.agent.backend);
    h.app.handleKey(.{ .ctrl = 'b' }); // cursor, ollama, grok, fm do not resolve
    try std.testing.expectEqual(Backend.abi, h.agent.backend);
    h.fake.resolvable = &.{};
    h.app.handleKey(.{ .ctrl = 'b' });
    try std.testing.expectEqual(Backend.abi, h.agent.backend);
    try std.testing.expectEqualStrings("backend: no other executor resolvable on this host", h.app.status());
}

// ---- golden frames ----

fn goldenApp(h: *Harness) !void {
    h.agent = .{ .backend = .abi, .model = "local", .agent_path = "/fake/bin/abi" };
    try h.init();
    const a = &h.app;
    a.cwd = "/Users/test/dev/abbey-zig";
    a.home = "/Users/test";
    a.chat = "4f1c2a9e-7b3d-4e21-9a55-0c1d2e3f4a5b";
    a.last_code = 0;
    try Harness.setLines(&a.history, &.{
        "2026-09-21T22:10:05.120Z  0b7e44d1-2c3a-4f55-8e10-9a1b2c3d4e5f  /Users/test/dev/abbey",
        "2026-09-22T03:22:37.004Z  4f1c2a9e-7b3d-4e21-9a55-0c1d2e3f4a5b  /Users/test/dev/abbey-zig",
        "2026-09-22T03:40:11.500Z  9c2d7e60-1a2b-4c3d-8e4f-5a6b7c8d9e0f  /private/tmp/a-very-long-scratch-directory/for/tests",
    });
    try Harness.setLines(&a.routes, &.{ "03:22 abbey/gemma local 0.95 -", "03:40 aviva/max local 0.85 -" });
    try Harness.setLines(&a.personas, &.{
        "persona: abbey (router/env/explicit)",
        "prior weights abbey=0.40 aviva=0.30 abi=0.30",
        "personas: Abbey (default) . Aviva (direct) . Abi (orchestrator)",
        "role.max -> max (executor model binding)",
        "role.gemma -> gemma (executor model binding)",
        "default_role: auto",
    });
    try Harness.setLines(&a.memory, &.{ "jsonl /state/memory/memory.jsonl", "stm              2", "ltm              1", "activity         4", "train_candidate  0", "pref: answer tersely" });
    a.skills.items = try host_mod.skillLines(a.skills.reset());
    a.models.items = try host_mod.modelLines(a.models.reset(), .abi);
    try Harness.setLines(&a.doctor, &.{ "abbey-zig 0.3.0-p3", "agent:     /fake/bin/abi", "model:     local", "chat:      4f1c2a9e-7b3d-4e21-9a55-0c1d2e3f4a5b", "backend:   abi (from env)" });
}

fn checkGolden(app: *App, name: []const u8) !void {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var f = try frame.Frame.init(gpa, 100, 30);
    defer f.deinit(gpa);
    app.ensureVisible(ui.listViewport(app, 30));
    ui.draw(&f, app);
    var got: Io.Writer.Allocating = .init(gpa);
    defer got.deinit();
    try f.writePlain(&got.writer);
    var pb: [128]u8 = undefined;
    const path = try std.fmt.bufPrint(&pb, "tests/golden/tui/{s}.txt", .{name});
    const want = Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(64 * 1024)) catch |e| {
        try writeActual(name, got.written());
        std.debug.print("missing golden {s}: {t} (actual in .zig-cache/tmp/tui-golden)\n", .{ path, e });
        return e;
    };
    defer gpa.free(want);
    if (!std.mem.eql(u8, want, got.written())) try writeActual(name, got.written());
    try std.testing.expectEqualStrings(want, got.written());
}

fn writeActual(name: []const u8, bytes: []const u8) !void {
    var pb: [128]u8 = undefined;
    try fsx.writeAll(std.testing.io, try std.fmt.bufPrint(&pb, ".zig-cache/tmp/tui-golden/{s}.txt", .{name}), bytes, .default_file);
}

test "golden frames for all seven tabs and both overlays at 100x30" {
    var h: Harness = .{};
    try goldenApp(&h);
    defer h.deinit();
    for (app_mod.Tab.all) |tab| {
        h.app.tab = tab;
        h.app.focus = if (tab == .home) .prompt else .panel;
        h.app.list_idx = 1;
        h.app.scroll = 0;
        try checkGolden(&h.app, @tagName(tab));
    }
    h.app.tab = .home;
    h.app.focus = .prompt;
    h.app.overlay = .help;
    try checkGolden(&h.app, "help");
    h.app.overlay = .palette;
    try h.app.overlay_query.appendSlice(std.testing.allocator, "c");
    try checkGolden(&h.app, "palette");
}

test "too-small terminals get a message, not a broken layout" {
    var h: Harness = .{};
    try goldenApp(&h);
    defer h.deinit();
    const gpa = std.testing.allocator;
    var f = try frame.Frame.init(gpa, 30, 8);
    defer f.deinit(gpa);
    ui.draw(&f, &h.app);
    var got: Io.Writer.Allocating = .init(gpa);
    defer got.deinit();
    try f.writePlain(&got.writer);
    try std.testing.expect(std.mem.startsWith(u8, got.written(), "terminal too small (30x8)\n"));
}

// ---- restore paths through the event loop ----

const NoRun = struct {
    fn noop(_: ?*anyopaque, _: *AgentConfig, _: []const u8, _: bool) u8 {
        return 0;
    }
    const runner: run.Runner = .{ .ud = null, .run = noop };
};

test "the loop restores termios on quit, on a terminal error, and on SIGTERM" {
    const gpa = std.testing.allocator;
    inline for (.{ "quit", "error", "signal" }) |mode| {
        var h: Harness = .{};
        try h.init();
        defer h.deinit();
        var f = term.Fake.init(gpa);
        defer f.deinit();
        const orig = f.attr;
        if (comptime std.mem.eql(u8, mode, "quit")) {
            f.script = &.{"\x11"};
            try std.testing.expectEqual(@as(u8, 0), try run.loop(gpa, f.terminal(), &h.app, NoRun.runner));
        } else if (comptime std.mem.eql(u8, mode, "error")) {
            f.fail_read_at = 1;
            try std.testing.expectError(error.TerminalFailed, run.loop(gpa, f.terminal(), &h.app, NoRun.runner));
        } else {
            f.on_read = struct {
                fn raise(fk: *term.Fake) void {
                    if (fk.reads == 1) std.posix.raise(.TERM) catch {};
                }
            }.raise;
            try std.testing.expectEqual(run.signal_exit, try run.loop(gpa, f.terminal(), &h.app, NoRun.runner));
        }
        try std.testing.expect(f.restoredTo(orig));
        try std.testing.expect(std.mem.endsWith(u8, f.written.items, term.leave_seq));
        try std.testing.expect(std.mem.find(u8, f.written.items, term.enter_seq) != null);
    }
}

// ---- the Ctrl-B continuity regression ----

const stub = "#!/bin/sh\nprintf '%s' \"$0\" >> \"$HOME/calls.log\"; for a in \"$@\"; do printf ' [%s]' \"$a\" >> \"$HOME/calls.log\"; done; printf '\\n' >> \"$HOME/calls.log\"; echo stub-output\n";

test "after Ctrl-B the next ask runs the LIVE backend, not the env's, and never adopts the old backend's chat" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var t = T.init(gpa, root);
    defer t.deinit();
    try t.env.put("HOME", root);
    try t.env.put("PATH", root);
    // Both environment signals point at cursor: a code path that re-read
    // them after the switch would run cursor-agent or resume its chat.
    try t.env.put("ABBEY_BACKEND", "cursor");
    try t.env.put("CURSOR_AGENT_CHAT_ID", "cursor-session");
    try t.env.put(edition.id.state_dir_env, try std.fs.path.join(a, &.{ root, "state" }));
    try t.env.put(edition.id.config_path_env, try std.fs.path.join(a, &.{ root, "none.toml" }));
    const cursor_bin = try std.fs.path.join(a, &.{ root, ".local/bin/cursor-agent" });
    const abi_bin = try std.fs.path.join(a, &.{ root, ".local/bin/abi" });
    try fsx.writeAll(io, cursor_bin, stub, .executable_file);
    try fsx.writeAll(io, abi_bin, stub, .executable_file);

    const c = t.ctx(gpa, io);
    var cfg = try config.load(c);
    defer cfg.deinit();
    const st = try state_mod.load(c, a);
    var agent: AgentConfig = .{ .backend = .cursor, .model = "auto", .agent_path = cursor_bin, .transcript_dir = try std.fs.path.join(a, &.{ st.state_dir, "fm" }) };
    var live = host_mod.Live.init(c, &cfg, &st, .{ .backend = .cursor, .source = "env" });
    live.home_only = true;
    defer live.deinit();
    var app = try App.init(gpa, live.host(), &agent, st.state_dir, .ink);
    defer app.deinit();
    app.load();
    try std.testing.expectEqualStrings("cursor-session", app.chat.?);

    var runner: run.LiveRunner = .{ .ctx = c, .cfg = &cfg, .st = &st };
    var f = term.Fake.init(gpa);
    defer f.deinit();
    const orig = f.attr;
    f.script = &.{ "\x02", "hello tui\r", "\x11" }; // Ctrl-B, a prompt, Ctrl-Q
    try std.testing.expectEqual(@as(u8, 0), try run.loop(gpa, f.terminal(), &app, runner.runner()));
    try std.testing.expect(f.restoredTo(orig));

    // ollama, grok, and fm do not resolve under this HOME; abi does.
    try std.testing.expectEqual(Backend.abi, agent.backend);
    try std.testing.expectEqualStrings(abi_bin, agent.agent_path);
    try std.testing.expect(std.mem.endsWith(u8, agent.transcript_dir.?, "/abi"));
    try std.testing.expectEqualStrings("cursor", c.getEnv("ABBEY_BACKEND").?);

    const calls = (try fsx.readOptional(io, a, try std.fs.path.join(a, &.{ root, "calls.log" }), 1 << 20)).?;
    try std.testing.expect(std.mem.find(u8, calls, "cursor-agent") == null);
    try std.testing.expect(std.mem.startsWith(u8, calls, abi_bin));
    try std.testing.expect(std.mem.find(u8, calls, "[complete]") != null);
    try std.testing.expect(std.mem.find(u8, calls, "hello tui") != null);
    try std.testing.expect(std.mem.find(u8, calls, "cursor-session") == null);
    // The run went through the canonical path (one route record) and the
    // chat it resumed is abi's own, not the cursor env session.
    try std.testing.expectEqual(@as(usize, 1), (try route_log.recent(a, io, st.state_dir, 10)).len);
    const abi_chat = (try state_mod.resolveChatFor(c, a, &st, .abi)).?;
    try std.testing.expect(!std.mem.eql(u8, abi_chat, "cursor-session"));
    try std.testing.expectEqualStrings(abi_chat, app.chat.?);
    try std.testing.expectEqual(@as(?u8, 0), app.last_code);
    try std.testing.expect(std.mem.find(u8, t.outText(), "stub-output") != null);
}

test "live panels read this tree's memory, personas, and history for the live backend" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var t = T.init(gpa, "/work/panel");
    defer t.deinit();
    try t.env.put("HOME", root);
    try t.env.put("PATH", root);
    try t.env.put("ABBEY_PERSONA", "aviva");
    try t.env.put(edition.id.state_dir_env, try std.fs.path.join(a, &.{ root, "state" }));
    try t.env.put(edition.id.config_path_env, try std.fs.path.join(a, &.{ root, "none.toml" }));
    const c = t.ctx(gpa, io);
    var cfg = try config.load(c);
    defer cfg.deinit();
    const st = try state_mod.load(c, a);
    const learn = @import("../learn.zig");
    _ = try learn.learnPreference(.{ .ctx = c, .arena = a, .state_dir = st.state_dir, .cwd = st.cwd }, "answer tersely");
    try state_mod.saveChat(c, a, &st, "chat-a");

    var agent: AgentConfig = .{ .backend = .claude, .model = "opus" };
    var live = host_mod.Live.init(c, &cfg, &st, .{ .backend = .claude, .source = "config" });
    live.home_only = true;
    defer live.deinit();
    var app = try App.init(gpa, live.host(), &agent, st.state_dir, .ink);
    defer app.deinit();
    app.load();
    const has = struct {
        fn f(lines: []const []const u8, needle: []const u8) bool {
            for (lines) |l| if (std.mem.find(u8, l, needle) != null) return true;
            return false;
        }
    }.f;
    try std.testing.expect(has(app.personas.items, "persona: aviva"));
    try std.testing.expect(has(app.personas.items, "default_role: auto"));
    try std.testing.expect(has(app.memory.items, "pref: "));
    try std.testing.expect(has(app.memory.items, "ltm              1"));
    try std.testing.expect(has(app.doctor.items, "backend:   claude (from config)"));
    try std.testing.expect(has(app.history.items, "chat-a  /work/panel"));
    try std.testing.expectEqualStrings("chat-a", app.chat.?);
    try std.testing.expectEqualStrings("opus      Claude Opus (Abbey's default Max binding)", app.models.items[0]);
    try std.testing.expect(has(app.skills.items, "Proposed"));
}
