const std = @import("std");
const teak_build = @import("teak");

/// Spec files shared by every Kerf stack are embedded through anonymous
/// imports (Zig forbids `@embedFile` outside a module's own directory).
/// In code: `@embedFile("spec_system_md")`.
fn addSpecImports(b: *std.Build, mod: *std.Build.Module) void {
    const files = [_]struct { name: []const u8, path: []const u8 }{
        .{ .name = "spec_system_md", .path = "../../spec/llm/system.md" },
        .{ .name = "spec_tools_json", .path = "../../spec/llm/tools.json" },
        .{ .name = "spec_font_json", .path = "../../spec/fonts/kerf-simplex.json" },
        .{ .name = "spec_style_json", .path = "../../spec/styles/kerf-standard.kerfstyle.json" },
        .{ .name = "sample_truss_json", .path = "../../spec/details/truss-bearing-cmu.kerf.json" },
        .{ .name = "sample_slab_json", .path = "../../spec/details/monopour-slab-door-recess.kerf.json" },
        .{ .name = "sample_strap_json", .path = "../../spec/details/flush-beam-strap.kerf.json" },
        .{ .name = "fixture_truss_drawing", .path = "fixtures/truss-bearing-cmu.drawing.json" },
        .{ .name = "fixture_slab_drawing", .path = "fixtures/monopour-slab-door-recess.drawing.json" },
        .{ .name = "fixture_strap_drawing", .path = "fixtures/flush-beam-strap.drawing.json" },
        .{ .name = "fixture_truss_mesh", .path = "fixtures/truss-bearing-cmu.mesh.json" },
    };
    for (files) |f| mod.addAnonymousImport(f.name, .{ .root_source_file = b.path(f.path) });
}

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // ── Pure-logic tests: no teak / zunk / engine dependency needed. ──
    const test_step = b.step("test", "Run pure-logic unit tests");
    const test_roots = [_][]const u8{
        "src/draw/mod.zig", // Drawing IR, stroke font, tessellator, rasterizer, PNG
        "src/llm/mod.zig", // Claude Messages harness + tool loop
        "src/app/logic.zig", // document/session logic (no UI framework)
    };
    for (test_roots) |root| {
        const mod = b.createModule(.{
            .root_source_file = b.path(root),
            .target = target,
            .optimize = optimize,
        });
        addSpecImports(b, mod);
        const t = b.addTest(.{ .root_module = mod });
        test_step.dependOn(&b.addRunArtifact(t).step);
    }

    // ── UI-layer tests (need teak; headless golden/snapshot tests). ──
    const teak_dep = b.dependency("teak", .{ .target = target, .optimize = optimize });
    const kerf_dep = b.dependency("kerf", .{ .target = target, .optimize = optimize });
    const ui_test_step = b.step("test-ui", "Run UI-layer tests (teak snapshot/golden)");
    {
        const mod = b.createModule(.{
            .root_source_file = b.path("src/ui_root.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{
                .{ .name = "teak", .module = teak_dep.module("teak") },
                .{ .name = "kerf", .module = kerf_dep.module("kerf") },
            },
        });
        addSpecImports(b, mod);
        const t = b.addTest(.{ .root_module = mod });
        ui_test_step.dependOn(&b.addRunArtifact(t).step);
    }

    // ── Web (wasm + WebGPU via zunk) -> dist/ ──
    const wasm_target = b.resolveTargetQuery(.{ .cpu_arch = .wasm32, .os_tag = .freestanding, .abi = .none });
    const web_optimize = b.option(std.builtin.OptimizeMode, "web-optimize", "Optimize mode of the wasm build (default ReleaseFast)") orelse .ReleaseFast;
    const web_teak = b.dependency("teak", .{ .target = wasm_target, .optimize = web_optimize });
    const web_kerf = b.dependency("kerf", .{ .target = wasm_target, .optimize = web_optimize });
    const web_exe = b.addExecutable(.{
        .name = "kerf-teak",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main_web.zig"),
            .target = wasm_target,
            .optimize = web_optimize,
            .imports = &.{.{ .name = "kerf", .module = web_kerf.module("kerf") }},
        }),
    });
    addSpecImports(b, web_exe.root_module);
    teak_build.linkWebWgpu(b, web_exe, .{});
    _ = web_teak;

    // ── Native desktop (X11/Win32 + wgpu-native) ──
    if (teak_build.hasNativeBackend(target.result.os.tag)) {
        const ui_exe = b.addExecutable(.{
            .name = "kerf-teak-ui",
            .root_module = b.createModule(.{
                .root_source_file = b.path("src/main_ui.zig"),
                .target = target,
                .optimize = optimize,
                .imports = &.{.{ .name = "kerf", .module = kerf_dep.module("kerf") }},
            }),
        });
        addSpecImports(b, ui_exe.root_module);
        teak_build.linkNativeWgpu(b, ui_exe, .{});
        const install_ui = b.addInstallArtifact(ui_exe, .{});
        const ui_step = b.step("ui", "Build (and with `run`, launch) the native desktop app");
        ui_step.dependOn(&install_ui.step);
        const run_ui = b.addRunArtifact(ui_exe);
        run_ui.step.dependOn(&install_ui.step);
        if (b.args) |args| run_ui.addArgs(args);
        b.step("run-ui", "Run the native desktop app").dependOn(&run_ui.step);
    }
}
