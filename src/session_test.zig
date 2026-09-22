//! End-to-end tests of the canonical path and the headless bypass with a
//! stub `abi` that echoes its argv. No real executor is spawned.
const std = @import("std");
const T = @import("ctx.zig").TestCtx;
const tmpPath = @import("ctx.zig").tmpPath;
const config = @import("config/config.zig");
const state_mod = @import("state/state.zig");
const session = @import("session.zig");
const actions = @import("actions.zig");
const capture = @import("capture.zig");
const route_log = @import("route_log.zig");
const fsx = @import("util/fsx.zig");
const edition = @import("edition.zig");
const AgentConfig = @import("agent/argv.zig").AgentConfig;

const stub_script = "#!/bin/sh\nprintf 'ARGV:'; for a in \"$@\"; do printf '[%s]' \"$a\"; done; printf '\\n'\n";

const Fixture = struct {
    tmp: std.testing.TmpDir,
    root: []u8,
    t: T,
    arena: std.heap.ArenaAllocator,
    cfg: config.Config,
    st: state_mod.State,
    stub: []const u8,

    fn init() !*Fixture {
        const gpa = std.testing.allocator;
        const io = std.testing.io;
        const f = try gpa.create(Fixture);
        errdefer gpa.destroy(f);
        f.tmp = std.testing.tmpDir(.{});
        f.root = try tmpPath(gpa, io, f.tmp.dir);
        f.t = T.init(gpa, f.root);
        f.arena = .init(gpa);
        const a = f.arena.allocator();
        try f.t.env.put("HOME", f.root);
        try f.t.env.put("PATH", "/usr/bin:/bin:/opt/homebrew/bin");
        try f.t.env.put(edition.id.state_dir_env, try std.fs.path.join(a, &.{ f.root, "state" }));
        try f.t.env.put(edition.id.config_path_env, try std.fs.path.join(a, &.{ f.root, "absent.toml" }));
        f.stub = try std.fs.path.join(a, &.{ f.root, "stub-abi" });
        try fsx.writeAll(io, f.stub, stub_script, .executable_file);
        const c = f.t.ctx(gpa, io);
        f.cfg = try config.load(c);
        f.st = try state_mod.load(c, a);
        return f;
    }

    fn deinit(f: *Fixture) void {
        const gpa = std.testing.allocator;
        f.cfg.deinit();
        f.arena.deinit();
        f.t.deinit();
        gpa.free(f.root);
        f.tmp.cleanup();
        gpa.destroy(f);
    }

    fn agent(f: *Fixture) !AgentConfig {
        return .{
            .agent_path = f.stub,
            .backend = .abi,
            .model = "local",
            .transcript_dir = try std.fs.path.join(f.arena.allocator(), &.{ f.st.state_dir, "abi" }),
        };
    }

    fn sess(f: *Fixture) session.Session {
        return .{ .ctx = f.t.ctx(std.testing.allocator, std.testing.io), .arena = f.arena.allocator(), .cfg = &f.cfg, .state = &f.st };
    }

    fn routeCount(f: *Fixture) !usize {
        return (try route_log.recent(f.arena.allocator(), std.testing.io, f.st.state_dir, 1000)).len;
    }
};

test "print bypass leaves route.jsonl untouched where ask appends one record" {
    const f = try Fixture.init();
    defer f.deinit();
    const a = f.arena.allocator();
    const io = std.testing.io;
    var ag = try f.agent();
    const rl = try route_log.path(a, f.st.state_dir);

    try std.testing.expectEqual(@as(u8, 0), try capture.runPrint(f.sess().ctx, a, &ag, &f.st, &.{"hello from print"}, null));
    try std.testing.expect(!fsx.exists(io, rl));
    try std.testing.expect(std.mem.find(u8, f.t.outText(), "[complete][--model][local][--][hello from print]") != null);

    var ag2 = try f.agent();
    try std.testing.expectEqual(@as(u8, 0), try actions.runAgent(f.sess(), &ag2, &.{"fix the compile error in main"}, actions.RunSpec.ask()));
    try std.testing.expectEqual(@as(usize, 1), try f.routeCount());
    const recs = try route_log.recent(a, io, f.st.state_dir, 1);
    try std.testing.expectEqualStrings("abbey", recs[0].persona);
    try std.testing.expectEqualStrings("gemma", recs[0].role);
    try std.testing.expectEqualStrings("local", recs[0].model);
    try std.testing.expectEqualStrings("persona=abbey role=gemma class=Code", recs[0].reason);
    try std.testing.expectEqual(@as(f32, 0.95), recs[0].confidence);
    try std.testing.expectEqualStrings(f.root, recs[0].cwd);

    // The argv the backend saw is exactly what Rust abbey 2.6.0 built for
    // the same verb and prompt (captured with the same stub, 2026-09-21).
    const rust_argv = "ARGV:[complete][--model][local][--][Answer the question. Do not modify files.][Worker role: Gemma (visual/conversational). Prioritize clear human-facing interpretation, tone, and multimodal description when relevant.\n\nAbbey: fix the compile error in main\n\nI\u{2019}ll approach this with warmth, creativity, and technical care while keeping uncertainty explicit.]";
    try std.testing.expect(std.mem.find(u8, f.t.outText(), rust_argv) != null);

    // A chat was minted, saved, and the turn recorded in the abi transcript.
    const chat = (try state_mod.resolveChatFor(f.sess().ctx, a, &f.st, .abi)).?;
    const tp = try std.fs.path.join(a, &.{ f.st.state_dir, "abi", try std.fmt.allocPrint(a, "{s}.transcript", .{chat}) });
    const tr = (try fsx.readOptional(io, a, tp, 1 << 20)).?;
    try std.testing.expect(std.mem.startsWith(u8, tr, "### user\nWorker role: Gemma"));
    try std.testing.expect(std.mem.find(u8, tr, "\n### abbey\nARGV:[complete]") != null);

    // A second print resumes nothing server-side and still writes no route.
    try std.testing.expectEqual(@as(u8, 0), try capture.runPrint(f.sess().ctx, a, &ag, &f.st, &.{"again"}, null));
    try std.testing.expectEqual(@as(usize, 1), try f.routeCount());

    // Activity memory was recorded by the canonical path only.
    const mem = @import("memory/store.zig");
    const s = try mem.open(std.testing.allocator, io, a, f.st.state_dir);
    const act = try s.filter(a, "activity", null, 10);
    try std.testing.expectEqual(@as(usize, 1), act.len);
    try std.testing.expectEqualStrings("route abbey/gemma", act[0].summary);
}

test "second ask carries bounded transcript context and a nonzero exit is not retried for abi" {
    const f = try Fixture.init();
    defer f.deinit();
    var ag = try f.agent();
    _ = try actions.runAgent(f.sess(), &ag, &.{"remember xyzzy"}, actions.RunSpec.ask());
    f.t.out.clearRetainingCapacity();
    var ag2 = try f.agent();
    _ = try actions.runAgent(f.sess(), &ag2, &.{"what word"}, actions.RunSpec.ask());
    try std.testing.expect(std.mem.find(u8, f.t.outText(), "[Previous conversation (context, oldest first, may be truncated):\n### user\n") != null);
    const failing = try std.fs.path.join(f.arena.allocator(), &.{ f.root, "fail-abi" });
    try fsx.writeAll(std.testing.io, failing, "#!/bin/sh\necho boom >&2\nexit 7\n", .executable_file);
    var ag3 = try f.agent();
    ag3.agent_path = failing;
    try std.testing.expectEqual(@as(u8, 7), try actions.runAgent(f.sess(), &ag3, &.{"x"}, actions.RunSpec.ask()));
    try std.testing.expect(std.mem.find(u8, f.t.errText(), "creating a new chat") == null);
    try std.testing.expectEqual(@as(usize, 3), try f.routeCount());
}
