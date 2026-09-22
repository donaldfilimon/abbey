//! Drawing: App state -> Frame (port of Rust `tui/ui.rs`, `widgets.rs`,
//! and the overlay drawing in `overlay.rs`). Pure over the App; `tick`
//! drives the header pulse and caret blink, so tests pin it.
//!
//! The layout follows the Rust constraints (header 3, KPI strip 1 on Home,
//! body, prompt 3, status 1; Home split 55/45 then 60/40), rendered by this
//! module rather than ratatui, so frames match in structure, not byte for
//! byte.
const std = @import("std");
const app_mod = @import("app.zig");
const App = app_mod.App;
const Tab = app_mod.Tab;
const frame = @import("frame.zig");
const Frame = frame.Frame;
const Rect = frame.Rect;
const Style = frame.Style;
const Theme = @import("theme.zig").Theme;

pub const min_w: u16 = 40;
pub const min_h: u16 = 12;

/// Rows of list viewport the body gives the active tab (ensureVisible).
pub fn listViewport(app: *const App, h: u16) usize {
    const kpi: u16 = if (app.tab == .home) 1 else 0;
    const body = h -| (3 + kpi + 3 + 1);
    if (app.tab == .home) return (body * 60 / 100) -| 2;
    return body -| 2;
}

fn dim(t: Theme) Style {
    return .{ .fg = t.fg_dim, .bg = t.bg };
}

fn accent(t: Theme) Style {
    return .{ .fg = t.accent, .bg = t.bg, .bold = true };
}

fn plain(t: Theme) Style {
    return .{ .fg = t.fg, .bg = t.bg };
}

fn border(t: Theme, focused: bool) Style {
    return .{ .fg = if (focused) t.border_focus else t.border, .bg = t.bg };
}

pub fn draw(f: *Frame, app: *const App) void {
    const t = Theme.from(app.theme_id);
    f.fill(f.area(), plain(t));
    if (f.w < min_w or f.h < min_h) {
        var buf: [64]u8 = undefined;
        const msg = std.fmt.bufPrint(&buf, "terminal too small ({d}x{d})", .{ f.w, f.h }) catch "terminal too small";
        _ = f.text(0, 0, f.w, msg, .{ .fg = t.warn, .bg = t.bg });
        return;
    }
    const kpi: u16 = if (app.tab == .home) 1 else 0;
    const body_h = f.h - (3 + kpi + 3 + 1);
    const header: Rect = .{ .x = 0, .y = 0, .w = f.w, .h = 3 };
    const body: Rect = .{ .x = 0, .y = 3 + kpi, .w = f.w, .h = body_h };
    const input: Rect = .{ .x = 0, .y = 3 + kpi + body_h, .w = f.w, .h = 3 };
    drawHeader(f, header, app, t);
    if (app.tab == .home) drawKpi(f, .{ .x = 0, .y = 3, .w = f.w, .h = 1 }, app, t);
    switch (app.tab) {
        .home => drawHome(f, body, app, t),
        .chats => drawList(f, body, app, t, " Chats . Enter activate . / filter "),
        .personas => drawList(f, body, app, t, " Personas . Max/Gemma roles "),
        .memory => drawList(f, body, app, t, " Memory . self-learn "),
        .skills => drawList(f, body, app, t, " Skills "),
        .models => drawList(f, body, app, t, " Models . Enter select "),
        .doctor => drawList(f, body, app, t, " Doctor "),
    }
    drawInput(f, input, app, t);
    drawStatus(f, f.h - 1, app, t);
    switch (app.overlay) {
        .none => {},
        .help => drawHelp(f, t),
        .palette => drawPalette(f, app, t),
    }
}

fn drawHeader(f: *Frame, r: Rect, app: *const App, t: Theme) void {
    const pulse = if (app.tick % 20 < 10) t.header_pulse else t.accent;
    const in = f.block(r, "", border(t, false), plain(t));
    var x = r.x + 1;
    x += f.text(x, r.y, r.w -| 2, " \u{2726} Abbey ", .{ .fg = pulse, .bg = t.bg, .bold = true });
    var tb: [16]u8 = undefined;
    const theme_title = std.fmt.bufPrint(&tb, " {s} ", .{app.theme_id.asStr()}) catch "";
    _ = f.text(x, r.y, (r.x + r.w -| 1) -| x, theme_title, dim(t));
    x = in.x;
    const end = in.x + in.w;
    for (Tab.all, 0..) |tab, i| {
        if (i > 0) x += f.text(x, in.y, end -| x, "\u{2502}", .{ .fg = t.border, .bg = t.bg });
        var nb: [16]u8 = undefined;
        const name = std.fmt.bufPrint(&nb, " {s} ", .{tab.title()}) catch tab.title();
        const style: Style = if (tab == app.tab) .{ .fg = t.bg, .bg = pulse, .bold = true } else dim(t);
        x += f.text(x, in.y, end -| x, name, style);
    }
}

fn drawKpi(f: *Frame, r: Rect, app: *const App, t: Theme) void {
    var buf: [7][2][]const u8 = undefined;
    var last: [4]u8 = undefined;
    var x = r.x;
    const end = r.x + r.w;
    for (app.kpiChips(&buf, &last), 0..) |chip, i| {
        if (i > 0) x += f.text(x, r.y, end -| x, " ", plain(t));
        x += f.text(x, r.y, end -| x, " ", .{ .bg = t.chip_bg });
        x += f.text(x, r.y, end -| x, chip[0], .{ .fg = t.fg_dim, .bg = t.chip_bg });
        x += f.text(x, r.y, end -| x, " ", .{ .bg = t.chip_bg });
        x += f.text(x, r.y, end -| x, chip[1], .{ .fg = t.chip_fg, .bg = t.chip_bg, .bold = true });
        x += f.text(x, r.y, end -| x, " ", .{ .bg = t.chip_bg });
    }
}

/// `~` for HOME, else the last 33 bytes behind `...` when longer than 36.
pub fn shortPath(buf: []u8, home: ?[]const u8, p: []const u8) []const u8 {
    if (home) |h| if (h.len > 1 and std.mem.startsWith(u8, p, h) and (p.len == h.len or p[h.len] == '/')) {
        return std.fmt.bufPrint(buf, "~{s}", .{p[h.len..]}) catch p;
    };
    if (p.len > 36) {
        var start = p.len - 33;
        while (start < p.len and (p[start] & 0xC0) == 0x80) start += 1;
        return std.fmt.bufPrint(buf, "...{s}", .{p[start..]}) catch p;
    }
    return p;
}

fn drawHome(f: *Frame, r: Rect, app: *const App, t: Theme) void {
    const left_w = r.w * 55 / 100;
    const left: Rect = .{ .x = r.x, .y = r.y, .w = left_w, .h = r.h };
    const in = f.block(left, " Session ", border(t, app.focus == .prompt), plain(t));
    var pb: [256]u8 = undefined;
    const rows = [_]struct { []const u8, []const u8, Style }{
        .{ "model  ", app.agent.model, .{ .fg = t.ok, .bg = t.bg, .bold = true } },
        .{ "chat   ", app.chat orelse "-", .{ .fg = t.accent, .bg = t.bg } },
        .{ "cwd    ", shortPath(&pb, app.home, app.cwd), plain(t) },
    };
    var y = in.y;
    for (rows) |row| {
        if (y >= in.y + in.h) return;
        const n = f.text(in.x, y, in.w, row[0], dim(t));
        _ = f.text(in.x + n, y, in.w -| n, row[1], row[2]);
        y += 1;
    }
    const keys = [_][]const u8{
        "",
        "Keys",
        "  Enter            run the prompt (resume chat)",
        "  `  Ctrl-L        toggle prompt <-> panel",
        "  Ctrl-K / Ctrl-T  palette / theme",
        "  Ctrl-B           cycle executor backend",
        "  Tab 1-7 . ?      tabs . help",
        "",
        "Composer-first . dual focus . dashboard chips.",
    };
    for (keys) |k| {
        if (y >= in.y + in.h) break;
        _ = f.text(in.x, y, in.w, k, if (std.mem.eql(u8, k, "Keys")) accent(t) else dim(t));
        y += 1;
    }
    const right_w = r.w - left_w;
    const recent_h = r.h * 60 / 100;
    drawRecent(f, .{ .x = r.x + left_w, .y = r.y, .w = right_w, .h = recent_h }, app, t);
    drawRoutes(f, .{ .x = r.x + left_w, .y = r.y + recent_h, .w = right_w, .h = r.h - recent_h }, app, t);
}

fn drawRoutes(f: *Frame, r: Rect, app: *const App, t: Theme) void {
    const in = f.block(r, " Routes . audit ", border(t, false), plain(t));
    if (app.routes.items.len == 0) {
        _ = f.text(in.x, in.y, in.w, "(no routes yet: run a prompt; audit only)", dim(t));
        return;
    }
    for (app.routes.items, 0..) |l, i| {
        if (i >= in.h) break;
        _ = f.text(in.x, in.y + @as(u16, @intCast(i)), in.w, l, dim(t));
    }
}

fn highlight(t: Theme) Style {
    return .{ .fg = t.selection_fg, .bg = t.selection_bg, .bold = true };
}

fn drawRecent(f: *Frame, r: Rect, app: *const App, t: Theme) void {
    const focused = app.focus == .panel;
    var tb: [96]u8 = undefined;
    const title = if (app.filter.items.len == 0) " Recent chats " else std.fmt.bufPrint(&tb, " Recent . /{s} ", .{app.filter.items}) catch " Recent ";
    const in = f.block(r, title, border(t, focused), plain(t));
    const len = app.listLen();
    var row: u16 = 0;
    var i = app.scroll;
    while (row < in.h and i < len) : ({
        row += 1;
        i += 1;
    }) {
        const line = app.filteredAt(i).?;
        const y = in.y + row;
        var it = std.mem.tokenizeAny(u8, line, " \t");
        const ts = it.next() orelse "";
        const chat = it.next();
        if (chat) |c| {
            var x = in.x;
            const clock = if (ts.len >= 19) ts[11..19] else ts;
            x += f.text(x, y, in.w, clock, dim(t));
            x += f.text(x, y, (in.x + in.w) -| x, " ", dim(t));
            x += f.text(x, y, (in.x + in.w) -| x, app_mod.utf8Prefix(c, 8), .{ .fg = t.accent, .bg = t.bg });
            x += f.text(x, y, (in.x + in.w) -| x, " ", dim(t));
            var pb: [256]u8 = undefined;
            _ = f.text(x, y, (in.x + in.w) -| x, shortPath(&pb, app.home, std.mem.trim(u8, it.rest(), " \t")), dim(t));
        } else _ = f.text(in.x, y, in.w, line, plain(t));
        if (focused and i == app.list_idx) f.restyle(.{ .x = in.x, .y = y, .w = in.w, .h = 1 }, highlight(t));
    }
    f.scrollbar(r, len, in.h, app.scroll, .{ .fg = t.border, .bg = t.bg }, .{ .fg = t.accent, .bg = t.bg });
}

fn drawList(f: *Frame, r: Rect, app: *const App, t: Theme, base: []const u8) void {
    const focused = app.focus == .panel;
    var tb: [160]u8 = undefined;
    const title = if (app.filtering or app.filter.items.len != 0) std.fmt.bufPrint(&tb, "{s} filter:{s}/ ", .{ base, app.filter.items }) catch base else base;
    const in = f.block(r, title, border(t, focused), plain(t));
    const len = app.listLen();
    var row: u16 = 0;
    var i = app.scroll;
    while (row < in.h and i < len) : ({
        row += 1;
        i += 1;
    }) {
        const y = in.y + row;
        _ = f.text(in.x, y, in.w, app.filteredAt(i).?, plain(t));
        if (focused and i == app.list_idx) f.restyle(.{ .x = in.x, .y = y, .w = in.w, .h = 1 }, highlight(t));
    }
    f.scrollbar(r, len, in.h, app.scroll, .{ .fg = t.border, .bg = t.bg }, .{ .fg = t.accent, .bg = t.bg });
}

fn drawInput(f: *Frame, r: Rect, app: *const App, t: Theme) void {
    const focused = app.focus == .prompt;
    const title = if (focused) " Prompt . Enter run . Up/Down history " else " Prompt (Ctrl-L / ` to focus) ";
    const in = f.block(r, title, .{ .fg = if (focused) t.prompt_border else t.border, .bg = t.bg }, plain(t));
    if (in.w == 0) return;
    const text = app.input.items;
    // Codepoint column of the cursor; scroll so the caret stays visible.
    const col = std.unicode.utf8CountCodepoints(text[0..app.cursor]) catch app.cursor;
    const skip = if (col >= in.w) col - in.w + 1 else 0;
    var i: usize = 0;
    var cp_idx: usize = 0;
    while (i < text.len and cp_idx < skip) : (cp_idx += 1) i += std.unicode.utf8ByteSequenceLength(text[i]) catch 1;
    _ = f.text(in.x, in.y, in.w, text[@min(i, text.len)..], plain(t));
    if (focused and app.tick % 2 == 0) {
        const cx = in.x + @as(u16, @intCast(col - skip));
        if (f.at(cx, in.y)) |cell| cell.style = .{ .fg = t.bg, .bg = t.accent };
    }
}

fn drawStatus(f: *Frame, y: u16, app: *const App, t: Theme) void {
    var x: u16 = 0;
    const w = f.w;
    x += f.text(x, y, w -| x, " \u{25B8} ", accent(t));
    x += f.text(x, y, w -| x, app.focus.label(), .{ .fg = t.accent_dim, .bg = t.bg });
    x += f.text(x, y, w -| x, " ", plain(t));
    // Off-default executors get the warn colour: which binary runs the next
    // prompt is the one thing a TUI user must never be surprised by.
    x += f.text(x, y, w -| x, app.agent.backend.label(), if (app.agent.backend == .ollama) dim(t) else .{ .fg = t.warn, .bg = t.bg });
    x += f.text(x, y, w -| x, " ", plain(t));
    x += f.text(x, y, w -| x, app.theme_id.asStr(), dim(t));
    x += f.text(x, y, w -| x, " ", plain(t));
    if (app.filter.items.len != 0) {
        var fb: [96]u8 = undefined;
        x += f.text(x, y, w -| x, std.fmt.bufPrint(&fb, "filter={s} ", .{app.filter.items}) catch "filter ", .{ .fg = t.warn, .bg = t.bg });
    }
    x += f.text(x, y, w -| x, app.status(), plain(t));
    if (app.last_code) |c| {
        var lb: [16]u8 = undefined;
        _ = f.text(x, y, w -| x, std.fmt.bufPrint(&lb, " last={d}", .{c}) catch "", .{ .fg = if (c == 0) t.ok else t.err, .bg = t.bg });
    }
}

fn centered(area: Rect, w: u16, h: u16) Rect {
    const ww = @min(w, area.w -| 2);
    const hh = @min(h, area.h -| 2);
    return .{ .x = area.x + (area.w - ww) / 2, .y = area.y + (area.h - hh) / 2, .w = ww, .h = hh };
}

fn drawHelp(f: *Frame, t: Theme) void {
    const lines = app_mod.help_lines;
    const r = centered(f.area(), 82, @min(lines.len + 2, 24));
    const in = f.block(r, " Help ", border(t, true), plain(t));
    for (lines, 0..) |l, i| {
        if (i >= in.h) break;
        _ = f.text(in.x, in.y + @as(u16, @intCast(i)), in.w, l, if (i == 0) accent(t) else plain(t));
    }
}

fn drawPalette(f: *Frame, app: *const App, t: Theme) void {
    var idx: [app_mod.palette.len]usize = undefined;
    const matches = app.paletteMatches(&idx);
    const r = centered(f.area(), 82, @intCast(@max(matches.len, 1) + 2));
    var tb: [96]u8 = undefined;
    const title = std.fmt.bufPrint(&tb, " Palette . {s}_ ", .{app.overlay_query.items}) catch " Palette ";
    const in = f.block(r, title, border(t, true), plain(t));
    if (matches.len == 0) {
        _ = f.text(in.x, in.y, in.w, "(no match)", dim(t));
        return;
    }
    for (matches, 0..) |m, i| {
        if (i >= in.h) break;
        const it = app_mod.palette[m];
        const y = in.y + @as(u16, @intCast(i));
        var x = in.x;
        x += f.text(x, y, in.w, it.label, accent(t));
        x += f.text(x, y, (in.x + in.w) -| x, "  ", plain(t));
        _ = f.text(x, y, (in.x + in.w) -| x, it.detail, dim(t));
        if (i == app.overlay_idx) f.restyle(.{ .x = in.x, .y = y, .w = in.w, .h = 1 }, highlight(t));
    }
}
