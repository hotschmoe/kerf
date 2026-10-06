const std = @import("std");

/// The default style and stroke font are embedded straight from spec/ (single source of truth).
fn addSpecImports(b: *std.Build, m: *std.Build.Module) void {
    m.addAnonymousImport("kerf_style_json", .{ .root_source_file = b.path("../../spec/styles/kerf-standard.kerfstyle.json") });
    m.addAnonymousImport("kerf_font_json", .{ .root_source_file = b.path("../../spec/fonts/kerf-simplex.json") });
}

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    // Debug info makes a ReleaseSafe Linux binary ~4x larger; release builds drop it unless `-Dstrip=false` (Debug keeps it).
    const strip = b.option(bool, "strip", "Strip debug info from the CLI (default: true unless -Doptimize=Debug)") orelse (optimize != .Debug);

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
            .strip = strip,
            .imports = &.{.{ .name = "kerf", .module = kerf }},
        }),
    });
    const ui_mod = uiAssetsModule(b, b.option([]const u8, "ui", "Directory of the built web UI to embed in `kerf serve` (for example ../../apps/web/dist-serve)"));
    exe.root_module.addImport("ui_assets", ui_mod);
    exe.root_module.addAnonymousImport("kerf_cli_guide", .{ .root_source_file = b.path("../../spec/llm/cli-guide.md") });
    exe.root_module.addAnonymousImport("kerf_system_md", .{ .root_source_file = b.path("../../spec/llm/system.md") });
    b.installArtifact(exe);
    const run = b.addRunArtifact(exe);
    if (b.args) |args| run.addArgs(args);
    b.step("run", "Run the kerf CLI").dependOn(&run.step);

    // Tests of the serve code (HTTP plumbing, access rules, proxy URL rules, agent templates, op log).
    const serve_test_mod = b.createModule(.{
        .root_source_file = b.path("src/serve.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{ .{ .name = "kerf", .module = kerf }, .{ .name = "ui_assets", .module = uiAssetsModule(b, null) } },
    });
    const serve_tests = b.addTest(.{ .root_module = serve_test_mod });
    const run_serve_tests = b.addRunArtifact(serve_tests);

    // Tests. The reference documents are embedded for integration tests.
    const test_mod = b.createModule(.{
        .root_source_file = b.path("src/kerf.zig"),
        .target = target,
        .optimize = optimize,
    });
    addSpecImports(b, test_mod);
    test_mod.addAnonymousImport("doc_truss", .{ .root_source_file = b.path("../../spec/details/truss-bearing-cmu.kerf.json") });
    test_mod.addAnonymousImport("doc_slab", .{ .root_source_file = b.path("../../spec/details/monopour-slab-door-recess.kerf.json") });
    test_mod.addAnonymousImport("doc_palmer", .{ .root_source_file = b.path("tests/docs/palmer-sd1-like.kerf.json") });
    test_mod.addAnonymousImport("doc_psl", .{ .root_source_file = b.path("tests/docs/flush-psl-2x6.kerf.json") });
    test_mod.addAnonymousImport("doc_beam", .{ .root_source_file = b.path("../../spec/details/flush-beam-strap.kerf.json") });
    const tests = b.addTest(.{ .root_module = test_mod });
    const run_tests = b.addRunArtifact(tests);
    run_tests.setCwd(b.path("."));
    const test_step = b.step("test", "Run unit tests");
    test_step.dependOn(&run_tests.step);
    test_step.dependOn(&run_serve_tests.step);

    // CLI tests (ASCII fold, guide text).
    const cli_test_mod = b.createModule(.{
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{ .{ .name = "kerf", .module = kerf }, .{ .name = "ui_assets", .module = uiAssetsModule(b, null) } },
    });
    cli_test_mod.addAnonymousImport("kerf_cli_guide", .{ .root_source_file = b.path("../../spec/llm/cli-guide.md") });
    cli_test_mod.addAnonymousImport("kerf_system_md", .{ .root_source_file = b.path("../../spec/llm/system.md") });
    test_step.dependOn(&b.addRunArtifact(b.addTest(.{ .root_module = cli_test_mod })).step);

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

/// The `ui_assets` module: `pub const files = &.{ .{ .path = "index.html", .data = @embedFile(...) }, ... }`.
/// With a directory, every regular file under it is copied next to a generated source file and embedded;
/// without one the module is the empty stub (the server then shows a "UI not embedded" page).
fn uiAssetsModule(b: *std.Build, ui_dir: ?[]const u8) *std.Build.Module {
    const dir = ui_dir orelse return b.createModule(.{ .root_source_file = b.path("src/ui_stub.zig") });
    const io = b.graph.io;
    const abs = b.pathFromRoot(dir);
    var d = std.Io.Dir.cwd().openDir(io, abs, .{ .iterate = true }) catch |e|
        std.debug.panic("-Dui={s}: cannot open directory {s}: {s}", .{ dir, abs, @errorName(e) });
    defer d.close(io);
    var walker = d.walk(b.allocator) catch @panic("OOM");
    var paths: std.ArrayList([]const u8) = .empty;
    while (walker.next(io) catch |e| std.debug.panic("-Dui walk failed: {s}", .{@errorName(e)})) |entry| {
        if (entry.kind != .file) continue;
        const rel = b.allocator.dupe(u8, entry.path) catch @panic("OOM");
        for (rel) |*c| if (c.* == '\\') {
            c.* = '/';
        };
        paths.append(b.allocator, rel) catch @panic("OOM");
    }
    std.mem.sort([]const u8, paths.items, {}, struct {
        fn lt(_: void, x: []const u8, y: []const u8) bool {
            return std.mem.lessThan(u8, x, y);
        }
    }.lt);
    if (paths.items.len == 0) std.debug.panic("-Dui={s}: directory is empty (build the web UI first)", .{dir});
    var src: std.ArrayList(u8) = .empty;
    src.appendSlice(b.allocator, "pub const File = struct { path: []const u8, data: []const u8 };\npub const files: []const File = &.{\n") catch @panic("OOM");
    for (paths.items) |p| {
        src.print(b.allocator, "    .{{ .path = \"{s}\", .data = @embedFile(\"ui/{s}\") }},\n", .{ p, p }) catch @panic("OOM");
    }
    src.appendSlice(b.allocator, "};\n") catch @panic("OOM");
    const wf = b.addWriteFiles();
    _ = wf.addCopyDirectory(.{ .cwd_relative = abs }, "ui", .{});
    const gen = wf.add("ui_assets.zig", src.items);
    return b.createModule(.{ .root_source_file = gen });
}
