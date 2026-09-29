//! Canonical run specs: one path for every generation surface (port of Rust
//! `actions.rs`). Everything but the three headless bypasses in
//! `capture.zig` goes through `runAgent` -> `session.hybridRun`.
const std = @import("std");
const session = @import("session.zig");
const roles = @import("roles.zig");
const AgentConfig = @import("agent/argv.zig").AgentConfig;

pub const RunSpec = struct {
    fresh: bool = false,
    mode: ?[]const u8 = null,
    role: ?roles.Role = null,
    print: bool = false,

    pub fn @"resume"() RunSpec {
        return .{};
    }
    pub fn fresh_() RunSpec {
        return .{ .fresh = true };
    }
    pub fn max() RunSpec {
        return .{ .role = .max };
    }
    pub fn gemma() RunSpec {
        return .{ .role = .gemma };
    }
    pub fn ask() RunSpec {
        return .{ .mode = "ask", .role = .gemma };
    }
    pub fn plan() RunSpec {
        return .{ .mode = "plan", .role = .max };
    }
};

pub fn runAgent(s: session.Session, agent: *AgentConfig, prompt: []const []const u8, spec: RunSpec) session.Error!u8 {
    if (spec.mode) |m| agent.mode = m;
    if (spec.print) agent.print = true;
    return session.hybridRun(s, agent, spec.fresh, prompt, spec.role);
}

test "run specs encode the surface contracts" {
    try std.testing.expect(!RunSpec.@"resume"().fresh);
    try std.testing.expect(RunSpec.fresh_().fresh);
    try std.testing.expectEqual(roles.Role.max, RunSpec.max().role.?);
    try std.testing.expectEqual(roles.Role.gemma, RunSpec.gemma().role.?);
    try std.testing.expectEqualStrings("ask", RunSpec.ask().mode.?);
    try std.testing.expectEqual(roles.Role.gemma, RunSpec.ask().role.?);
    try std.testing.expectEqualStrings("plan", RunSpec.plan().mode.?);
    try std.testing.expectEqual(roles.Role.max, RunSpec.plan().role.?);
}
