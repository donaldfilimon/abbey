//! Max / Gemma worker roles and the task-class heuristic (port of Rust
//! `roles.rs`). Roles are executor model bindings, not bundled weights.
const std = @import("std");

pub const Role = enum {
    max,
    gemma,
    auto,

    pub fn label(self: Role) []const u8 {
        return @tagName(self);
    }

    pub fn parse(s: []const u8) ?Role {
        const t = std.mem.trim(u8, s, &std.ascii.whitespace);
        const names = [_]struct { []const u8, Role }{
            .{ "max", .max },     .{ "qwen", .max },     .{ "technical", .max },
            .{ "gemma", .gemma }, .{ "visual", .gemma }, .{ "chat", .gemma },
            .{ "auto", .auto },   .{ "default", .auto },
        };
        for (names) |n| if (std.ascii.eqlIgnoreCase(t, n[0])) return n[1];
        return null;
    }
};

pub const TaskClass = enum {
    code,
    tool,
    math,
    visual,
    conversational,
    hybrid,
    other,

    /// Rust `{:?}` spelling, used verbatim in route reasons.
    pub fn debugName(self: TaskClass) []const u8 {
        return switch (self) {
            .code => "Code",
            .tool => "Tool",
            .math => "Math",
            .visual => "Visual",
            .conversational => "Conversational",
            .hybrid => "Hybrid",
            .other => "Other",
        };
    }
};

fn anyIn(lower: []const u8, keys: []const []const u8) bool {
    for (keys) |k| if (std.mem.find(u8, lower, k) != null) return true;
    return false;
}

pub fn classify(arena: std.mem.Allocator, input: []const u8) error{OutOfMemory}!TaskClass {
    const lower = try std.ascii.allocLowerString(arena, input);
    const code = anyIn(lower, &.{ "code", "refactor", "compile", "rust", "patch", "test", "bug", "fix", "implement", "function", "cargo", "clippy", "api", "debug" });
    const tool = anyIn(lower, &.{ "tool", "shell", "command", "mcp", "deploy", "git ", "curl" });
    const math = anyIn(lower, &.{ "math", "prove", "equation", "integral", "derive", "calculate" });
    const visual = anyIn(lower, &.{ "image", "screenshot", "photo", "visual", "ui mock", "diagram", "picture", "ocr", "video", "clip", "frame", "recording", ".png", ".jpg", ".jpeg", ".webp", ".gif", ".mp4", ".mov", ".mkv", ".webm" });
    const conv = anyIn(lower, &.{ "feel", "tone", "explain gently", "conversation", "empath", "how are you", "story" });
    const tech = code or tool or math;
    const soft = visual or conv;
    if (tech and soft) return .hybrid;
    if (code) return .code;
    if (tool) return .tool;
    if (math) return .math;
    if (visual) return .visual;
    if (conv) return .conversational;
    return .other;
}

pub const Decision = struct {
    primary: Role,
    confidence: f32,
    alternate: ?Role = null,
    fallback: ?[]const u8 = null,
    class: TaskClass,
};

/// Classify and score; never re-invokes on low confidence (audit only).
pub fn decide(arena: std.mem.Allocator, input: []const u8, override: ?Role, env_role: ?[]const u8) error{OutOfMemory}!Decision {
    const class = try classify(arena, input);
    if (override) |r| if (r != .auto) return .{ .primary = r, .confidence = 0.95, .class = class };
    if (env_role) |v| if (Role.parse(v)) |r| if (r != .auto) return .{ .primary = r, .confidence = 0.9, .class = class };
    return switch (class) {
        .code, .tool, .math => .{ .primary = .max, .confidence = 0.85, .class = class },
        .visual, .conversational => .{ .primary = .gemma, .confidence = 0.85, .class = class },
        .hybrid => .{ .primary = .max, .confidence = 0.7, .alternate = .gemma, .fallback = "hybrid: prefer hybrid-loop or /gemma for visual half", .class = class },
        .other => .{ .primary = .max, .confidence = 0.55, .alternate = .gemma, .fallback = "low-confidence Other class; Max default, Gemma alternate", .class = class },
    };
}

pub fn defaultModelForRole(r: Role) []const u8 {
    return switch (r) {
        .max, .auto => "opus",
        .gemma => "composer",
    };
}

pub fn systemNote(r: Role) []const u8 {
    return switch (r) {
        .max, .auto => "Worker role: Max (technical). Prefer complete, testable changes; inspect before editing; report what was and was not verified.",
        .gemma => "Worker role: Gemma (visual/conversational). Prioritize clear human-facing interpretation, tone, and multimodal description when relevant.",
    };
}

test "classify and decide match the Rust heuristic" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expectEqual(TaskClass.code, try classify(a, "refactor this rust function"));
    try std.testing.expectEqual(Role.max, (try decide(a, "fix the compile error", .auto, null)).primary);
    try std.testing.expectEqual(TaskClass.visual, try classify(a, "describe this screenshot"));
    try std.testing.expectEqual(Role.gemma, (try decide(a, "look at ./demo.mp4", null, null)).primary);
    try std.testing.expectEqual(Role.gemma, (try decide(a, "refactor code", .gemma, null)).primary);
    const h = try decide(a, "refactor this UI screenshot layout in rust", null, null);
    try std.testing.expectEqual(Role.max, h.primary);
    try std.testing.expectEqual(Role.gemma, h.alternate.?);
    try std.testing.expect(h.fallback != null and h.confidence < 0.85);
    const o = try decide(a, "hmm what about that", null, null);
    try std.testing.expectEqual(TaskClass.other, o.class);
    try std.testing.expect(o.confidence < 0.7);
    const e = try decide(a, "hmm", null, "gemma");
    try std.testing.expectEqual(Role.gemma, e.primary);
    try std.testing.expectEqual(@as(f32, 0.9), e.confidence);
}
