//! `POST /api/llm`: forward one HTTP request to an LLM provider (avoids browser CORS) and stream the
//! response back verbatim. The URL rules live in `plan` (pure, unit tested): only https:// URLs, plus
//! http:// to localhost/LAN hosts for the `custom` provider. Keys travel in `headers` and are never
//! stored or logged.

const std = @import("std");
const kerf = @import("kerf");
const http = @import("http.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const Header = std.http.Header;

pub const Plan = struct {
    url: []const u8,
    host: []const u8,
    method: std.http.Method,
    headers: []const Header,
    body: ?[]const u8,
};

pub const Rejection = struct { status: u16 = 400, code: []const u8, message: []const u8 };

pub const PlanResult = union(enum) { ok: Plan, rejected: Rejection };

const providers = [_]struct { id: []const u8, base: ?[]const u8 }{
    .{ .id = "anthropic", .base = "https://api.anthropic.com" },
    .{ .id = "openai", .base = "https://api.openai.com" },
    .{ .id = "gemini", .base = "https://generativelanguage.googleapis.com" },
    .{ .id = "xai", .base = "https://api.x.ai" },
    .{ .id = "openrouter", .base = "https://openrouter.ai/api" },
    .{ .id = "custom", .base = null },
};

const blocked_headers = [_][]const u8{
    "host",            "content-length", "connection",       "transfer-encoding", "accept-encoding",     "upgrade",
    "te",              "trailer",        "expect",           "keep-alive",        "proxy-authorization", "proxy-connection",
    "x-forwarded-for", "forwarded",      "content-encoding",
};

fn reject(code: []const u8, message: []const u8) PlanResult {
    return .{ .rejected = .{ .code = code, .message = message } };
}

/// True for localhost, loopback, private (RFC 1918 / link-local / ULA) addresses, `.local` names and
/// single-label host names (typical LAN machines such as `ollama` or `mybox`).
pub fn isLocalOrLan(host: []const u8) bool {
    if (host.len == 0) return true;
    if (std.ascii.eqlIgnoreCase(host, "localhost") or std.ascii.endsWithIgnoreCase(host, ".localhost")) return true;
    if (std.ascii.endsWithIgnoreCase(host, ".local") or std.ascii.endsWithIgnoreCase(host, ".lan") or std.ascii.endsWithIgnoreCase(host, ".internal")) return true;
    if (std.Io.net.Ip4Address.parse(host, 0)) |ip4| return isPrivate4(ip4.bytes) else |_| {}
    if (std.Io.net.Ip6Address.parse(host, 0)) |ip6| {
        const b = ip6.bytes;
        if (std.mem.allEqual(u8, &b, 0)) return true; // ::
        if (std.mem.allEqual(u8, b[0..15], 0) and b[15] == 1) return true; // ::1
        if ((b[0] & 0xfe) == 0xfc) return true; // fc00::/7
        if (b[0] == 0xfe and (b[1] & 0xc0) == 0x80) return true; // fe80::/10
        if (std.mem.allEqual(u8, b[0..10], 0) and b[10] == 0xff and b[11] == 0xff) return isPrivate4(b[12..16].*); // ::ffff:a.b.c.d
        return false;
    } else |_| {}
    return std.mem.indexOfScalar(u8, host, '.') == null;
}

fn isPrivate4(b: [4]u8) bool {
    if (b[0] == 127 or b[0] == 10 or b[0] == 0) return true;
    if (b[0] == 172 and b[1] >= 16 and b[1] <= 31) return true;
    if (b[0] == 192 and b[1] == 168) return true;
    if (b[0] == 169 and b[1] == 254) return true;
    if (b[0] == 100 and b[1] >= 64 and b[1] <= 127) return true; // CGNAT
    return false;
}

/// Hosts a non-`custom` provider may reach: a real DNS name (not an IP literal, not numeric-looking).
fn isPublicName(host: []const u8) bool {
    if (isLocalOrLan(host)) return false;
    if (std.mem.indexOfScalar(u8, host, '.') == null) return false;
    const dot = std.mem.lastIndexOfScalar(u8, host, '.').?;
    const tld = host[dot + 1 ..];
    if (tld.len == 0) return false;
    var all_digits = true;
    for (tld) |c| if (!std.ascii.isDigit(c)) {
        all_digits = false;
    };
    if (all_digits) return false;
    if (std.ascii.startsWithIgnoreCase(tld, "0x")) return false;
    return std.mem.indexOfScalar(u8, host, ':') == null;
}

fn validPath(path: []const u8) bool {
    if (path.len == 0 or path[0] != '/') return false;
    if (path.len > 4096) return false;
    for (path) |c| if (c <= 0x20 or c == 0x7f or c == '\\') return false;
    if (std.mem.indexOf(u8, path, "://") != null) return false;
    if (std.mem.startsWith(u8, path, "//")) return false;
    return true;
}

fn validHeaderName(n: []const u8) bool {
    if (n.len == 0 or n.len > 100) return false;
    for (n) |c| switch (c) {
        'a'...'z', 'A'...'Z', '0'...'9', '-', '_' => {},
        else => return false,
    };
    return true;
}

/// Validate the `/api/llm` request object and build the outgoing request. All strings live in `a`.
pub fn plan(a: Allocator, root: kerf.json.Value) Allocator.Error!PlanResult {
    if (root != .object) return reject("E_INPUT", "body must be a JSON object { provider, base_url?, path, headers, body }");
    const provider = (if (root.get("provider")) |v| v.str() else null) orelse
        return reject("E_INPUT", "missing \"provider\" (anthropic | openai | gemini | xai | openrouter | custom)");
    var default_base: ?[]const u8 = null;
    var known = false;
    for (providers) |p| if (std.mem.eql(u8, p.id, provider)) {
        known = true;
        default_base = p.base;
    };
    if (!known) return reject("E_PROVIDER", "unknown provider; use anthropic, openai, gemini, xai, openrouter or custom");
    const is_custom = std.mem.eql(u8, provider, "custom");

    const base_in: ?[]const u8 = if (root.get("base_url")) |v| (if (v.isNull()) null else (v.str() orelse return reject("E_INPUT", "\"base_url\" must be a string"))) else null;
    const base_raw = base_in orelse default_base orelse return reject("E_INPUT", "provider \"custom\" needs \"base_url\" (for example http://localhost:11434/v1)");
    const base = std.mem.trimEnd(u8, base_raw, "/");

    const path = (if (root.get("path")) |v| v.str() else null) orelse return reject("E_INPUT", "missing \"path\" (for example /v1/messages)");
    if (!validPath(path)) return reject("E_PATH", "\"path\" must start with a single \"/\" and contain no spaces, backslashes or \"://\"");

    const url = try std.mem.concat(a, u8, &.{ base, path });
    const uri = std.Uri.parse(url) catch return reject("E_URL", "base_url + path is not a valid URL");
    if (uri.user != null or uri.password != null) return reject("E_URL", "credentials in the URL are not allowed; send keys in \"headers\"");
    if (uri.fragment != null) return reject("E_URL", "URL fragments are not allowed");
    const https = std.ascii.eqlIgnoreCase(uri.scheme, "https");
    const httpp = std.ascii.eqlIgnoreCase(uri.scheme, "http");
    if (!https and !httpp) return reject("E_URL", "only https:// URLs are allowed (http:// only for localhost/LAN with provider \"custom\")");
    // Not `HostName.fromUri`: it applies the RFC 1123 hostname grammar, which rejects IPv6 literals such as `[::1]`.
    const hbuf = try a.alloc(u8, std.Io.net.HostName.max_len);
    const host = (uri.host orelse return reject("E_URL", "URL has no valid host")).toRaw(hbuf) catch return reject("E_URL", "URL has no valid host");
    if (is_custom) {
        if (httpp and !isLocalOrLan(host)) return reject("E_URL", "plain http:// is only allowed for localhost and LAN hosts; use https:// for public servers");
    } else {
        if (!https) return reject("E_URL", "only https:// URLs are allowed for this provider (use provider \"custom\" for a local server)");
        if (!isPublicName(host)) return reject("E_URL", "this provider must be a public https host; use provider \"custom\" for localhost/LAN servers");
    }

    var hdrs: std.ArrayList(Header) = .empty;
    if (root.get("headers")) |hv| {
        if (hv != .object) return reject("E_INPUT", "\"headers\" must be an object of string values");
        for (hv.object) |m| {
            const val = m.value.str() orelse return reject("E_HEADER", "every header value must be a string");
            if (!validHeaderName(m.key)) return reject("E_HEADER", "invalid header name");
            for (val) |c| if (c == '\r' or c == '\n' or c == 0) return reject("E_HEADER", "header values must not contain line breaks");
            var skip = false;
            for (blocked_headers) |b| if (std.ascii.eqlIgnoreCase(b, m.key)) {
                skip = true;
            };
            if (skip) continue;
            try hdrs.append(a, .{ .name = m.key, .value = val });
        }
    }

    var body: ?[]const u8 = null;
    if (root.get("body")) |bv| {
        if (!bv.isNull()) {
            if (bv.str()) |s| {
                body = s;
            } else {
                var out: std.ArrayList(u8) = .empty;
                try kerf.json.writeCompact(&out, a, bv);
                body = out.items;
            }
        }
    }
    var method: std.http.Method = if (body != null) .POST else .GET;
    if (root.get("method")) |mv| if (mv.str()) |ms| {
        if (std.ascii.eqlIgnoreCase(ms, "GET")) {
            method = .GET;
        } else if (std.ascii.eqlIgnoreCase(ms, "POST")) {
            method = .POST;
        } else return reject("E_INPUT", "\"method\" must be GET or POST");
    };
    if (method == .GET and body != null) return reject("E_INPUT", "a GET request cannot have a \"body\"");

    return .{ .ok = .{ .url = url, .host = try a.dupe(u8, host), .method = method, .headers = hdrs.items, .body = body } };
}

const forwarded_response_headers = [_][]const u8{ "content-type", "content-encoding", "retry-after", "request-id", "x-request-id", "x-should-retry" };

fn forwardResponseHeader(name: []const u8) bool {
    for (forwarded_response_headers) |h| if (std.ascii.eqlIgnoreCase(h, name)) return true;
    return std.ascii.startsWithIgnoreCase(name, "x-ratelimit-") or std.ascii.startsWithIgnoreCase(name, "anthropic-ratelimit-") or std.ascii.startsWithIgnoreCase(name, "openai-");
}

pub const ForwardError = error{ Upstream, HeadSent, OutOfMemory };

/// Send `p` upstream and stream the answer to `w` (status, a few headers, then the raw body; the
/// connection is closed at the end). Errors before the response head was written come back as
/// `error.Upstream` with `detail` set; after that failures only end the stream.
pub fn forward(a: Allocator, client: *std.http.Client, p: Plan, w: *Io.Writer, detail: *[]const u8) ForwardError!void {
    return forwardInner(a, client, p, w, detail, null, undefined);
}

/// Stall and size limits of one proxied call (V-12). `first_byte_ms` covers connecting plus the provider's thinking
/// time before the response head; `idle_ms` is the longest pause between two chunks (also: a client that stopped
/// reading); `total_ms` and `max_bytes` bound the whole call.
pub const Limits = struct {
    first_byte_ms: u64 = 5 * 60_000,
    idle_ms: u64 = 2 * 60_000,
    total_ms: u64 = 30 * 60_000,
    max_bytes: usize = 256 << 20,
};

/// Progress shared between the forwarding task and the watching handler.
const Progress = struct {
    last_ns: std.atomic.Value(i64),
    bytes: std.atomic.Value(usize) = .init(0),
    head_sent: std.atomic.Value(bool) = .init(false),
    done: std.atomic.Value(bool) = .init(false),
    io: Io,
    max_bytes: usize,

    fn touch(pr: *Progress) void {
        pr.last_ns.store(@intCast(Io.Timestamp.now(pr.io, .awake).nanoseconds), .release);
    }
};

fn forwardTask(a: Allocator, client: *std.http.Client, p: Plan, w: *Io.Writer, detail: *[]const u8, res: *ForwardError!void, pr: *Progress) void {
    res.* = forwardInner(a, client, p, w, detail, pr, pr.io);
    pr.done.store(true, .release);
}

/// `forward` with timeouts: the call runs in its own task, the caller watches progress and cancels the task when the
/// upstream (or the downstream client) stalls past `lim`.
pub fn forwardLimited(io: Io, a: Allocator, client: *std.http.Client, p: Plan, w: *Io.Writer, detail: *[]const u8, lim: Limits) ForwardError!void {
    var pr: Progress = .{ .last_ns = .init(@intCast(Io.Timestamp.now(io, .awake).nanoseconds)), .io = io, .max_bytes = lim.max_bytes };
    const start_ns: i64 = pr.last_ns.load(.acquire);
    var res: ForwardError!void = {};
    var g: Io.Group = .init;
    g.concurrent(io, forwardTask, .{ a, client, p, w, detail, &res, &pr }) catch return forwardInner(a, client, p, w, detail, null, io);
    var timed_out = false;
    while (!pr.done.load(.acquire)) {
        io.sleep(Io.Duration.fromMilliseconds(50), .awake) catch {};
        const now: i64 = @intCast(Io.Timestamp.now(io, .awake).nanoseconds);
        const idle_limit = if (pr.head_sent.load(.acquire)) lim.idle_ms else lim.first_byte_ms;
        if (now - pr.last_ns.load(.acquire) > @as(i64, @intCast(idle_limit)) * std.time.ns_per_ms or now - start_ns > @as(i64, @intCast(lim.total_ms)) * std.time.ns_per_ms) {
            timed_out = true;
            break;
        }
    }
    if (timed_out) g.cancel(io) else g.await(io) catch {};
    if (timed_out and !pr.head_sent.load(.acquire)) {
        detail.* = "the upstream did not answer in time";
        return error.Upstream;
    }
    return res;
}

fn forwardInner(a: Allocator, client: *std.http.Client, p: Plan, w: *Io.Writer, detail: *[]const u8, pr: ?*Progress, io: Io) ForwardError!void {
    _ = io;
    const uri = std.Uri.parse(p.url) catch {
        detail.* = "invalid URL";
        return error.Upstream;
    };
    var user_agent_buf: [64]u8 = undefined;
    const ua = std.fmt.bufPrint(&user_agent_buf, "kerf-serve/{s}", .{kerf.version}) catch "kerf-serve";
    var req = client.request(p.method, uri, .{
        .redirect_behavior = .unhandled,
        .keep_alive = false,
        .headers = .{ .accept_encoding = .{ .override = "identity" }, .user_agent = .{ .override = ua } },
        .extra_headers = p.headers,
    }) catch |e| {
        detail.* = a.print("could not connect to {s}: {s}", .{ p.host, @errorName(e) }) catch "could not connect";
        return error.Upstream;
    };
    defer req.deinit();
    if (p.body) |b| {
        const mutable = try a.dupe(u8, b);
        req.transfer_encoding = .{ .content_length = mutable.len };
        req.sendBodyComplete(mutable) catch |e| {
            detail.* = a.print("sending the request to {s} failed: {s}", .{ p.host, @errorName(e) }) catch "send failed";
            return error.Upstream;
        };
    } else {
        req.sendBodiless() catch |e| {
            detail.* = a.print("sending the request to {s} failed: {s}", .{ p.host, @errorName(e) }) catch "send failed";
            return error.Upstream;
        };
    }
    var redirect_buf: [2048]u8 = undefined;
    var resp = req.receiveHead(&redirect_buf) catch |e| {
        detail.* = a.print("no valid response from {s}: {s}", .{ p.host, @errorName(e) }) catch "bad response";
        return error.Upstream;
    };
    var extra: std.ArrayList(u8) = .empty;
    var it = resp.head.iterateHeaders();
    while (it.next()) |h| {
        if (!forwardResponseHeader(h.name)) continue;
        if (std.ascii.eqlIgnoreCase(h.name, "content-encoding") and std.ascii.eqlIgnoreCase(h.value, "identity")) continue;
        try extra.print(a, "{s}: {s}\r\n", .{ h.name, h.value });
    }
    const status: u16 = @backingInt(resp.head.status);
    try extra.appendSlice(a, "X-Accel-Buffering: no\r\n");
    http.writeHead(w, .{ .status = status, .content_type = null, .content_length = null, .keep_alive = false, .extra = extra.items }) catch return error.HeadSent;
    w.flush() catch return error.HeadSent;
    if (pr) |x| {
        x.head_sent.store(true, .release);
        x.touch();
    }

    var transfer: [8192]u8 = undefined;
    const rd = resp.reader(&transfer);
    while (true) {
        const chunk = rd.peekGreedy(1) catch break;
        w.writeAll(chunk) catch break;
        rd.toss(chunk.len);
        w.flush() catch break;
        if (pr) |x| {
            x.touch();
            if (x.bytes.fetchAdd(chunk.len, .acq_rel) + chunk.len > x.max_bytes) break; // runaway upstream
        }
    }
}

test "plan: provider defaults and https rule" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var pe: kerf.json.ParseError = undefined;
    const v = (try kerf.json.parse(a, "{\"provider\":\"anthropic\",\"path\":\"/v1/messages\",\"headers\":{\"x-api-key\":\"k\",\"Host\":\"evil\",\"content-type\":\"application/json\"},\"body\":{\"a\":1}}", &pe)).?;
    const r = try plan(a, v);
    try std.testing.expect(r == .ok);
    try std.testing.expectEqualStrings("https://api.anthropic.com/v1/messages", r.ok.url);
    try std.testing.expectEqualStrings("api.anthropic.com", r.ok.host);
    try std.testing.expectEqual(std.http.Method.POST, r.ok.method);
    try std.testing.expectEqual(@as(usize, 2), r.ok.headers.len); // Host dropped
    try std.testing.expectEqualStrings("{\"a\":1}", r.ok.body.?);
}

fn planOf(a: Allocator, text: []const u8) !PlanResult {
    var pe: kerf.json.ParseError = undefined;
    return plan(a, (try kerf.json.parse(a, text, &pe)).?);
}

test "plan: URL rules" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    // non-custom: https + public name only
    try std.testing.expect((try planOf(a, "{\"provider\":\"openai\",\"base_url\":\"http://api.openai.com\",\"path\":\"/v1/x\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"openai\",\"base_url\":\"https://localhost:8443\",\"path\":\"/v1/x\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"openai\",\"base_url\":\"https://169.254.169.254\",\"path\":\"/latest\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"openai\",\"base_url\":\"https://2130706433\",\"path\":\"/\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"openai\",\"base_url\":\"https://myresource.openai.azure.com\",\"path\":\"/openai/x\"}")) == .ok);
    // custom: http only for local/LAN
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://localhost:11434/v1\",\"path\":\"/chat/completions\"}")) == .ok);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://192.168.1.20:8080\",\"path\":\"/v1/chat/completions\"}")) == .ok);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://[::1]:8080\",\"path\":\"/v1\"}")) == .ok);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://ollama:11434\",\"path\":\"/v1\"}")) == .ok);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://example.com\",\"path\":\"/v1\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://8.8.8.8\",\"path\":\"/v1\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"https://example.com/v1\",\"path\":\"/chat\"}")) == .ok);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"path\":\"/chat\"}")) == .rejected); // base_url required
    // schemes and tricks
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"file:///etc\",\"path\":\"/passwd\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"ftp://localhost\",\"path\":\"/x\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"custom\",\"base_url\":\"http://user:pw@localhost\",\"path\":\"/x\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"anthropic\",\"path\":\"//evil.com/x\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"anthropic\",\"path\":\"/a b\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"anthropic\",\"path\":\"v1/messages\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"nope\",\"path\":\"/x\"}")) == .rejected);
    try std.testing.expect((try planOf(a, "{\"provider\":\"anthropic\",\"path\":\"/x\",\"headers\":{\"a\":\"b\\r\\nHost: evil\"}}")) == .rejected);
    try std.testing.expect((try planOf(a, "[1]")) == .rejected);
}

test "isLocalOrLan" {
    try std.testing.expect(isLocalOrLan("localhost"));
    try std.testing.expect(isLocalOrLan("127.0.0.1"));
    try std.testing.expect(isLocalOrLan("10.1.2.3"));
    try std.testing.expect(isLocalOrLan("172.20.0.1"));
    try std.testing.expect(!isLocalOrLan("172.32.0.1"));
    try std.testing.expect(isLocalOrLan("192.168.0.9"));
    try std.testing.expect(isLocalOrLan("fd12::1"));
    try std.testing.expect(isLocalOrLan("::ffff:10.0.0.1"));
    try std.testing.expect(!isLocalOrLan("api.openai.com"));
    try std.testing.expect(!isLocalOrLan("1.1.1.1"));
    try std.testing.expect(isLocalOrLan("printer.local"));
}
