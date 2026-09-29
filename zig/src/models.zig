//! Friendly model aliases to executor model ids (port of Rust `models.rs`).
//! Cursor-shaped aliases stay cursor-agent ids; the ollama and abi backends
//! normalize through `agent/argv.zig` instead.
const std = @import("std");

/// Default local Ollama tag. `gemma:27b-mlx` is accepted as an alias.
pub const ollama_default_model = "gemma4:26b-mlx";

const Alias = struct { []const []const u8, []const u8 };

const table = [_]Alias{
    .{ &.{ "auto", "smart", "default" }, "auto" },
    .{ &.{ "fable", "fable5", "fable-5", "claude-fable", "claude-fable-5", "fable-thinking", "fable-thinking-high", "fable5-thinking", "fable5-thinking-high" }, "claude-fable-5-thinking-high" },
    .{ &.{ "fable-thinking-xhigh", "fable-xhigh-thinking" }, "claude-fable-5-thinking-xhigh" },
    .{ &.{"fable-thinking-low"}, "claude-fable-5-thinking-low" },
    .{ &.{"fable-thinking-medium"}, "claude-fable-5-thinking-medium" },
    .{ &.{"fable-thinking-max"}, "claude-fable-5-thinking-max" },
    .{ &.{"fable-low"}, "claude-fable-5-low" },
    .{ &.{"fable-medium"}, "claude-fable-5-medium" },
    .{ &.{"fable-high"}, "claude-fable-5-high" },
    .{ &.{"fable-xhigh"}, "claude-fable-5-xhigh" },
    .{ &.{"fable-max"}, "claude-fable-5-max" },
    .{ &.{ "opus", "opus5", "opus-5", "claude-opus", "claude-opus-5", "opus-thinking", "opus-thinking-high" }, "claude-opus-5-thinking-high" },
    .{ &.{ "opus-thinking-fast", "opus-fast" }, "claude-opus-5-thinking-high-fast" },
    .{ &.{ "opus-thinking-xhigh", "opus-xhigh-thinking", "opus-xhigh" }, "claude-opus-5-thinking-xhigh" },
    .{ &.{ "opus-thinking-max", "opus-max-thinking", "opus-max" }, "claude-opus-5-thinking-max" },
    .{ &.{"opus-thinking-low"}, "claude-opus-5-thinking-low" },
    .{ &.{"opus-thinking-medium"}, "claude-opus-5-thinking-medium" },
    .{ &.{"opus-low"}, "claude-opus-5-low" },
    .{ &.{"opus-medium"}, "claude-opus-5-medium" },
    .{ &.{"opus-high"}, "claude-opus-5-high" },
    .{ &.{ "opus48", "opus-4.8", "opus-4-8", "claude-opus-4-8" }, "claude-opus-4-8-thinking-high" },
    .{ &.{ "opus48-fast", "opus-4.8-fast" }, "claude-opus-4-8-thinking-high-fast" },
    .{ &.{ "gpt", "gpt5", "gpt-5.2", "gpt5.2" }, "gpt-5.2" },
    .{ &.{ "gpt55", "gpt-5.5", "gpt5.5" }, "gpt-5.5-high" },
    .{ &.{"gpt55-fast"}, "gpt-5.5-high-fast" },
    .{ &.{ "sol", "gpt56", "gpt-5.6" }, "gpt-5.6-sol-high" },
    .{ &.{ "sol-xhigh", "gpt56-xhigh" }, "gpt-5.6-sol-xhigh" },
    .{ &.{"sol-fast"}, "gpt-5.6-sol-high-fast" },
    .{ &.{ "terra", "gpt-5.6-terra" }, "gpt-5.6-terra-medium" },
    .{ &.{"terra-high"}, "gpt-5.6-terra-high" },
    .{ &.{ "codex", "codex53", "gpt-5.3-codex" }, "gpt-5.3-codex" },
    .{ &.{"codex-high"}, "gpt-5.3-codex-high" },
    .{ &.{"codex-xhigh"}, "gpt-5.3-codex-xhigh" },
    .{ &.{"codex-fast"}, "gpt-5.3-codex-fast" },
    .{ &.{"codex-low"}, "gpt-5.3-codex-low" },
    .{ &.{ "grok", "grok45", "grok-4.5", "cursor-grok" }, "cursor-grok-4.5-high" },
    .{ &.{"grok-fast"}, "cursor-grok-4.5-high-fast" },
    .{ &.{"grok-low"}, "cursor-grok-4.5-low" },
    .{ &.{"grok-medium"}, "cursor-grok-4.5-medium" },
    .{ &.{ "composer", "composer2", "composer-2.5" }, "composer-2.5" },
    .{ &.{"composer-fast"}, "composer-2.5-fast" },
    .{ &.{ "kimi", "kimi-k3", "k3" }, "kimi-k3-high" },
    .{ &.{ "max", "qwen", "qwen3", "qwen-3.5" }, "claude-opus-5-thinking-high" },
    .{ &.{ "gemma", "gemma4", "gemma-4" }, "composer-2.5" },
};

/// Resolve an alias or pass a full model id through. The result is either a
/// static string or a slice of `raw`, except the `fable-5-*`/`opus-5-*`
/// prefix forms, which are allocated in `arena`.
pub fn resolveModel(arena: std.mem.Allocator, raw: []const u8) error{OutOfMemory}![]const u8 {
    const trimmed = std.mem.trim(u8, raw, &std.ascii.whitespace);
    var lower_buf: [128]u8 = undefined;
    if (trimmed.len > lower_buf.len) return trimmed;
    const lower = std.ascii.lowerString(&lower_buf, trimmed);
    for (table) |entry| {
        for (entry[0]) |alias| if (std.mem.eql(u8, alias, lower)) return entry[1];
    }
    const prefixes = [_]struct { []const u8, []const u8 }{
        .{ "fable-5-", "claude-fable-5-" }, .{ "fable5-", "claude-fable-5-" },
        .{ "opus-5-", "claude-opus-5-" },   .{ "opus5-", "claude-opus-5-" },
    };
    for (prefixes) |p| {
        if (std.mem.startsWith(u8, lower, p[0])) return std.mem.concat(arena, u8, &.{ p[1], lower[p[0].len..] });
    }
    return trimmed;
}

test "aliases resolve" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expectEqualStrings("claude-fable-5-thinking-high", try resolveModel(a, "Fable"));
    try std.testing.expectEqualStrings("claude-opus-5-thinking-high", try resolveModel(a, "opus"));
    try std.testing.expectEqualStrings("auto", try resolveModel(a, "auto"));
    try std.testing.expectEqualStrings("claude-opus-5-thinking-high", try resolveModel(a, "max"));
    try std.testing.expectEqualStrings("gpt-5.6-sol-high", try resolveModel(a, "sol"));
    try std.testing.expectEqualStrings("claude-opus-5-thinking-low", try resolveModel(a, "opus-thinking-low"));
    try std.testing.expectEqualStrings("composer-2.5", try resolveModel(a, "gemma"));
    try std.testing.expectEqualStrings("claude-opus-5-high", try resolveModel(a, "claude-opus-5-high"));
    try std.testing.expectEqualStrings("claude-opus-5-foo", try resolveModel(a, "opus5-foo"));
}
