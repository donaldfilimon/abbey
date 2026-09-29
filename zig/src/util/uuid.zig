//! RFC 4122 version-4 UUIDs, lowercase 8-4-4-4-12, from `Io.random`.
const std = @import("std");
const Io = std.Io; // std: lib/std/Io.zig (random)

pub const len = 36;

pub fn v4(io: Io, out: *[len]u8) []const u8 {
    var b: [16]u8 = undefined;
    io.random(&b);
    return format(b, out);
}

pub fn format(bytes: [16]u8, out: *[len]u8) []const u8 {
    var b = bytes;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    const hex = "0123456789abcdef";
    var o: usize = 0;
    for (b, 0..) |byte, i| {
        if (i == 4 or i == 6 or i == 8 or i == 10) {
            out[o] = '-';
            o += 1;
        }
        out[o] = hex[byte >> 4];
        out[o + 1] = hex[byte & 0xf];
        o += 2;
    }
    return out[0..len];
}

test "uuid v4 sets version and variant" {
    var out: [len]u8 = undefined;
    const s = format(@splat(0xff), &out);
    try std.testing.expectEqualStrings("ffffffff-ffff-4fff-bfff-ffffffffffff", s);
    const r = v4(std.testing.io, &out);
    try std.testing.expectEqual(@as(u8, '4'), r[14]);
    try std.testing.expect(std.mem.indexOfScalar(u8, "89ab", r[19]) != null);
}
