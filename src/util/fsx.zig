//! Absolute-path file helpers over `std.Io.Dir.cwd()`.
//!
//! std: lib/std/Io/Dir.zig (createDirPath, createFile, readFileAlloc,
//! statFile), lib/std/Io/File.zig (length, writePositionalAll, Lock).
const std = @import("std");
const Io = std.Io;
const Dir = Io.Dir;
const File = Io.File;

pub const ReadError = Dir.ReadFileAllocError;
pub const WriteError = Dir.CreateDirPathError || File.OpenError || File.WritePositionalError ||
    File.LengthError || File.SetLengthError;

/// Read a whole file, or null when it does not exist.
pub fn readOptional(io: Io, gpa: std.mem.Allocator, path: []const u8, limit: usize) ReadError!?[]u8 {
    return Dir.cwd().readFileAlloc(io, path, gpa, .limited(limit)) catch |e| switch (e) {
        error.FileNotFound => null,
        else => e,
    };
}

pub fn isFile(io: Io, path: []const u8) bool {
    const st = Dir.cwd().statFile(io, path, .{}) catch return false;
    return st.kind == .file;
}

pub fn exists(io: Io, path: []const u8) bool {
    _ = Dir.cwd().statFile(io, path, .{}) catch return false;
    return true;
}

pub fn makePath(io: Io, path: []const u8) Dir.CreateDirPathError!void {
    return Dir.cwd().createDirPath(io, path);
}

fn ensureParent(io: Io, path: []const u8) Dir.CreateDirPathError!void {
    if (std.fs.path.dirname(path)) |parent| try makePath(io, parent);
}

/// Append `bytes` under an exclusive advisory lock, creating the file (and
/// its parent directory) when absent. The lock serializes cooperating
/// writers; the write lands at the current end of file.
pub fn appendLocked(io: Io, path: []const u8, bytes: []const u8, mode: File.Permissions) WriteError!void {
    try ensureParent(io, path);
    const f = try Dir.cwd().createFile(io, path, .{ .truncate = false, .lock = .exclusive, .permissions = mode });
    defer f.close(io);
    const end = try f.length(io);
    try f.writePositionalAll(io, bytes, end);
}

/// Replace a file's contents (create/truncate), creating the parent directory.
pub fn writeAll(io: Io, path: []const u8, bytes: []const u8, mode: File.Permissions) WriteError!void {
    try ensureParent(io, path);
    const f = try Dir.cwd().createFile(io, path, .{ .truncate = true, .permissions = mode });
    defer f.close(io);
    try f.writePositionalAll(io, bytes, 0);
}

pub const owner_only: File.Permissions = if (@import("builtin").os.tag == .windows) .default_file else .fromMode(0o600);

test "append creates, then appends" {
    const gpa = std.testing.allocator;
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const root = try @import("../ctx.zig").tmpPath(gpa, io, tmp.dir);
    defer gpa.free(root);
    const p = try std.fs.path.join(gpa, &.{ root, "sub", "log.jsonl" });
    defer gpa.free(p);
    try std.testing.expect((try readOptional(io, gpa, p, 1024)) == null);
    try appendLocked(io, p, "a\n", .default_file);
    try appendLocked(io, p, "b\n", .default_file);
    const got = (try readOptional(io, gpa, p, 1024)).?;
    defer gpa.free(got);
    try std.testing.expectEqualStrings("a\nb\n", got);
    try std.testing.expect(isFile(io, p));
}
