//! Lexical similarity over memory: deterministic signed feature hashing of
//! byte 1/2/3-grams into 32 dims, then cosine. Port of abi-ai
//! `embedding.rs` (itself a port of the original Zig helper), using
//! `std.hash.Wyhash` directly, which the Rust `abi_foundation::wyhash` was
//! written to reproduce; the golden vectors below pin that equivalence.
//! Not a learned embedding: learned semantic search is Proposed.
const std = @import("std");
const Record = @import("record.zig").Record;
const Wyhash = std.hash.Wyhash; // std: lib/std/hash/wyhash.zig (hash(seed, input))

pub const dim = 32;
pub const Vec = [dim]f32;

pub fn embedBytes(input: []const u8) Vec {
    var out: Vec = @splat(0);
    if (input.len == 0) {
        out[0] = 1;
        return out;
    }
    const grams = [_]struct { usize, f32 }{ .{ 1, 0.5 }, .{ 2, 1.0 }, .{ 3, 1.5 } };
    var window: [3]u8 = undefined;
    for (grams) |g| {
        const n = g[0];
        var i: usize = 0;
        while (i + n <= input.len) : (i += 1) {
            for (0..n) |k| window[k] = std.ascii.toLower(input[i + k]);
            const h = Wyhash.hash(n, window[0..n]);
            const bucket: usize = @intCast(h % dim);
            out[bucket] += if ((h >> 63) & 1 == 0) g[1] else -g[1];
        }
    }
    var norm: f32 = 0;
    for (out) |v| norm += v * v;
    if (norm == 0) {
        out = @splat(0);
        out[0] = 1;
        return out;
    }
    const scale = @sqrt(norm);
    for (&out) |*v| v.* /= scale;
    return out;
}

/// Summary plus tags, like the Rust `embed_record`.
pub fn embedRecord(arena: std.mem.Allocator, r: *const Record) error{OutOfMemory}!Vec {
    if (r.tags.len == 0) return embedBytes(r.summary);
    const tags = try std.mem.join(arena, " ", r.tags);
    return embedBytes(try std.mem.concat(arena, u8, &.{ r.summary, " ", tags }));
}

pub fn cosine(a: *const Vec, b: *const Vec) f32 {
    var s: f32 = 0;
    for (a, b) |x, y| s += x * y;
    return s;
}

pub const Hit = struct { score: f32, record: Record };

/// Rank `records` against `query`, most similar first (stable on ties).
pub fn rank(arena: std.mem.Allocator, records: []const Record, query: *const Vec, limit: usize) error{OutOfMemory}![]Hit {
    const hits = try arena.alloc(Hit, records.len);
    for (records, hits) |r, *h| {
        const v = try embedRecord(arena, &r);
        h.* = .{ .score = cosine(&v, query), .record = r };
    }
    std.mem.sort(Hit, hits, {}, struct {
        fn gt(_: void, x: Hit, y: Hit) bool {
            return x.score > y.score;
        }
    }.gt);
    return hits[0..@min(limit, hits.len)];
}

test "wyhash matches the abi-foundation golden vectors" {
    // wdbx/crates/abi-foundation/src/wyhash.rs tests.
    try std.testing.expectEqual(@as(u64, 290_873_116_282_709_081), Wyhash.hash(0, ""));
    try std.testing.expectEqual(@as(u64, 8_151_176_929_567_807_605), Wyhash.hash(1, "h"));
    try std.testing.expectEqual(@as(u64, 5_976_454_099_606_738_610), Wyhash.hash(2, "he"));
    try std.testing.expectEqual(@as(u64, 10_846_395_113_768_030_678), Wyhash.hash(3, "hel"));
    try std.testing.expectEqual(@as(u64, 15_812_251_943_945_396_959), Wyhash.hash(0xA11CE, "hello world"));
}

test "embeddings are unit, deterministic, case-insensitive, empty is e0" {
    for ([_][]const u8{ "a", "hello world", "\u{65e5}\u{672c}\u{8a9e}" }) |s| {
        const v = embedBytes(s);
        try std.testing.expect(@abs(@sqrt(cosine(&v, &v)) - 1.0) < 0.001);
    }
    const e = embedBytes("");
    try std.testing.expectEqual(@as(f32, 1), e[0]);
    const a = embedBytes("Hello World");
    const b = embedBytes("hello world");
    try std.testing.expectEqualSlices(f32, &a, &b);
    const base = embedBytes("the quick brown fox jumps");
    const near = embedBytes("the quick brown fox leaps");
    const far = embedBytes("zzzz qqqq vvvv");
    try std.testing.expect(cosine(&base, &near) > cosine(&base, &far));
}

test "shared n-grams outrank unrelated text and a typo still matches" {
    var arena: std.heap.ArenaAllocator = .init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const recs = [_]Record{
        .{ .id = "1", .timestamp = "t", .payload = "b", .summary = "wdbx checkpoint epoch", .tags = &.{"wdbx"} },
        .{ .id = "2", .timestamp = "t", .payload = "b", .summary = "install premium voices", .tags = &.{"voice"} },
    };
    const q = embedBytes("chekpoint");
    const ranked = try rank(a, &recs, &q, 2);
    try std.testing.expectEqualStrings("1", ranked[0].record.id);
    try std.testing.expect(ranked[0].score >= ranked[1].score);
}
