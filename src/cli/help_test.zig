//! Help goldens: each command's help must equal tests/golden/help/<cmd>.txt
//! (safe-edition spelling; the personal edition substitutes its binary name).
//! Runs with the repository root as cwd (build.zig `setCwd`).
const std = @import("std");
const Command = @import("args.zig").Command;
const help = @import("help.zig");
const bin = @import("../edition.zig").id.binary_name;

test "help text matches every golden" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    inline for (@typeInfo(Command).@"enum".field_names) |name| {
        const path = "tests/golden/help/" ++ name ++ ".txt";
        const golden = std.Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(64 * 1024)) catch |e| {
            std.debug.print("missing golden {s}: {t}\n", .{ path, e });
            return e;
        };
        defer gpa.free(golden);
        const expected = try std.mem.replaceOwned(u8, gpa, golden, "abbey-zig", bin);
        defer gpa.free(expected);
        try std.testing.expectEqualStrings(expected, help.forCommand(@field(Command, name)));
    }
}
