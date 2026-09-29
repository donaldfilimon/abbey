//! The headless bypasses of the canonical path, all through this one file:
//! `print` and `commit` (Current) and `voice-ask` (Proposed, not present).
//! A bypass captures one backend run while resuming only the LIVE backend's
//! conversation; it writes no route record and no activity memory.
const std = @import("std");
const Ctx = @import("ctx.zig").Ctx;
const AgentConfig = @import("agent/argv.zig").AgentConfig;
const run = @import("agent/run.zig");
const state_mod = @import("state/state.zig");
const proc = @import("proc.zig");

pub const Error = run.Error || error{ NotARepo, NothingStaged, GitFailed };

pub fn runPrint(ctx: Ctx, arena: std.mem.Allocator, agent: *AgentConfig, st: *const state_mod.State, prompt: []const []const u8, abi_bin: ?[]const u8) Error!u8 {
    agent.print = true;
    const chat = try state_mod.resolveChatFor(ctx, arena, st, agent.backend);
    const exe = try run.execPath(ctx, arena, agent, abi_bin);
    const r = try run.runCapture(ctx, arena, agent, exe, chat, prompt);
    defer r.deinit(ctx.gpa);
    try ctx.err.writeAll(r.stderr);
    try ctx.out.writeAll(r.stdout);
    return r.code();
}

fn git(ctx: Ctx, arena: std.mem.Allocator, args: []const []const u8) Error![]const u8 {
    const full = try std.mem.concat(arena, []const u8, &.{ &.{"git"}, args });
    const r = proc.capture(ctx.gpa, ctx.io, full, .{ .env = ctx.env, .cwd = ctx.cwd, .limits = .{ .timeout_ms = 60_000 } }) catch return error.GitFailed;
    defer r.deinit(ctx.gpa);
    if (!r.success()) return error.GitFailed;
    return arena.dupe(u8, r.stdout);
}

/// Keep the first `max_lines` lines, noting the truncation (Rust `truncate_diff`).
pub fn truncateDiff(arena: std.mem.Allocator, diff: []const u8, max_lines: usize) error{OutOfMemory}![]const u8 {
    var total: usize = 0;
    var it = std.mem.splitScalar(u8, diff, '\n');
    while (it.next()) |_| total += 1;
    if (diff.len > 0 and diff[diff.len - 1] == '\n') total -= 1; // `lines()` ignores a trailing newline
    if (total <= max_lines) return diff;
    var out: std.ArrayList(u8) = .empty;
    var lines = std.mem.splitScalar(u8, diff, '\n');
    var i: usize = 0;
    while (lines.next()) |line| : (i += 1) {
        if (i == max_lines) break;
        if (i != 0) try out.append(arena, '\n');
        try out.appendSlice(arena, line);
    }
    try out.print(arena, "\n... [truncated at {d} lines; {d} total]\n", .{ max_lines, total });
    return out.items;
}

pub fn buildCommitPrompt(ctx: Ctx, arena: std.mem.Allocator) Error![]const u8 {
    _ = git(ctx, arena, &.{ "rev-parse", "--is-inside-work-tree" }) catch return error.NotARepo;
    const stat = try git(ctx, arena, &.{ "diff", "--cached", "--stat" });
    if (std.mem.trim(u8, stat, &std.ascii.whitespace).len == 0) return error.NothingStaged;
    const diff = try truncateDiff(arena, try git(ctx, arena, &.{ "diff", "--cached" }), 3000);
    return std.fmt.allocPrint(arena, "Write a concise conventional commit message for the staged changes below. Reply with ONLY the commit message (subject + optional body), no code fences, no explanation.\n\n```diff\n{s}\n```", .{diff});
}

pub fn runCommit(ctx: Ctx, arena: std.mem.Allocator, agent: *AgentConfig, st: *const state_mod.State, abi_bin: ?[]const u8) Error!u8 {
    const prompt = buildCommitPrompt(ctx, arena) catch |e| {
        switch (e) {
            error.NotARepo => try ctx.err.writeAll("abbey: not a git repository\n"),
            error.NothingStaged => try ctx.err.writeAll("abbey: nothing staged. git add files first, then: commit\n"),
            else => try ctx.err.print("abbey: git failed: {t}\n", .{e}),
        }
        return e;
    };
    return runPrint(ctx, arena, agent, st, &.{prompt}, abi_bin);
}

test "truncate diff caps lines" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expectEqualStrings("a\nb\nc", try truncateDiff(a, "a\nb\nc", 10));
    var buf: std.ArrayList(u8) = .empty;
    for (0..20) |i| try buf.print(a, "{s}line{d}", .{ if (i == 0) "" else "\n", i });
    const out = try truncateDiff(a, buf.items, 5);
    try std.testing.expect(std.mem.find(u8, out, "truncated at 5 lines; 20 total") != null);
    try std.testing.expect(std.mem.find(u8, out, "line4\n") != null and std.mem.find(u8, out, "line5") == null);
}
