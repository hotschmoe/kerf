//! Build helper for `zig build wasm` (REVIEW SIZ-1): runs binaryen's `wasm-opt` on the module, or copies it
//! unchanged when `wasm-opt` is not on PATH and the step is optional.
//!
//! usage: wasm_opt <auto|required> <in.wasm> <out.wasm> [wasm-opt flags...]

const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len < 4) std.process.fatal("usage: wasm_opt <auto|required> <in.wasm> <out.wasm> [wasm-opt flags...]", .{});
    const required = std.mem.eql(u8, args[1], "required");
    const in = args[2];
    const out = args[3];

    var argv: std.ArrayList([]const u8) = .empty;
    try argv.appendSlice(init.gpa, &.{ "wasm-opt", in, "-o", out });
    for (args[4..]) |a| try argv.append(init.gpa, a);
    defer argv.deinit(init.gpa);

    var child = std.process.spawn(io, .{ .argv = argv.items }) catch |err| switch (err) {
        // not on PATH (Windows reports the last candidate's error when PATH has no usable match)
        error.FileNotFound, error.AccessDenied, error.InvalidExe => {
            if (required) std.process.fatal("-Dwasm-opt: cannot run wasm-opt (binaryen) from PATH: {t}; install it or drop -Dwasm-opt", .{err});
            const cwd = std.Io.Dir.cwd();
            return cwd.copyFile(in, cwd, out, io, .{});
        },
        else => return err,
    };
    const term = try child.wait(io);
    if (!term.success()) std.process.fatal("wasm-opt failed: {any}", .{term});
}
