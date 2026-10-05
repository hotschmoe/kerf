const std = @import("std");
const root = @import("root.zig");

/// Fuzz test entry point
/// This is a placeholder for fuzz testing infrastructure
/// The CI runs `zig build test --fuzz` with a timeout
pub fn main(init: std.process.Init) !void {
    _ = init;
    // Future: add fuzzing for color parsing, markup parsing, etc.
    std.debug.print("Fuzz testing infrastructure ready\n", .{});
}

test "fuzz placeholder" {
    // Basic sanity test to ensure module compiles
    const allocator = std.testing.allocator;
    _ = allocator;
}
