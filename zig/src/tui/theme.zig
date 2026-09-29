//! TUI palettes and theme persistence (port of Rust `tui/theme.rs`).
//!
//! Resolve order: ABBEY_TUI_THEME > `<state>/tui-theme` > ink. The caller
//! reads the environment and the file (library code never touches either
//! directly); `resolve` is the pure precedence.
const std = @import("std");

pub const env_var = "ABBEY_TUI_THEME";
pub const file_name = "tui-theme";

pub const Rgb = struct {
    r: u8,
    g: u8,
    b: u8,

    pub fn eql(a: Rgb, b: Rgb) bool {
        return a.r == b.r and a.g == b.g and a.b == b.b;
    }
};

fn rgb(r: u8, g: u8, b: u8) Rgb {
    return .{ .r = r, .g = g, .b = b };
}

pub const Id = enum {
    ink,
    violet,
    mono,

    /// `ink`, `violet`, or `mono`, case-insensitive, whitespace trimmed.
    pub fn parse(s: []const u8) ?Id {
        const t = std.mem.trim(u8, s, &std.ascii.whitespace);
        inline for (@typeInfo(Id).@"enum".field_names) |name| {
            if (std.ascii.eqlIgnoreCase(t, name)) return @field(Id, name);
        }
        return null;
    }

    pub fn asStr(self: Id) []const u8 {
        return @tagName(self);
    }

    /// ink -> violet -> mono -> ink.
    pub fn cycle(self: Id) Id {
        return switch (self) {
            .ink => .violet,
            .violet => .mono,
            .mono => .ink,
        };
    }

    /// Precedence over already-read sources: env value, then file text.
    pub fn resolve(env_value: ?[]const u8, file_text: ?[]const u8) Id {
        if (env_value) |v| if (parse(v)) |id| return id;
        if (file_text) |v| if (parse(v)) |id| return id;
        return .ink;
    }
};

pub const Theme = struct {
    bg: Rgb,
    fg: Rgb,
    fg_dim: Rgb,
    accent: Rgb,
    accent_dim: Rgb,
    ok: Rgb,
    warn: Rgb,
    err: Rgb,
    border: Rgb,
    border_focus: Rgb,
    chip_bg: Rgb,
    chip_fg: Rgb,
    selection_bg: Rgb,
    selection_fg: Rgb,
    prompt_border: Rgb,
    header_pulse: Rgb,

    pub fn from(id: Id) Theme {
        return switch (id) {
            .ink => .{
                .bg = rgb(14, 18, 26),
                .fg = rgb(235, 225, 200),
                .fg_dim = rgb(150, 140, 118),
                .accent = rgb(72, 188, 176),
                .accent_dim = rgb(48, 128, 120),
                .ok = rgb(96, 196, 140),
                .warn = rgb(224, 176, 88),
                .err = rgb(224, 96, 96),
                .border = rgb(56, 72, 84),
                .border_focus = rgb(72, 188, 176),
                .chip_bg = rgb(24, 34, 44),
                .chip_fg = rgb(235, 225, 200),
                .selection_bg = rgb(28, 52, 58),
                .selection_fg = rgb(245, 240, 228),
                .prompt_border = rgb(72, 188, 176),
                .header_pulse = rgb(96, 210, 196),
            },
            .violet => .{
                .bg = rgb(18, 12, 28),
                .fg = rgb(230, 228, 240),
                .fg_dim = rgb(120, 120, 140),
                .accent = rgb(180, 120, 255),
                .accent_dim = rgb(120, 80, 180),
                .ok = rgb(120, 220, 160),
                .warn = rgb(240, 180, 80),
                .err = rgb(255, 100, 120),
                .border = rgb(80, 60, 100),
                .border_focus = rgb(180, 120, 255),
                .chip_bg = rgb(40, 30, 60),
                .chip_fg = rgb(240, 236, 248),
                .selection_bg = rgb(40, 30, 60),
                .selection_fg = rgb(255, 255, 255),
                .prompt_border = rgb(180, 120, 255),
                .header_pulse = rgb(200, 150, 255),
            },
            .mono => .{
                .bg = rgb(20, 20, 22),
                .fg = rgb(200, 200, 200),
                .fg_dim = rgb(120, 120, 120),
                .accent = rgb(180, 180, 180),
                .accent_dim = rgb(100, 100, 100),
                .ok = rgb(120, 220, 160),
                .warn = rgb(240, 180, 80),
                .err = rgb(255, 90, 90),
                .border = rgb(60, 60, 60),
                .border_focus = rgb(140, 140, 140),
                .chip_bg = rgb(35, 35, 38),
                .chip_fg = rgb(200, 200, 200),
                .selection_bg = rgb(50, 50, 55),
                .selection_fg = rgb(255, 255, 255),
                .prompt_border = rgb(100, 100, 100),
                .header_pulse = rgb(160, 160, 160),
            },
        };
    }
};

test "theme ids parse, cycle, and resolve env over file over ink" {
    try std.testing.expectEqual(Id.ink, Id.parse("INK").?);
    try std.testing.expectEqual(Id.violet, Id.parse("  Violet ").?);
    try std.testing.expect(Id.parse("sepia") == null);
    try std.testing.expect(Id.parse("") == null);
    try std.testing.expectEqual(Id.violet, Id.ink.cycle());
    try std.testing.expectEqual(Id.mono, Id.violet.cycle());
    try std.testing.expectEqual(Id.ink, Id.mono.cycle());
    try std.testing.expectEqual(Id.ink, Id.resolve(null, null));
    try std.testing.expectEqual(Id.violet, Id.resolve(null, "violet\n"));
    try std.testing.expectEqual(Id.mono, Id.resolve("mono", "violet\n"));
    try std.testing.expectEqual(Id.violet, Id.resolve("sepia", "violet"));
    // Palettes match the Rust values.
    try std.testing.expect(Theme.from(.violet).accent.eql(rgb(180, 120, 255)));
    try std.testing.expect(Theme.from(.mono).ok.eql(rgb(120, 220, 160)));
    try std.testing.expect(!Theme.from(.ink).bg.eql(Theme.from(.mono).bg));
}
