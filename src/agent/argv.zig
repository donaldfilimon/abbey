//! Per-backend argv grammars (port of Rust `agent/argv.rs`). Each grammar is
//! built from scratch: no executor ever receives another executor's flags.
//! Pure except for transcript reads (abi/ollama context prefix, fm/claude
//! session presence), which go through `io`.
const std = @import("std");
const Io = std.Io;
const Backend = @import("backend.zig").Backend;
const models = @import("../models.zig");
const fsx = @import("../util/fsx.zig");

pub const Worktree = union(enum) { auto, named: []const u8 };

pub const AgentConfig = struct {
    agent_path: []const u8 = "",
    model: []const u8 = "auto",
    auto_review: bool = true,
    trust: bool = true,
    force: bool = false,
    no_resume: bool = false,
    mode: ?[]const u8 = null,
    print: bool = false,
    output_format: ?[]const u8 = null,
    worktree: ?Worktree = null,
    workspace: ?[]const u8 = null,
    add_dirs: []const []const u8 = &.{},
    sandbox: ?[]const u8 = null,
    extra_args: []const []const u8 = &.{},
    backend: Backend = .ollama,
    transcript_dir: ?[]const u8 = null,
};

/// How much transcript tail rides into an abi/ollama turn as context.
pub const context_tail_bytes = 8 * 1024;
/// Per-prompt argv ceiling on Unix hosts (Rust `max_prompt_argv_bytes`).
pub const max_prompt_argv_bytes = 96 * 1024;

pub const Error = error{OutOfMemory};

/// Map an Abbey model/alias onto `fm`'s vocabulary (`system` | `pcc`).
pub fn fmModel(requested: []const u8) []const u8 {
    const t = std.mem.trim(u8, requested, &std.ascii.whitespace);
    for ([_][]const u8{ "pcc", "private-cloud-compute", "private_cloud_compute" }) |alias| {
        if (std.ascii.eqlIgnoreCase(t, alias)) return "pcc";
    }
    return "system";
}

fn cursorStyleBinding(lower: []const u8) bool {
    if (std.mem.find(u8, lower, "thinking") != null) return true;
    for ([_][]const u8{ "-fast", "-high", "-xhigh", "-medium", "-low", "-max" }) |suf| {
        if (std.mem.endsWith(u8, lower, suf)) return true;
    }
    return false;
}

fn eqAny(s: []const u8, set: []const []const u8) bool {
    for (set) |x| if (std.mem.eql(u8, s, x)) return true;
    return false;
}

fn startsAny(s: []const u8, set: []const []const u8) bool {
    for (set) |x| if (std.mem.startsWith(u8, s, x)) return true;
    return false;
}

fn lowerTrim(buf: []u8, s: []const u8) ?[]const u8 {
    const t = std.mem.trim(u8, s, &std.ascii.whitespace);
    if (t.len > buf.len) return null;
    return std.ascii.lowerString(buf, t);
}

/// Normalize a model string for the abi backend. `fable` stays a local tag
/// and cursor-expanded leftovers collapse to `local`, so no role alias can
/// silently select abi's live transport. Returns a static string, a slice of
/// `requested`, or (for unknown ids) a lowercased copy in `arena`.
pub fn abiNormalizeModel(arena: std.mem.Allocator, requested: []const u8) Error![]const u8 {
    const t = std.mem.trim(u8, requested, &std.ascii.whitespace);
    var buf: [256]u8 = undefined;
    const lower = lowerTrim(&buf, t) orelse return t;
    if (eqAny(lower, &.{ "live", "anthropic" })) return "live";
    if (eqAny(lower, &.{ "local", "auto", "smart", "default", "abi", "" })) return "local";
    if (std.mem.startsWith(u8, lower, "claude-")) return if (cursorStyleBinding(lower)) "local" else t;
    if (eqAny(lower, &.{ "fable", "fable5", "fable-5", "max", "composer", "composer2", "composer-2.5", "gemma", "gemma4", "qwen", "kimi", "opus", "opus5", "grok", "codex", "sol", "terra" })) return "local";
    if (startsAny(lower, &.{ "cursor-", "gpt-", "composer-", "kimi-" }) or std.mem.find(u8, lower, "thinking") != null) return "local";
    // Non-matching ids pass through lowercased, as in Rust (`other.to_string()`).
    return try arena.dupe(u8, lower);
}

pub const AbiTransport = union(enum) { local, live: ?[]const u8 };

pub fn abiTransport(arena: std.mem.Allocator, requested: []const u8) Error!AbiTransport {
    const n = try abiNormalizeModel(arena, requested);
    if (std.ascii.eqlIgnoreCase(n, "live") or std.ascii.eqlIgnoreCase(n, "anthropic")) return .{ .live = null };
    if (std.ascii.startsWithIgnoreCase(n, "claude-")) return .{ .live = n };
    return .local;
}

fn claudeStripCursorBinding(lower: []const u8) []const u8 {
    var s = lower;
    while (true) {
        var stripped = false;
        for ([_][]const u8{ "-fast", "-high", "-xhigh", "-medium", "-low", "-max", "-thinking" }) |suf| {
            if (std.mem.endsWith(u8, s, suf)) {
                s = s[0 .. s.len - suf.len];
                stripped = true;
            }
        }
        if (!stripped) return s;
    }
}

/// Map an Abbey model/alias onto the Claude Code CLI's vocabulary; null means
/// omit `--model` and let the plan pick its default.
pub fn claudeModel(arena: std.mem.Allocator, requested: []const u8) Error!?[]const u8 {
    var buf: [256]u8 = undefined;
    const lower = lowerTrim(&buf, requested) orelse return "opus";
    if (eqAny(lower, &.{ "", "auto", "smart", "default" })) return null;
    if (eqAny(lower, &.{ "opus", "opus5", "opus-5", "max" })) return "opus";
    if (eqAny(lower, &.{ "sonnet", "sonnet5", "sonnet-5" })) return "sonnet";
    if (eqAny(lower, &.{"haiku"})) return "haiku";
    if (eqAny(lower, &.{ "fable", "fable5", "fable-5" })) return "fable";
    if (eqAny(lower, &.{ "gemma", "gemma4", "gemma-4", "composer", "composer2", "composer-2.5" })) return "sonnet";
    if (std.mem.startsWith(u8, lower, "claude-")) return try arena.dupe(u8, claudeStripCursorBinding(lower));
    return "opus";
}

/// Map an Abbey model/alias onto a local Ollama tag.
pub fn ollamaNormalizeModel(requested: []const u8) []const u8 {
    const t = std.mem.trim(u8, requested, &std.ascii.whitespace);
    var buf: [256]u8 = undefined;
    const lower = lowerTrim(&buf, t) orelse return t;
    if (eqAny(lower, &.{ "", "auto", "smart", "default", "local", "ollama", "gemma", "gemma4", "gemma-4", "gemma:27b-mlx", "gemma4:26b-mlx", "gemma4:26b", "max", "composer", "composer2", "composer-2.5", "opus", "opus5", "fable", "fable5", "qwen", "kimi", "grok", "codex", "sol", "terra" })) return models.ollama_default_model;
    if (startsAny(lower, &.{ "cursor-", "claude-", "gpt-", "composer-", "kimi-" }) or std.mem.find(u8, lower, "thinking") != null) return models.ollama_default_model;
    return t;
}

/// The trailing <= `max_bytes` of `s`, cut on a UTF-8 boundary.
pub fn utf8Tail(s: []const u8, max_bytes: usize) []const u8 {
    if (s.len <= max_bytes) return s;
    var start = s.len - max_bytes;
    while (start < s.len and (s[start] & 0xC0) == 0x80) start += 1;
    return s[start..];
}

fn floorBoundary(s: []const u8, end_in: usize) usize {
    var end = @min(end_in, s.len);
    while (end > 0 and end < s.len and (s[end] & 0xC0) == 0x80) end -= 1;
    return end;
}

/// Truncate `s` on a UTF-8 boundary to <= `max_bytes`, with a marker.
pub fn truncateUtf8Bytes(arena: std.mem.Allocator, s: []const u8, max_bytes: usize) Error![]const u8 {
    if (s.len <= max_bytes) return s;
    const marker = try std.fmt.allocPrint(arena, "\n\n\u{2026} [truncated for OS argv limit; kept {d} of {d} bytes]", .{ max_bytes, s.len });
    const budget = max_bytes -| marker.len;
    const end = floorBoundary(s, budget);
    const out = try std.mem.concat(arena, u8, &.{ s[0..end], marker });
    if (out.len > max_bytes) return out[0..floorBoundary(out, max_bytes)];
    return out;
}

/// Whether a user's prompt would reach a cursor-style backend in option position.
pub fn looksLikeFlags(prompts: []const []const u8) bool {
    for (prompts) |p| {
        const t = std.mem.trim(u8, p, &std.ascii.whitespace);
        if (t.len == 0) continue;
        return t[0] == '-';
    }
    return false;
}

fn modeNote(arena: std.mem.Allocator, mode: []const u8) Error![]const u8 {
    if (std.mem.eql(u8, mode, "ask")) return "Answer the question. Do not modify files.";
    if (std.mem.eql(u8, mode, "plan")) return "Produce a plan only. Do not write the implementation.";
    return std.fmt.allocPrint(arena, "Mode: {s}.", .{mode});
}

pub fn transcriptPath(arena: std.mem.Allocator, cfg: *const AgentConfig, chat_id: []const u8) Error!?[]const u8 {
    const dir = cfg.transcript_dir orelse return null;
    const name = try std.fmt.allocPrint(arena, "{s}.transcript", .{chat_id});
    return try std.fs.path.join(arena, &.{ dir, name });
}

const L = std.ArrayList([]const u8);

fn appendClamped(arena: std.mem.Allocator, args: *L, prompts: []const []const u8) Error!void {
    for (prompts) |p| try args.append(arena, try truncateUtf8Bytes(arena, p, max_prompt_argv_bytes));
}

fn contextPrefix(arena: std.mem.Allocator, io: Io, cfg: *const AgentConfig, resume_id: ?[]const u8) Error!?[]const u8 {
    const id = resume_id orelse return null;
    if (id.len == 0) return null;
    const path = (try transcriptPath(arena, cfg, id)) orelse return null;
    const prev = (fsx.readOptional(io, arena, path, 64 * 1024 * 1024) catch null) orelse return null;
    const tail = utf8Tail(prev, context_tail_bytes);
    if (std.mem.trim(u8, tail, &std.ascii.whitespace).len == 0) return null;
    return try std.fmt.allocPrint(arena, "Previous conversation (context, oldest first, may be truncated):\n{s}\n--- end of context; answer the next message ---", .{tail});
}

fn buildFm(arena: std.mem.Allocator, io: Io, cfg: *const AgentConfig, resume_id: ?[]const u8, prompts: []const []const u8) Error![]const []const u8 {
    var args: L = .empty;
    try args.appendSlice(arena, &.{ "respond", "--model", fmModel(cfg.model) });
    if (cfg.mode) |m| try args.appendSlice(arena, &.{ "--instructions", try modeNote(arena, m) });
    if (cfg.print) try args.append(arena, "--no-stream");
    if (resume_id) |id| if (id.len != 0) {
        if (try transcriptPath(arena, cfg, id)) |p| {
            if (fsx.isFile(io, p)) try args.appendSlice(arena, &.{ "--resume", p });
            try args.appendSlice(arena, &.{ "--save-transcript", p });
        }
    };
    try appendClamped(arena, &args, prompts);
    return args.items;
}

fn buildAbi(arena: std.mem.Allocator, io: Io, cfg: *const AgentConfig, resume_id: ?[]const u8, prompts: []const []const u8) Error![]const []const u8 {
    var args: L = .empty;
    try args.append(arena, "complete");
    switch (try abiTransport(arena, cfg.model)) {
        .local => try args.appendSlice(arena, &.{ "--model", try abiNormalizeModel(arena, cfg.model) }),
        .live => |id| {
            try args.append(arena, "--live");
            if (id) |m| try args.appendSlice(arena, &.{ "--model", m });
        },
    }
    try args.append(arena, "--");
    if (try contextPrefix(arena, io, cfg, resume_id)) |c| try args.append(arena, c);
    if (cfg.mode) |m| try args.append(arena, try modeNote(arena, m));
    try appendClamped(arena, &args, prompts);
    return args.items;
}

fn buildClaude(arena: std.mem.Allocator, io: Io, cfg: *const AgentConfig, resume_id: ?[]const u8, prompts: []const []const u8) Error![]const []const u8 {
    var args: L = .empty;
    if (try claudeModel(arena, cfg.model)) |m| try args.appendSlice(arena, &.{ "--model", m });
    if (cfg.print) {
        try args.append(arena, "--print");
        if (cfg.output_format) |f| try args.appendSlice(arena, &.{ "--output-format", f });
    }
    if (cfg.force) {
        try args.appendSlice(arena, &.{ "--permission-mode", "bypassPermissions" });
    } else if (cfg.mode) |m| if (std.mem.eql(u8, m, "plan")) {
        try args.appendSlice(arena, &.{ "--permission-mode", "plan" });
    };
    if (cfg.mode) |m| try args.appendSlice(arena, &.{ "--append-system-prompt", try modeNote(arena, m) });
    for (cfg.add_dirs) |d| try args.appendSlice(arena, &.{ "--add-dir", d });
    if (resume_id) |id| if (id.len != 0) {
        const established = if (try transcriptPath(arena, cfg, id)) |p| fsx.isFile(io, p) else false;
        try args.appendSlice(arena, &.{ if (established) "--resume" else "--session-id", id });
    };
    try appendClamped(arena, &args, prompts);
    return args.items;
}

fn buildOllama(arena: std.mem.Allocator, io: Io, cfg: *const AgentConfig, resume_id: ?[]const u8, prompts: []const []const u8) Error![]const []const u8 {
    var args: L = .empty;
    try args.appendSlice(arena, &.{ "run", "--nowordwrap", ollamaNormalizeModel(cfg.model), "--" });
    if (try contextPrefix(arena, io, cfg, resume_id)) |c| try args.append(arena, c);
    if (cfg.mode) |m| try args.append(arena, try modeNote(arena, m));
    try appendClamped(arena, &args, prompts);
    return args.items;
}

fn buildCursor(arena: std.mem.Allocator, cfg: *const AgentConfig, resume_id: ?[]const u8, prompts: []const []const u8) Error![]const []const u8 {
    var args: L = .empty;
    try args.appendSlice(arena, &.{ "--model", cfg.model });
    if (cfg.auto_review) try args.append(arena, "--auto-review");
    if (cfg.trust) try args.append(arena, "--trust");
    if (cfg.force) try args.append(arena, "--force");
    if (cfg.mode) |m| try args.appendSlice(arena, &.{ "--mode", m });
    if (cfg.print) {
        try args.append(arena, "--print");
        if (cfg.output_format) |f| try args.appendSlice(arena, &.{ "--output-format", f });
    }
    if (cfg.worktree) |wt| {
        try args.append(arena, "--worktree");
        switch (wt) {
            .auto => {},
            .named => |n| try args.append(arena, n),
        }
    }
    if (cfg.workspace) |ws| try args.appendSlice(arena, &.{ "--workspace", ws });
    for (cfg.add_dirs) |d| try args.appendSlice(arena, &.{ "--add-dir", d });
    if (cfg.sandbox) |sb| try args.appendSlice(arena, &.{ "--sandbox", sb });
    try args.appendSlice(arena, cfg.extra_args);
    if (resume_id) |id| if (id.len != 0) try args.appendSlice(arena, &.{ "--resume", id });
    try appendClamped(arena, &args, prompts);
    return args.items;
}

/// The argv (without argv[0]) the configured backend receives.
pub fn buildArgs(arena: std.mem.Allocator, io: Io, cfg: *const AgentConfig, resume_id: ?[]const u8, prompts: []const []const u8) Error![]const []const u8 {
    return switch (cfg.backend) {
        .fm => buildFm(arena, io, cfg, resume_id, prompts),
        .abi => buildAbi(arena, io, cfg, resume_id, prompts),
        .claude => buildClaude(arena, io, cfg, resume_id, prompts),
        .ollama => buildOllama(arena, io, cfg, resume_id, prompts),
        .cursor, .grok => buildCursor(arena, cfg, resume_id, prompts),
    };
}

// ---- tests (ported from agent/argv/tests.rs) ----

fn maximalCursorConfig() AgentConfig {
    return .{
        .agent_path = "/usr/bin/fm",
        .model = "claude-fable-5-thinking-high",
        .auto_review = true,
        .trust = true,
        .force = true,
        .no_resume = true,
        .mode = "plan",
        .print = true,
        .output_format = "json",
        .worktree = .{ .named = "wt" },
        .workspace = "/ws",
        .add_dirs = &.{"/extra"},
        .sandbox = "enabled",
        .extra_args = &.{ "--debug", "--max-turns", "7" },
        .backend = .fm,
    };
}

fn contains(argv: []const []const u8, s: []const u8) bool {
    for (argv) |a| if (std.mem.eql(u8, a, s)) return true;
    return false;
}

fn indexOfArg(argv: []const []const u8, s: []const u8) ?usize {
    for (argv, 0..) |a, i| if (std.mem.eql(u8, a, s)) return i;
    return null;
}

const cursor_flags = [_][]const u8{ "--auto-review", "--trust", "--force", "--mode", "--worktree", "--workspace", "--sandbox", "--debug", "--max-turns", "--resume", "--print", "--output-format" };

test "looks like flags" {
    try std.testing.expect(looksLikeFlags(&.{"--force"}));
    try std.testing.expect(looksLikeFlags(&.{ "  ", "-p" }));
    try std.testing.expect(!looksLikeFlags(&.{"explain --force"}));
    try std.testing.expect(!looksLikeFlags(&.{}));
}

test "fm argv never leaks cursor flags" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const cfg = maximalCursorConfig();
    const argv = try buildArgs(arena.allocator(), std.testing.io, &cfg, null, &.{"hello"});
    for (cursor_flags) |f| {
        if (std.mem.eql(u8, f, "--resume")) continue;
        try std.testing.expect(!contains(argv, f));
    }
    try std.testing.expectEqualStrings("system", argv[2]);
    const i = indexOfArg(argv, "--instructions").?;
    try std.testing.expect(std.mem.find(u8, argv[i + 1], "Produce a plan only") != null);
}

test "fm model collapses cursor ids to system" {
    try std.testing.expectEqualStrings("system", fmModel("claude-fable-5-thinking-high"));
    try std.testing.expectEqualStrings("system", fmModel("auto"));
    try std.testing.expectEqualStrings("pcc", fmModel("pcc"));
    try std.testing.expectEqualStrings("pcc", fmModel("Private_Cloud_Compute"));
    try std.testing.expectEqualStrings("system", fmModel("Private Cloud Compute"));
    try std.testing.expectEqualStrings("system", fmModel("my-cloud-model"));
}

test "abi argv never leaks cursor or fm flags and the prompt follows --" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    var cfg = maximalCursorConfig();
    cfg.backend = .abi;
    const argv = try buildArgs(arena.allocator(), std.testing.io, &cfg, "chat-1", &.{"--literal"});
    for (cursor_flags) |f| try std.testing.expect(!contains(argv, f));
    try std.testing.expect(!contains(argv, "--instructions"));
    try std.testing.expect(!contains(argv, "chat-1"));
    const dd = indexOfArg(argv, "--").?;
    try std.testing.expect(indexOfArg(argv, "--literal").? > dd);
    try std.testing.expectEqualStrings("local", argv[indexOfArg(argv, "--model").? + 1]);
}

test "abi transport is live only for explicit aliases" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expect((try abiTransport(a, "auto")) == .local);
    try std.testing.expect((try abiTransport(a, "fable")) == .local);
    try std.testing.expect((try abiTransport(a, "composer-2.5")) == .local);
    try std.testing.expectEqualStrings("claude-fable-5", (try abiTransport(a, "claude-fable-5")).live.?);
    try std.testing.expectEqualStrings("local", try abiNormalizeModel(a, "claude-fable-5-thinking-high"));
    try std.testing.expectEqualStrings("my-model", try abiNormalizeModel(a, "My-Model"));
    try std.testing.expect((try abiTransport(a, "my-live-model")) == .local);
    try std.testing.expect((try abiTransport(a, "anthropic-ish")) == .local);
    try std.testing.expect((try abiTransport(a, "live")).live == null);
    try std.testing.expect((try abiTransport(a, "anthropic")).live == null);
}

test "abi resume carries bounded transcript context" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var cfg = maximalCursorConfig();
    cfg.backend = .abi;
    cfg.mode = null;
    cfg.model = "auto";
    cfg.transcript_dir = root;
    var argv = try buildArgs(a, io, &cfg, "chat-9", &.{"next"});
    for (argv) |x| try std.testing.expect(std.mem.find(u8, x, "Previous conversation") == null);
    var pad: std.ArrayList(u8) = .empty;
    try pad.appendSlice(a, "### user\nremember the word xyzzy\n### abbey\nnoted\n");
    for (0..4000) |_| try pad.appendSlice(a, "pad ");
    try fsx.writeAll(io, try std.fs.path.join(a, &.{ root, "chat-9.transcript" }), pad.items, .default_file);
    argv = try buildArgs(a, io, &cfg, "chat-9", &.{"next"});
    var ctx_pos: ?usize = null;
    for (argv, 0..) |x, i| if (std.mem.find(u8, x, "Previous conversation") != null) {
        ctx_pos = i;
        try std.testing.expect(x.len <= context_tail_bytes + 200);
    };
    try std.testing.expect(ctx_pos.? > indexOfArg(argv, "--").?);
    try std.testing.expectEqualStrings("next", argv[argv.len - 1]);
}

test "abi mode rides in the input text" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    var cfg: AgentConfig = .{ .backend = .abi, .mode = "ask", .model = "local" };
    const argv = try buildArgs(arena.allocator(), std.testing.io, &cfg, null, &.{"q"});
    const dd = indexOfArg(argv, "--").?;
    try std.testing.expect(std.mem.find(u8, argv[dd + 1], "Do not modify files") != null);
    try std.testing.expectEqualStrings("q", argv[argv.len - 1]);
    cfg.model = "live";
    const live = try buildArgs(arena.allocator(), std.testing.io, &cfg, null, &.{"q"});
    try std.testing.expectEqualStrings("--live", live[1]);
}

test "claude argv clamps vocabulary and never leaks cursor flags" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var cfg = maximalCursorConfig();
    cfg.backend = .claude;
    cfg.model = "claude-fable-5-thinking-high";
    const argv = try buildArgs(a, std.testing.io, &cfg, null, &.{"hello"});
    for ([_][]const u8{ "--auto-review", "--trust", "--force", "--mode", "--worktree", "--workspace", "--sandbox", "--debug", "--max-turns", "--instructions" }) |f| try std.testing.expect(!contains(argv, f));
    try std.testing.expectEqualStrings("claude-fable-5", argv[indexOfArg(argv, "--model").? + 1]);
    try std.testing.expectEqualStrings("bypassPermissions", argv[indexOfArg(argv, "--permission-mode").? + 1]);
    try std.testing.expectEqualStrings("hello", argv[argv.len - 1]);
    try std.testing.expectEqualStrings("opus", (try claudeModel(a, "max")).?);
    try std.testing.expectEqualStrings("sonnet", (try claudeModel(a, "composer-2.5")).?);
    try std.testing.expect((try claudeModel(a, "auto")) == null);
    try std.testing.expectEqualStrings("opus", (try claudeModel(a, "gpt-5.6-sol-high")).?);
    try std.testing.expectEqualStrings("claude-opus-5", (try claudeModel(a, "claude-opus-5-thinking-high")).?);
}

test "claude session mints then resumes" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const cfg: AgentConfig = .{ .backend = .claude, .transcript_dir = root };
    var argv = try buildArgs(a, io, &cfg, "chat-7", &.{"x"});
    try std.testing.expectEqualStrings("chat-7", argv[indexOfArg(argv, "--session-id").? + 1]);
    try std.testing.expect(!contains(argv, "--resume"));
    try fsx.writeAll(io, try std.fs.path.join(a, &.{ root, "chat-7.transcript" }), "claude session established\n", .default_file);
    argv = try buildArgs(a, io, &cfg, "chat-7", &.{"x"});
    try std.testing.expectEqualStrings("chat-7", argv[indexOfArg(argv, "--resume").? + 1]);
    try std.testing.expect(!contains(argv, "--session-id"));
}

test "ollama argv and aliases" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    var cfg = maximalCursorConfig();
    cfg.backend = .ollama;
    const argv = try buildArgs(arena.allocator(), std.testing.io, &cfg, "chat-1", &.{"hi"});
    for (cursor_flags) |f| try std.testing.expect(!contains(argv, f));
    try std.testing.expect(!contains(argv, "chat-1"));
    try std.testing.expectEqualStrings("run", argv[0]);
    try std.testing.expectEqualStrings(models.ollama_default_model, argv[2]);
    for ([_][]const u8{ "gemma", "gemma:27b-mlx", "opus", "claude-opus-5-thinking-high", "auto", "composer-2.5" }) |m| {
        try std.testing.expectEqualStrings(models.ollama_default_model, ollamaNormalizeModel(m));
    }
    try std.testing.expectEqualStrings("gemma4:12b-mlx", ollamaNormalizeModel("gemma4:12b-mlx"));
}

test "cursor argv carries its own grammar" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    var cfg = maximalCursorConfig();
    cfg.backend = .cursor;
    const argv = try buildArgs(arena.allocator(), std.testing.io, &cfg, "chat-1", &.{"hi"});
    for ([_][]const u8{ "--model", "--auto-review", "--trust", "--force", "--mode", "--print", "--output-format", "--worktree", "wt", "--workspace", "--add-dir", "--sandbox", "--debug", "--resume", "chat-1", "hi" }) |f| try std.testing.expect(contains(argv, f));
}

test "utf8 tail and truncation respect boundaries and caps" {
    const tail = utf8Tail("\u{e9}\u{e9}\u{e9}\u{e9}", 5);
    try std.testing.expect(tail.len <= 5);
    try std.testing.expect(std.unicode.utf8ValidateSlice(tail));
    try std.testing.expectEqualStrings("short", utf8Tail("short", 100));
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    var big: std.ArrayList(u8) = .empty;
    for (0..300) |_| try big.appendSlice(arena.allocator(), "\u{e9}");
    const out = try truncateUtf8Bytes(arena.allocator(), big.items, 200);
    try std.testing.expect(out.len <= 200);
    try std.testing.expect(std.mem.find(u8, out, "truncated for OS argv limit") != null);
    try std.testing.expect(std.unicode.utf8ValidateSlice(out));
}
