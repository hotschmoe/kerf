//! Wasm entry. Zunk owns the rAF loop and calls the exported init/frame/resize;
//! all loop logic lives in `teak.Runtime`.

const std = @import("std");
const teak = @import("teak");
const platform = @import("teak-platform-wasm");
const gpu_web = @import("teak-gpu-web");
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

export fn frame(_: f32) void {
    runtime.frame() catch @panic("teak: frame failed (out of memory)");
}
