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
pub const wdbx_bridge = @import("wdbx_bridge.zig");
pub const claims = @import("claims.zig");
pub const doctor = @import("doctor.zig");
pub const cli_args = @import("cli/args.zig");
pub const help = @import("cli/help.zig");
pub const dispatch = @import("cli/dispatch.zig");
pub const memory_cmd = @import("cli/memory_cmd.zig");
pub const daemon_text = @import("daemon/text.zig");
pub const route_audit = @import("daemon/route_audit.zig");
pub const daemon_protocol = @import("daemon/protocol.zig");
pub const daemon_sys = @import("daemon/sys.zig");
pub const daemon_config = @import("daemon/config.zig");
pub const daemon_server = @import("daemon/server.zig");
pub const daemon_client = @import("daemon/client.zig");

test {
    _ = @import("learn_test.zig");
    _ = @import("session_test.zig");
    _ = @import("cli/help_test.zig");
    _ = @import("daemon/protocol_test.zig");
    _ = @import("daemon/server_test.zig");
}

test {
    std.testing.refAllDecls(@This());
}
