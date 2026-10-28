//! Zig twin of bench/bench.rs: the same case set through `kerf.call` of engines/zig, in-process.
//!   zig build -Doptimize=fast --prefix <dir>   (or small)   -> <dir>/bin/kerf-bench <cases dir> [--min-ms N] [--min-reps N] [--filter S]
const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const kerf = b.createModule(.{
        .root_source_file = b.path("../../../zig/src/kerf.zig"),
        .target = target,
        .optimize = optimize,
    });
    kerf.addAnonymousImport("kerf_style_json", .{ .root_source_file = b.path("../../../../spec/styles/kerf-standard.kerfstyle.json") });
    kerf.addAnonymousImport("kerf_font_json", .{ .root_source_file = b.path("../../../../spec/fonts/kerf-simplex.json") });
    const opts = b.addOptions();
    opts.addOption([]const u8, "version", "0.1.0-alpha.11");
    kerf.addOptions("build_options", opts);
    const exe = b.addExecutable(.{
        .name = "kerf-bench",
        .root_module = b.createModule(.{
            .root_source_file = b.path("bench.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{.{ .name = "kerf", .module = kerf }},
        }),
    });
    b.installArtifact(exe);
}
