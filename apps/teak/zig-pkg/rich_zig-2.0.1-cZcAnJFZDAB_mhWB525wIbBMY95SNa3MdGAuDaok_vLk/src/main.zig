const std = @import("std");
const rich = @import("rich_zig");

pub fn main(init: std.process.Init) !void {
    _ = rich.terminal.enableUtf8();
    _ = rich.terminal.enableVirtualTerminal();

    const allocator = init.gpa;
    const io = init.io;

    var console = rich.Console.init(allocator, io, init.minimal.environ);
    defer console.deinit();

    const panel = rich.Panel.fromText(allocator, "Run examples with: zig build run\n\nIndividual examples:\n  zig build example-hello\n  zig build example-panel\n  zig build example-table\n  zig build example-progress\n  zig build example-tree\n  zig build example-layout\n  zig build example-json_syntax\n  zig build example-terminal\n  zig build example-markdown\n  zig build example-split\n  zig build example-traceback\n  zig build example-logging\n  zig build example-advanced_table\n  zig build example-advanced_panel\n  zig build example-advanced_progress\n  zig build example-advanced_syntax\n  zig build example-color_features")
        .withTitle("rich_zig Examples")
        .withWidth(50);
    try console.printRenderable(panel);
}

test "simple test" {
    const gpa = std.testing.allocator;
    var list: std.ArrayList(i32) = .empty;
    defer list.deinit(gpa);
    try list.append(gpa, 42);
    try std.testing.expectEqual(@as(i32, 42), list.pop());
}
