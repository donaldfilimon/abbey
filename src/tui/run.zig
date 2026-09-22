//! The TUI event loop and the `tui` verb (port of Rust `run_tui`).
//!
//! Order per iteration, as in Rust: draw; if an action is pending, leave raw
//! mode and the alternate screen, run it through the `Runner` with the App's
//! LIVE `agent`, then re-enter; quit if asked; wait up to 100 ms for input.
//! Raw mode is left by `defer` on every exit path (quit, error, signal).
const std = @import("std");
const Io = std.Io;
const Ctx = @import("../ctx.zig").Ctx;
const app_mod = @import("app.zig");
const App = app_mod.App;
const term = @import("term.zig");
const input = @import("input.zig");
const frame = @import("frame.zig");
const ui = @import("ui.zig");
const theme = @import("theme.zig");
const host_mod = @import("host.zig");
const AgentConfig = @import("../agent/argv.zig").AgentConfig;
const config = @import("../config/config.zig");
const state_mod = @import("../state/state.zig");
const backend = @import("../agent/backend.zig");
const session = @import("../session.zig");
const actions = @import("../actions.zig");
const fsx = @import("../util/fsx.zig");

/// Executes a pending run. `prompt` is the trimmed composer text (may be
/// empty). Returns the exit code shown as `last=`.
pub const Runner = struct {
    ud: ?*anyopaque,
    run: *const fn (?*anyopaque, *AgentConfig, []const u8, bool) u8,
};

pub const Error = term.Error || error{OutOfMemory};

/// Exit status for a loop stopped by SIGINT/SIGTERM/SIGHUP.
pub const signal_exit: u8 = 130;

/// Draw one frame of `app` at `size` into ANSI bytes.
fn render(gpa: std.mem.Allocator, app: *App, size: term.Size, out: *Io.Writer.Allocating) Error!void {
    var f = try frame.Frame.init(gpa, size.cols, size.rows);
    defer f.deinit(gpa);
    app.ensureVisible(ui.listViewport(app, size.rows));
    ui.draw(&f, app);
    out.clearRetainingCapacity();
    f.writeAnsi(&out.writer) catch return error.OutOfMemory;
}

pub fn loop(gpa: std.mem.Allocator, t: term.Terminal, app: *App, runner: Runner) Error!u8 {
    const signals = term.Signals.install();
    defer signals.restore();
    var sess = try term.Session.enter(t);
    sess.arm();
    defer {
        sess.disarm();
        sess.leave();
    }
    var out: Io.Writer.Allocating = .init(gpa);
    defer out.deinit();
    var code: u8 = 0;
    var buf: [256]u8 = undefined;
    var keys: [256]app_mod.Key = undefined;
    while (true) {
        if (term.stop_requested.load(.acquire)) return signal_exit;
        _ = term.resized.swap(false, .acq_rel); // size is re-read every frame
        try render(gpa, app, t.size(), &out);
        try t.write(out.written());

        if (app.pending != .none) {
            const fresh = app.pending == .run_fresh;
            app.pending = .none;
            sess.leave();
            const prompt = std.mem.trim(u8, app.input.items, &std.ascii.whitespace);
            const rc = runner.run(runner.ud, app.agent, prompt, fresh);
            app.last_code = rc;
            app.setStatus("agent exited {d} . Enter to run again", .{rc});
            app.clearInput();
            app.overlay = .none;
            app.host.refresh(app.host.ud, app, .doctor);
            app.host.refresh(app.host.ud, app, .memory);
            code = rc;
            try sess.resumeRaw();
            continue;
        }
        if (app.should_quit) break;

        const n = try t.read(&buf, 100);
        for (input.decodeAll(buf[0..n], &keys)) |k| app.handleKey(k);
        app.tick +%= 1;
    }
    return code;
}

/// The canonical path for a TUI run: `actions.runAgent` with the live
/// `AgentConfig` (never one rebuilt from the environment).
pub const LiveRunner = struct {
    ctx: Ctx,
    cfg: *const config.Config,
    st: *const state_mod.State,

    pub fn runner(self: *LiveRunner) Runner {
        return .{ .ud = self, .run = run };
    }

    fn run(ud: ?*anyopaque, agent: *AgentConfig, prompt: []const u8, fresh: bool) u8 {
        const self: *LiveRunner = @ptrCast(@alignCast(ud.?));
        var arena: std.heap.ArenaAllocator = .init(self.ctx.gpa);
        defer arena.deinit();
        const s: session.Session = .{ .ctx = self.ctx, .arena = arena.allocator(), .cfg = self.cfg, .state = self.st };
        const words: []const []const u8 = if (prompt.len == 0) &.{} else &.{prompt};
        const spec = if (fresh) actions.RunSpec.fresh_() else actions.RunSpec.@"resume"();
        const rc = actions.runAgent(s, agent, words, spec) catch |e| blk: {
            self.ctx.err.print("abbey: {t}\n", .{e}) catch {};
            break :blk 1;
        };
        self.ctx.out.flush() catch {};
        self.ctx.err.flush() catch {};
        return rc;
    }
};

/// `abbey-zig tui`: dispatched after config and state load, with the
/// startup-resolved `agent`, which the App then owns by pointer.
pub fn cli(ctx: Ctx, cfg: *const config.Config, st: *const state_mod.State, agent: *AgentConfig, sel: backend.Selection) !u8 {
    const stdin_tty = Io.File.stdin().isTty(ctx.io) catch false; // std: lib/std/Io/File.zig
    const stdout_tty = Io.File.stdout().isTty(ctx.io) catch false;
    if (!stdin_tty or !stdout_tty) {
        try ctx.err.writeAll("abbey: tui needs an interactive terminal (stdin and stdout must be TTYs); use `ask` or `print` headless\n");
        return 2;
    }
    var live = host_mod.Live.init(ctx, cfg, st, sel);
    defer live.deinit();
    const theme_file = std.fs.path.join(ctx.gpa, &.{ st.state_dir, theme.file_name }) catch return error.OutOfMemory;
    defer ctx.gpa.free(theme_file);
    const theme_text = fsx.readOptional(ctx.io, ctx.gpa, theme_file, 64) catch null;
    defer if (theme_text) |tt| ctx.gpa.free(tt);
    var app = try App.init(ctx.gpa, live.host(), agent, st.state_dir, theme.Id.resolve(ctx.getEnv(theme.env_var), theme_text));
    defer app.deinit();
    app.cwd = st.cwd;
    app.home = ctx.envNonEmpty("HOME");
    app.load();
    var runner: LiveRunner = .{ .ctx = ctx, .cfg = cfg, .st = st };
    var posix_term = term.Posix.init(ctx.io);
    try ctx.out.flush();
    return loop(ctx.gpa, posix_term.terminal(), &app, runner.runner());
}
