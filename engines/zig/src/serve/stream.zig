//! Streaming endpoints: the server-sent event feed of the folder (/api/events) and the LLM proxy (/api/llm).

const std = @import("std");
const http = @import("../http.zig");
const events = @import("../events.zig");
const proxy = @import("../proxy.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const serve = @import("../serve.zig");
const Server = serve.Server;
const max_sse_streams = serve.max_sse_streams;
const max_proxy_calls = serve.max_proxy_calls;
const arm = serve.arm;
const badRequest = serve.badRequest;
const parseBody = serve.parseBody;

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

pub fn apiLlm(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { provider, base_url?, path, headers, body }");
    const planned = try proxy.plan(a, body);
    switch (planned) {
        .rejected => |rej| {
            try http.sendError(a, w, rej.status, req.keep_alive, extra, rej.code, rej.message);
            return req.keep_alive;
        },
        .ok => |p| {
            if (s.proxy_active.fetchAdd(1, .acq_rel) >= max_proxy_calls) {
                _ = s.proxy_active.fetchSub(1, .acq_rel);
                try http.sendError(a, w, 429, false, extra, "E_BUSY", "too many LLM calls in flight; retry in a moment");
                return false;
            }
            defer _ = s.proxy_active.fetchSub(1, .acq_rel);
            arm(0); // the proxy enforces its own stall limits
            var detail: []const u8 = "upstream error";
            proxy.forwardLimited(s.io, a, &s.client, p, w, &detail, s.cfg.llm_limits) catch |e| switch (e) {
                error.Upstream => {
                    try http.sendError(a, w, 502, false, extra, "E_UPSTREAM", detail);
                    return false;
                },
                error.HeadSent => return false,
                error.OutOfMemory => return e,
            };
            return false; // the response was close-delimited
        },
    }
}
