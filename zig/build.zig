const std = @import("std");

/// abbey-zig: stdlib-only Zig rewrite of Abbey (P1 CLI core, P2 daemon v1).
///
/// `-Dpersonal=true` compiles the personal edition. Editions differ only in
/// identity and state/config namespaces; neither edition adds runtime
/// authority. std source: lib/std/Build.zig (addOptions, addTest).
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const personal = b.option(bool, "personal", "Build the personal edition (identity/state separation only)") orelse false;

    const options = b.addOptions();
    options.addOption(bool, "personal", personal);
    // Build identity reported by `daemon status`, like the Rust build.rs
    // ABBEY_BUILD_GIT / ABBEY_BUILD_TARGET. `-Dbuild-git=` overrides; a
    // non-repository build reports "unknown" rather than guessing.
    var git_code: u8 = 0;
    const git_out = b.runAllowFail(&.{ "git", "rev-parse", "--short=12", "HEAD" }, &git_code, .ignore) catch "unknown";
    const build_git = b.option([]const u8, "build-git", "Build identity reported by `daemon status`") orelse std.mem.trim(u8, git_out, " \t\r\n");
    options.addOption([]const u8, "build_git", if (build_git.len == 0) "unknown" else build_git);
    options.addOption([]const u8, "build_target", b.fmt("{t}-{t}", .{ target.result.cpu.arch, target.result.os.tag }));

    const lib_mod = b.addModule("abbey", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
    });
    lib_mod.addOptions("build_options", options);

    const exe_mod = b.createModule(.{
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{.{ .name = "abbey", .module = lib_mod }},
    });

    const exe = b.addExecutable(.{
        .name = if (personal) "abbey-zig-personal" else "abbey-zig",
        .root_module = exe_mod,
    });
    b.installArtifact(exe);

    // `abbeyd-zig` is the daemon entry point; it serves the same protocol v1
    // as `abbey-zig daemon serve` and shares every module with the CLI.
    const daemon_mod = b.createModule(.{
        .root_source_file = b.path("src/abbeyd.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{.{ .name = "abbey", .module = lib_mod }},
    });
    b.installArtifact(b.addExecutable(.{
        .name = if (personal) "abbeyd-zig-personal" else "abbeyd-zig",
        .root_module = daemon_mod,
    }));

    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    const run_step = b.step("run", "Run abbey-zig");
    run_step.dependOn(&run_cmd.step);

    const lib_tests = b.addTest(.{ .root_module = lib_mod });
    const run_lib_tests = b.addRunArtifact(lib_tests);
    // Goldens and fixtures resolve against the repository root.
    run_lib_tests.setCwd(b.path("."));
    const exe_tests = b.addTest(.{ .root_module = exe_mod });
    const run_exe_tests = b.addRunArtifact(exe_tests);
    run_exe_tests.setCwd(b.path("."));

    // `zig build test-bin` installs the test executables so the gate can run
    // them directly and quote the runner's own "All N tests passed" line
    // (a cached `zig build test` prints no count).
    const test_bin_step = b.step("test-bin", "Install test executables to zig-out/bin");
    test_bin_step.dependOn(&b.addInstallArtifact(lib_tests, .{ .dest_sub_path = if (personal) "abbey-zig-personal-lib-tests" else "abbey-zig-lib-tests" }).step);
    test_bin_step.dependOn(&b.addInstallArtifact(exe_tests, .{ .dest_sub_path = if (personal) "abbey-zig-personal-main-tests" else "abbey-zig-main-tests" }).step);

    const test_step = b.step("test", "Run library and entry tests (leak-checked testing allocator)");
    test_step.dependOn(&run_lib_tests.step);
    test_step.dependOn(&run_exe_tests.step);
}
