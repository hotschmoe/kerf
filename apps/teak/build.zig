const std = @import("std");

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
}
