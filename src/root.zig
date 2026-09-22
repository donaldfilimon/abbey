//! abbey-zig library root. Every subsystem is reachable from here so
//! `zig build test` compiles and runs all of their tests.
const std = @import("std");

pub const build_options = @import("build_options");
pub const json = @import("util/json.zig");
pub const time = @import("util/time.zig");
pub const uuid = @import("util/uuid.zig");
pub const fsx = @import("util/fsx.zig");
pub const ctx = @import("ctx.zig");
pub const edition = @import("edition.zig");
pub const config = @import("config/config.zig");
pub const proc = @import("proc.zig");
pub const models = @import("models.zig");
pub const backend = @import("agent/backend.zig");
pub const argv = @import("agent/argv.zig");
pub const run = @import("agent/run.zig");
pub const persona = @import("persona/router.zig");
pub const roles = @import("roles.zig");
pub const route_log = @import("route_log.zig");
pub const state = @import("state/state.zig");
pub const memory_record = @import("memory/record.zig");
pub const memory = @import("memory/store.zig");
pub const similarity = @import("memory/similarity.zig");
pub const memory_new = @import("memory/new.zig");
pub const learn = @import("learn.zig");
pub const learn_improve = @import("learn_improve.zig");
pub const session = @import("session.zig");
pub const actions = @import("actions.zig");
pub const capture = @import("capture.zig");

test {
    _ = @import("learn_test.zig");
    _ = @import("session_test.zig");
}

test {
    std.testing.refAllDecls(@This());
}
