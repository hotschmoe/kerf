//! Minimal HTTP/1.1 server plumbing over std.Io readers/writers: request head + body parsing, response
//! writing, query/percent decoding, a few helpers. No chunked request bodies, no pipelining beyond
//! sequential keep-alive. Everything borrows from the per-request arena.

const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const max_head = 16 * 1024;
pub const max_body: usize = 64 << 20;
pub const max_headers = 100;

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

/// RFC 9110 `tchar`: the characters allowed in a method or header name.
fn isTokenChar(c: u8) bool {
    return switch (c) {
        'a'...'z', 'A'...'Z', '0'...'9', '!', '#', '$', '%', '&', '\'', '*', '+', '-', '.', '^', '_', '`', '|', '~' => true,
        else => false,
    };
}

/// Strictly parse a request head (request line + header lines, no blank line). Rejects what proxies and
/// servers disagree about (request smuggling shapes): bare CR/LF, control bytes, obsolete line folding,
/// whitespace in header names or before the colon, spaces in the target, duplicate or conflicting
/// `Content-Length` / `Transfer-Encoding`, and non-decimal lengths.
pub fn parseHead(head: []const u8) HeadError!Request {
    for (head) |c| if ((c < 0x20 and c != '\r' and c != '\n' and c != '\t') or c == 0x7f) return error.Malformed;
    var lines = std.mem.splitSequence(u8, head, "\r\n");
    const line = lines.next() orelse return error.Malformed;
    if (std.mem.indexOfAny(u8, line, "\r\n") != null) return error.Malformed;
    const sp1 = std.mem.indexOfScalar(u8, line, ' ') orelse return error.Malformed;
    const sp2 = std.mem.indexOfScalarPos(u8, line, sp1 + 1, ' ') orelse return error.Malformed;
    const method = line[0..sp1];
    const target = line[sp1 + 1 .. sp2];
    const version = line[sp2 + 1 ..];
    if (method.len == 0 or method.len > 16 or target.len == 0 or target[0] != '/') return error.Malformed;
    for (method) |c| if (!isTokenChar(c)) return error.Malformed;
    if (std.mem.indexOfScalar(u8, target, ' ') != null) return error.Malformed; // (sp2 is the first space after the target; the version may not contain one)
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
    var n_headers: usize = 0;
    var seen_cl = false;
    var seen_te = false;
    var seen_host = false;
    var seen_conn = false;
    var seen_expect = false;
    while (lines.next()) |l| {
        if (l.len == 0) return error.Malformed;
        if (std.mem.indexOfAny(u8, l, "\r\n") != null) return error.Malformed; // bare CR or LF
        if (l[0] == ' ' or l[0] == '\t') return error.Malformed; // obsolete line folding
        const colon = std.mem.indexOfScalar(u8, l, ':') orelse return error.Malformed;
        const name = l[0..colon];
        if (name.len == 0) return error.Malformed;
        for (name) |c| if (!isTokenChar(c)) return error.Malformed; // also rejects "Content-Length : 5"
        n_headers += 1;
        if (n_headers > max_headers) return error.HeadTooLarge;
        const value = std.mem.trim(u8, l[colon + 1 ..], " \t");
        if (std.ascii.eqlIgnoreCase(name, "content-length")) {
            if (seen_cl) return error.Malformed;
            seen_cl = true;
            req.content_length = parseLength(value) orelse return error.Malformed;
        } else if (std.ascii.eqlIgnoreCase(name, "transfer-encoding")) {
            if (seen_te) return error.Malformed;
            seen_te = true;
            if (std.ascii.eqlIgnoreCase(value, "chunked")) req.chunked = true else return error.Unsupported;
        } else if (std.ascii.eqlIgnoreCase(name, "host")) {
            if (seen_host) return error.Malformed;
            seen_host = true;
        } else if (std.ascii.eqlIgnoreCase(name, "connection")) {
            if (seen_conn) return error.Malformed;
            seen_conn = true;
            if (std.ascii.findIgnoreCase(value, "close") != null) req.keep_alive = false;
            if (!http11 and std.ascii.findIgnoreCase(value, "keep-alive") != null) req.keep_alive = true;
        } else if (std.ascii.eqlIgnoreCase(name, "expect")) {
            if (seen_expect) return error.Malformed;
            seen_expect = true;
            req.expect_continue = std.ascii.findIgnoreCase(value, "100-continue") != null;
        }
    }
    if (seen_cl and seen_te) return error.Malformed; // request smuggling shape
    return req;
}

/// Decimal digits only (no sign, no underscore, no list), at most 18 of them.
fn parseLength(v: []const u8) ?usize {
    if (v.len == 0 or v.len > 18) return null;
    var n: usize = 0;
    for (v) |c| {
        if (c < '0' or c > '9') return null;
        n = n * 10 + (c - '0');
    }
    return n;
}

pub const BodyError = error{ TooLarge, ReadFailed, ShortBody, BadChunk, OutOfMemory, WriteFailed };

/// Read the request body (sending `100 Continue` first when asked), at most `max` bytes. The declared
/// length is checked against `max` before anything is allocated, and the buffer grows as data arrives.
pub fn readBody(a: Allocator, r: *Io.Reader, w: *Io.Writer, req: *Request, max: usize) BodyError!void {
    if (req.chunked) {
        if (req.expect_continue) {
            try w.writeAll("HTTP/1.1 100 Continue\r\n\r\n");
            try w.flush();
        }
        return readChunked(a, r, req, max);
    }
    if (req.content_length == 0) return;
    if (req.content_length > max) return error.TooLarge;
    if (req.expect_continue) {
        try w.writeAll("HTTP/1.1 100 Continue\r\n\r\n");
        try w.flush();
    }
    var body: std.ArrayList(u8) = .empty;
    try readInto(a, r, &body, req.content_length);
    req.body = body.items;
}

/// Append exactly `n` bytes from `r` to `body`, growing in steps so a lying length costs nothing.
fn readInto(a: Allocator, r: *Io.Reader, body: *std.ArrayList(u8), n: usize) BodyError!void {
    var left = n;
    while (left > 0) {
        const step = @min(left, 64 * 1024);
        const dest = try body.addManyAsSlice(a, step);
        r.readSliceAll(dest) catch |e| switch (e) {
            error.EndOfStream => return error.ShortBody,
            error.ReadFailed => return error.ReadFailed,
        };
        left -= step;
    }
}

const max_trailer_lines = 32;

fn readChunked(a: Allocator, r: *Io.Reader, req: *Request, max: usize) BodyError!void {
    var body: std.ArrayList(u8) = .empty;
    while (true) {
        const line = r.takeDelimiterInclusive('\n') catch |e| switch (e) {
            error.EndOfStream => return error.ShortBody,
            error.ReadFailed => return error.ReadFailed,
            error.StreamTooLong => return error.BadChunk,
        };
        if (line.len < 3 or line[line.len - 2] != '\r') return error.BadChunk;
        var t = line[0 .. line.len - 2];
        if (std.mem.indexOfScalar(u8, t, ';')) |semi| t = t[0..semi]; // chunk extensions are ignored
        // At most 8 hex digits (a u32): the value can never wrap `usize`, and nothing is added before the compare.
        if (t.len == 0 or t.len > 8) return error.BadChunk;
        for (t) |c| if (!std.ascii.isHex(c)) return error.BadChunk; // parseInt would accept a leading '+'
        const size = std.fmt.parseInt(u32, t, 16) catch return error.BadChunk;
        if (size == 0) break;
        if (size > max - body.items.len) return error.TooLarge;
        try readInto(a, r, &body, size);
        const crlf = r.take(2) catch |e| switch (e) {
            error.EndOfStream => return error.ShortBody,
            error.ReadFailed => return error.ReadFailed,
        };
        if (!std.mem.eql(u8, crlf, "\r\n")) return error.BadChunk;
    }
    // Trailers until the blank line: bounded, and ignored.
    var lines: usize = 0;
    while (true) {
        const line = r.takeDelimiterInclusive('\n') catch |e| switch (e) {
            error.EndOfStream => return error.ShortBody,
            error.ReadFailed => return error.ReadFailed,
            error.StreamTooLong => return error.BadChunk,
        };
        if (std.mem.trim(u8, line, " \t\r\n").len == 0) break;
        lines += 1;
        if (lines > max_trailer_lines) return error.BadChunk;
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
    try readChunked(a, &r, &req, max_body);
    try std.testing.expectEqualStrings("hello world", req.body);
    try std.testing.expectEqualStrings("NEXT", r.buffered());
}

fn chunkedErr(input: []const u8, max: usize) anyerror!void {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    var r = Io.Reader.fixed(input);
    var req = try parseHead("POST /x HTTP/1.1\r\nTransfer-Encoding: chunked");
    return readChunked(arena.allocator(), &r, &req, max);
}

test "V-1: a huge chunk size is an error, never an overflow (regression: panicked in readChunked)" {
    // one 1-byte chunk, then ffffffffffffffff: 17 hex digits worth of wrap used to kill the process
    try std.testing.expectError(error.BadChunk, chunkedErr("1\r\nx\r\nffffffffffffffff\r\n", max_body));
    // 8 digits is the most accepted; a size over the cap is TooLarge, also after earlier chunks
    try std.testing.expectError(error.TooLarge, chunkedErr("1\r\nx\r\nffffffff\r\n", max_body));
    try std.testing.expectError(error.TooLarge, chunkedErr("ffffffff\r\n", max_body));
    try std.testing.expectError(error.TooLarge, chunkedErr("3\r\nabc\r\n3\r\nabc\r\n0\r\n\r\n", 5));
    try std.testing.expectError(error.BadChunk, chunkedErr("+5\r\nhello\r\n0\r\n\r\n", max_body));
    try std.testing.expectError(error.BadChunk, chunkedErr("5\nhello\r\n0\r\n\r\n", max_body)); // bare LF
    try std.testing.expectError(error.BadChunk, chunkedErr("5\r\nhelloXX0\r\n\r\n", max_body)); // missing CRLF after data
    try std.testing.expectError(error.ShortBody, chunkedErr("5\r\nhel", max_body));
    // unbounded trailers
    var tr: std.ArrayList(u8) = .empty;
    defer tr.deinit(std.testing.allocator);
    try tr.appendSlice(std.testing.allocator, "0\r\n");
    for (0..40) |_| try tr.appendSlice(std.testing.allocator, "X: 1\r\n");
    try tr.appendSlice(std.testing.allocator, "\r\n");
    try std.testing.expectError(error.BadChunk, chunkedErr(tr.items, max_body));
}

test "content-length body: declared length is checked first, then read incrementally" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var out: [64]u8 = undefined;
    var w = Io.Writer.fixed(&out);
    var r = Io.Reader.fixed("hello world");
    var req = try parseHead("POST /x HTTP/1.1\r\nContent-Length: 11");
    try readBody(a, &r, &w, &req, 100);
    try std.testing.expectEqualStrings("hello world", req.body);
    var r2 = Io.Reader.fixed("hello");
    var req2 = try parseHead("POST /x HTTP/1.1\r\nContent-Length: 11");
    try std.testing.expectError(error.ShortBody, readBody(a, &r2, &w, &req2, 100)); // lies: only 5 bytes arrive
    var r3 = Io.Reader.fixed("");
    var req3 = try parseHead("POST /x HTTP/1.1\r\nContent-Length: 11");
    try std.testing.expectError(error.TooLarge, readBody(a, &r3, &w, &req3, 10));
}

test "V-9: strict head parsing rejects the request-smuggling shapes" {
    const bad = [_][]const u8{
        "POST /x HTTP/1.1\r\nContent-Length : 5", // space before the colon
        "POST /x HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 5", // duplicate, even when equal
        "POST /x HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 6", // conflicting
        "POST /x HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked", // CL + TE
        "POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 5",
        "POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked",
        "POST /x HTTP/1.1\r\nContent-Length: +5",
        "POST /x HTTP/1.1\r\nContent-Length: 5, 5",
        "POST /x HTTP/1.1\r\nContent-Length: 0x5",
        "POST /x HTTP/1.1\r\nContent-Length: 5_0",
        "POST /x HTTP/1.1\r\nContent-Length: ",
        "POST /x HTTP/1.1\r\nContent-Length: 99999999999999999999",
        "GET /a b HTTP/1.1", // space in the target
        "GET  /a HTTP/1.1",
        "GET /a  HTTP/1.1",
        "GET /a HTTP/1.1 ",
        "GET /a HTTP/1.1\r\n X: folded", // obsolete line folding
        "GET /a HTTP/1.1\r\nX Y: z", // whitespace in a header name
        "GET /a HTTP/1.1\r\n: nameless",
        "GET /a HTTP/1.1\r\nNoColon",
        "GET /a HTTP/1.1\nHost: x", // bare LF
        "GET /a HTTP/1.1\r\nHost: x\nX: y",
        "GET /a HTTP/1.1\r\nHost: x\rX: y", // bare CR
        "GET /a HTTP/1.1\r\nX: a\x00b", // NUL
        "GET /a HTTP/1.1\r\nHost: a\r\nHost: b", // two Hosts
        "G(T /a HTTP/1.1",
    };
    for (bad) |h| {
        if (parseHead(h)) |_| {
            std.debug.print("accepted: {any}\n", .{h});
            return error.TestExpectedError;
        } else |_| {}
    }
    try std.testing.expectError(error.Unsupported, parseHead("POST /x HTTP/1.1\r\nTransfer-Encoding: chunked, gzip"));
    // still fine
    const ok = try parseHead("POST /x?a=b HTTP/1.1\r\nHost: h\r\ncontent-length:   42  \r\nX-A: b:c");
    try std.testing.expectEqual(@as(usize, 42), ok.content_length);
    const many = "GET /x HTTP/1.1" ++ "\r\nX: 1" ** (max_headers + 1);
    try std.testing.expectError(error.HeadTooLarge, parseHead(many));
}
