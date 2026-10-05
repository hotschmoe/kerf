//! Minimal HTTP/1.1 server plumbing over std.Io readers/writers: request head + body parsing, response
//! writing, query/percent decoding, a few helpers. No chunked request bodies, no pipelining beyond
//! sequential keep-alive. Everything borrows from the per-request arena.

const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const max_head = 16 * 1024;
pub const max_body: usize = 64 << 20;

pub const Request = struct {
    method: []const u8,
    /// Raw request target (`/api/docs?x=1`).
    target: []const u8,
    /// Path part of the target, still percent-encoded.
    path: []const u8,
    query: []const u8,
    /// Header block: request line + headers, without the blank line.
    head: []const u8,
    content_length: usize,
    /// `Transfer-Encoding: chunked` request body (decoded by `readBody`).
    chunked: bool = false,
    keep_alive: bool,
    expect_continue: bool,
    body: []const u8 = "",

    /// Case-insensitive header lookup.
    pub fn header(r: Request, name: []const u8) ?[]const u8 {
        var it = std.mem.splitSequence(u8, r.head, "\r\n");
        _ = it.next(); // request line
        while (it.next()) |line| {
            const colon = std.mem.indexOfScalar(u8, line, ':') orelse continue;
            if (std.ascii.eqlIgnoreCase(line[0..colon], name)) return std.mem.trim(u8, line[colon + 1 ..], " \t");
        }
        return null;
    }

    /// Percent-decoded query parameter (`+` is a space).
    pub fn param(r: Request, a: Allocator, name: []const u8) !?[]u8 {
        return queryParam(a, r.query, name);
    }
};

pub const HeadError = error{ Closed, HeadTooLarge, Malformed, Unsupported, ReadFailed, OutOfMemory };

/// Read one request head (and nothing of the body). Slices are copied into `a`.
pub fn readHead(a: Allocator, r: *Io.Reader) HeadError!Request {
    var head_end: usize = 0;
    while (true) {
        const buf = r.buffered();
        if (std.mem.indexOf(u8, buf, "\r\n\r\n")) |i| {
            head_end = i;
            break;
        }
        if (buf.len >= r.buffer.len) return error.HeadTooLarge;
        r.fillMore() catch |e| switch (e) {
            error.EndOfStream => return if (buf.len == 0) error.Closed else error.Malformed,
            error.ReadFailed => return error.ReadFailed,
        };
    }
    const head = try a.dupe(u8, r.buffered()[0..head_end]);
    r.toss(head_end + 4);
    return parseHead(head);
}

pub fn parseHead(head: []const u8) HeadError!Request {
    const line_end = std.mem.indexOf(u8, head, "\r\n") orelse head.len;
    const line = head[0..line_end];
    const sp1 = std.mem.indexOfScalar(u8, line, ' ') orelse return error.Malformed;
    const sp2 = std.mem.lastIndexOfScalar(u8, line, ' ') orelse return error.Malformed;
    if (sp2 <= sp1) return error.Malformed;
    const method = line[0..sp1];
    const target = line[sp1 + 1 .. sp2];
    const version = line[sp2 + 1 ..];
    if (method.len == 0 or target.len == 0 or target[0] != '/') return error.Malformed;
    const http11 = std.mem.eql(u8, version, "HTTP/1.1");
    if (!http11 and !std.mem.eql(u8, version, "HTTP/1.0")) return error.Malformed;
    const q = std.mem.indexOfScalar(u8, target, '?');
    var req: Request = .{
        .method = method,
        .target = target,
        .path = if (q) |i| target[0..i] else target,
        .query = if (q) |i| target[i + 1 ..] else "",
        .head = head,
        .content_length = 0,
        .keep_alive = http11,
        .expect_continue = false,
    };
    if (req.header("transfer-encoding")) |te| {
        if (std.ascii.eqlIgnoreCase(te, "chunked")) {
            req.chunked = true;
        } else if (!std.ascii.eqlIgnoreCase(te, "identity")) return error.Unsupported;
    }
    if (req.header("content-length")) |cl| {
        req.content_length = std.fmt.parseInt(usize, cl, 10) catch return error.Malformed;
    }
    if (req.header("connection")) |c| {
        if (std.ascii.findIgnoreCase(c, "close") != null) req.keep_alive = false;
        if (!http11 and std.ascii.findIgnoreCase(c, "keep-alive") != null) req.keep_alive = true;
    }
    if (req.header("expect")) |e| req.expect_continue = std.ascii.findIgnoreCase(e, "100-continue") != null;
    return req;
}

pub const BodyError = error{ TooLarge, ReadFailed, ShortBody, OutOfMemory, WriteFailed };

/// Read `req.content_length` body bytes (sending `100 Continue` first when asked).
pub fn readBody(a: Allocator, r: *Io.Reader, w: *Io.Writer, req: *Request) BodyError!void {
    if (req.chunked) {
        if (req.expect_continue) {
            try w.writeAll("HTTP/1.1 100 Continue\r\n\r\n");
            try w.flush();
        }
        return readChunked(a, r, req);
    }
    if (req.content_length == 0) return;
    if (req.content_length > max_body) return error.TooLarge;
    if (req.expect_continue) {
        try w.writeAll("HTTP/1.1 100 Continue\r\n\r\n");
        try w.flush();
    }
    req.body = r.readAlloc(a, req.content_length) catch |e| switch (e) {
        error.OutOfMemory => return error.OutOfMemory,
        error.EndOfStream => return error.ShortBody,
        error.ReadFailed => return error.ReadFailed,
    };
}

fn readChunked(a: Allocator, r: *Io.Reader, req: *Request) BodyError!void {
    var body: std.ArrayList(u8) = .empty;
    while (true) {
        const line = r.takeDelimiterInclusive('\n') catch |e| switch (e) {
            error.EndOfStream => return error.ShortBody,
            error.ReadFailed => return error.ReadFailed,
            error.StreamTooLong => return error.ShortBody,
        };
        var t = std.mem.trim(u8, line, " \t\r\n");
        if (std.mem.indexOfScalar(u8, t, ';')) |semi| t = t[0..semi];
        const size = std.fmt.parseInt(usize, t, 16) catch return error.ShortBody;
        if (size == 0) break;
        if (body.items.len + size > max_body) return error.TooLarge;
        const chunk = r.readAlloc(a, size) catch |e| switch (e) {
            error.OutOfMemory => return error.OutOfMemory,
            error.EndOfStream => return error.ShortBody,
            error.ReadFailed => return error.ReadFailed,
        };
        try body.appendSlice(a, chunk);
        r.discardAll(2) catch return error.ShortBody; // the CRLF after the chunk
    }
    // trailers until the blank line
    while (true) {
        const line = r.takeDelimiterInclusive('\n') catch return error.ShortBody;
        if (std.mem.trim(u8, line, " \t\r\n").len == 0) break;
    }
    req.body = body.items;
    req.content_length = body.items.len;
}

pub fn reason(status: u16) []const u8 {
    return switch (status) {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        422 => "Unprocessable Content",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        else => "Status",
    };
}

pub const Head = struct {
    status: u16 = 200,
    content_type: ?[]const u8 = null,
    /// null = no Content-Length (the body ends when the connection closes).
    content_length: ?usize = null,
    keep_alive: bool = true,
    /// Pre-formatted extra header lines, each ending in "\r\n".
    extra: []const u8 = "",
    /// API/SSE responses are never cached.
    no_store: bool = true,
};

pub fn writeHead(w: *Io.Writer, h: Head) Io.Writer.Error!void {
    try w.print("HTTP/1.1 {d} {s}\r\n", .{ h.status, reason(h.status) });
    if (h.content_type) |ct| try w.print("Content-Type: {s}\r\n", .{ct});
    if (h.content_length) |n| try w.print("Content-Length: {d}\r\n", .{n});
    if (h.no_store) try w.writeAll("Cache-Control: no-store\r\n");
    try w.writeAll("X-Content-Type-Options: nosniff\r\n");
    try w.print("Connection: {s}\r\n", .{if (h.keep_alive and h.content_length != null) "keep-alive" else "close"});
    try w.writeAll(h.extra);
    try w.writeAll("\r\n");
}

/// Head + body in one go, flushed.
pub fn send(w: *Io.Writer, h: Head, body: []const u8) Io.Writer.Error!void {
    var hh = h;
    hh.content_length = body.len;
    try writeHead(w, hh);
    try w.writeAll(body);
    try w.flush();
}

pub fn sendJson(w: *Io.Writer, status: u16, keep_alive: bool, extra: []const u8, body: []const u8) Io.Writer.Error!void {
    try send(w, .{ .status = status, .content_type = "application/json; charset=utf-8", .keep_alive = keep_alive, .extra = extra }, body);
}

/// `{"error":{"code":"E_X","message":"…"}}` with `status`.
pub fn sendError(a: Allocator, w: *Io.Writer, status: u16, keep_alive: bool, extra: []const u8, code: []const u8, message: []const u8) !void {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"error\":{\"code\":");
    try jsonString(&out, a, code);
    try out.appendSlice(a, ",\"message\":");
    try jsonString(&out, a, message);
    try out.appendSlice(a, "}}\n");
    try sendJson(w, status, keep_alive, extra, out.items);
}

pub fn jsonString(out: *std.ArrayList(u8), a: Allocator, s: []const u8) Allocator.Error!void {
    try out.append(a, '"');
    for (s) |c| switch (c) {
        '"' => try out.appendSlice(a, "\\\""),
        '\\' => try out.appendSlice(a, "\\\\"),
        '\n' => try out.appendSlice(a, "\\n"),
        '\r' => try out.appendSlice(a, "\\r"),
        '\t' => try out.appendSlice(a, "\\t"),
        0...8, 11, 12, 14...31 => try out.print(a, "\\u{x:0>4}", .{c}),
        else => try out.append(a, c),
    };
    try out.append(a, '"');
}

/// Percent-decode `s` (`+` -> space when `plus`). Invalid escapes pass through.
pub fn percentDecode(a: Allocator, s: []const u8, plus: bool) Allocator.Error![]u8 {
    var out = try a.alloc(u8, s.len);
    var n: usize = 0;
    var i: usize = 0;
    while (i < s.len) : (i += 1) {
        const c = s[i];
        if (c == '%' and i + 2 < s.len) {
            if (std.fmt.parseInt(u8, s[i + 1 .. i + 3], 16)) |v| {
                out[n] = v;
                n += 1;
                i += 2;
                continue;
            } else |_| {}
        }
        out[n] = if (plus and c == '+') ' ' else c;
        n += 1;
    }
    return out[0..n];
}

pub fn queryParam(a: Allocator, query: []const u8, name: []const u8) Allocator.Error!?[]u8 {
    var it = std.mem.splitScalar(u8, query, '&');
    while (it.next()) |pair| {
        if (pair.len == 0) continue;
        const eq = std.mem.indexOfScalar(u8, pair, '=');
        const k = try percentDecode(a, if (eq) |e| pair[0..e] else pair, true);
        if (!std.mem.eql(u8, k, name)) continue;
        return try percentDecode(a, if (eq) |e| pair[e + 1 ..] else "", true);
    }
    return null;
}

/// Constant-time equality for secrets.
pub fn secretEql(a: []const u8, b: []const u8) bool {
    var diff: usize = a.len ^ b.len;
    const n = @min(a.len, b.len);
    for (0..n) |i| diff |= a[i] ^ b[i];
    return diff == 0;
}

pub fn mimeFor(path: []const u8) []const u8 {
    const ext = std.fs.path.extension(path);
    const table = [_]struct { []const u8, []const u8 }{
        .{ ".html", "text/html; charset=utf-8" },
        .{ ".js", "text/javascript; charset=utf-8" },
        .{ ".mjs", "text/javascript; charset=utf-8" },
        .{ ".css", "text/css; charset=utf-8" },
        .{ ".json", "application/json; charset=utf-8" },
        .{ ".wasm", "application/wasm" },
        .{ ".svg", "image/svg+xml" },
        .{ ".png", "image/png" },
        .{ ".jpg", "image/jpeg" },
        .{ ".jpeg", "image/jpeg" },
        .{ ".gif", "image/gif" },
        .{ ".webp", "image/webp" },
        .{ ".ico", "image/x-icon" },
        .{ ".woff2", "font/woff2" },
        .{ ".woff", "font/woff" },
        .{ ".ttf", "font/ttf" },
        .{ ".otf", "font/otf" },
        .{ ".txt", "text/plain; charset=utf-8" },
        .{ ".md", "text/markdown; charset=utf-8" },
        .{ ".map", "application/json" },
        .{ ".webmanifest", "application/manifest+json" },
        .{ ".pdf", "application/pdf" },
        .{ ".dxf", "application/dxf" },
    };
    for (table) |e| if (std.ascii.eqlIgnoreCase(ext, e[0])) return e[1];
    return "application/octet-stream";
}

test "parseHead basics" {
    const req = try parseHead("POST /api/docs/a.kerf.json/apply?x=1&y=a%20b HTTP/1.1\r\nHost: localhost:7700\r\nContent-Length: 12\r\nAuthorization: Bearer tok\r\nExpect: 100-continue");
    try std.testing.expectEqualStrings("POST", req.method);
    try std.testing.expectEqualStrings("/api/docs/a.kerf.json/apply", req.path);
    try std.testing.expectEqualStrings("x=1&y=a%20b", req.query);
    try std.testing.expectEqual(@as(usize, 12), req.content_length);
    try std.testing.expect(req.keep_alive);
    try std.testing.expect(req.expect_continue);
    try std.testing.expectEqualStrings("Bearer tok", req.header("authorization").?);
    try std.testing.expect(req.header("nope") == null);
}

test "parseHead rejects junk" {
    try std.testing.expectError(error.Malformed, parseHead("GET /x"));
    try std.testing.expectError(error.Malformed, parseHead("GET x HTTP/1.1"));
    try std.testing.expectError(error.Malformed, parseHead("GET /x HTTP/2"));
    try std.testing.expectError(error.Unsupported, parseHead("POST /x HTTP/1.1\r\nTransfer-Encoding: gzip"));
    try std.testing.expectError(error.Malformed, parseHead("POST /x HTTP/1.1\r\nContent-Length: abc"));
    const r = try parseHead("GET /x HTTP/1.0\r\nHost: a");
    try std.testing.expect(!r.keep_alive);
    const r2 = try parseHead("GET /x HTTP/1.1\r\nConnection: close");
    try std.testing.expect(!r2.keep_alive);
}

test "query decoding" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try std.testing.expectEqualStrings("a b/c", (try queryParam(a, "x=1&v=a%20b%2Fc", "v")).?);
    try std.testing.expectEqualStrings("a b", (try queryParam(a, "v=a+b", "v")).?);
    try std.testing.expect((try queryParam(a, "x=1", "v")) == null);
    try std.testing.expectEqualStrings("100%", (try queryParam(a, "v=100%", "v")).?);
}

test "secretEql" {
    try std.testing.expect(secretEql("abc", "abc"));
    try std.testing.expect(!secretEql("abc", "abd"));
    try std.testing.expect(!secretEql("abc", "ab"));
}

test "readChunked" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var r = Io.Reader.fixed("5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nX-T: 1\r\n\r\nNEXT");
    var req = try parseHead("POST /x HTTP/1.1\r\nTransfer-Encoding: chunked");
    try std.testing.expect(req.chunked);
    try readChunked(a, &r, &req);
    try std.testing.expectEqualStrings("hello world", req.body);
    try std.testing.expectEqualStrings("NEXT", r.buffered());
}
