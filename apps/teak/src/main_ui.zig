//! Native entry (X11 + wgpu-native on Linux, Win32 + wgpu on Windows).

const std = @import("std");
const teak = @import("teak");
const platform = @import("teak-platform-native");
const gpu_native = @import("teak-gpu-native");
const App = @import("app/app.zig");

pub fn main() !void {
    const gpa = std.heap.c_allocator;
    var host = try platform.Host.init("KERF - DETAIL WORKSTATION", 1440, 900);
    defer host.deinit();
    // IBM Plex Mono, three weights (DESIGN §1), shared by the measurer and the rasterizer.
    try host.registerFont(.mono, .regular, @embedFile("font_plex_regular"));
    try host.registerFont(.mono, .medium, @embedFile("font_plex_medium"));
    try host.registerFont(.mono, .bold, @embedFile("font_plex_bold"));
    var gpu = try gpu_native.Gpu.initWithOptions(host.nativeHandle(), 1440, 900, .{ .msaa = true });
    defer gpu.deinit();
    try teak.run(App, gpa, &host, &gpu, .{ .clear_color = .{ 0.949, 0.937, 0.902, 1 } });
}
