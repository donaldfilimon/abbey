//! Abbey config: role bindings, persona policy, memory backend, executor
//! backend, abi binary. Port of the Rust `parse_toml_lite` subset parser
//! (flat keys plus `[roles]`; `[embeddings]` and unknown tables are skipped
//! because learned embeddings are Proposed in this rewrite).
const std = @import("std");
const Ctx = @import("../ctx.zig").Ctx;
const edition = @import("../edition.zig");
const fsx = @import("../util/fsx.zig");

pub const Error = error{ OutOfMemory, NoHome } || fsx.ReadError;

pub const Config = struct {
    arena: std.heap.ArenaAllocator,
    path: []const u8 = "",
    file_present: bool = false,
    persona_policy: []const u8 = "auto",
    default_role: []const u8 = "auto",
    role_max: []const u8 = "max",
    role_gemma: []const u8 = "gemma",
    /// Requested backend name. This rewrite always stores memory as JSONL;
    /// any other value is reported by `doctor`, never silently honored.
    memory_backend: []const u8 = "jsonl",
    abi_bin: ?[]const u8 = null,
    backend: ?[]const u8 = null,

    pub fn deinit(self: *Config) void {
        self.arena.deinit();
    }
};

pub fn empty(gpa: std.mem.Allocator) Config {
    return .{ .arena = .init(gpa) };
}

/// Load the active edition's config file (absent file = defaults), then
/// apply the shared behavior overrides: ABBEY_ROLE, ABBEY_PERSONA,
/// ABBEY_MEMORY_BACKEND, ABBEY_ABI_BIN.
pub fn load(ctx: Ctx) Error!Config {
    var cfg = empty(ctx.gpa);
    errdefer cfg.deinit();
    const a = cfg.arena.allocator();
    cfg.path = try edition.configPath(ctx, a);
    if (try fsx.readOptional(ctx.io, a, cfg.path, 1024 * 1024)) |text| {
        cfg.file_present = true;
        try parseLite(&cfg, text);
    }
    try applyEnv(&cfg, ctx);
    return cfg;
}

fn applyEnv(cfg: *Config, ctx: Ctx) error{OutOfMemory}!void {
    const a = cfg.arena.allocator();
    if (ctx.envNonEmpty("ABBEY_ROLE")) |v| cfg.default_role = try std.ascii.allocLowerString(a, v);
    if (ctx.envNonEmpty("ABBEY_PERSONA")) |v| cfg.persona_policy = try std.ascii.allocLowerString(a, v);
    if (ctx.envNonEmpty("ABBEY_MEMORY_BACKEND")) |v| cfg.memory_backend = try std.ascii.allocLowerString(a, v);
    if (ctx.envNonEmpty("ABBEY_ABI_BIN")) |v| cfg.abi_bin = try a.dupe(u8, v);
}

const Section = enum { root, roles, unknown };

/// Parse the flat TOML subset. Values are copied into the config arena.
pub fn parseLite(cfg: *Config, text: []const u8) error{OutOfMemory}!void {
    const a = cfg.arena.allocator();
    var section: Section = .root;
    var lines = std.mem.splitScalar(u8, text, '\n');
    while (lines.next()) |raw| {
        const before_hash = raw[0 .. std.mem.findScalar(u8, raw, '#') orelse raw.len];
        const line = std.mem.trim(u8, before_hash, &std.ascii.whitespace);
        if (line.len == 0) continue;
        if (std.mem.eql(u8, line, "[roles]")) {
            section = .roles;
            continue;
        }
        if (line[0] == '[') {
            section = .unknown;
            continue;
        }
        const eq = std.mem.findScalar(u8, line, '=') orelse continue;
        const k = std.mem.trim(u8, line[0..eq], &std.ascii.whitespace);
        const v = try a.dupe(u8, stripTomlStr(line[eq + 1 ..]));
        switch (section) {
            .roles => {
                if (std.mem.eql(u8, k, "max")) cfg.role_max = v;
                if (std.mem.eql(u8, k, "gemma")) cfg.role_gemma = v;
            },
            .root => {
                if (std.mem.eql(u8, k, "persona_policy")) cfg.persona_policy = v;
                if (std.mem.eql(u8, k, "default_role")) cfg.default_role = v;
                if (std.mem.eql(u8, k, "memory_backend")) cfg.memory_backend = v;
                if (std.mem.eql(u8, k, "abi_bin")) cfg.abi_bin = v;
                if (std.mem.eql(u8, k, "backend")) cfg.backend = try std.ascii.allocLowerString(a, v);
            },
            .unknown => {},
        }
    }
}

fn stripTomlStr(raw: []const u8) []const u8 {
    const s = std.mem.trimEnd(u8, std.mem.trim(u8, raw, &std.ascii.whitespace), ",");
    if (s.len >= 2 and ((s[0] == '"' and s[s.len - 1] == '"') or (s[0] == '\'' and s[s.len - 1] == '\''))) {
        return s[1 .. s.len - 1];
    }
    return s;
}

pub const default_toml =
    \\# Abbey (Zig) config. Role bindings are executor model ids/aliases; the
    \\# portable max/gemma aliases resolve per backend. They are roles, not
    \\# bundled Abbey weights.
    \\persona_policy = "auto"
    \\default_role = "auto"
    \\# This rewrite stores memory as append-only JSONL under the state dir.
    \\memory_backend = "jsonl"
    \\
    \\# Default executor backend: "ollama" (preferred when installed) | "grok" | "fm" | "abi" | "claude" | "cursor".
    \\# ABBEY_BACKEND overrides this. The unchosen default prefers ollama, then grok/fm/abi/claude, then cursor last.
    \\# backend = "ollama"
    \\
    \\# Path to a real `abi` binary (a shell alias will not do). ABBEY_ABI_BIN overrides.
    \\# abi_bin = "/path/to/abi"
    \\
    \\[roles]
    \\max = "max"
    \\gemma = "gemma"
    \\
;

pub fn statusLines(cfg: *const Config, w: *std.Io.Writer) std.Io.Writer.Error!void {
    try w.print("config:          {s}{s}\n", .{ cfg.path, if (cfg.file_present) "" else " (absent; defaults)" });
    try w.print("persona_policy:  {s}\n", .{cfg.persona_policy});
    try w.print("default_role:    {s}\n", .{cfg.default_role});
    try w.print("role.max ->      {s}\n", .{cfg.role_max});
    try w.print("role.gemma ->    {s}\n", .{cfg.role_gemma});
    try w.print("memory_backend:  {s}\n", .{cfg.memory_backend});
    try w.print("backend:         {s}\n", .{cfg.backend orelse "(ollama default)"});
    try w.print("abi_bin:         {s}\n", .{cfg.abi_bin orelse "(PATH)"});
}

test "parse default shape activates nothing" {
    var cfg = empty(std.testing.allocator);
    defer cfg.deinit();
    try parseLite(&cfg, default_toml);
    try std.testing.expectEqualStrings("max", cfg.role_max);
    try std.testing.expectEqualStrings("gemma", cfg.role_gemma);
    try std.testing.expectEqualStrings("jsonl", cfg.memory_backend);
    try std.testing.expect(cfg.backend == null);
    try std.testing.expect(cfg.abi_bin == null);
}

test "parse flat keys, roles table, quotes, comments, unknown tables" {
    var cfg = empty(std.testing.allocator);
    defer cfg.deinit();
    try parseLite(&cfg,
        \\backend = "ABI"  # comment
        \\abi_bin = '/opt/abi'
        \\[embeddings]
        \\model = "ignored"
        \\max = "not-a-role"
        \\[roles]
        \\max = "opus",
        \\gemma = composer
    );
    try std.testing.expectEqualStrings("abi", cfg.backend.?);
    try std.testing.expectEqualStrings("/opt/abi", cfg.abi_bin.?);
    try std.testing.expectEqualStrings("opus", cfg.role_max);
    try std.testing.expectEqualStrings("composer", cfg.role_gemma);
}

test "no commented line masquerades as an assignment" {
    var lines = std.mem.splitScalar(u8, default_toml, '\n');
    while (lines.next()) |line| {
        const body = std.mem.trim(u8, line, " ");
        if (!std.mem.startsWith(u8, body, "# ")) continue;
        const rest = body[2..];
        const eq = std.mem.findScalar(u8, rest, '=') orelse continue;
        const key = std.mem.trim(u8, rest[0..eq], " ");
        if (std.mem.findScalar(u8, key, ' ') != null) continue;
        const value = std.mem.trim(u8, rest[eq + 1 ..], " ");
        try std.testing.expect(value.len > 1 and value[0] == '"' and value[value.len - 1] == '"');
    }
}

test "load applies shared env overrides over the edition config file" {
    const T = @import("../ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    const cfg_path = try std.fs.path.join(gpa, &.{ root, "config.toml" });
    defer gpa.free(cfg_path);
    try fsx.writeAll(io, cfg_path, "default_role = \"max\"\nbackend = \"claude\"\n", .default_file);
    var t = T.init(gpa, root);
    defer t.deinit();
    try t.env.put("HOME", root);
    try t.env.put(edition.id.config_path_env, cfg_path);
    try t.env.put("ABBEY_ROLE", " Gemma ");
    var cfg = try load(t.ctx(gpa, io));
    defer cfg.deinit();
    try std.testing.expect(cfg.file_present);
    try std.testing.expectEqualStrings("gemma", cfg.default_role);
    try std.testing.expectEqualStrings("claude", cfg.backend.?);
}
