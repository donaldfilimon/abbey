//! Executor invocation: bounded capture, interactive hand-off, Abbey-side
//! transcripts for local one-shot backends, and the resilient
//! resume-or-create session (port of Rust `agent/mod.rs`).
const std = @import("std");
const Io = std.Io;
const Ctx = @import("../ctx.zig").Ctx;
const argv_mod = @import("argv.zig");
const AgentConfig = argv_mod.AgentConfig;
const backend = @import("backend.zig");
const proc = @import("../proc.zig");
const fsx = @import("../util/fsx.zig");
const uuid = @import("../util/uuid.zig");
const state_mod = @import("../state/state.zig");

pub const max_local_transcript_bytes = 1024 * 1024;
pub const max_transcript_prompt_bytes = 16 * 1024;
pub const max_transcript_output_bytes = 48 * 1024;

pub const Error = error{
    ExecutorNotFound,
    CaptureTooLarge,
    CaptureTimedOut,
    SpawnFailed,
    CreateChatFailed,
} || state_mod.Error || state_mod.IdError || argv_mod.Error || Io.Writer.Error;

/// Where transcripts for `chat_id` live, or null without a transcript dir.
pub fn transcriptPath(arena: std.mem.Allocator, cfg: *const AgentConfig, chat_id: []const u8) error{OutOfMemory}!?[]const u8 {
    return argv_mod.transcriptPath(arena, cfg, chat_id);
}

/// The binary to spawn: the startup-resolved path, else resolved now for
/// the LIVE backend (the ABBEY_AGENT override belongs to the env backend).
pub fn execPath(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, abi_bin: ?[]const u8) Error![]const u8 {
    if (cfg.agent_path.len != 0) return cfg.agent_path;
    if (cfg.backend == .cursor) if (backend.legacyAgentPath(ctx)) |p| return p;
    return backend.resolveFor(.{ .ctx = ctx, .arena = arena, .abi_bin = abi_bin }, cfg.backend) catch |e| switch (e) {
        error.OutOfMemory => error.OutOfMemory,
        else => {
            ctx.err.print("abbey: {s}\n", .{backend.notFoundMessage(cfg.backend)}) catch {};
            return error.ExecutorNotFound;
        },
    };
}

/// Headless capture of one run. Caller frees via `Result.deinit`.
pub fn runCapture(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, exe: []const u8, resume_id: ?[]const u8, prompts: []const []const u8) Error!proc.Result {
    var c = cfg.*;
    c.print = true;
    if (!c.backend.isOneshotLocal() and argv_mod.looksLikeFlags(prompts)) {
        try ctx.err.writeAll("abbey: prompt begins with `-`: the backend will read it as options, not text (e.g. `--force` enables always-approve)\n");
    }
    const args = try argv_mod.buildArgs(arena, ctx.io, &c, resume_id, prompts);
    const full = try std.mem.concat(arena, []const u8, &.{ &.{exe}, args });
    return proc.capture(ctx.gpa, ctx.io, full, .{ .env = ctx.env }) catch |e| switch (e) {
        error.StreamTooLong => {
            try ctx.err.print("abbey: agent output exceeded the {d}-byte limit\n", .{proc.Limits.stdout_bytes_default});
            return error.CaptureTooLarge;
        },
        error.Timeout => {
            try ctx.err.writeAll("abbey: agent capture exceeded the 30-minute limit\n");
            return error.CaptureTimedOut;
        },
        error.OutOfMemory => error.OutOfMemory,
        else => {
            try ctx.err.print("abbey: exec {s}: {t}\n", .{ exe, e });
            return error.SpawnFailed;
        },
    };
}

/// Interactive hand-off with inherited stdio.
fn runInteractive(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, exe: []const u8, resume_id: ?[]const u8, prompts: []const []const u8) Error!u8 {
    if (!cfg.backend.isOneshotLocal() and argv_mod.looksLikeFlags(prompts)) {
        try ctx.err.writeAll("abbey: prompt begins with `-`: the backend will read it as options, not text (e.g. `--force` enables always-approve)\n");
    }
    const args = try argv_mod.buildArgs(arena, ctx.io, cfg, resume_id, prompts);
    const full = try std.mem.concat(arena, []const u8, &.{ &.{exe}, args });
    try ctx.out.flush();
    try ctx.err.flush();
    // std: lib/std/process.zig (spawn, SpawnOptions), lib/std/process/Child.zig (wait)
    var child = std.process.spawn(ctx.io, .{ .argv = full, .environ_map = ctx.env }) catch |e| {
        try ctx.err.print("abbey: exec {s}: {t}\n", .{ exe, e });
        return error.SpawnFailed;
    };
    const term = child.wait(ctx.io) catch return error.SpawnFailed;
    return switch (term) {
        .exited => |code| code,
        else => 1,
    };
}

/// Record one local one-shot turn (best effort: never fails the run).
pub fn appendLocalTranscript(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, chat_id: []const u8, prompts: []const []const u8, output: []const u8) void {
    appendLocalTranscriptInner(ctx, arena, cfg, chat_id, prompts, output) catch {};
}

fn appendLocalTranscriptInner(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, chat_id: []const u8, prompts: []const []const u8, output: []const u8) !void {
    const path = (try transcriptPath(arena, cfg, chat_id)) orelse return;
    const joined = try std.mem.join(arena, " ", prompts);
    const prompt = try argv_mod.truncateUtf8Bytes(arena, std.mem.trim(u8, joined, &std.ascii.whitespace), max_transcript_prompt_bytes);
    const out = try argv_mod.truncateUtf8Bytes(arena, std.mem.trim(u8, output, &std.ascii.whitespace), max_transcript_output_bytes);
    const entry = try std.fmt.allocPrint(arena, "### user\n{s}\n### abbey\n{s}\n", .{ prompt, out });
    if (std.fs.path.dirname(path)) |d| try fsx.makePath(ctx.io, d);
    const f = try Io.Dir.cwd().createFile(ctx.io, path, .{ .truncate = false, .read = true, .lock = .exclusive });
    const existing = f.length(ctx.io) catch 0;
    if (existing + entry.len > max_local_transcript_bytes) {
        f.close(ctx.io);
        const prev = try std.fmt.allocPrint(arena, "{s}.prev", .{path});
        Io.Dir.cwd().deleteFile(ctx.io, prev) catch {};
        try Io.Dir.cwd().rename(path, Io.Dir.cwd(), prev, ctx.io);
        const body = try std.mem.concat(arena, u8, &.{ "### earlier turns omitted at transcript size limit; previous file retained as .transcript.prev\n", entry });
        try fsx.writeAll(ctx.io, path, body, .default_file);
        return;
    }
    defer f.close(ctx.io);
    try f.writePositionalAll(ctx.io, entry, existing);
}

/// Serialize concurrent turns of one local conversation.
fn lockLocalTurn(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, chat_id: []const u8) !?Io.File {
    const path = (try transcriptPath(arena, cfg, chat_id)) orelse return null;
    if (std.fs.path.dirname(path)) |d| try fsx.makePath(ctx.io, d);
    const lock_path = try std.fmt.allocPrint(arena, "{s}.lock", .{path});
    return try Io.Dir.cwd().createFile(ctx.io, lock_path, .{ .truncate = false, .lock = .exclusive });
}

fn touchClaudeMarker(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, chat_id: []const u8) void {
    const path = (transcriptPath(arena, cfg, chat_id) catch return) orelse return;
    fsx.writeAll(ctx.io, path, "claude session established\n", .default_file) catch {};
}

/// Mint a chat id: local backends get a uuid backed by a transcript file;
/// server backends ask the executor (`create-chat`).
pub fn createChat(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, exe_opt: ?[]const u8) Error![]const u8 {
    if (!cfg.backend.hasServerSessions()) {
        var buf: [uuid.len]u8 = undefined;
        const id = try arena.dupe(u8, uuid.v4(ctx.io, &buf));
        if (cfg.transcript_dir) |d| try fsx.makePath(ctx.io, d);
        return id;
    }
    const exe = exe_opt orelse return error.ExecutorNotFound;
    const r = proc.capture(ctx.gpa, ctx.io, &.{ exe, "create-chat" }, .{ .env = ctx.env, .limits = .{ .timeout_ms = 60_000, .stdout_bytes = 64 * 1024 } }) catch return error.CreateChatFailed;
    defer r.deinit(ctx.gpa);
    const id = std.mem.trim(u8, r.stdout, &std.ascii.whitespace);
    if (!r.success() or id.len == 0) {
        try ctx.err.print("abbey: create-chat failed: {s}\n", .{r.stderr});
        return error.CreateChatFailed;
    }
    return arena.dupe(u8, id);
}

fn runOnce(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, exe: []const u8, resume_id: ?[]const u8, prompts: []const []const u8) Error!u8 {
    const capture_print = cfg.print and cfg.output_format == null;
    if (capture_print or cfg.backend.isOneshotLocal()) {
        var lock: ?Io.File = null;
        if (cfg.backend.isOneshotLocal()) if (resume_id) |id| if (id.len != 0) {
            lock = lockLocalTurn(ctx, arena, cfg, id) catch null;
        };
        defer if (lock) |l| l.close(ctx.io);
        const r = try runCapture(ctx, arena, cfg, exe, resume_id, prompts);
        defer r.deinit(ctx.gpa);
        try ctx.err.writeAll(r.stderr);
        if (r.success()) if (resume_id) |id| if (id.len != 0) {
            if (cfg.backend.isOneshotLocal()) appendLocalTranscript(ctx, arena, cfg, id, prompts, r.stdout);
            if (cfg.backend == .claude) touchClaudeMarker(ctx, arena, cfg, id);
        };
        try ctx.out.writeAll(r.stdout);
        return r.code();
    }
    const code = try runInteractive(ctx, arena, cfg, exe, resume_id, prompts);
    if (code == 0 and cfg.backend == .claude) if (resume_id) |id| if (id.len != 0) touchClaudeMarker(ctx, arena, cfg, id);
    return code;
}

/// Which chat id stays after a failed resume forced a retry.
pub fn chatToPersist(original: []const u8, retried: []const u8, retry_code: u8) []const u8 {
    return if (retry_code == 0) retried else original;
}

/// Resume or create; on failure (server backends only) create once and retry.
pub fn runResilient(ctx: Ctx, arena: std.mem.Allocator, cfg: *const AgentConfig, st: *const state_mod.State, fresh: bool, prompts: []const []const u8, abi_bin: ?[]const u8) Error!u8 {
    const exe = try execPath(ctx, arena, cfg, abi_bin);
    if (fresh or cfg.no_resume) {
        if (fresh) {
            const id = try createChat(ctx, arena, cfg, exe);
            try state_mod.saveChat(ctx, arena, st, id);
            try ctx.err.print("abbey: new chat {s}\n", .{id});
            return runOnce(ctx, arena, cfg, exe, id, prompts);
        }
        return runOnce(ctx, arena, cfg, exe, null, prompts);
    }
    const chat = if (try state_mod.resolveChatFor(ctx, arena, st, cfg.backend)) |id| id else blk: {
        const id = try createChat(ctx, arena, cfg, exe);
        try state_mod.saveChat(ctx, arena, st, id);
        try ctx.err.print("abbey: created chat {s}\n", .{id});
        break :blk id;
    };
    const code = try runOnce(ctx, arena, cfg, exe, chat, prompts);
    if (code == 0) {
        try state_mod.saveChat(ctx, arena, st, chat);
        return 0;
    }
    if (!cfg.backend.hasServerSessions()) return code;
    try ctx.err.print("abbey: resume of {s} failed (exit {d}); creating a new chat\n", .{ chat, code });
    const id = try createChat(ctx, arena, cfg, exe);
    try ctx.err.print("abbey: new chat {s}\n", .{id});
    const retry = try runOnce(ctx, arena, cfg, exe, id, prompts);
    try state_mod.saveChat(ctx, arena, st, chatToPersist(chat, id, retry));
    if (retry != 0) try ctx.err.print("abbey: new chat also failed (exit {d}); keeping {s}\n", .{ retry, chat });
    return retry;
}

test "failed retry keeps the original chat" {
    try std.testing.expectEqualStrings("new", chatToPersist("old", "new", 0));
    try std.testing.expectEqualStrings("old", chatToPersist("old", "new", 1));
}

test "local transcript rolls over and keeps the previous file" {
    const T = @import("../ctx.zig").TestCtx;
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    var t = T.init(gpa, root);
    defer t.deinit();
    const c = t.ctx(gpa, io);
    var arena: std.heap.ArenaAllocator = .init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const cfg: AgentConfig = .{ .backend = .abi, .transcript_dir = root };
    appendLocalTranscript(c, a, &cfg, "kept", &.{"first"}, "one");
    const large = try a.alloc(u8, max_transcript_output_bytes);
    @memset(large, 'x');
    for (0..40) |i| appendLocalTranscript(c, a, &cfg, "kept", &.{try std.fmt.allocPrint(a, "large-{d}", .{i})}, large);
    const cur = (try fsx.readOptional(io, a, try std.fs.path.join(a, &.{ root, "kept.transcript" }), 4 << 20)).?;
    try std.testing.expect(std.mem.find(u8, cur, "previous file retained as .transcript.prev") != null);
    try std.testing.expect(cur.len <= max_local_transcript_bytes);
    const prev = (try fsx.readOptional(io, a, try std.fs.path.join(a, &.{ root, "kept.transcript.prev" }), 4 << 20)).?;
    try std.testing.expect(std.mem.find(u8, prev, "### user") != null);
}
