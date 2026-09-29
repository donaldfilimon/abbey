//! Executor backend selection and binary resolution (port of Rust
//! `agent/backend.rs` + `host.rs` candidate paths).
//!
//! Precedence: ABBEY_BACKEND env > config `backend` > legacy ABBEY_AGENT
//! cursor path > ollama when it resolves and lists its default model >
//! grok, fm, abi, claude, then cursor last. A set-but-unknown ABBEY_BACKEND
//! selects ollama and never falls through to the config key.
const std = @import("std");
const Ctx = @import("../ctx.zig").Ctx;
const fsx = @import("../util/fsx.zig");
const proc = @import("../proc.zig");
const models = @import("../models.zig");

pub const Backend = enum {
    cursor,
    grok,
    fm,
    abi,
    claude,
    ollama,

    pub fn parse(value: []const u8) ?Backend {
        const t = std.mem.trim(u8, value, &std.ascii.whitespace);
        const names = [_]struct { []const u8, Backend }{
            .{ "cursor", .cursor },      .{ "cursor-agent", .cursor },
            .{ "grok", .grok },          .{ "grok-build", .grok },
            .{ "xai", .grok },           .{ "fm", .fm },
            .{ "apple", .fm },           .{ "foundation", .fm },
            .{ "on-device", .fm },       .{ "abi", .abi },
            .{ "abi-cli", .abi },        .{ "claude", .claude },
            .{ "claude-code", .claude }, .{ "ollama", .ollama },
            .{ "ollama-cli", .ollama },
        };
        for (names) |n| if (std.ascii.eqlIgnoreCase(t, n[0])) return n[1];
        return null;
    }

    pub fn label(self: Backend) []const u8 {
        return switch (self) {
            .cursor => "cursor-agent",
            .grok => "grok",
            .fm => "fm",
            .abi => "abi",
            .claude => "claude",
            .ollama => "ollama",
        };
    }

    /// State subdirectory holding this backend's conversation transcripts.
    pub fn transcriptSubdir(self: Backend) []const u8 {
        return switch (self) {
            .abi => "abi",
            .claude => "claude",
            .ollama => "ollama",
            else => "fm",
        };
    }

    pub fn isOnDevice(self: Backend) bool {
        return self == .fm;
    }

    /// Local one-shot CLIs with Abbey-side transcript continuity.
    pub fn isOneshotLocal(self: Backend) bool {
        return self == .abi or self == .ollama;
    }

    pub fn hasServerSessions(self: Backend) bool {
        return self == .cursor or self == .grok;
    }

    pub fn cycleNext(self: Backend) Backend {
        return switch (self) {
            .ollama => .grok,
            .grok => .fm,
            .fm => .abi,
            .abi => .claude,
            .claude => .cursor,
            .cursor => .ollama,
        };
    }
};

pub const Selection = struct { backend: Backend, source: []const u8 };

/// Pure precedence over explicit sources. `null` means the unchosen default.
pub fn selectFromSources(env_backend: ?[]const u8, config_backend: ?[]const u8, has_legacy_agent: bool, err: ?*std.Io.Writer) ?Selection {
    if (env_backend) |v| if (std.mem.trim(u8, v, &std.ascii.whitespace).len != 0) {
        return .{ .backend = Backend.parse(v) orelse .ollama, .source = "env" };
    };
    if (config_backend) |v| if (std.mem.trim(u8, v, &std.ascii.whitespace).len != 0) {
        const parsed = Backend.parse(v) orelse blk: {
            if (err) |w| w.print("abbey: config `backend = \"{s}\"` is not one of ollama|cursor|grok|fm|abi|claude, using ollama\n", .{v}) catch {};
            break :blk .ollama;
        };
        return .{ .backend = parsed, .source = "config" };
    };
    if (has_legacy_agent) return .{ .backend = .cursor, .source = "ABBEY_AGENT" };
    return null;
}

/// The unchosen default with injectable probes (exactly the Rust shape).
pub fn pickDefault(resolves: *const fn (?*anyopaque, Backend) bool, ollama_ready: *const fn (?*anyopaque) bool, userdata: ?*anyopaque) Selection {
    const ollama_installed = resolves(userdata, .ollama);
    if (ollama_installed and ollama_ready(userdata)) return .{ .backend = .ollama, .source = "default" };
    for ([_]Backend{ .grok, .fm, .abi, .claude, .cursor }) |c| {
        if (resolves(userdata, c)) return .{
            .backend = c,
            .source = if (ollama_installed) "auto (ollama daemon/default model unavailable)" else "auto (ollama not installed)",
        };
    }
    return .{ .backend = .ollama, .source = "default (no ready executor found)" };
}

/// Filesystem view used by resolution; tests pin HOME/PATH and may restrict
/// candidates to HOME (the Rust `ABBEY_TEST_HOME_AGENTS_ONLY` hook).
pub const Probe = struct {
    ctx: Ctx,
    arena: std.mem.Allocator,
    abi_bin: ?[]const u8 = null,
    home_only: bool = false,
};

pub const ResolveError = error{ NotFound, NoHome, OutOfMemory };

fn candidatePaths(p: Probe, backend: Backend, home: []const u8) error{OutOfMemory}![]const []const u8 {
    const a = p.arena;
    var out: std.ArrayList([]const u8) = .empty;
    const H = struct {
        fn j(al: std.mem.Allocator, h: []const u8, rel: []const u8) ![]const u8 {
            return std.fs.path.join(al, &.{ h, rel });
        }
    };
    switch (backend) {
        .grok => {
            try out.append(a, try H.j(a, home, ".grok/bin/grok"));
            try out.append(a, try H.j(a, home, ".local/bin/grok"));
            try out.append(a, "/opt/homebrew/bin/grok");
        },
        .fm => try out.append(a, "/usr/bin/fm"),
        .abi => {
            try out.append(a, try H.j(a, home, ".local/bin/abi"));
            try out.append(a, try H.j(a, home, ".cargo/bin/abi"));
            try out.append(a, "/opt/homebrew/bin/abi");
        },
        .claude => {
            try out.append(a, try H.j(a, home, ".local/bin/claude"));
            try out.append(a, try H.j(a, home, ".claude/local/claude"));
            try out.append(a, "/opt/homebrew/bin/claude");
        },
        .ollama => {
            try out.append(a, try H.j(a, home, ".local/bin/ollama"));
            try out.append(a, "/opt/homebrew/bin/ollama");
            try out.append(a, "/usr/local/bin/ollama");
        },
        .cursor => {
            try out.append(a, try H.j(a, home, ".local/bin/cursor-agent"));
            try out.append(a, try H.j(a, home, ".local/bin/agent"));
        },
    }
    if (p.home_only) {
        var kept: std.ArrayList([]const u8) = .empty;
        for (out.items) |c| if (std.mem.startsWith(u8, c, home)) try kept.append(a, c);
        return kept.items;
    }
    return out.items;
}

/// First regular file named `bin` on PATH.
pub fn whichBin(p: Probe, bin: []const u8) error{OutOfMemory}!?[]const u8 {
    const path = p.ctx.getEnv("PATH") orelse return null;
    var it = std.mem.tokenizeScalar(u8, path, ':');
    while (it.next()) |dir| {
        const c = try std.fs.path.join(p.arena, &.{ dir, bin });
        if (fsx.isFile(p.ctx.io, c)) return c;
    }
    return null;
}

/// Resolve the executor binary for `backend`, ignoring ABBEY_AGENT (that
/// override belongs to the env-chosen backend only).
pub fn resolveFor(p: Probe, backend: Backend) ResolveError![]const u8 {
    const home = p.ctx.envNonEmpty("HOME") orelse return error.NoHome;
    if (backend == .abi) {
        if (p.abi_bin) |b| if (fsx.isFile(p.ctx.io, b)) return b;
    }
    for (try candidatePaths(p, backend, home)) |c| {
        if (!fsx.isFile(p.ctx.io, c)) continue;
        if (backend == .cursor and std.mem.eql(u8, std.fs.path.basename(c), "agent")) {
            var buf: [std.fs.max_path_bytes]u8 = undefined;
            const n = std.Io.Dir.cwd().readLink(p.ctx.io, c, &buf) catch 0;
            // Skip Grok Build's `agent` when Cursor is wanted.
            if (n > 0 and std.mem.find(u8, buf[0..n], "cursor-agent") == null) continue;
        }
        return c;
    }
    const path_name = switch (backend) {
        .cursor => "cursor-agent",
        else => @tagName(backend),
    };
    if (try whichBin(p, path_name)) |found| return found;
    return error.NotFound;
}

pub fn notFoundMessage(backend: Backend) []const u8 {
    return switch (backend) {
        .fm => "`fm` not found: the on-device backend needs the Apple Foundation Models CLI (macOS 26+). Unset ABBEY_BACKEND=fm to use the default ollama backend.",
        .abi => "`abi` not found: ABBEY_BACKEND=abi needs a real `abi` binary (a shell alias will not do). Build it with `cargo build -p abi-cli` in ../abi, then set ABBEY_ABI_BIN or `abi_bin` in config.toml.",
        .claude => "`claude` not found: ABBEY_BACKEND=claude needs the Claude Code CLI on PATH, or select another backend.",
        .ollama => "`ollama` not found: ABBEY_BACKEND=ollama needs the Ollama CLI on PATH (https://ollama.com). The default model is gemma4:26b-mlx (alias gemma:27b-mlx).",
        else => "executor not found: generation needs an executor backend. Install any of ollama, grok, fm, abi, claude, or cursor-agent (ABBEY_BACKEND=ollama|cursor|grok|fm|abi|claude picks one explicitly, ABBEY_AGENT points at a binary directly); local verbs work without one",
    };
}

/// The legacy ABBEY_AGENT path, when it names an existing file.
pub fn legacyAgentPath(ctx: Ctx) ?[]const u8 {
    const v = ctx.getEnv("ABBEY_AGENT") orelse return null;
    if (v.len == 0 or !fsx.isFile(ctx.io, v)) return null;
    return v;
}

/// True only when `ollama list` already names `model` (refuses to let
/// `ollama run` pull a missing tag). One retry when the probe itself failed.
pub fn ollamaListsModel(ctx: Ctx, path: []const u8, model: []const u8) bool {
    if (ollamaListOnce(ctx, path, model)) |answer| return answer;
    ctx.io.sleep(.fromMilliseconds(50), .awake) catch {};
    return ollamaListOnce(ctx, path, model) orelse false;
}

fn ollamaListOnce(ctx: Ctx, path: []const u8, model: []const u8) ?bool {
    const r = proc.capture(ctx.gpa, ctx.io, &.{ path, "list" }, .{ .limits = .{
        .stdout_bytes = 64 * 1024,
        .stderr_bytes = 4 * 1024,
        .timeout_ms = 2000,
    } }) catch |e| return switch (e) {
        error.StreamTooLong => false, // deterministic truncation is an answer
        else => null,
    };
    defer r.deinit(ctx.gpa);
    if (!r.success()) return false;
    var lines = std.mem.splitScalar(u8, r.stdout, '\n');
    _ = lines.next(); // header
    while (lines.next()) |line| {
        var words = std.mem.tokenizeAny(u8, line, &std.ascii.whitespace);
        if (words.next()) |first| if (std.mem.eql(u8, first, model)) return true;
    }
    return false;
}

const LiveProbe = struct {
    p: Probe,
    fn resolves(ud: ?*anyopaque, b: Backend) bool {
        const self: *LiveProbe = @ptrCast(@alignCast(ud.?));
        _ = resolveFor(self.p, b) catch return false;
        return true;
    }
    fn ready(ud: ?*anyopaque) bool {
        const self: *LiveProbe = @ptrCast(@alignCast(ud.?));
        const path = resolveFor(self.p, .ollama) catch return false;
        return ollamaListsModel(self.p.ctx, path, models.ollama_default_model);
    }
};

/// Full resolution from env + config + host. Done once per invocation by
/// the caller and threaded as a value (never re-read from a cache).
pub fn select(p: Probe, config_backend: ?[]const u8) Selection {
    if (selectFromSources(p.ctx.getEnv("ABBEY_BACKEND"), config_backend, legacyAgentPath(p.ctx) != null, p.ctx.err)) |s| return s;
    var live: LiveProbe = .{ .p = p };
    return pickDefault(LiveProbe.resolves, LiveProbe.ready, &live);
}

// ---- tests (ported from agent/backend.rs) ----

const Fake = struct {
    installed: []const Backend,
    ready: bool,
    fn resolves(ud: ?*anyopaque, b: Backend) bool {
        const self: *Fake = @ptrCast(@alignCast(ud.?));
        return std.mem.findScalar(Backend, self.installed, b) != null;
    }
    fn isReady(ud: ?*anyopaque) bool {
        const self: *Fake = @ptrCast(@alignCast(ud.?));
        return self.ready;
    }
    fn pick(installed: []const Backend, ready: bool) Selection {
        var f: Fake = .{ .installed = installed, .ready = ready };
        return pickDefault(resolves, isReady, &f);
    }
};

test "default backend prefers ollama but never requires cursor" {
    const all = [_]Backend{ .cursor, .grok, .fm, .abi, .claude, .ollama };
    var s = Fake.pick(&all, true);
    try std.testing.expectEqual(Backend.ollama, s.backend);
    try std.testing.expectEqualStrings("default", s.source);
    s = Fake.pick(&.{.cursor}, false);
    try std.testing.expectEqual(Backend.cursor, s.backend);
    try std.testing.expect(std.mem.startsWith(u8, s.source, "auto"));
    s = Fake.pick(&.{.abi}, false);
    try std.testing.expectEqual(Backend.abi, s.backend);
    try std.testing.expect(std.mem.startsWith(u8, s.source, "auto"));
    s = Fake.pick(&.{ .grok, .abi }, false);
    try std.testing.expectEqual(Backend.grok, s.backend);
    s = Fake.pick(&.{.claude}, false);
    try std.testing.expectEqual(Backend.claude, s.backend);
    s = Fake.pick(&.{}, false);
    try std.testing.expectEqual(Backend.ollama, s.backend);
    try std.testing.expectEqualStrings("default (no ready executor found)", s.source);
    s = Fake.pick(&.{ .ollama, .abi }, false);
    try std.testing.expectEqual(Backend.abi, s.backend);
    try std.testing.expect(std.mem.find(u8, s.source, "default model unavailable") != null);
}

test "backend aliases parse" {
    try std.testing.expectEqual(Backend.claude, Backend.parse("claude").?);
    try std.testing.expectEqual(Backend.claude, Backend.parse("Claude-Code").?);
    try std.testing.expectEqual(Backend.ollama, Backend.parse("Ollama-CLI").?);
    try std.testing.expectEqual(Backend.abi, Backend.parse(" abi ").?);
    try std.testing.expect(Backend.parse("nope") == null);
}

test "configured backend outranks the legacy agent path; unknown env selects ollama" {
    var s = selectFromSources("abi", "ollama", true, null).?;
    try std.testing.expectEqual(Backend.abi, s.backend);
    try std.testing.expectEqualStrings("env", s.source);
    s = selectFromSources(null, "ollama", true, null).?;
    try std.testing.expectEqual(Backend.ollama, s.backend);
    try std.testing.expectEqualStrings("config", s.source);
    s = selectFromSources(null, null, true, null).?;
    try std.testing.expectEqual(Backend.cursor, s.backend);
    try std.testing.expectEqualStrings("ABBEY_AGENT", s.source);
    try std.testing.expect(selectFromSources(null, null, false, null) == null);
    s = selectFromSources("not-a-backend", "claude", true, null).?;
    try std.testing.expectEqual(Backend.ollama, s.backend);
    try std.testing.expectEqualStrings("env", s.source);
    // A blank env value is unset and falls through to config.
    s = selectFromSources("  ", "claude", false, null).?;
    try std.testing.expectEqual(Backend.claude, s.backend);
}

test "server-session and one-shot predicates" {
    try std.testing.expect(Backend.cursor.hasServerSessions());
    try std.testing.expect(Backend.grok.hasServerSessions());
    for ([_]Backend{ .fm, .abi, .claude, .ollama }) |b| try std.testing.expect(!b.hasServerSessions());
    try std.testing.expect(Backend.ollama.isOneshotLocal());
    try std.testing.expect(Backend.abi.isOneshotLocal());
    try std.testing.expect(!Backend.fm.isOneshotLocal());
    try std.testing.expectEqual(Backend.ollama, Backend.cursor.cycleNext());
}

test "abi resolution uses configured abi_bin and never falls through to cursor" {
    const T = @import("../ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const cursor = try std.fs.path.join(a, &.{ root, ".local/bin/cursor-agent" });
    try fsx.writeAll(io, cursor, "#!/bin/sh\n", .default_file);
    var t = T.init(gpa, root);
    defer t.deinit();
    try t.env.put("HOME", root);
    try t.env.put("PATH", root);
    var p: Probe = .{ .ctx = t.ctx(gpa, io), .arena = a, .home_only = true };
    try std.testing.expectError(error.NotFound, resolveFor(p, .abi));
    try std.testing.expectEqualStrings(cursor, try resolveFor(p, .cursor));
    const abi = try std.fs.path.join(a, &.{ root, "my-abi" });
    try fsx.writeAll(io, abi, "#!/bin/sh\n", .default_file);
    p.abi_bin = abi;
    try std.testing.expectEqualStrings(abi, try resolveFor(p, .abi));
}

test "ollama probe requires the default model in ollama list" {
    const T = @import("../ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    const stub = try std.fs.path.join(gpa, &.{ root, "ollama" });
    defer gpa.free(stub);
    var t = T.init(gpa, root);
    defer t.deinit();
    const c = t.ctx(gpa, io);
    try fsx.writeAll(io, stub, "#!/bin/sh\nprintf 'NAME ID SIZE\\ngemma4:26b-mlx digest 1GB\\n'\n", .executable_file);
    try std.testing.expect(ollamaListsModel(c, stub, models.ollama_default_model));
    try fsx.writeAll(io, stub, "#!/bin/sh\nprintf 'NAME ID SIZE\\nother:latest digest 1GB\\n'\n", .executable_file);
    try std.testing.expect(!ollamaListsModel(c, stub, models.ollama_default_model));
}
