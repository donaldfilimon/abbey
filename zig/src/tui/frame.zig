//! Cell grid the TUI draws into, plus its two encoders: plain text (the
//! golden-test form) and ANSI (truecolor SGR, absolute cursor moves).
//!
//! Every codepoint occupies exactly one cell: East Asian wide and combining
//! characters are not measured, so such text can misalign a row (it cannot
//! escape the grid). Control characters and invalid UTF-8 from data lines
//! are replaced before they reach a cell, so file contents (history, memory
//! summaries, route reasons) can never inject terminal escape sequences.
const std = @import("std");
const Io = std.Io;
const Rgb = @import("theme.zig").Rgb;

pub const Style = struct {
    fg: ?Rgb = null,
    bg: ?Rgb = null,
    bold: bool = false,

    fn eql(a: Style, b: Style) bool {
        return optEql(a.fg, b.fg) and optEql(a.bg, b.bg) and a.bold == b.bold;
    }
    fn optEql(a: ?Rgb, b: ?Rgb) bool {
        if (a == null or b == null) return a == null and b == null;
        return a.?.eql(b.?);
    }
};

pub const Cell = struct { cp: u21 = ' ', style: Style = .{} };

pub const Rect = struct {
    x: u16,
    y: u16,
    w: u16,
    h: u16,

    /// Inside a one-cell border.
    pub fn inner(r: Rect) Rect {
        if (r.w < 2 or r.h < 2) return .{ .x = r.x, .y = r.y, .w = 0, .h = 0 };
        return .{ .x = r.x + 1, .y = r.y + 1, .w = r.w - 2, .h = r.h - 2 };
    }
};

/// Replace a codepoint that must not reach the terminal verbatim.
pub fn sanitize(cp: u21) u21 {
    if (cp < 0x20 or cp == 0x7f or (cp >= 0x80 and cp < 0xa0)) return 0xFFFD;
    return cp;
}

pub const Frame = struct {
    w: u16,
    h: u16,
    cells: []Cell,

    pub fn init(gpa: std.mem.Allocator, w: u16, h: u16) error{OutOfMemory}!Frame {
        const cells = try gpa.alloc(Cell, @as(usize, w) * h);
        @memset(cells, .{});
        return .{ .w = w, .h = h, .cells = cells };
    }

    pub fn deinit(self: *Frame, gpa: std.mem.Allocator) void {
        gpa.free(self.cells);
    }

    pub fn area(self: *const Frame) Rect {
        return .{ .x = 0, .y = 0, .w = self.w, .h = self.h };
    }

    pub fn at(self: *Frame, x: u16, y: u16) ?*Cell {
        if (x >= self.w or y >= self.h) return null;
        return &self.cells[@as(usize, y) * self.w + x];
    }

    pub fn put(self: *Frame, x: u16, y: u16, cp: u21, style: Style) void {
        const c = self.at(x, y) orelse return;
        c.* = .{ .cp = sanitize(cp), .style = style };
    }

    /// Fill `r` with blanks in `style`.
    pub fn fill(self: *Frame, r: Rect, style: Style) void {
        var y = r.y;
        while (y < r.y +| r.h and y < self.h) : (y += 1) {
            var x = r.x;
            while (x < r.x +| r.w and x < self.w) : (x += 1) self.put(x, y, ' ', style);
        }
    }

    /// Restyle `r` keeping its characters (list highlight).
    pub fn restyle(self: *Frame, r: Rect, style: Style) void {
        var y = r.y;
        while (y < r.y +| r.h and y < self.h) : (y += 1) {
            var x = r.x;
            while (x < r.x +| r.w and x < self.w) : (x += 1) self.at(x, y).?.style = style;
        }
    }

    /// Write `text` at (x, y), clipped to `max` cells; returns cells used.
    pub fn text(self: *Frame, x: u16, y: u16, max: u16, s: []const u8, style: Style) u16 {
        var used: u16 = 0;
        var i: usize = 0;
        while (i < s.len and used < max) {
            // std: lib/std/unicode.zig (a lone invalid lead byte is U+FFFD;
            // utf8Decode does not validate a 1-byte slice).
            const len: usize = std.unicode.utf8ByteSequenceLength(s[i]) catch 0;
            const ok = len != 0 and i + len <= s.len;
            const cp: u21 = if (ok) (std.unicode.utf8Decode(s[i .. i + len]) catch 0xFFFD) else 0xFFFD;
            i += if (ok) len else 1;
            self.put(x +| used, y, cp, style);
            used += 1;
        }
        return used;
    }

    /// Rounded border with a left-aligned title; returns the inner rect.
    pub fn block(self: *Frame, r: Rect, title: []const u8, border: Style, body: Style) Rect {
        if (r.w < 2 or r.h < 2) return r.inner();
        self.fill(r, body);
        const x1 = r.x + r.w - 1;
        const y1 = r.y + r.h - 1;
        var x = r.x + 1;
        while (x < x1) : (x += 1) {
            self.put(x, r.y, 0x2500, border);
            self.put(x, y1, 0x2500, border);
        }
        var y = r.y + 1;
        while (y < y1) : (y += 1) {
            self.put(r.x, y, 0x2502, border);
            self.put(x1, y, 0x2502, border);
        }
        self.put(r.x, r.y, 0x256D, border);
        self.put(x1, r.y, 0x256E, border);
        self.put(r.x, y1, 0x2570, border);
        self.put(x1, y1, 0x256F, border);
        if (title.len != 0) _ = self.text(r.x + 1, r.y, r.w - 2, title, body);
        return r.inner();
    }

    /// Vertical scrollbar on the right border of `r` when content overflows.
    pub fn scrollbar(self: *Frame, r: Rect, content: usize, visible: usize, offset: usize, track: Style, thumb: Style) void {
        if (content <= visible or r.h < 4 or r.w == 0) return;
        const x = r.x + r.w - 1;
        const span: usize = r.h - 2;
        self.put(x, r.y, 0x2191, track);
        self.put(x, r.y + r.h - 1, 0x2193, track);
        const tlen = @max(1, span * visible / content);
        const tpos = @min(span - tlen, (span * offset + content - 1) / content);
        var i: usize = 0;
        while (i < span) : (i += 1) {
            const on = i >= tpos and i < tpos + tlen;
            self.put(x, r.y + 1 + @as(u16, @intCast(i)), if (on) 0x2588 else 0x2551, if (on) thumb else track);
        }
    }

    /// Plain text: one line per row, trailing blanks trimmed.
    pub fn writePlain(self: *const Frame, w: *Io.Writer) Io.Writer.Error!void {
        var row: u16 = 0;
        while (row < self.h) : (row += 1) {
            const cells = self.cells[@as(usize, row) * self.w ..][0..self.w];
            var end: usize = cells.len;
            while (end > 0 and cells[end - 1].cp == ' ') end -= 1;
            for (cells[0..end]) |c| try writeCp(w, c.cp);
            try w.writeByte('\n');
        }
    }

    /// ANSI: home, then each row at an absolute position with SGR emitted
    /// only when the style changes; ends reset.
    pub fn writeAnsi(self: *const Frame, w: *Io.Writer) Io.Writer.Error!void {
        var cur: ?Style = null;
        var row: u16 = 0;
        while (row < self.h) : (row += 1) {
            try w.print("\x1b[{d};1H", .{row + 1});
            for (self.cells[@as(usize, row) * self.w ..][0..self.w]) |c| {
                if (cur == null or !cur.?.eql(c.style)) {
                    try writeSgr(w, c.style);
                    cur = c.style;
                }
                try writeCp(w, c.cp);
            }
        }
        try w.writeAll("\x1b[0m");
    }
};

fn writeCp(w: *Io.Writer, cp: u21) Io.Writer.Error!void {
    var buf: [4]u8 = undefined;
    const n = std.unicode.utf8Encode(cp, &buf) catch return w.writeAll("\u{FFFD}");
    try w.writeAll(buf[0..n]);
}

fn writeSgr(w: *Io.Writer, s: Style) Io.Writer.Error!void {
    try w.writeAll("\x1b[0");
    if (s.bold) try w.writeAll(";1");
    if (s.fg) |c| try w.print(";38;2;{d};{d};{d}", .{ c.r, c.g, c.b });
    if (s.bg) |c| try w.print(";48;2;{d};{d};{d}", .{ c.r, c.g, c.b });
    try w.writeByte('m');
}

test "frame clips text, sanitizes controls, and encodes truecolor SGR" {
    const gpa = std.testing.allocator;
    var f = try Frame.init(gpa, 6, 2);
    defer f.deinit(gpa);
    const red: Style = .{ .fg = .{ .r = 255, .g = 0, .b = 0 }, .bold = true };
    try std.testing.expectEqual(@as(u16, 6), f.text(0, 0, 6, "a\x1b[2Jbcdefgh", red));
    _ = f.text(0, 1, 6, "\xffx\u{e9}", .{});
    var plain: Io.Writer.Allocating = .init(gpa);
    defer plain.deinit();
    try f.writePlain(&plain.writer);
    try std.testing.expectEqualStrings("a\u{FFFD}[2Jb\n\u{FFFD}x\u{e9}\n", plain.written());
    var ansi: Io.Writer.Allocating = .init(gpa);
    defer ansi.deinit();
    try f.writeAnsi(&ansi.writer);
    const out = ansi.written();
    try std.testing.expect(std.mem.startsWith(u8, out, "\x1b[1;1H\x1b[0;1;38;2;255;0;0ma"));
    // The data's own ESC never reaches the terminal: it renders as U+FFFD.
    try std.testing.expect(std.mem.find(u8, out, "\x1b[2J") == null);
    try std.testing.expect(std.mem.find(u8, out, "\u{FFFD}[2J") != null);
    try std.testing.expect(std.mem.endsWith(u8, out, "\x1b[0m"));
}

test "block draws rounded borders and a clipped title" {
    const gpa = std.testing.allocator;
    var f = try Frame.init(gpa, 8, 3);
    defer f.deinit(gpa);
    const in = f.block(f.area(), " Title long ", .{}, .{});
    try std.testing.expectEqual(Rect{ .x = 1, .y = 1, .w = 6, .h = 1 }, in);
    var plain: Io.Writer.Allocating = .init(gpa);
    defer plain.deinit();
    try f.writePlain(&plain.writer);
    try std.testing.expectEqualStrings("\u{256D} Title\u{256E}\n\u{2502}      \u{2502}\n\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256F}\n", plain.written());
}
