const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // The importable library module (`@import("kerf")`). Style and font are embedded from src/data.
    const kerf = b.addModule("kerf", .{
        .root_source_file = b.path("src/kerf.zig"),
        .target = target,
        .optimize = optimize,
    });

    // CLI.
    const exe = b.addExecutable(.{
        .name = "kerf",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{.{ .name = "kerf", .module = kerf }},
        }),
    });
    b.installArtifact(exe);
    const run = b.addRunArtifact(exe);
    if (b.args) |args| run.addArgs(args);
    b.step("run", "Run the kerf CLI").dependOn(&run.step);

    // Tests. The reference documents are embedded for integration tests.
    const test_mod = b.createModule(.{
        .root_source_file = b.path("src/kerf.zig"),
        .target = target,
        .optimize = optimize,
    });
    test_mod.addAnonymousImport("doc_truss", .{ .root_source_file = b.path("../../spec/details/truss-bearing-cmu.kerf.json") });
    test_mod.addAnonymousImport("doc_slab", .{ .root_source_file = b.path("../../spec/details/monopour-slab-door-recess.kerf.json") });
    test_mod.addAnonymousImport("doc_beam", .{ .root_source_file = b.path("../../spec/details/flush-beam-strap.kerf.json") });
    const tests = b.addTest(.{ .root_module = test_mod });
    const run_tests = b.addRunArtifact(tests);
    run_tests.setCwd(b.path("."));
    b.step("test", "Run unit tests").dependOn(&run_tests.step);

    // wasm32-freestanding, raw ABI (SPEC 13.1). `-Dwasm-optimize=ReleaseFast` to compare speed.
    const wasm_opt = b.option(std.builtin.OptimizeMode, "wasm-optimize", "Optimize mode of the wasm build (default ReleaseSmall)") orelse .ReleaseSmall;
    const wasm_target = b.resolveTargetQuery(.{ .cpu_arch = .wasm32, .os_tag = .freestanding });
    const wasm_kerf = b.createModule(.{
        .root_source_file = b.path("src/kerf.zig"),
        .target = wasm_target,
        .optimize = wasm_opt,
    });
    const wasm = b.addExecutable(.{
        .name = "kerf",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/wasm.zig"),
            .target = wasm_target,
            .optimize = wasm_opt,
            .strip = true,
            .single_threaded = true,
            .imports = &.{.{ .name = "kerf", .module = wasm_kerf }},
        }),
    });
    wasm.entry = .disabled;
    wasm.rdynamic = true;
    wasm.stack_size = 256 * 1024;
    const install_wasm = b.addInstallFileWithDir(wasm.getEmittedBin(), .{ .custom = "../dist" }, "kerf.wasm");
    b.step("wasm", "Build dist/kerf.wasm").dependOn(&install_wasm.step);
}
