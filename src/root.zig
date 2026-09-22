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

test {
    std.testing.refAllDecls(@This());
}
