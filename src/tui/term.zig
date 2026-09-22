//! Terminal access behind an injectable interface: raw mode with guaranteed
//! termios restore, the alternate screen, window size, and bounded reads.
//!
//! Restore paths: `Session.leave` runs from `defer` in the event loop, so a
//! normal quit and an error returned from the loop body both restore;
//! SIGINT/SIGTERM/SIGHUP only set `stop_requested` (async-signal-safe) and
//! the loop exits through the same `defer`; `emergencyRestore` is for the
//! process panic handler. Raw mode turns ISIG off, so Ctrl-C arrives as a
//! key, not a signal.
//! std: lib/std/posix.zig (tcgetattr, tcsetattr, termios, TCSA, winsize,
//! poll, read, sigaction), lib/std/c.zig (darwin termios flag structs, V,
//! T.IOCGWINSZ), lib/std/Io.zig (Operation.device_io_control, as in
//! lib/std/Progress.zig maybeUpdateSize).
const std = @import("std");
const builtin = @import("builtin");
const posix = std.posix;
const Io = std.Io;

pub const Attr = posix.termios;

pub const Error = error{ NotATerminal, TerminalFailed };

pub const Size = struct { cols: u16, rows: u16 };

pub const Terminal = struct {
    ud: ?*anyopaque,
    vtable: *const VTable,

    pub const VTable = struct {
        get_attr: *const fn (?*anyopaque) Error!Attr,
        set_attr: *const fn (?*anyopaque, Attr) Error!void,
        write: *const fn (?*anyopaque, []const u8) Error!void,
        size: *const fn (?*anyopaque) Size,
        /// Wait up to `timeout_ms` for input; 0 bytes means timeout.
        read: *const fn (?*anyopaque, []u8, i32) Error!usize,
    };

    pub fn getAttr(t: Terminal) Error!Attr {
        return t.vtable.get_attr(t.ud);
    }
    pub fn setAttr(t: Terminal, a: Attr) Error!void {
        return t.vtable.set_attr(t.ud, a);
    }
    pub fn write(t: Terminal, bytes: []const u8) Error!void {
        return t.vtable.write(t.ud, bytes);
    }
    pub fn size(t: Terminal) Size {
        return t.vtable.size(t.ud);
    }
    pub fn read(t: Terminal, buf: []u8, timeout_ms: i32) Error!usize {
        return t.vtable.read(t.ud, buf, timeout_ms);
    }
};

/// cfmakeraw(3) semantics, except VMIN=0/VTIME=0: the loop polls.
pub fn makeRaw(orig: Attr) Attr {
    var a = orig;
    a.iflag.IGNBRK = false;
    a.iflag.BRKINT = false;
    a.iflag.PARMRK = false;
    a.iflag.ISTRIP = false;
    a.iflag.INLCR = false;
    a.iflag.IGNCR = false;
    a.iflag.ICRNL = false;
    a.iflag.IXON = false;
    a.oflag.OPOST = false;
    a.lflag.ECHO = false;
    a.lflag.ECHONL = false;
    a.lflag.ICANON = false;
    a.lflag.ISIG = false;
    a.lflag.IEXTEN = false;
    a.cflag.PARENB = false;
    a.cflag.CSIZE = .CS8;
    a.cc[@backingInt(posix.V.MIN)] = 0;
    a.cc[@backingInt(posix.V.TIME)] = 0;
    return a;
}

pub const enter_seq = "\x1b[?1049h\x1b[?25l\x1b[2J";
pub const leave_seq = "\x1b[0m\x1b[?25h\x1b[?1049l";

/// The session the panic handler must undo, if any.
var active: ?*Session = null;

/// Raw mode + alternate screen, with the original attributes kept.
pub const Session = struct {
    term: Terminal,
    saved: Attr,
    raw: bool = false,

    /// Save the attributes, go raw, enter the alternate screen. On failure
    /// after the attributes changed, they are restored before returning.
    pub fn enter(term: Terminal) Error!Session {
        const saved = try term.getAttr();
        var s: Session = .{ .term = term, .saved = saved };
        try s.resumeRaw();
        return s;
    }

    /// (Re-)enter raw mode after `leave` (the loop does this around runs).
    pub fn resumeRaw(self: *Session) Error!void {
        if (self.raw) return;
        try self.term.setAttr(makeRaw(self.saved));
        self.raw = true;
        errdefer self.leave();
        try self.term.write(enter_seq);
    }

    /// Leave the alternate screen and restore the saved attributes.
    /// Idempotent; errors are swallowed because this runs on exit paths.
    pub fn leave(self: *Session) void {
        if (!self.raw) return;
        self.raw = false;
        self.term.write(leave_seq) catch {};
        self.term.setAttr(self.saved) catch {};
    }

    /// Make this session the one `emergencyRestore` undoes.
    pub fn arm(self: *Session) void {
        active = self;
    }

    pub fn disarm(self: *Session) void {
        if (active == self) active = null;
    }
};

/// Restore the armed session, if any (panic handler; best effort).
pub fn emergencyRestore() void {
    if (active) |s| {
        active = null;
        s.leave();
    }
}

// ---- signals ----

pub var stop_requested: std.atomic.Value(bool) = .init(false);
pub var resized: std.atomic.Value(bool) = .init(false);

fn onStop(_: posix.SIG) callconv(.c) void {
    stop_requested.store(true, .release);
}

fn onWinch(_: posix.SIG) callconv(.c) void {
    resized.store(true, .release);
}

pub const Signals = struct {
    old: [4]posix.Sigaction = undefined,

    const sigs = [_]posix.SIG{ .INT, .TERM, .HUP, .WINCH };

    /// Install handlers that only store atomics; `restore` puts the previous
    /// dispositions back.
    pub fn install() Signals {
        stop_requested.store(false, .release);
        resized.store(false, .release);
        var s: Signals = .{};
        for (sigs, 0..) |sig, i| {
            const act: posix.Sigaction = .{
                .handler = .{ .handler = if (sig == .WINCH) onWinch else onStop },
                .mask = posix.sigemptyset(),
                .flags = 0,
            };
            posix.sigaction(sig, &act, &s.old[i]);
        }
        return s;
    }

    pub fn restore(self: *const Signals) void {
        for (sigs, 0..) |sig, i| posix.sigaction(sig, &self.old[i], null);
    }
};

// ---- the real terminal (stdin for input, stdout for output) ----

pub const Posix = struct {
    io: Io,
    in: posix.fd_t,
    out: Io.File,

    pub fn init(io: Io) Posix {
        return .{ .io = io, .in = Io.File.stdin().handle, .out = Io.File.stdout() };
    }

    pub fn terminal(self: *Posix) Terminal {
        return .{ .ud = self, .vtable = &vtable };
    }

    const vtable: Terminal.VTable = .{ .get_attr = getAttr, .set_attr = setAttr, .write = write, .size = size, .read = read };

    fn cast(ud: ?*anyopaque) *Posix {
        return @ptrCast(@alignCast(ud.?));
    }

    fn getAttr(ud: ?*anyopaque) Error!Attr {
        return posix.tcgetattr(cast(ud).in) catch |e| switch (e) {
            error.NotATerminal => error.NotATerminal,
            else => error.TerminalFailed,
        };
    }

    fn setAttr(ud: ?*anyopaque, a: Attr) Error!void {
        posix.tcsetattr(cast(ud).in, .FLUSH, a) catch |e| return switch (e) {
            error.NotATerminal => error.NotATerminal,
            else => error.TerminalFailed,
        };
    }

    fn write(ud: ?*anyopaque, bytes: []const u8) Error!void {
        const self = cast(ud);
        self.out.writeStreamingAll(self.io, bytes) catch return error.TerminalFailed;
    }

    fn size(ud: ?*anyopaque) Size {
        const self = cast(ud);
        var ws: posix.winsize = .{ .row = 0, .col = 0, .xpixel = 0, .ypixel = 0 };
        const rc = (self.io.operate(.{ .device_io_control = .{
            .file = self.out,
            .code = posix.T.IOCGWINSZ,
            .arg = &ws,
        } }) catch return .{ .cols = 80, .rows = 24 }).device_io_control;
        if (rc < 0 or ws.col == 0 or ws.row == 0) return .{ .cols = 80, .rows = 24 };
        return .{ .cols = ws.col, .rows = ws.row };
    }

    fn read(ud: ?*anyopaque, buf: []u8, timeout_ms: i32) Error!usize {
        const self = cast(ud);
        var fds = [_]posix.pollfd{.{ .fd = self.in, .events = posix.POLL.IN, .revents = 0 }};
        // A failed wait reports a timeout so the loop re-checks its flags.
        const n = posix.poll(&fds, timeout_ms) catch return 0;
        if (n == 0) return 0;
        return posix.read(self.in, buf) catch |e| switch (e) {
            error.WouldBlock => 0,
            else => error.TerminalFailed,
        };
    }
};

// ---- a recording fake for tests ----

pub const Fake = struct {
    attr: Attr,
    set_calls: std.ArrayList(Attr) = .empty,
    written: std.ArrayList(u8) = .empty,
    gpa: std.mem.Allocator,
    cols: u16 = 100,
    rows: u16 = 30,
    /// Scripted reads, one chunk per call; afterwards reads time out.
    script: []const []const u8 = &.{},
    reads: usize = 0,
    /// Called before each read (tests use it to raise a signal).
    on_read: ?*const fn (*Fake) void = null,
    /// The read with this index fails (error-path tests).
    fail_read_at: ?usize = null,

    pub fn init(gpa: std.mem.Allocator) Fake {
        var a: Attr = std.mem.zeroes(Attr);
        a.lflag.ECHO = true;
        a.lflag.ICANON = true;
        a.lflag.ISIG = true;
        a.iflag.ICRNL = true;
        a.oflag.OPOST = true;
        return .{ .attr = a, .gpa = gpa };
    }

    pub fn deinit(self: *Fake) void {
        self.set_calls.deinit(self.gpa);
        self.written.deinit(self.gpa);
    }

    pub fn terminal(self: *Fake) Terminal {
        return .{ .ud = self, .vtable = &vt };
    }

    const vt: Terminal.VTable = .{ .get_attr = fGet, .set_attr = fSet, .write = fWrite, .size = fSize, .read = fRead };

    fn cast(ud: ?*anyopaque) *Fake {
        return @ptrCast(@alignCast(ud.?));
    }
    fn fGet(ud: ?*anyopaque) Error!Attr {
        return cast(ud).attr;
    }
    fn fSet(ud: ?*anyopaque, a: Attr) Error!void {
        const self = cast(ud);
        self.set_calls.append(self.gpa, a) catch return error.TerminalFailed;
        self.attr = a;
    }
    fn fWrite(ud: ?*anyopaque, b: []const u8) Error!void {
        const self = cast(ud);
        self.written.appendSlice(self.gpa, b) catch return error.TerminalFailed;
    }
    fn fSize(ud: ?*anyopaque) Size {
        const self = cast(ud);
        return .{ .cols = self.cols, .rows = self.rows };
    }
    fn fRead(ud: ?*anyopaque, buf: []u8, _: i32) Error!usize {
        const self = cast(ud);
        if (self.on_read) |f| f(self);
        defer self.reads += 1;
        if (self.fail_read_at) |i| if (i == self.reads) return error.TerminalFailed;
        if (self.reads >= self.script.len) return 0;
        const chunk = self.script[self.reads];
        const n = @min(chunk.len, buf.len);
        @memcpy(buf[0..n], chunk[0..n]);
        return n;
    }

    /// True when the attributes now equal the ones the fake started with.
    pub fn restoredTo(self: *const Fake, orig: Attr) bool {
        return std.mem.eql(u8, std.mem.asBytes(&self.attr), std.mem.asBytes(&orig));
    }
};

test "makeRaw clears canonical mode, echo, signals, and CR translation" {
    const f = Fake.init(std.testing.allocator);
    const raw = makeRaw(f.attr);
    try std.testing.expect(!raw.lflag.ECHO and !raw.lflag.ICANON and !raw.lflag.ISIG and !raw.lflag.IEXTEN);
    try std.testing.expect(!raw.iflag.ICRNL and !raw.iflag.IXON and !raw.oflag.OPOST);
    try std.testing.expectEqual(@as(u8, 0), raw.cc[@backingInt(posix.V.MIN)]);
}

test "session restores termios once, and emergencyRestore undoes an armed session" {
    var f = Fake.init(std.testing.allocator);
    defer f.deinit();
    const orig = f.attr;
    var s = try Session.enter(f.terminal());
    try std.testing.expect(!f.attr.lflag.ICANON);
    try std.testing.expect(std.mem.endsWith(u8, f.written.items, enter_seq));
    s.leave();
    s.leave();
    try std.testing.expect(f.restoredTo(orig));
    try std.testing.expectEqual(@as(usize, 2), f.set_calls.items.len);
    try std.testing.expect(std.mem.endsWith(u8, f.written.items, leave_seq));

    try s.resumeRaw();
    s.arm();
    emergencyRestore();
    try std.testing.expect(f.restoredTo(orig));
    emergencyRestore(); // disarmed: a second call is a no-op
    try std.testing.expectEqual(@as(usize, 4), f.set_calls.items.len);
}
