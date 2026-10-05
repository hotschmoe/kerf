//! Wasm entry. Zunk owns the rAF loop and calls the exported init/frame/resize;
//! all loop logic lives in `teak.Runtime`.

const std = @import("std");
const teak = @import("teak");
const platform = @import("teak-platform-wasm");
const gpu_web = @import("teak-gpu-web");
const zunk = @import("zunk");
const zapp = zunk.web.app;
const App = @import("app/app.zig");

const Host = platform.Host;
const Gpu = gpu_web.Gpu;
const Runtime = teak.Runtime(App, Host, Gpu);

comptime {
    teak.validateHost(Host);
    teak.validateGpu(Gpu);
}

var host: Host = undefined;
var gpu: Gpu = undefined;
var runtime: Runtime = undefined;

const W = 1440;
const H = 900;

export fn init() void {
    host = Host.init("KERF - DETAIL WORKSTATION", W, H) catch @panic("host init failed");
    host.activate();
    gpu = Gpu.initWithOptions(host.nativeHandle(), W, H, .{ .msaa = true }) catch @panic("gpu init failed");
    runtime = Runtime.init(std.heap.wasm_allocator, &host, &gpu, .{ .clear_color = .{ 0.949, 0.937, 0.902, 1 } }) catch @panic("runtime init failed");
}

export fn resize(w: u32, h: u32) void {
    gpu.resize(w, h);
}

// Timing probe: logs the time of the first frame and the mean frame cost every 60 frames.
var frames: u32 = 0;
var acc_ms: f64 = 0;

export fn frame(_: f32) void {
    const t0 = zapp.performanceNow();
    runtime.frame() catch @panic("teak: frame failed (out of memory)");
    const dt = zapp.performanceNow() - t0;
    frames += 1;
    acc_ms += dt;
    var buf: [96]u8 = undefined;
    if (frames == 1) {
        const s = std.fmt.bufPrint(&buf, "kerf: first frame at {d:.0} ms (frame cost {d:.1} ms)", .{ t0, dt }) catch return;
        zapp.logInfo(s);
    } else if (frames % 60 == 0) {
        const s = std.fmt.bufPrint(&buf, "kerf: mean frame cost {d:.2} ms over 60 frames", .{acc_ms / 60.0}) catch return;
        zapp.logInfo(s);
        acc_ms = 0;
    }
}
