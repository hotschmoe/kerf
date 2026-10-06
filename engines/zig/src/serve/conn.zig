//! Connections (spec/SERVE.md): limits and deadlines, the per-connection request loop with its watchdog, and the access rules (token, Host and Origin) applied before any route is served.

const std = @import("std");
const kerf = @import("kerf");
const http = @import("../http.zig");
const proxy = @import("../proxy.zig");
const Io = std.Io;
const net = std.Io.net;
const Allocator = std.mem.Allocator;
const serve = @import("../serve.zig");
const Server = serve.Server;
const handleRequest = serve.handleRequest;

pub const max_conns: u32 = 256;

pub const max_sse_streams: u32 = 32;

pub const max_proxy_calls: u32 = 8;

pub const max_requests_per_conn: u32 = 1000;

pub const busy_ms: u32 = 120_000; // one handler, apart from SSE and the LLM proxy

pub const Timeouts = struct {
    /// Keep-alive wait for the next request.
    idle_ms: u32 = 30_000,
    /// From the first byte of a request to the end of its head (the body gets three times this, plus 1 s per 512 KiB declared).
    head_ms: u32 = 10_000,
    /// One blocking write to a streaming (SSE) client.
    write_ms: u32 = 20_000,
};

/// One open connection. `deadline_ns` (awake clock, 0 = none) is armed around every blocking socket operation; the
/// watchdog task shuts the socket down when it passes, which unblocks the reader or writer (a thread-per-connection
/// server needs this, there are no read timeouts to lean on).
pub const Conn = struct {
    stream: net.Stream,
    deadline_ns: std.atomic.Value(i64) = .init(0),
    expired: bool = false,
};

threadlocal var tl_conn: ?*Conn = null;

threadlocal var tl_io: ?Io = null;

pub fn nowNs(io: Io) i64 {
    return @intCast(Io.Timestamp.now(io, .awake).nanoseconds);
}

/// Arm the current connection's deadline `ms` from now (0 = disarm).
pub fn arm(ms: u64) void {
    const c = tl_conn orelse return;
    const io = tl_io orelse return;
    c.deadline_ns.store(if (ms == 0) 0 else nowNs(io) + @as(i64, @intCast(ms)) * std.time.ns_per_ms, .release);
}

pub fn connWatchdog(s: *Server) void {
    while (true) {
        s.io.sleep(Io.Duration.fromMilliseconds(500), .awake) catch return;
        const now = nowNs(s.io);
        s.conns_mu.lockUncancelable(s.io);
        for (s.conns.items) |c| {
            const d = c.deadline_ns.load(.acquire);
            if (d != 0 and now > d and !c.expired) {
                c.expired = true;
                c.stream.shutdown(s.io, .both) catch {};
            }
        }
        s.conns_mu.unlock(s.io);
    }
}

pub fn serveConn(s: *Server, stream: net.Stream) void {
    const io = s.io;
    defer {
        stream.close(io);
        _ = s.active_conns.fetchSub(1, .acq_rel);
    }
    var conn: Conn = .{ .stream = stream };
    s.conns_mu.lockUncancelable(io);
    s.conns.append(s.gpa, &conn) catch {
        s.conns_mu.unlock(io);
        return;
    };
    s.conns_mu.unlock(io);
    defer {
        s.conns_mu.lockUncancelable(io);
        for (s.conns.items, 0..) |c, i| if (c == &conn) {
            _ = s.conns.swapRemove(i);
            break;
        };
        s.conns_mu.unlock(io);
    }
    tl_conn = &conn;
    tl_io = io;
    defer {
        tl_conn = null;
        tl_io = null;
    }

    var rbuf: [http.max_head]u8 = undefined;
    var wbuf: [16 * 1024]u8 = undefined;
    var sr = stream.reader(io, &rbuf);
    var sw = stream.writer(io, &wbuf);
    const w = &sw.interface;
    var served: u32 = 0;
    while (served < max_requests_per_conn) : (served += 1) {
        var arena = std.heap.ArenaAllocator.init(s.gpa);
        defer arena.deinit();
        const a = arena.allocator();
        arm(s.cfg.timeouts.idle_ms);
        _ = sr.interface.peek(1) catch return; // closed, or idle too long
        arm(s.cfg.timeouts.head_ms);
        var req = http.readHead(a, &sr.interface) catch |e| {
            switch (e) {
                error.HeadTooLarge => http.sendError(a, w, 431, false, "", "E_HEAD", "request headers too large") catch {},
                error.Malformed => http.sendError(a, w, 400, false, "", "E_HTTP", "malformed HTTP request") catch {},
                error.Unsupported => http.sendError(a, w, 501, false, "", "E_HTTP", "unsupported Transfer-Encoding (only chunked or Content-Length bodies)") catch {},
                else => {},
            }
            return;
        };
        arm(busy_ms);
        const keep = handleRequest(s, a, &req, &sr.interface, w) catch |e| blk: {
            http.sendError(a, w, 500, false, "", "E_INTERNAL", @errorName(e)) catch {};
            break :blk false;
        };
        arm(s.cfg.timeouts.write_ms);
        w.flush() catch return;
        if (!keep or !req.keep_alive) return;
    }
}

pub const Reject = struct { status: u16, code: []const u8, message: []const u8 };

pub fn hostIsLoopbackName(host_header: []const u8) bool {
    var h = host_header;
    if (std.mem.startsWith(u8, h, "[")) {
        const close = std.mem.indexOfScalar(u8, h, ']') orelse return false;
        h = h[1..close];
    } else if (std.mem.lastIndexOfScalar(u8, h, ':')) |c| {
        h = h[0..c];
    }
    return std.ascii.eqlIgnoreCase(h, "localhost") or std.ascii.endsWithIgnoreCase(h, ".localhost") or std.mem.eql(u8, h, "127.0.0.1") or std.mem.eql(u8, h, "::1");
}

pub fn originMatchesHost(origin: []const u8, host: []const u8) bool {
    const sep = std.mem.indexOf(u8, origin, "://") orelse return false;
    return std.ascii.eqlIgnoreCase(origin[sep + 3 ..], host);
}

/// Token, Host and Origin checks for /api. Returns the rejection or null when allowed.
pub fn checkAccess(s: *Server, a: Allocator, req: http.Request) ?Reject {
    const host = req.header("host") orelse "";
    if (req.header("origin")) |origin| {
        const same = originMatchesHost(origin, host);
        const extra = if (s.cfg.allow_origin) |ao| std.mem.eql(u8, ao, origin) else false;
        if (!same and !extra) return .{ .status = 403, .code = "E_ORIGIN", .message = "cross-origin request refused (the UI must be served by this kerf serve)" };
    }
    if (s.cfg.token) |tok| {
        if (req.method.len == 7 and std.mem.eql(u8, req.method, "OPTIONS")) return null;
        var supplied: ?[]const u8 = null;
        if (req.header("authorization")) |auth| {
            if (auth.len > 7 and std.ascii.eqlIgnoreCase(auth[0..7], "bearer ")) supplied = std.mem.trim(u8, auth[7..], " ");
        }
        if (supplied == null) supplied = (http.queryParam(a, req.query, "token") catch null);
        if (supplied == null or !http.secretEql(supplied.?, tok)) return .{ .status = 401, .code = "E_TOKEN", .message = "missing or wrong token: send Authorization: Bearer <token> (SSE and downloads: ?token=<token>)" };
    } else if (s.loopback_only and !hostIsLoopbackName(host)) {
        return .{ .status = 403, .code = "E_HOST", .message = "unexpected Host header; use http://localhost or http://127.0.0.1" };
    }
    return null;
}
