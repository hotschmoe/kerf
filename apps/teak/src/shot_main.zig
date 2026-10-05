//! Headless native screenshots of the whole app (no display needed; needs a Vulkan device).
//!
//!   zig build shot -- out.png [--sample truss|slab|strap] [--tab 0|3d|sheet] [--select id]
//!                     [--insp parts|notes|diff|diag] [--demo] [--prompt "text"] [--size 1440x900]
//!                     [--click x,y]... [--frames n]
//!
//! Startup parameters are answered through the same effect path the web build uses (`?sample=...`).
//! Native text is stb_truetype with IBM Plex Mono; web draws with the browser's canvas text engine.

const std = @import("std");
const teak = @import("teak");
const Host = @import("teak-platform-headless").Host;
const Gpu = @import("teak-gpu-headless").Gpu;
const App = @import("app/app.zig");

pub fn main(init: std.process.Init) !void {
    const gpa = std.heap.c_allocator;
    var args = init.minimal.args.iterate();
    _ = args.next();
    var out_path: []const u8 = "shot.png";
    var w: u32 = 1440;
    var h: u32 = 900;
    var frames: u32 = 8;
    var params: [8]struct { name: []const u8, value: []const u8 } = undefined;
    var n_params: usize = 0;
    var clicks: [8][2]f32 = undefined;
    var n_clicks: usize = 0;
    var first = true;
    while (args.next()) |a| {
        if (first and !std.mem.startsWith(u8, a, "--")) {
            out_path = a;
            first = false;
            continue;
        }
        first = false;
        if (std.mem.eql(u8, a, "--demo")) {
            params[n_params] = .{ .name = "demo", .value = "1" };
            n_params += 1;
        } else if (std.mem.eql(u8, a, "--size")) {
            const v = args.next() orelse break;
            const x = std.mem.indexOfScalar(u8, v, 'x') orelse continue;
            w = try std.fmt.parseInt(u32, v[0..x], 10);
            h = try std.fmt.parseInt(u32, v[x + 1 ..], 10);
        } else if (std.mem.eql(u8, a, "--frames")) {
            frames = try std.fmt.parseInt(u32, args.next() orelse break, 10);
        } else if (std.mem.eql(u8, a, "--click")) {
            const v = args.next() orelse break;
            const c = std.mem.indexOfScalar(u8, v, ',') orelse continue;
            clicks[n_clicks] = .{ try std.fmt.parseFloat(f32, v[0..c]), try std.fmt.parseFloat(f32, v[c + 1 ..]) };
            n_clicks += 1;
        } else if (std.mem.startsWith(u8, a, "--") and n_params < params.len) {
            params[n_params] = .{ .name = a[2..], .value = args.next() orelse "" };
            n_params += 1;
        }
    }

    var host = try Host.init(gpa, w, h);
    defer host.deinit();
    try host.registerFont(.mono, .regular, @embedFile("font_plex_regular"));
    try host.registerFont(.mono, .medium, @embedFile("font_plex_medium"));
    try host.registerFont(.mono, .bold, @embedFile("font_plex_bold"));
    var gpu = try Gpu.initOffscreen(w, h, .{ .msaa = true });
    defer gpu.deinit();
    var rt = try teak.Runtime(App, Host, Gpu).init(gpa, &host, &gpu, .{ .clear_color = .{ 0.949, 0.937, 0.902, 1 } });
    defer rt.deinit();

    // Frame 1 submits the boot effects; answer them like the web host would.
    try rt.frame();
    for (host.submittedEffects()) |c| switch (c.kind) {
        .query_param => {
            var val: ?[]const u8 = null;
            for (params[0..n_params]) |p| if (std.mem.eql(u8, p.name, c.name)) {
                val = p.value;
            };
            host.injectEffectResult(.{ .query_value = .{ .id = c.id, .value = val } });
        },
        .storage_get => host.injectEffectResult(.{ .storage_value = .{ .id = c.id, .value = null } }),
        .clock => host.injectEffectResult(.{ .clock = .{ .id = c.id, .unix_ms = 1791214920000, .utc_offset_min = 0 } }),
        else => {},
    };
    host.clearSubmittedEffects();
    var steps: std.ArrayList(teak.headless.Step) = .empty;
    defer steps.deinit(gpa);
    try steps.append(gpa, .{ .frames = 3 });
    for (clicks[0..n_clicks]) |c| try steps.append(gpa, .{ .click = c });
    try steps.append(gpa, .{ .frames = frames });
    try teak.headless.play(&rt, &host, steps.items);
    try teak.headless.writeFramePng(&gpu, gpa, out_path);
}
