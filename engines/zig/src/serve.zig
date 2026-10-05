//! `kerf serve`: local workspace server (spec/SERVE.md). One process serves the embedded web UI and a
//! JSON/SSE API over a folder of `*.kerf.json` documents. The folder is the source of truth: a 500 ms
//! poller (mtime + size) turns CLI/agent edits into SSE events, designer edits go through `apply`
//! with optimistic concurrency (ETag / if_match) and are logged like CLI edits.
//!
//! Threads: `std.Io.Threaded` (the default `init.io`); every connection is a `Group.concurrent` task, so
//! long-lived SSE streams never block other requests.

const std = @import("std");
const builtin = @import("builtin");
const kerf = @import("kerf");
const ws = @import("workspace.zig");
const http = @import("http.zig");
const events = @import("events.zig");
const agents = @import("agents.zig");
const proxy = @import("proxy.zig");
const ui_assets = @import("ui_assets");
const Io = std.Io;
const net = std.Io.net;
const Allocator = std.mem.Allocator;

pub const default_port: u16 = 7700;
const poll_ms = 500;

pub const Config = struct {
    dir_path: []const u8 = ".",
    host: []const u8 = "127.0.0.1",
    port: u16 = default_port,
    token: ?[]const u8 = null,
    /// One extra browser origin allowed to call the API (for `vite dev` against a running server).
    allow_origin: ?[]const u8 = null,
    open: bool = false,
};

const DocInfo = struct {
    id: []u8,
    title: []u8,
    components: usize,
    views: usize,
    errors: usize,
    warnings: usize,
};

const FileState = struct {
    name: []u8,
    mtime_ns: i96,
    size: u64,
    /// Bytes of `<name>.log.jsonl` already turned into `log` events.
    log_size: u64,
    info: ?DocInfo = null,
};

pub const Server = struct {
    gpa: Allocator,
    io: Io,
    cfg: Config,
    dir: Io.Dir,
    dir_abs: []u8,
    hub: events.Hub,
    scan_mu: Io.Mutex = .init,
    files: std.ArrayList(FileState) = .empty,
    write_mu: Io.Mutex = .init,
    client: std.http.Client,
    agent_mgr: agents.Manager,
    group: Io.Group = .init,
    loopback_only: bool,
    started_ns: i96,

    fn freeInfo(s: *Server, info: DocInfo) void {
        s.gpa.free(info.id);
        s.gpa.free(info.title);
    }

    fn findState(s: *Server, name: []const u8) ?*FileState {
        for (s.files.items) |*f| if (std.mem.eql(u8, f.name, name)) return f;
        return null;
    }

    fn cors(s: *Server, a: Allocator, req: http.Request) []const u8 {
        const ao = s.cfg.allow_origin orelse return "";
        const origin = req.header("origin") orelse return "";
        if (!std.mem.eql(u8, origin, ao)) return "";
        return std.fmt.allocPrint(a, "Access-Control-Allow-Origin: {s}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: authorization, content-type, if-match\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Expose-Headers: etag\r\n", .{origin}) catch "";
    }

    // ---- folder scan: the 500 ms poller and every listing call share this ----

    /// Scan the folder; publish doc_added / doc_changed / doc_removed / log events for what changed
    /// since the last scan (nothing on the very first scan: `emit` false).
    pub fn scan(s: *Server, emit: bool) void {
        s.scan_mu.lockUncancelable(s.io);
        defer s.scan_mu.unlock(s.io);
        s.scanLocked(emit);
    }

    const NewLog = struct { name: []const u8, lines: []const []const u8 };

    fn scanLocked(s: *Server, emit: bool) void {
        const io = s.io;
        var arena = std.heap.ArenaAllocator.init(s.gpa);
        defer arena.deinit();
        const a = arena.allocator();

        var names: std.ArrayList([]const u8) = .empty;
        var it = s.dir.iterate();
        while (it.next(io) catch null) |e| {
            if (e.kind != .file and e.kind != .sym_link) continue;
            if (!ws.validDocFile(e.name)) continue;
            names.append(a, a.dupe(u8, e.name) catch continue) catch continue;
        }
        std.mem.sort([]const u8, names.items, {}, lessStr);

        var added: std.ArrayList([]const u8) = .empty;
        var changed: std.ArrayList([]const u8) = .empty;
        for (names.items) |name| {
            const st = s.dir.statFile(io, name, .{}) catch continue;
            if (s.findState(name)) |f| {
                if (f.mtime_ns != st.mtime.nanoseconds or f.size != st.size) {
                    f.mtime_ns = st.mtime.nanoseconds;
                    f.size = st.size;
                    if (f.info) |old| s.freeInfo(old);
                    f.info = null;
                    changed.append(a, name) catch {};
                }
            } else {
                const dup = s.gpa.dupe(u8, name) catch continue;
                var log_size: u64 = 0;
                if (!emit) log_size = s.logSize(a, name);
                s.files.append(s.gpa, .{ .name = dup, .mtime_ns = st.mtime.nanoseconds, .size = st.size, .log_size = log_size }) catch {
                    s.gpa.free(dup);
                    continue;
                };
                added.append(a, name) catch {};
            }
        }
        var removed: std.ArrayList([]const u8) = .empty;
        var i: usize = 0;
        while (i < s.files.items.len) {
            const f = s.files.items[i];
            var present = false;
            for (names.items) |n| if (std.mem.eql(u8, n, f.name)) {
                present = true;
            };
            if (present) {
                i += 1;
                continue;
            }
            removed.append(a, a.dupe(u8, f.name) catch "") catch {};
            if (f.info) |old| s.freeInfo(old);
            s.gpa.free(f.name);
            _ = s.files.orderedRemove(i);
        }

        // New log lines per document (before emitting, so doc_changed can carry `who`).
        var logs: std.ArrayList(NewLog) = .empty;
        for (s.files.items) |*f| {
            const lines = s.readNewLog(a, f) catch continue;
            if (lines.len > 0) logs.append(a, .{ .name = f.name, .lines = lines }) catch {};
        }

        if (!emit) return;
        for (added.items) |n| s.publishDoc("doc_added", a, n, null);
        for (changed.items) |n| {
            var who: ?[]const u8 = null;
            for (logs.items) |l| if (std.mem.eql(u8, l.name, n)) {
                who = whoOf(a, l.lines[l.lines.len - 1]);
            };
            s.publishDoc("doc_changed", a, n, who);
        }
        for (removed.items) |n| {
            var out: std.ArrayList(u8) = .empty;
            out.appendSlice(a, "{\"file\":") catch continue;
            http.jsonString(&out, a, n) catch continue;
            out.append(a, '}') catch continue;
            s.hub.publish(io, "doc_removed", out.items);
        }
        for (logs.items) |l| for (l.lines) |line| s.publishLog(a, l.name, line);
    }

    fn lessStr(_: void, x: []const u8, y: []const u8) bool {
        return std.mem.lessThan(u8, x, y);
    }

    fn whoOf(a: Allocator, line: []const u8) ?[]const u8 {
        var pe: kerf.json.ParseError = undefined;
        const v = (kerf.json.parse(a, line, &pe) catch return null) orelse return null;
        return if (v.get("who")) |w| w.str() else null;
    }

    fn publishDoc(s: *Server, kind: []const u8, a: Allocator, name: []const u8, who: ?[]const u8) void {
        const f = s.findState(name) orelse return;
        var out: std.ArrayList(u8) = .empty;
        out.appendSlice(a, "{\"file\":") catch return;
        http.jsonString(&out, a, name) catch return;
        out.print(a, ",\"mtime_ms\":{d}", .{@divTrunc(f.mtime_ns, std.time.ns_per_ms)}) catch return;
        if (who) |w| {
            out.appendSlice(a, ",\"who\":") catch return;
            http.jsonString(&out, a, w) catch return;
        }
        out.append(a, '}') catch return;
        s.hub.publish(s.io, kind, out.items);
    }

    fn publishLog(s: *Server, a: Allocator, name: []const u8, line: []const u8) void {
        var out: std.ArrayList(u8) = .empty;
        out.appendSlice(a, "{\"file\":") catch return;
        http.jsonString(&out, a, name) catch return;
        out.appendSlice(a, ",\"entry\":") catch return;
        out.appendSlice(a, line) catch return;
        out.append(a, '}') catch return;
        s.hub.publish(s.io, "log", out.items);
    }

    fn logSize(s: *Server, a: Allocator, name: []const u8) u64 {
        const lp = ws.logPath(a, name) catch return 0;
        const st = s.dir.statFile(s.io, lp, .{}) catch return 0;
        return st.size;
    }

    /// Complete, valid-JSON lines appended to the log since `f.log_size`; advances `f.log_size`.
    fn readNewLog(s: *Server, a: Allocator, f: *FileState) ![]const []const u8 {
        const lp = try ws.logPath(a, f.name);
        const st = s.dir.statFile(s.io, lp, .{}) catch {
            f.log_size = 0;
            return &.{};
        };
        if (st.size < f.log_size) {
            f.log_size = st.size;
            return &.{};
        }
        if (st.size == f.log_size) return &.{};
        const want: usize = @intCast(@min(st.size - f.log_size, 4 << 20));
        var file = try s.dir.openFile(s.io, lp, .{});
        defer file.close(s.io);
        const buf = try a.alloc(u8, want);
        const got = try file.readPositionalAll(s.io, buf, f.log_size);
        const data = buf[0..got];
        const last_nl = std.mem.lastIndexOfScalar(u8, data, '\n') orelse return &.{};
        f.log_size += last_nl + 1;
        var out: std.ArrayList([]const u8) = .empty;
        var lines = std.mem.splitScalar(u8, data[0..last_nl], '\n');
        while (lines.next()) |raw| {
            const line = std.mem.trim(u8, raw, " \t\r");
            if (line.len == 0) continue;
            var pe: kerf.json.ParseError = undefined;
            const ok = (kerf.json.parse(a, line, &pe) catch null) != null;
            if (ok) try out.append(a, line);
        }
        // A burst (for example a doc dropped in next to a long old log): keep the newest 200.
        if (out.items.len > 200) return out.items[out.items.len - 200 ..];
        return out.items;
    }

    fn ensureInfo(s: *Server, a: Allocator, f: *FileState) ?DocInfo {
        if (f.info) |i| return i;
        const rd = s.readDoc(a, f.name) catch return null;
        const info = computeInfo(s.gpa, a, rd.bytes) catch return null;
        f.info = info;
        return info;
    }

    // ---- documents ----

    const DocRead = struct { bytes: []u8, mtime_ns: i96, size: u64 };

    fn readDoc(s: *Server, a: Allocator, name: []const u8) !DocRead {
        var f = try s.dir.openFile(s.io, name, .{});
        defer f.close(s.io);
        const st = try f.stat(s.io);
        var rb: [8192]u8 = undefined;
        var fr = f.reader(s.io, &rb);
        const bytes = try fr.interface.allocRemaining(a, .limited(256 << 20));
        return .{ .bytes = bytes, .mtime_ns = st.mtime.nanoseconds, .size = st.size };
    }

    /// After the server itself wrote `name` (document and optionally one log line): refresh the scan
    /// state and publish the events with `who`. Must be called with `scan_mu` held.
    fn noteWriteLocked(s: *Server, a: Allocator, name: []const u8, is_new: bool, who: []const u8, log_line: ?[]const u8) void {
        const st = s.dir.statFile(s.io, name, .{}) catch return;
        var f: *FileState = undefined;
        if (s.findState(name)) |ex| {
            f = ex;
            f.mtime_ns = st.mtime.nanoseconds;
            f.size = st.size;
            if (f.info) |old| s.freeInfo(old);
            f.info = null;
        } else {
            const dup = s.gpa.dupe(u8, name) catch return;
            s.files.append(s.gpa, .{ .name = dup, .mtime_ns = st.mtime.nanoseconds, .size = st.size, .log_size = 0 }) catch {
                s.gpa.free(dup);
                return;
            };
            f = &s.files.items[s.files.items.len - 1];
        }
        // Our own log append is consumed here (the poller must not report it a second time).
        if (log_line) |_| f.log_size = s.logSize(a, name);
        s.publishDoc(if (is_new) "doc_added" else "doc_changed", a, name, who);
        if (log_line) |line| s.publishLog(a, name, line);
    }

    fn etagOf(a: Allocator, mtime_ns: i96, size: u64) ![]u8 {
        return std.fmt.allocPrint(a, "\"{d}-{d}\"", .{ @divTrunc(mtime_ns, std.time.ns_per_ms), size });
    }
};

fn computeInfo(gpa: Allocator, a: Allocator, text: []const u8) !DocInfo {
    var pe: kerf.json.ParseError = undefined;
    const parsed = (try kerf.json.parse(a, text, &pe)) orelse
        return .{ .id = try gpa.dupe(u8, ""), .title = try gpa.dupe(u8, "(invalid JSON)"), .components = 0, .views = 0, .errors = 1, .warnings = 0 };
    const id = if (parsed.get("id")) |v| (v.str() orelse "") else "";
    const title = if (parsed.get("title")) |v| (v.str() orelse "") else "";
    const comps = if (parsed.get("components")) |v| (if (v.arr()) |x| x.len else 0) else 0;
    const views = if (parsed.get("views")) |v| (if (v.arr()) |x| x.len else 0) else 0;
    var errors: usize = 0;
    var warnings: usize = 0;
    const input = try std.fmt.allocPrint(a, "{{\"doc\":{s}}}", .{std.mem.trim(u8, text, " \t\r\n")});
    const r = try kerf.call(a, "check", input);
    if (r.ok) {
        if (try kerf.json.parse(a, r.bytes, &pe)) |res| if (res.get("diagnostics")) |dl| if (dl.arr()) |items| for (items) |d| {
            const lv = if (d.get("level")) |l| (l.str() orelse "") else "";
            if (std.mem.eql(u8, lv, "error")) errors += 1 else if (std.mem.eql(u8, lv, "warning")) warnings += 1;
        };
    } else errors += 1;
    return .{ .id = try gpa.dupe(u8, id), .title = try gpa.dupe(u8, title), .components = comps, .views = views, .errors = errors, .warnings = warnings };
}

// ---------------------------------------------------------------------------------------------
// HTTP: connection loop and routing
// ---------------------------------------------------------------------------------------------

fn serveConn(s: *Server, stream: net.Stream) void {
    const io = s.io;
    defer stream.close(io);
    var rbuf: [http.max_head]u8 = undefined;
    var wbuf: [16 * 1024]u8 = undefined;
    var sr = stream.reader(io, &rbuf);
    var sw = stream.writer(io, &wbuf);
    const w = &sw.interface;
    while (true) {
        var arena = std.heap.ArenaAllocator.init(s.gpa);
        defer arena.deinit();
        const a = arena.allocator();
        var req = http.readHead(a, &sr.interface) catch |e| {
            switch (e) {
                error.HeadTooLarge => http.sendError(a, w, 431, false, "", "E_HEAD", "request headers too large") catch {},
                error.Malformed => http.sendError(a, w, 400, false, "", "E_HTTP", "malformed HTTP request") catch {},
                error.Unsupported => http.sendError(a, w, 501, false, "", "E_HTTP", "chunked request bodies are not supported; send Content-Length") catch {},
                else => {},
            }
            return;
        };
        http.readBody(a, &sr.interface, w, &req) catch |e| {
            switch (e) {
                error.TooLarge => http.sendError(a, w, 413, false, "", "E_TOO_LARGE", "request body over 64 MiB") catch {},
                else => {},
            }
            return;
        };
        const keep = dispatch(s, a, req, w) catch |e| blk: {
            http.sendError(a, w, 500, false, "", "E_INTERNAL", @errorName(e)) catch {};
            break :blk false;
        };
        w.flush() catch return;
        if (!keep or !req.keep_alive) return;
    }
}

const Reject = struct { status: u16, code: []const u8, message: []const u8 };

fn hostIsLoopbackName(host_header: []const u8) bool {
    var h = host_header;
    if (std.mem.startsWith(u8, h, "[")) {
        const close = std.mem.indexOfScalar(u8, h, ']') orelse return false;
        h = h[1..close];
    } else if (std.mem.lastIndexOfScalar(u8, h, ':')) |c| {
        h = h[0..c];
    }
    return std.ascii.eqlIgnoreCase(h, "localhost") or std.ascii.endsWithIgnoreCase(h, ".localhost") or std.mem.eql(u8, h, "127.0.0.1") or std.mem.eql(u8, h, "::1");
}

fn originMatchesHost(origin: []const u8, host: []const u8) bool {
    const sep = std.mem.indexOf(u8, origin, "://") orelse return false;
    return std.ascii.eqlIgnoreCase(origin[sep + 3 ..], host);
}

/// Token, Host and Origin checks for /api. Returns the rejection or null when allowed.
fn checkAccess(s: *Server, a: Allocator, req: http.Request) ?Reject {
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

fn dispatch(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer) !bool {
    const path = try http.percentDecode(a, req.path, false);
    if (std.mem.eql(u8, path, "/api") or std.mem.startsWith(u8, path, "/api/")) return api(s, a, req, w, path);
    if (std.mem.eql(u8, req.method, "GET") or std.mem.eql(u8, req.method, "HEAD")) return static(s, a, req, w, path);
    try http.sendError(a, w, 405, req.keep_alive, "", "E_METHOD", "method not allowed");
    return req.keep_alive;
}

fn api(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, path: []const u8) !bool {
    const extra = s.cors(a, req);
    const ka = req.keep_alive;
    if (std.mem.eql(u8, req.method, "OPTIONS")) {
        try http.send(w, .{ .status = 204, .keep_alive = ka, .extra = extra }, "");
        return ka;
    }
    const is_info = std.mem.eql(u8, path, "/api/info");
    if (checkAccess(s, a, req)) |rej| {
        if (is_info and rej.status == 401) {
            // Let the UI discover that a token is needed.
            const body = try std.fmt.allocPrint(a, "{{\"version\":\"{s}\",\"token_required\":true,\"authenticated\":false}}\n", .{kerf.version});
            try http.sendJson(w, 200, ka, extra, body);
            return ka;
        }
        try http.sendError(a, w, rej.status, ka, extra, rej.code, rej.message);
        return ka;
    }
    const is_get = std.mem.eql(u8, req.method, "GET");
    const is_post = std.mem.eql(u8, req.method, "POST");
    const rest = path["/api".len..]; // "" or "/..."

    if (is_info) {
        if (!is_get) return methodNotAllowed(s, a, req, w, extra);
        return apiInfo(s, a, req, w, extra);
    }
    if (std.mem.eql(u8, rest, "/docs")) {
        if (is_get) return apiList(s, a, req, w, extra);
        if (is_post) return apiCreate(s, a, req, w, extra);
        return methodNotAllowed(s, a, req, w, extra);
    }
    if (std.mem.startsWith(u8, rest, "/docs/")) {
        const tail = rest["/docs/".len..];
        var parts = std.mem.splitScalar(u8, tail, '/');
        const file = parts.next().?;
        const action = parts.next();
        if (parts.next() != null) return notFound(s, a, req, w, extra);
        if (!ws.validDocFile(file)) {
            try http.sendError(a, w, 400, ka, extra, "E_FILE", "file must be a plain NAME.kerf.json (no directories)");
            return ka;
        }
        if (action == null) {
            if (!is_get) return methodNotAllowed(s, a, req, w, extra);
            return apiGetDoc(s, a, req, w, extra, file);
        }
        if (std.mem.eql(u8, action.?, "apply")) {
            if (!is_post) return methodNotAllowed(s, a, req, w, extra);
            return apiApply(s, a, req, w, extra, file);
        }
        if (std.mem.eql(u8, action.?, "log")) {
            if (!is_get) return methodNotAllowed(s, a, req, w, extra);
            return apiLog(s, a, req, w, extra, file);
        }
        if (std.mem.eql(u8, action.?, "export")) {
            if (!is_get) return methodNotAllowed(s, a, req, w, extra);
            return apiExport(s, a, req, w, extra, file);
        }
        return notFound(s, a, req, w, extra);
    }
    if (std.mem.eql(u8, rest, "/events")) {
        if (!is_get) return methodNotAllowed(s, a, req, w, extra);
        return apiEvents(s, a, w, extra);
    }
    if (std.mem.eql(u8, rest, "/llm")) {
        if (!is_post) return methodNotAllowed(s, a, req, w, extra);
        return apiLlm(s, a, req, w, extra);
    }
    if (std.mem.eql(u8, rest, "/agent/run")) {
        if (!is_post) return methodNotAllowed(s, a, req, w, extra);
        return apiAgentRun(s, a, req, w, extra);
    }
    if (std.mem.eql(u8, rest, "/agent/stop")) {
        if (!is_post) return methodNotAllowed(s, a, req, w, extra);
        return apiAgentStop(s, a, req, w, extra);
    }
    return notFound(s, a, req, w, extra);
}

fn methodNotAllowed(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    _ = s;
    try http.sendError(a, w, 405, req.keep_alive, extra, "E_METHOD", "method not allowed for this path");
    return req.keep_alive;
}

fn notFound(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    _ = s;
    try http.sendError(a, w, 404, req.keep_alive, extra, "E_NOT_FOUND", "no such endpoint");
    return req.keep_alive;
}

fn badRequest(a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, code: []const u8, msg: []const u8) !bool {
    try http.sendError(a, w, 400, req.keep_alive, extra, code, msg);
    return req.keep_alive;
}

fn parseBody(a: Allocator, req: http.Request) ?kerf.json.Value {
    var pe: kerf.json.ParseError = undefined;
    return (kerf.json.parse(a, req.body, &pe) catch return null) orelse null;
}

// ---- /api/info ----

fn apiInfo(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    var out: std.ArrayList(u8) = .empty;
    try out.print(a, "{{\"version\":\"{s}\",\"dir\":", .{kerf.version});
    try http.jsonString(&out, a, s.dir_abs);
    try out.print(a, ",\"token_required\":{s},\"authenticated\":true,\"proxy\":true,\"agents\":[", .{if (s.cfg.token != null) "true" else "false"});
    const templates = try agents.loadTemplates(a, s.io, s.dir);
    for (templates, 0..) |t, i| {
        if (i > 0) try out.append(a, ',');
        const d = s.agent_mgr.detectCached(s.io, t);
        try out.appendSlice(a, "{\"id\":");
        try http.jsonString(&out, a, t.id);
        try out.appendSlice(a, ",\"name\":");
        try http.jsonString(&out, a, t.name);
        try out.print(a, ",\"available\":{s}", .{if (d.available) "true" else "false"});
        if (d.version) |v| {
            try out.appendSlice(a, ",\"version\":");
            try http.jsonString(&out, a, v);
        }
        if (d.reason) |r| {
            try out.appendSlice(a, ",\"reason\":");
            try http.jsonString(&out, a, r);
        }
        try out.append(a, '}');
    }
    try out.appendSlice(a, "],\"active_run\":");
    if (s.agent_mgr.activeInfo(s.io, a)) |ar| {
        try out.appendSlice(a, "{\"run_id\":");
        try http.jsonString(&out, a, ar.run_id);
        try out.appendSlice(a, ",\"agent\":");
        try http.jsonString(&out, a, ar.agent);
        try out.append(a, '}');
    } else try out.appendSlice(a, "null");
    try out.appendSlice(a, "}\n");
    try http.sendJson(w, 200, req.keep_alive, extra, out.items);
    return req.keep_alive;
}

// ---- documents ----

fn apiList(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    s.scan_mu.lockUncancelable(s.io);
    defer s.scan_mu.unlock(s.io);
    s.scanLocked(true);
    var out: std.ArrayList(u8) = .empty;
    try out.append(a, '[');
    var first = true;
    for (s.files.items) |*f| {
        const info = s.ensureInfo(a, f) orelse continue;
        if (!first) try out.append(a, ',');
        first = false;
        try out.appendSlice(a, "{\"file\":");
        try http.jsonString(&out, a, f.name);
        try out.appendSlice(a, ",\"id\":");
        try http.jsonString(&out, a, info.id);
        try out.appendSlice(a, ",\"title\":");
        try http.jsonString(&out, a, info.title);
        try out.print(a, ",\"mtime_ms\":{d},\"size\":{d},\"components\":{d},\"views\":{d},\"errors\":{d},\"warnings\":{d}}}", .{
            @divTrunc(f.mtime_ns, std.time.ns_per_ms), f.size, info.components, info.views, info.errors, info.warnings,
        });
    }
    try out.appendSlice(a, "]\n");
    try http.sendJson(w, 200, req.keep_alive, extra, out.items);
    return req.keep_alive;
}

fn apiGetDoc(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const rd = s.readDoc(a, file) catch |e| switch (e) {
        error.FileNotFound => {
            try http.sendError(a, w, 404, req.keep_alive, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
            return req.keep_alive;
        },
        else => return e,
    };
    const etag = try Server.etagOf(a, rd.mtime_ns, rd.size);
    const hdr = try std.fmt.allocPrint(a, "ETag: {s}\r\n{s}", .{ etag, extra });
    if (req.header("if-none-match")) |inm| if (std.mem.eql(u8, inm, etag)) {
        try http.send(w, .{ .status = 304, .keep_alive = req.keep_alive, .extra = hdr }, "");
        return req.keep_alive;
    };
    try http.sendJson(w, 200, req.keep_alive, hdr, rd.bytes);
    return req.keep_alive;
}

fn apiCreate(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { file, title?, id? }");
    const file_in = (if (body.get("file")) |v| v.str() else null) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"file\" (for example \"truss-cmu.kerf.json\")");
    const file = if (std.mem.endsWith(u8, file_in, ws.doc_suffix)) file_in else try std.mem.concat(a, u8, &.{ file_in, ws.doc_suffix });
    if (!ws.validDocFile(file)) return badRequest(a, req, w, extra, "E_FILE", "file must be a plain NAME.kerf.json: letters, digits, . _ - space ( ), no directories");
    const title = if (body.get("title")) |v| (v.str() orelse "") else "";
    const id_src = if (body.get("id")) |v| (v.str() orelse ws.docStem(file)) else ws.docStem(file);
    const text = try ws.newDocText(a, id_src, title);

    s.write_mu.lockUncancelable(s.io);
    defer s.write_mu.unlock(s.io);
    s.scan_mu.lockUncancelable(s.io);
    defer s.scan_mu.unlock(s.io);
    s.scanLocked(true); // report anything that appeared since the last tick before we add ours
    var af = s.dir.createFileAtomic(s.io, file, .{}) catch |e| return e;
    defer af.deinit(s.io);
    af.file.writeStreamingAll(s.io, text) catch |e| return e;
    af.link(s.io) catch |e| switch (e) {
        error.PathAlreadyExists => {
            try http.sendError(a, w, 409, req.keep_alive, extra, "E_EXISTS", try std.fmt.allocPrint(a, "{s} already exists", .{file}));
            return req.keep_alive;
        },
        else => return e,
    };
    const summary = ws.checkSummary(a, text) catch "";
    const created_ops = try std.fmt.allocPrint(a, "[{{\"op\":\"create\",\"file\":\"{s}\"}}]", .{file});
    const line = try ws.buildEntry(a, s.io, .{ .who = "designer", .tool = "kerf-serve", .why = "create" }, created_ops, &.{}, summary);
    const lp = try ws.logPath(a, file);
    ws.appendLine(s.io, s.dir, lp, line) catch {};
    s.noteWriteLocked(a, file, true, "designer", line);
    const st = s.dir.statFile(s.io, file, .{}) catch return error.StatFailed;
    const etag = try Server.etagOf(a, st.mtime.nanoseconds, st.size);
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"file\":");
    try http.jsonString(&out, a, file);
    try out.appendSlice(a, ",\"etag\":");
    try http.jsonString(&out, a, etag);
    try out.appendSlice(a, ",\"summary\":");
    try http.jsonString(&out, a, summary);
    try out.appendSlice(a, "}\n");
    const hdr = try std.fmt.allocPrint(a, "ETag: {s}\r\nLocation: /api/docs/{s}\r\n{s}", .{ etag, file, extra });
    try http.sendJson(w, 201, req.keep_alive, hdr, out.items);
    return req.keep_alive;
}

fn apiApply(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const ka = req.keep_alive;
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { ops, why, actor, if_match? }");
    if (body != .object) return badRequest(a, req, w, extra, "E_INPUT", "body must be a JSON object");
    const ops_v = body.get("ops") orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"ops\": an array of ops (see SPEC 14)");
    const why = if (body.get("why")) |v| (v.str() orelse "") else "";
    var who: []const u8 = "designer";
    if (body.get("actor")) |v| if (v.str()) |ac| {
        var ok = ac.len > 0 and ac.len <= 32;
        for (ac) |c| if (!(std.ascii.isAlphanumeric(c) or c == '-' or c == '_')) {
            ok = false;
        };
        if (ok) who = ac;
    };
    const if_match: ?[]const u8 = if (body.get("if_match")) |v| v.str() else null;

    s.write_mu.lockUncancelable(s.io);
    defer s.write_mu.unlock(s.io);
    const rd = s.readDoc(a, file) catch |e| switch (e) {
        error.FileNotFound => {
            try http.sendError(a, w, 404, ka, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
            return ka;
        },
        else => return e,
    };
    const etag = try Server.etagOf(a, rd.mtime_ns, rd.size);
    if (if_match) |im| {
        const want = std.mem.trim(u8, im, " ");
        const cur = std.mem.trim(u8, etag, "\"");
        if (!std.mem.eql(u8, std.mem.trim(u8, want, "\""), cur)) {
            const msg = try std.fmt.allocPrint(a, "{s} was changed by someone else (current ETag {s}); reload it and re-apply", .{ file, etag });
            var out: std.ArrayList(u8) = .empty;
            try out.appendSlice(a, "{\"error\":{\"code\":\"E_CONFLICT\",\"message\":");
            try http.jsonString(&out, a, msg);
            try out.appendSlice(a, "},\"etag\":");
            try http.jsonString(&out, a, etag);
            try out.appendSlice(a, "}\n");
            const hdr = try std.fmt.allocPrint(a, "ETag: {s}\r\n{s}", .{ etag, extra });
            try http.sendJson(w, 409, ka, hdr, out.items);
            return ka;
        }
    }
    var ops_text: std.ArrayList(u8) = .empty;
    try kerf.json.writeCompact(&ops_text, a, ops_v);
    const input = try std.fmt.allocPrint(a, "{{\"doc\":{s},\"ops\":{s},\"actor\":\"designer\"}}", .{ std.mem.trim(u8, rd.bytes, " \t\r\n"), ops_text.items });
    const r = try kerf.call(a, "apply", input);
    if (!r.ok) {
        try http.sendJson(w, 400, ka, extra, r.bytes);
        return ka;
    }
    var pe: kerf.json.ParseError = undefined;
    const res = (try kerf.json.parse(a, r.bytes, &pe)) orelse return error.BadEngineOutput;
    const ok = if (res.get("ok")) |v| (v == .bool and v.bool) else false;
    var final_etag = etag;
    if (ok) {
        const dv = res.get("doc") orelse return error.BadEngineOutput;
        const text = try kerf.canon.write(a, dv);
        var changed: std.ArrayList([]const u8) = .empty;
        if (res.get("changed")) |cv| if (cv.arr()) |items| for (items) |it| if (it.str()) |cs| try changed.append(a, cs);
        const summary = if (res.get("summary")) |sv| (sv.str() orelse "") else "";
        const line = try ws.buildEntry(a, s.io, .{ .who = who, .tool = "kerf-serve", .why = why }, ops_text.items, changed.items, summary);
        const lp = try ws.logPath(a, file);
        s.scan_mu.lockUncancelable(s.io);
        defer s.scan_mu.unlock(s.io);
        ws.writeFileAtomic(s.io, s.dir, file, text) catch |e| {
            try http.sendError(a, w, 500, ka, extra, "E_WRITE", try std.fmt.allocPrint(a, "could not write {s}: {s}", .{ file, @errorName(e) }));
            return ka;
        };
        ws.appendLine(s.io, s.dir, lp, line) catch {};
        s.noteWriteLocked(a, file, false, who, line);
        if (s.dir.statFile(s.io, file, .{})) |st| final_etag = try Server.etagOf(a, st.mtime.nanoseconds, st.size) else |_| {}
    }
    // engine output + the new etag (the engine JSON object ends with "}\n")
    const trimmed = std.mem.trimEnd(u8, r.bytes, " \r\n");
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, trimmed[0 .. trimmed.len - 1]);
    try out.appendSlice(a, ",\"etag\":");
    try http.jsonString(&out, a, final_etag);
    try out.appendSlice(a, "}\n");
    const hdr = try std.fmt.allocPrint(a, "ETag: {s}\r\n{s}", .{ final_etag, extra });
    try http.sendJson(w, 200, ka, hdr, out.items);
    return ka;
}

fn apiLog(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const since: usize = if (try req.param(a, "since")) |sv| (std.fmt.parseInt(usize, sv, 10) catch return badRequest(a, req, w, extra, "E_INPUT", "since must be a line index (integer)")) else 0;
    if (s.dir.statFile(s.io, file, .{})) |_| {} else |_| {
        try http.sendError(a, w, 404, req.keep_alive, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
        return req.keep_alive;
    }
    const lp = try ws.logPath(a, file);
    const data: []u8 = s.dir.readFileAlloc(s.io, lp, a, .limited(64 << 20)) catch &.{};
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"entries\":[");
    var idx: usize = 0;
    var emitted: usize = 0;
    var it = std.mem.splitScalar(u8, data, '\n');
    while (it.next()) |raw| {
        const line = std.mem.trim(u8, raw, " \t\r");
        if (line.len == 0) continue;
        defer idx += 1;
        if (idx < since) continue;
        if (emitted > 0) try out.append(a, ',');
        emitted += 1;
        var pe: kerf.json.ParseError = undefined;
        if ((kerf.json.parse(a, line, &pe) catch null) != null) {
            try out.appendSlice(a, line);
        } else {
            try out.appendSlice(a, "{\"raw\":");
            try http.jsonString(&out, a, line);
            try out.append(a, '}');
        }
    }
    try out.print(a, "],\"next\":{d}}}\n", .{idx});
    try http.sendJson(w, 200, req.keep_alive, extra, out.items);
    return req.keep_alive;
}

fn apiExport(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const ka = req.keep_alive;
    const view = (try req.param(a, "view")) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing ?view=<view id>, for example view=A");
    const format = (try req.param(a, "format")) orelse try a.dupe(u8, "png");
    const Fmt = struct { name: []const u8, mime: []const u8, ext: []const u8 };
    const fmts = [_]Fmt{
        .{ .name = "png", .mime = "image/png", .ext = "png" },
        .{ .name = "svg", .mime = "image/svg+xml", .ext = "svg" },
        .{ .name = "dxf", .mime = "application/dxf", .ext = "dxf" },
        .{ .name = "pdf", .mime = "application/pdf", .ext = "pdf" },
    };
    var fmt: ?Fmt = null;
    for (fmts) |f| if (std.mem.eql(u8, f.name, format)) {
        fmt = f;
    };
    const f = fmt orelse return badRequest(a, req, w, extra, "E_INPUT", "format must be png, svg, dxf or pdf");
    const sheet_p = try req.param(a, "sheet");
    const sheet = if (sheet_p) |v| (std.mem.eql(u8, v, "1") or std.mem.eql(u8, v, "true")) else false;
    var input: std.ArrayList(u8) = .empty;
    const rd = s.readDoc(a, file) catch |e| switch (e) {
        error.FileNotFound => {
            try http.sendError(a, w, 404, ka, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
            return ka;
        },
        else => return e,
    };
    try input.appendSlice(a, "{\"doc\":");
    try input.appendSlice(a, std.mem.trim(u8, rd.bytes, " \t\r\n"));
    try input.appendSlice(a, ",\"view\":");
    try http.jsonString(&input, a, view);
    try input.print(a, ",\"format\":\"{s}\",\"sheet\":{s}", .{ f.name, if (sheet) "true" else "false" });
    if (try req.param(a, "px")) |px| {
        _ = std.fmt.parseInt(u32, px, 10) catch return badRequest(a, req, w, extra, "E_INPUT", "px must be an integer (output width in pixels)");
        try input.print(a, ",\"px\":{s}", .{px});
    }
    try input.append(a, '}');
    const r = try kerf.call(a, "export", input.items);
    if (!r.ok) {
        const status: u16 = if (std.mem.indexOf(u8, r.bytes, "E_VIEW") != null) 404 else 400;
        try http.sendJson(w, status, ka, extra, r.bytes);
        return ka;
    }
    var safe_view: std.ArrayList(u8) = .empty;
    for (view) |c| try safe_view.append(a, if (std.ascii.isAlphanumeric(c) or c == '-' or c == '_') c else '_');
    const disp = if (try req.param(a, "inline")) |_| "inline" else "attachment";
    const hdr = try std.fmt.allocPrint(a, "Content-Disposition: {s}; filename=\"{s}-{s}.{s}\"\r\n{s}", .{ disp, ws.docStem(file), safe_view.items, f.ext, extra });
    try http.send(w, .{ .status = 200, .content_type = f.mime, .keep_alive = ka, .extra = hdr }, r.bytes);
    return ka;
}

// ---- SSE ----

fn apiEvents(s: *Server, a: Allocator, w: *Io.Writer, extra: []const u8) !bool {
    const hdr = try std.fmt.allocPrint(a, "X-Accel-Buffering: no\r\n{s}", .{extra});
    try http.writeHead(w, .{ .status = 200, .content_type = "text/event-stream; charset=utf-8", .content_length = null, .keep_alive = false, .extra = hdr });
    try w.writeAll("retry: 2000\n\nevent: ping\ndata: {}\n\n");
    try w.flush();
    var last = s.hub.current(s.io);
    var buf: std.ArrayList(u8) = .empty;
    defer buf.deinit(s.gpa);
    while (true) {
        buf.clearRetainingCapacity();
        if (!s.hub.wait(s.io, &last, &buf, s.gpa)) return false;
        w.writeAll(buf.items) catch return false;
        w.flush() catch return false;
    }
}

// ---- /api/llm ----

fn apiLlm(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { provider, base_url?, path, headers, body }");
    const planned = try proxy.plan(a, body);
    switch (planned) {
        .rejected => |rej| {
            try http.sendError(a, w, rej.status, req.keep_alive, extra, rej.code, rej.message);
            return req.keep_alive;
        },
        .ok => |p| {
            var detail: []const u8 = "upstream error";
            proxy.forward(a, &s.client, p, w, &detail) catch |e| switch (e) {
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

// ---- agents ----

fn apiAgentRun(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const ka = req.keep_alive;
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { agent, message, session_id?, file? }");
    const agent_id = (if (body.get("agent")) |v| v.str() else null) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"agent\" (claude, grok, codex, or an id from .kerf/agents.json)");
    const message = (if (body.get("message")) |v| v.str() else null) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"message\"");
    if (std.mem.trim(u8, message, " \t\r\n").len == 0 or message.len > 200_000) return badRequest(a, req, w, extra, "E_INPUT", "\"message\" must be 1..200000 characters");
    var session: ?[]const u8 = null;
    if (body.get("session_id")) |v| if (v.str()) |sid| if (sid.len > 0) {
        var ok = sid.len <= 200 and sid[0] != '-';
        for (sid) |c| if (!(std.ascii.isAlphanumeric(c) or c == '-' or c == '_' or c == '.' or c == ':')) {
            ok = false;
        };
        if (!ok) return badRequest(a, req, w, extra, "E_INPUT", "invalid session_id");
        session = sid;
    };
    var file: ?[]const u8 = null;
    if (body.get("file")) |v| if (v.str()) |f| if (f.len > 0) {
        if (!ws.validDocFile(f)) return badRequest(a, req, w, extra, "E_FILE", "file must be a plain NAME.kerf.json");
        file = f;
    };
    const templates = try agents.loadTemplates(a, s.io, s.dir);
    var tmpl: ?agents.Template = null;
    for (templates) |t| if (std.mem.eql(u8, t.id, agent_id)) {
        tmpl = t;
    };
    const t = tmpl orelse {
        try http.sendError(a, w, 404, ka, extra, "E_AGENT", try std.fmt.allocPrint(a, "unknown agent '{s}'; GET /api/info lists them", .{agent_id}));
        return ka;
    };
    const res = try agents.start(&s.agent_mgr, s.io, a, &s.group, .{ .template = t, .message = message, .session_id = session, .file = file });
    switch (res) {
        .started => |run_id| {
            var out: std.ArrayList(u8) = .empty;
            try out.appendSlice(a, "{\"run_id\":");
            try http.jsonString(&out, a, run_id);
            try out.appendSlice(a, "}\n");
            try http.sendJson(w, 200, ka, extra, out.items);
        },
        .failed => |f| switch (f) {
            .busy => |id| try http.sendError(a, w, 409, ka, extra, "E_BUSY", try std.fmt.allocPrint(a, "run {s} is still active (one run at a time); stop it first", .{id})),
            .unknown_agent => try http.sendError(a, w, 404, ka, extra, "E_AGENT", "unknown agent"),
            .unavailable => |why| try http.sendError(a, w, 409, ka, extra, "E_UNAVAILABLE", try std.fmt.allocPrint(a, "agent '{s}' is not available: {s}", .{ agent_id, why })),
            .spawn_failed => |why| try http.sendError(a, w, 500, ka, extra, "E_SPAWN", why),
        },
    }
    return ka;
}

fn apiAgentStop(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { run_id }");
    const run_id = (if (body.get("run_id")) |v| v.str() else null) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"run_id\"");
    const r = agents.stop(&s.agent_mgr, s.io, run_id);
    const text = switch (r) {
        .stopped => "{\"stopped\":true}\n",
        .already_finished => "{\"stopped\":false,\"reason\":\"already finished\"}\n",
        .no_such_run => {
            try http.sendError(a, w, 404, req.keep_alive, extra, "E_NOT_FOUND", "no such active run");
            return req.keep_alive;
        },
    };
    try http.sendJson(w, 200, req.keep_alive, extra, text);
    return req.keep_alive;
}

// ---- static UI ----

fn static(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, path: []const u8) !bool {
    _ = s;
    const ka = req.keep_alive;
    const head_only = std.mem.eql(u8, req.method, "HEAD");
    var rel = std.mem.trimStart(u8, path, "/");
    if (rel.len == 0) rel = "index.html";
    if (std.mem.indexOf(u8, rel, "..") != null or std.mem.indexOfScalar(u8, rel, '\\') != null) {
        try http.sendError(a, w, 400, ka, "", "E_PATH", "bad path");
        return ka;
    }
    for (ui_assets.files) |f| {
        if (!std.mem.eql(u8, f.path, rel)) continue;
        try http.send(w, .{ .status = 200, .content_type = http.mimeFor(f.path), .keep_alive = ka, .no_store = false, .extra = "Cache-Control: no-cache\r\n" }, if (head_only) "" else f.data);
        return ka;
    }
    if (ui_assets.files.len == 0 and std.mem.eql(u8, rel, "index.html")) {
        try http.send(w, .{ .status = 200, .content_type = "text/html; charset=utf-8", .keep_alive = ka }, no_ui_page);
        return ka;
    }
    try http.sendError(a, w, 404, ka, "", "E_NOT_FOUND", "not found");
    return ka;
}

const no_ui_page =
    \\<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
    \\<title>kerf serve</title>
    \\<style>body{font:15px/1.5 ui-monospace,Menlo,Consolas,monospace;max-width:46rem;margin:3rem auto;padding:0 1rem;background:#f4efe3;color:#1d2a3a}
    \\code,pre{background:#e9e2d0;padding:.1rem .3rem}pre{padding:.7rem;overflow:auto}h1{font-size:1.2rem;border-bottom:2px solid #1d2a3a}</style></head><body>
    \\<h1>KERF SERVE</h1>
    \\<p>The server is running, but this binary was built <b>without the web UI</b>.</p>
    \\<p>Build the UI, then rebuild the CLI with it embedded:</p>
    \\<pre>cd apps/web &amp;&amp; npm ci &amp;&amp; npm run build:serve
    \\cd ../../engines/zig &amp;&amp; zig build -Doptimize=ReleaseSmall -Dui=../../apps/web/dist-serve</pre>
    \\<p>Release binaries from GitHub already include the UI. The API works regardless:
    \\<a href="/api/info">/api/info</a> &middot; <a href="/api/docs">/api/docs</a></p>
    \\</body></html>
;

// ---------------------------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------------------------

fn pollerTask(s: *Server) void {
    var tick: u32 = 0;
    while (true) {
        s.io.sleep(Io.Duration.fromMilliseconds(poll_ms), .awake) catch return;
        s.scan(true);
        tick += 1;
        if (tick % 30 == 0) s.hub.publish(s.io, "ping", "{}");
    }
}

fn acceptLoop(s: *Server, server: *net.Server) void {
    while (true) {
        const stream = server.accept(s.io) catch |e| switch (e) {
            error.Canceled, error.SocketNotListening => return,
            else => {
                s.io.sleep(Io.Duration.fromMilliseconds(50), .awake) catch return;
                continue;
            },
        };
        s.group.concurrent(s.io, serveConn, .{ s, stream }) catch {
            const msg = "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
            var b: [128]u8 = undefined;
            var sw = stream.writer(s.io, &b);
            sw.interface.writeAll(msg) catch {};
            sw.interface.flush() catch {};
            stream.close(s.io);
        };
    }
}

fn prefetchAgents(s: *Server) void {
    var arena = std.heap.ArenaAllocator.init(s.gpa);
    defer arena.deinit();
    const templates = agents.loadTemplates(arena.allocator(), s.io, s.dir) catch return;
    for (templates) |t| _ = s.agent_mgr.detectCached(s.io, t);
}

fn openBrowser(s: *Server, url: []const u8) void {
    const argv: []const []const u8 = switch (builtin.os.tag) {
        .windows => &.{ "cmd", "/c", "start", "", url },
        .macos => &.{ "open", url },
        else => &.{ "xdg-open", url },
    };
    var child = std.process.spawn(s.io, .{ .argv = argv, .stdin = .ignore, .stdout = .ignore, .stderr = .ignore, .create_no_window = true }) catch return;
    _ = child.wait(s.io) catch {};
}

/// Best guess of this machine's LAN IPv4 address: a connected UDP socket toward a documentation
/// address (nothing is sent) reveals the interface the OS would route through.
fn lanIp(io: Io) ?[4]u8 {
    const target: net.IpAddress = .{ .ip4 = net.Ip4Address.parse("192.0.2.1", 9) catch return null };
    const stream = target.connect(io, .{ .mode = .dgram }) catch return null;
    defer stream.close(io);
    return switch (stream.socket.address) {
        .ip4 => |a4| if (std.mem.eql(u8, &a4.bytes, &[_]u8{ 0, 0, 0, 0 })) null else a4.bytes,
        else => null,
    };
}

fn isLoopbackHost(host: []const u8) bool {
    return std.ascii.eqlIgnoreCase(host, "localhost") or std.mem.eql(u8, host, "::1") or std.mem.startsWith(u8, host, "127.");
}

const serve_usage =
    \\usage: kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --no-token] [--allow-origin URL]
    \\  --dir DIR         folder of *.kerf.json documents (default: current directory)
    \\  --host HOST       bind address; 0.0.0.0 serves the LAN and then requires a token
    \\  --port N          default 7700 (0 = pick a free port)
    \\  --open            open the UI in the default browser
    \\  --token T         require this token (Authorization: Bearer T, or ?token=T)
    \\  --no-token        no token even when not on localhost (trusted network only)
    \\  --allow-origin U  also accept browser requests from origin U (dev server, e.g. http://localhost:5173)
    \\
;

pub fn cliMain(gpa: Allocator, io: Io, args: []const []const u8, err: *Io.Writer, environ: *const std.process.Environ.Map) !u8 {
    var cfg: Config = .{};
    var no_token = false;
    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        const a = args[i];
        const takes_value = std.mem.eql(u8, a, "--dir") or std.mem.eql(u8, a, "--host") or std.mem.eql(u8, a, "--port") or std.mem.eql(u8, a, "--token") or std.mem.eql(u8, a, "--allow-origin");
        if (takes_value) {
            if (i + 1 >= args.len) {
                try err.print("kerf serve: {s} needs a value\n{s}", .{ a, serve_usage });
                return 2;
            }
            i += 1;
            const v = args[i];
            if (std.mem.eql(u8, a, "--dir")) cfg.dir_path = v else if (std.mem.eql(u8, a, "--host")) cfg.host = v else if (std.mem.eql(u8, a, "--token")) cfg.token = v else if (std.mem.eql(u8, a, "--allow-origin")) cfg.allow_origin = v else cfg.port = std.fmt.parseInt(u16, v, 10) catch {
                try err.print("kerf serve: --port must be 0..65535, got '{s}'\n", .{v});
                return 2;
            };
        } else if (std.mem.eql(u8, a, "--open")) cfg.open = true else if (std.mem.eql(u8, a, "--no-token")) no_token = true else if (std.mem.eql(u8, a, "-h") or std.mem.eql(u8, a, "--help")) {
            try err.writeAll(serve_usage);
            return 0;
        } else {
            try err.print("kerf serve: unknown option {s}\n{s}", .{ a, serve_usage });
            return 2;
        }
    }
    if (no_token and cfg.token != null) {
        try err.writeAll("kerf serve: use either --token or --no-token\n");
        return 2;
    }
    const loopback = isLoopbackHost(cfg.host);
    if (!loopback and !no_token and cfg.token == null) {
        var raw: [16]u8 = undefined;
        io.random(&raw);
        cfg.token = try std.fmt.allocPrint(gpa, "{x}", .{&raw});
    }
    if (cfg.token) |t| if (t.len == 0) {
        try err.writeAll("kerf serve: --token must not be empty\n");
        return 2;
    };

    var dir = std.Io.Dir.cwd().openDir(io, cfg.dir_path, .{ .iterate = true }) catch |e| {
        try err.print("kerf serve: cannot open --dir '{s}': {s}\n", .{ cfg.dir_path, @errorName(e) });
        return 1;
    };
    defer dir.close(io);
    var abs_buf: [std.fs.max_path_bytes]u8 = undefined;
    const abs_len = dir.realPath(io, &abs_buf) catch 0;
    const dir_abs = try gpa.dupe(u8, if (abs_len > 0) abs_buf[0..abs_len] else cfg.dir_path);
    defer gpa.free(dir_abs);

    const bind_host = if (std.ascii.eqlIgnoreCase(cfg.host, "localhost")) "127.0.0.1" else cfg.host;
    var addr = net.IpAddress.parse(bind_host, cfg.port) catch {
        try err.print("kerf serve: --host must be an IP address (127.0.0.1, 0.0.0.0, ::1 …), got '{s}'\n", .{cfg.host});
        return 2;
    };
    var listener = addr.listen(io, .{ .reuse_address = true }) catch |e| {
        switch (e) {
            error.AddressInUse => try err.print("kerf serve: port {d} is already in use; try --port {d}\n", .{ cfg.port, cfg.port +% 1 }),
            else => try err.print("kerf serve: cannot listen on {s}:{d}: {s}\n", .{ cfg.host, cfg.port, @errorName(e) }),
        }
        return 1;
    };
    defer listener.deinit(io);
    const port = listener.socket.address.getPort();

    var exe_buf: [std.fs.max_path_bytes]u8 = undefined;
    const exe_dir_len = std.process.executableDirPath(io, &exe_buf) catch 0;
    const exe_dir = try gpa.dupe(u8, exe_buf[0..exe_dir_len]);
    defer gpa.free(exe_dir);

    var server: Server = .{
        .gpa = gpa,
        .io = io,
        .cfg = cfg,
        .dir = dir,
        .dir_abs = dir_abs,
        .hub = events.Hub.init(gpa),
        .client = .{ .allocator = gpa, .io = io },
        .agent_mgr = undefined,
        .loopback_only = loopback,
        .started_ns = Io.Timestamp.now(io, .awake).nanoseconds,
    };
    server.cfg.port = port;
    server.agent_mgr = .{ .gpa = gpa, .hub = &server.hub, .dir = dir, .dir_abs = dir_abs, .environ = environ, .exe_dir = exe_dir };
    defer server.client.deinit();
    defer server.hub.deinit();
    defer server.agent_mgr.deinit();

    server.scan(false);

    // Banner (stdout; scripts parse the first line).
    var out_buf: [2048]u8 = undefined;
    var fw = Io.File.stdout().writer(io, &out_buf);
    const o = &fw.interface;
    const local_host = if (loopback) bind_host else "127.0.0.1";
    try o.print("kerf serve: http://{s}:{d}/  (dir {s})\n", .{ local_host, port, dir_abs });
    if (!loopback) {
        const any = std.mem.eql(u8, bind_host, "0.0.0.0") or std.mem.eql(u8, bind_host, "::");
        var lan_buf: [32]u8 = undefined;
        const lan: ?[]const u8 = if (any) (if (lanIp(io)) |b| (std.fmt.bufPrint(&lan_buf, "{d}.{d}.{d}.{d}", .{ b[0], b[1], b[2], b[3] }) catch null) else null) else bind_host;
        const shown = lan orelse "<this-machine-ip>";
        if (cfg.token) |t| try o.print("LAN:  http://{s}:{d}/?token={s}\n", .{ shown, port, t }) else try o.print("LAN:  http://{s}:{d}/   (NO TOKEN: anyone on this network can edit the folder and run agents)\n", .{ shown, port });
    }
    if (ui_assets.files.len == 0) try o.writeAll("note: this binary has no embedded web UI (build with -Dui=<dist dir>); the API is available.\n");
    try o.flush();

    server.group.concurrent(io, pollerTask, .{&server}) catch {};
    server.group.concurrent(io, prefetchAgents, .{&server}) catch {};
    if (cfg.open) {
        const url = if (cfg.token) |t| try std.fmt.allocPrint(gpa, "http://{s}:{d}/?token={s}", .{ local_host, port, t }) else try std.fmt.allocPrint(gpa, "http://{s}:{d}/", .{ local_host, port });
        defer gpa.free(url);
        openBrowser(&server, url);
    }
    acceptLoop(&server, &listener);
    return 0;
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

test "hostIsLoopbackName / originMatchesHost" {
    try std.testing.expect(hostIsLoopbackName("localhost:7700"));
    try std.testing.expect(hostIsLoopbackName("127.0.0.1:7700"));
    try std.testing.expect(hostIsLoopbackName("[::1]:7700"));
    try std.testing.expect(!hostIsLoopbackName("evil.example.com"));
    try std.testing.expect(!hostIsLoopbackName("127.0.0.1.evil.com"));
    try std.testing.expect(originMatchesHost("http://localhost:7700", "localhost:7700"));
    try std.testing.expect(!originMatchesHost("http://evil.com", "localhost:7700"));
    try std.testing.expect(!originMatchesHost("http://localhost:5173", "localhost:7700"));
}

fn testServer(gpa: Allocator, io: Io, tok: ?[]const u8, loopback: bool) Server {
    return .{
        .gpa = gpa,
        .io = io,
        .cfg = .{ .token = tok },
        .dir = Io.Dir.cwd(),
        .dir_abs = &.{},
        .hub = events.Hub.init(gpa),
        .client = .{ .allocator = gpa, .io = io },
        .agent_mgr = undefined,
        .loopback_only = loopback,
        .started_ns = 0,
    };
}

test "access: token, host, origin" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    var open = testServer(std.testing.allocator, io, null, true);
    defer open.hub.deinit();
    const ok = try http.parseHead("GET /api/info HTTP/1.1\r\nHost: localhost:7700");
    try std.testing.expect(checkAccess(&open, a, ok) == null);
    const rebind = try http.parseHead("GET /api/info HTTP/1.1\r\nHost: evil.com:7700");
    try std.testing.expectEqual(@as(u16, 403), checkAccess(&open, a, rebind).?.status);
    const xo = try http.parseHead("POST /api/docs HTTP/1.1\r\nHost: localhost:7700\r\nOrigin: http://evil.com");
    try std.testing.expectEqualStrings("E_ORIGIN", checkAccess(&open, a, xo).?.code);
    const so = try http.parseHead("POST /api/docs HTTP/1.1\r\nHost: localhost:7700\r\nOrigin: http://localhost:7700");
    try std.testing.expect(checkAccess(&open, a, so) == null);

    var locked = testServer(std.testing.allocator, io, "s3cret", false);
    defer locked.hub.deinit();
    const none = try http.parseHead("GET /api/docs HTTP/1.1\r\nHost: 192.168.1.5:7700");
    try std.testing.expectEqual(@as(u16, 401), checkAccess(&locked, a, none).?.status);
    const bearer = try http.parseHead("GET /api/docs HTTP/1.1\r\nHost: 192.168.1.5:7700\r\nAuthorization: Bearer s3cret");
    try std.testing.expect(checkAccess(&locked, a, bearer) == null);
    const wrong = try http.parseHead("GET /api/docs HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer nope");
    try std.testing.expectEqual(@as(u16, 401), checkAccess(&locked, a, wrong).?.status);
    const query = try http.parseHead("GET /api/events?token=s3cret HTTP/1.1\r\nHost: x");
    try std.testing.expect(checkAccess(&locked, a, query) == null);
}

test "scan reports added, changed, removed and log lines" {
    const gpa = std.testing.allocator;
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    var tmp = std.testing.tmpDir(.{ .iterate = true });
    defer tmp.cleanup();
    var s = testServer(gpa, io, null, true);
    s.dir = tmp.dir;
    defer {
        for (s.files.items) |f| {
            if (f.info) |i| s.freeInfo(i);
            gpa.free(f.name);
        }
        s.files.deinit(gpa);
        s.hub.deinit();
    }
    s.scan(false); // empty folder
    try std.testing.expectEqual(@as(u64, 0), s.hub.current(io));
    try tmp.dir.writeFile(io, .{ .sub_path = "a.kerf.json", .data = "{}" });
    s.scan(true);
    try std.testing.expectEqual(@as(u64, 1), s.hub.current(io)); // doc_added
    try tmp.dir.writeFile(io, .{ .sub_path = "a.kerf.json", .data = "{\"x\":1}" });
    try tmp.dir.writeFile(io, .{ .sub_path = "a.kerf.json.log.jsonl", .data = "{\"ts\":\"t\",\"who\":\"agent\"}\n{\"ts\":\"u\"" });
    s.scan(true);
    // doc_changed + one complete log line (the torn second line waits for its newline)
    try std.testing.expectEqual(@as(u64, 3), s.hub.current(io));
    try std.testing.expect(std.mem.indexOf(u8, s.hub.frames.items[1].bytes, "\"who\":\"agent\"") != null);
    try tmp.dir.deleteFile(io, "a.kerf.json");
    s.scan(true);
    try std.testing.expect(std.mem.indexOf(u8, s.hub.frames.items[3].bytes, "doc_removed") != null);
}

test {
    _ = ws;
    _ = http;
    _ = events;
    _ = agents;
    _ = proxy;
}
