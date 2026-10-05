const std = @import("std");

/// The default style and stroke font are embedded straight from spec/ (single source of truth).
fn addSpecImports(b: *std.Build, m: *std.Build.Module) void {
    m.addAnonymousImport("kerf_style_json", .{ .root_source_file = b.path("../../spec/styles/kerf-standard.kerfstyle.json") });
    m.addAnonymousImport("kerf_font_json", .{ .root_source_file = b.path("../../spec/fonts/kerf-simplex.json") });
}

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // The importable library module (`@import("kerf")`). Style and font are embedded from src/data.
    const kerf = b.addModule("kerf", .{
        .root_source_file = b.path("src/kerf.zig"),
        .target = target,
        .optimize = optimize,
    });
    addSpecImports(b, kerf);

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
    exe.root_module.addAnonymousImport("kerf_cli_guide", .{ .root_source_file = b.path("../../spec/llm/cli-guide.md") });
    exe.root_module.addAnonymousImport("kerf_system_md", .{ .root_source_file = b.path("../../spec/llm/system.md") });
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
    addSpecImports(b, test_mod);
    test_mod.addAnonymousImport("doc_truss", .{ .root_source_file = b.path("../../spec/details/truss-bearing-cmu.kerf.json") });
    test_mod.addAnonymousImport("doc_slab", .{ .root_source_file = b.path("../../spec/details/monopour-slab-door-recess.kerf.json") });
    test_mod.addAnonymousImport("doc_beam", .{ .root_source_file = b.path("../../spec/details/flush-beam-strap.kerf.json") });
    const tests = b.addTest(.{ .root_module = test_mod });
    const run_tests = b.addRunArtifact(tests);
    run_tests.setCwd(b.path("."));
    b.step("test", "Run unit tests").dependOn(&run_tests.step);

    // wasm32-freestanding, raw ABI (SPEC 13.1). `-Dwasm-optimize=ReleaseFast` to compare speed.
    const wasm_opt = b.option(std.builtin.OptimizeMode, "wasm-optimize", "Optimize mode of the wasm build (default ReleaseSmall)") orelse .ReleaseSmall;
    const wasm_strip = b.option(bool, "wasm-strip", "Strip the wasm build (default true; false keeps the name section for twiggy)") orelse true;
    const wasm_target = b.resolveTargetQuery(.{ .cpu_arch = .wasm32, .os_tag = .freestanding });
    const wasm_kerf = b.createModule(.{
        .root_source_file = b.path("src/kerf.zig"),
        .target = wasm_target,
        .optimize = wasm_opt,
    });
    addSpecImports(b, wasm_kerf);
    const wasm = b.addExecutable(.{
        .name = "kerf",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/wasm.zig"),
            .target = wasm_target,
            .optimize = wasm_opt,
            .strip = wasm_strip,
            .single_threaded = true,
            .error_tracing = false,
            .unwind_tables = .none,
            .imports = &.{.{ .name = "kerf", .module = wasm_kerf }},
        }),
    });
    wasm.entry = .disabled;
    wasm.rdynamic = true;
    wasm.stack_size = 256 * 1024;
    const install_wasm = b.addInstallFileWithDir(wasm.getEmittedBin(), .{ .custom = "../dist" }, "kerf.wasm");
    b.step("wasm", "Build dist/kerf.wasm").dependOn(&install_wasm.step);
}
