//! Persona routing and frozen response contracts, ported from `../abi`
//! `crates/abi-ai/src/{router,keywords,identity}.rs` (themselves ports of the
//! original Zig `router_*.zig`). Routing compares f32 sums accumulated in
//! declaration order as `score * 0.1` in f32, so near-ties match bit for bit.
const std = @import("std");

pub const Profile = enum {
    abbey,
    aviva,
    abi,

    pub fn label(self: Profile) []const u8 {
        return @tagName(self);
    }

    pub fn parse(s: []const u8) ?Profile {
        const t = std.mem.trim(u8, s, &std.ascii.whitespace);
        inline for (.{ Profile.abbey, Profile.aviva, Profile.abi }) |p| {
            if (std.ascii.eqlIgnoreCase(t, @tagName(p))) return p;
        }
        return null;
    }
};

pub const Contract = struct {
    response_prefix: []const u8,
    response_suffix: []const u8,
};

/// Frozen prefix/suffix pairs. Abbey's suffix opens with U+2019 in "I’ll".
pub fn contract(p: Profile) Contract {
    return switch (p) {
        .abbey => .{
            .response_prefix = "Abbey: ",
            .response_suffix = "\n\nI\u{2019}ll approach this with warmth, creativity, and technical care while keeping uncertainty explicit.",
        },
        .aviva => .{
            .response_prefix = "Aviva direct expert: ",
            .response_suffix = "\n\nLeading with the concrete answer, assumptions, and next action.",
        },
        .abi => .{
            .response_prefix = "ABI orchestration review: ",
            .response_suffix = "\n\nEvaluating intent, risk, context, and the appropriate response mode.",
        },
    };
}

pub const Weights = struct {
    abbey: f32,
    aviva: f32,
    abi: f32,

    pub const prior: Weights = .{ .abbey = 0.40, .aviva = 0.30, .abi = 0.30 };

    pub fn only(p: Profile) Weights {
        return switch (p) {
            .abbey => .{ .abbey = 1, .aviva = 0, .abi = 0 },
            .aviva => .{ .abbey = 0, .aviva = 1, .abi = 0 },
            .abi => .{ .abbey = 0, .aviva = 0, .abi = 1 },
        };
    }

    fn normalize(self: *Weights) void {
        const total = self.abbey + self.aviva + self.abi;
        if (total > 0) {
            self.abbey /= total;
            self.aviva /= total;
            self.abi /= total;
        }
    }
};

const Keyword = struct { word: []const u8, abbey: f32, aviva: f32, abi: f32 };

/// The 29-entry sentiment table, verbatim and in declaration order.
pub const keywords = [_]Keyword{
    .{ .word = "analyze", .abbey = 0.9, .aviva = 0.5, .abi = 0.2 },
    .{ .word = "structure", .abbey = 0.9, .aviva = 0.6, .abi = 0.2 },
    .{ .word = "logical", .abbey = 0.85, .aviva = 0.7, .abi = 0.2 },
    .{ .word = "compare", .abbey = 0.8, .aviva = 0.7, .abi = 0.2 },
    .{ .word = "explain", .abbey = 0.95, .aviva = 0.4, .abi = 0.2 },
    .{ .word = "creative", .abbey = 0.95, .aviva = 0.3, .abi = 0.1 },
    .{ .word = "imagine", .abbey = 0.95, .aviva = 0.3, .abi = 0.1 },
    .{ .word = "explore", .abbey = 0.9, .aviva = 0.4, .abi = 0.2 },
    .{ .word = "brainstorm", .abbey = 0.95, .aviva = 0.3, .abi = 0.1 },
    .{ .word = "help", .abbey = 0.95, .aviva = 0.4, .abi = 0.2 },
    .{ .word = "learn", .abbey = 0.95, .aviva = 0.3, .abi = 0.2 },
    .{ .word = "frustrated", .abbey = 0.95, .aviva = 0.2, .abi = 0.1 },
    .{ .word = "run", .abbey = 0.3, .aviva = 0.95, .abi = 0.2 },
    .{ .word = "execute", .abbey = 0.3, .aviva = 0.95, .abi = 0.2 },
    .{ .word = "deploy", .abbey = 0.3, .aviva = 0.95, .abi = 0.2 },
    .{ .word = "build", .abbey = 0.5, .aviva = 0.9, .abi = 0.2 },
    .{ .word = "fix", .abbey = 0.5, .aviva = 0.95, .abi = 0.2 },
    .{ .word = "quick", .abbey = 0.3, .aviva = 0.95, .abi = 0.1 },
    .{ .word = "direct", .abbey = 0.3, .aviva = 0.95, .abi = 0.1 },
    .{ .word = "concise", .abbey = 0.3, .aviva = 0.95, .abi = 0.1 },
    .{ .word = "orchestrate", .abbey = 0.2, .aviva = 0.2, .abi = 0.95 },
    .{ .word = "routing", .abbey = 0.2, .aviva = 0.2, .abi = 0.95 },
    .{ .word = "governance", .abbey = 0.3, .aviva = 0.3, .abi = 0.95 },
    .{ .word = "policy", .abbey = 0.3, .aviva = 0.4, .abi = 0.9 },
    .{ .word = "profile", .abbey = 0.3, .aviva = 0.3, .abi = 0.9 },
    .{ .word = "safe", .abbey = 0.8, .aviva = 0.5, .abi = 0.5 },
    .{ .word = "risk", .abbey = 0.8, .aviva = 0.6, .abi = 0.6 },
    .{ .word = "design", .abbey = 0.9, .aviva = 0.5, .abi = 0.3 },
    .{ .word = "pattern", .abbey = 0.85, .aviva = 0.5, .abi = 0.3 },
};

/// Zig's std.ascii whitespace set, which includes vertical tab (0x0B).
fn isZigWhitespace(b: u8) bool {
    return switch (b) {
        ' ', '\t', '\n', '\r', 0x0B, 0x0C => true,
        else => false,
    };
}

/// An exact ASCII persona name at the very start, followed by end, Zig
/// whitespace, `,` or `:`. `@` may precede it.
pub fn explicitSelector(input: []const u8) ?Profile {
    var rest = std.mem.trim(u8, input, " \t\r\n");
    if (rest.len > 0 and rest[0] == '@') rest = rest[1..];
    var end: usize = 0;
    while (end < rest.len and std.ascii.isAlphabetic(rest[end])) end += 1;
    if (end == 0) return null;
    if (end < rest.len) {
        const sep = rest[end];
        if (!isZigWhitespace(sep) and sep != ',' and sep != ':') return null;
    }
    return Profile.parse(rest[0..end]);
}

pub fn analyze(input: []const u8) Weights {
    if (explicitSelector(input)) |p| return Weights.only(p);
    var w = Weights.prior;
    // Rust `split_ascii_whitespace` (space, \t, \n, \x0C, \r; not 0x0B).
    var it = std.mem.tokenizeAny(u8, input, " \t\n\x0C\r");
    while (it.next()) |word| {
        const trimmed = std.mem.trimEnd(u8, word, ".,!?:;\"'");
        for (keywords) |k| {
            if (std.ascii.startsWithIgnoreCase(trimmed, k.word)) {
                w.abbey += k.abbey * 0.1;
                w.aviva += k.aviva * 0.1;
                w.abi += k.abi * 0.1;
            }
        }
    }
    w.normalize();
    return w;
}

/// Ties break Abbey, then Aviva, then ABI.
pub fn selectBest(w: Weights) Profile {
    if (w.abbey >= w.aviva and w.abbey >= w.abi) return .abbey;
    if (w.aviva >= w.abi) return .aviva;
    return .abi;
}

pub fn route(input: []const u8) Profile {
    return selectBest(analyze(input));
}

/// ABBEY_PERSONA (known name) > explicit leading address > keyword router.
pub fn select(env_persona: ?[]const u8, input: []const u8) Profile {
    if (env_persona) |v| if (Profile.parse(v)) |p| return p;
    if (explicitSelector(input)) |p| return p;
    return route(input);
}

/// Wrap a prompt with the frozen contract prefix/suffix.
pub fn wrap(arena: std.mem.Allocator, p: Profile, user: []const u8) error{OutOfMemory}![]const u8 {
    const c = contract(p);
    return std.mem.concat(arena, u8, &.{ c.response_prefix, std.mem.trimEnd(u8, user, &std.ascii.whitespace), c.response_suffix });
}

test "neutral input defaults to abbey; weights normalize" {
    try std.testing.expectEqual(Profile.abbey, route("hello there"));
    try std.testing.expectEqual(Profile.abbey, route("hello world"));
    const w = analyze("analyze the logical structure of this system");
    try std.testing.expect(w.abbey > w.aviva and w.abbey > w.abi);
    try std.testing.expect(@abs(w.abbey + w.aviva + w.abi - 1.0) < 0.01);
}

test "action favors aviva, orchestration favors abi" {
    const a = analyze("execute deploy run the build quickly");
    try std.testing.expect(a.aviva > a.abbey and a.aviva > a.abi);
    const b = analyze("orchestrate routing governance policy profile");
    try std.testing.expect(b.abi > b.abbey and b.abi > b.aviva);
}

test "suffix false positives do not shift routing; prefix stems match" {
    const neutral = analyze("zzzqqq");
    for ([_][]const u8{ "overrun", "unsafe", "prefix", "redesign" }) |word| {
        const w = analyze(word);
        try std.testing.expect(@abs(neutral.abbey - w.abbey) < 0.0001);
        try std.testing.expect(@abs(neutral.aviva - w.aviva) < 0.0001);
    }
    try std.testing.expect(analyze("quickly").aviva > neutral.aviva);
    try std.testing.expect(analyze("running").aviva > neutral.aviva);
}

test "explicit selector rules" {
    try std.testing.expectEqual(Profile.aviva, explicitSelector("Aviva, summarize this file").?);
    try std.testing.expectEqual(Profile.abi, explicitSelector("ABI: orchestrate").?);
    try std.testing.expectEqual(Profile.abbey, explicitSelector("@abbey hi").?);
    try std.testing.expectEqual(Profile.abbey, explicitSelector("Abbey\x0bhello").?);
    try std.testing.expect(explicitSelector("avivacious") == null);
    try std.testing.expect(explicitSelector("Please ask Aviva") == null);
    try std.testing.expect(explicitSelector("Aviva-x") == null);
    try std.testing.expectEqual(Profile.aviva, select("aviva", "hello"));
    try std.testing.expectEqual(Profile.abbey, select("nope", "hello"));
}

test "wrap uses the frozen contract bytes" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const out = try wrap(arena.allocator(), .abbey, "hello  \n");
    try std.testing.expectEqualStrings("Abbey: hello\n\nI\xe2\x80\x99ll approach this with warmth, creativity, and technical care while keeping uncertainty explicit.", out);
    try std.testing.expectEqual(@as(usize, 29), keywords.len);
}
