//! Streaming endpoints: the server-sent event feed of the folder (/api/events).

const std = @import("std");
const http = @import("../http.zig");
const events = @import("../events.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const serve = @import("../serve.zig");
const Server = serve.Server;
const max_sse_streams = serve.max_sse_streams;
const arm = serve.arm;

pub fn apiEvents(s: *Server, a: Allocator, w: *Io.Writer, extra: []const u8) !bool {
    if (s.sse_active.fetchAdd(1, .acq_rel) >= max_sse_streams) {
        _ = s.sse_active.fetchSub(1, .acq_rel);
        try http.sendError(a, w, 503, false, extra, "E_BUSY", "too many open event streams");
        return false;
    }
    defer _ = s.sse_active.fetchSub(1, .acq_rel);
    const hdr = try a.print("X-Accel-Buffering: no\r\n{s}", .{extra});
    arm(s.cfg.timeouts.write_ms);
    try http.writeHead(w, .{ .status = 200, .content_type = "text/event-stream; charset=utf-8", .content_length = null, .keep_alive = false, .extra = hdr });
    try w.writeAll("retry: 2000\n\nevent: ping\ndata: {}\n\n");
    try w.flush();
    var last = s.hub.current(s.io);
    var buf: std.ArrayList(u8) = .empty;
    defer buf.deinit(s.gpa);
    while (true) {
        arm(0); // waiting for events: no deadline
        buf.clearRetainingCapacity();
        if (!s.hub.wait(s.io, &last, &buf, s.gpa)) return false;
        arm(s.cfg.timeouts.write_ms); // a client that stopped reading is cut off instead of wedging this thread
        w.writeAll(buf.items) catch return false;
        w.flush() catch return false;
    }
}
