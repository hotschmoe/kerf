//! `kerf serve`: local workspace server (spec/SERVE.md). One process serves the embedded web UI and a
//! JSON/SSE API over a folder of `*.kerf.json` documents. The folder is the source of truth: a 500 ms
//! poller (mtime + size) turns CLI/agent edits into SSE events, designer edits go through `apply`
//! with optimistic concurrency (ETag / if_match) and are logged like CLI edits.
//!
//! Threads: `std.Io.Threaded` (the default `init.io`); every connection is a `Group.concurrent` task, so
//! long-lived SSE streams never block other requests.
//!
//! This file holds the `Server` (the folder scan, the edit log, the ETag rules) and its `Config`. The rest is in `serve/`:
//! `cli` (arguments, bind, startup), `conn` (limits, deadlines, access rules), `routes` (the route table, body caps and the
//! request dispatcher), the handlers `docs`, `agent` and `stream`, and `ui` (static files and the CSP).

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
pub const poll_ms = 500;
const static = ui_mod.static;
const default_ui_csp = ui_mod.default_ui_csp;
const ui_mod = @import("serve/ui.zig");
const agent_mod = @import("serve/agent.zig");
const stream_mod = @import("serve/stream.zig");
const docs_mod = @import("serve/docs.zig");
pub const max_sse_streams = conn_mod.max_sse_streams;
pub const max_proxy_calls = conn_mod.max_proxy_calls;
const Timeouts = conn_mod.Timeouts;
const Conn = conn_mod.Conn;
pub const arm = conn_mod.arm;
const hostIsLoopbackName = conn_mod.hostIsLoopbackName;
const originMatchesHost = conn_mod.originMatchesHost;
const checkAccess = conn_mod.checkAccess;
const conn_mod = @import("serve/conn.zig");
const Kind = routes_mod.Kind;
const Route = routes_mod.Route;
const routeOf = routes_mod.routeOf;
const bodyCap = routes_mod.bodyCap;
pub const handleRequest = routes_mod.handleRequest;
pub const badRequest = routes_mod.badRequest;
pub const parseBody = routes_mod.parseBody;
const routes_mod = @import("serve/routes.zig");
pub const cliMain = cli_mod.cliMain;
const cli_mod = @import("serve/cli.zig");

pub const Config = struct {
    dir_path: []const u8 = ".",
    host: []const u8 = "127.0.0.1",
    port: u16 = default_port,
    token: ?[]const u8 = null,
    /// One extra browser origin allowed to call the API (for `vite dev` against a running server).
    allow_origin: ?[]const u8 = null,
    open: bool = false,
    /// Run agents defined in `<dir>/.kerf/agents.json` (they execute commands from the folder; V-2).
    trust_agents: bool = false,
    timeouts: Timeouts = .{},
    /// Stall limits of proxied LLM calls.
    llm_limits: proxy.Limits = .{},
    /// Wall-clock limit of one agent run, seconds (0 = none).
    agent_timeout_s: u32 = agents.default_max_run_s,
};

pub const DocInfo = struct {
    id: []u8,
    title: []u8,
    components: usize,
    views: usize,
    errors: usize,
    warnings: usize,
    /// Wyhash of the document bytes: the content part of its ETag.
    hash: u64 = 0,
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
    /// Open connections (the watchdog shuts down the ones past their deadline) and the caps (V-4).
    conns_mu: Io.Mutex = .init,
    conns: std.ArrayList(*Conn) = .empty,
    active_conns: std.atomic.Value(u32) = .init(0),
    sse_active: std.atomic.Value(u32) = .init(0),
    proxy_active: std.atomic.Value(u32) = .init(0),
    /// `Content-Security-Policy` of the embedded UI (script hashes of its inline scripts are added at startup).
    ui_csp: []const u8 = default_ui_csp,

    fn freeInfo(s: *Server, info: DocInfo) void {
        s.gpa.free(info.id);
        s.gpa.free(info.title);
    }

    fn findState(s: *Server, name: []const u8) ?*FileState {
        for (s.files.items) |*f| if (std.mem.eql(u8, f.name, name)) return f;
        return null;
    }

    pub fn cors(s: *Server, a: Allocator, req: http.Request) Allocator.Error![]const u8 {
        const ao = s.cfg.allow_origin orelse return "";
        const origin = req.header("origin") orelse return "";
        if (!std.mem.eql(u8, origin, ao)) return "";
        return a.print("Access-Control-Allow-Origin: {s}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: authorization, content-type, if-match\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Expose-Headers: etag\r\n", .{origin});
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

    pub fn scanLocked(s: *Server, emit: bool) void {
        const io = s.io;
        var arena = std.heap.ArenaAllocator.init(s.gpa);
        defer arena.deinit();
        const a = arena.allocator();

        var names: std.ArrayList([]const u8) = .empty;
        var it = s.dir.iterate();
        while (it.next(io) catch null) |e| {
            if (e.kind != .file and e.kind != .unknown) continue; // a symlink is never served (V-10)
            if (!ws.validDocFile(e.name)) continue;
            names.append(a, a.dupe(u8, e.name) catch continue) catch continue;
        }
        kerf.sort.stable([]const u8, names.items, {}, lessStr);

        var added: std.ArrayList([]const u8) = .empty;
        var changed: std.ArrayList([]const u8) = .empty;
        for (names.items) |name| {
            const st = s.dir.statFile(io, name, .{ .follow_symlinks = false }) catch continue;
            if (st.kind != .file) continue;
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
        const st = s.dir.statFile(s.io, lp, .{ .follow_symlinks = false }) catch return 0;
        return st.size;
    }

    /// Complete, valid-JSON lines appended to the log since `f.log_size`; advances `f.log_size`.
    fn readNewLog(s: *Server, a: Allocator, f: *FileState) ![]const []const u8 {
        const lp = try ws.logPath(a, f.name);
        const st = s.dir.statFile(s.io, lp, .{ .follow_symlinks = false }) catch {
            f.log_size = 0;
            return &.{};
        };
        if (st.kind != .file) return &.{};
        if (st.size < f.log_size) {
            f.log_size = st.size;
            return &.{};
        }
        if (st.size == f.log_size) return &.{};
        const want: usize = @intCast(@min(st.size - f.log_size, 4 << 20));
        var file = try s.dir.openFile(s.io, lp, .{ .follow_symlinks = false });
        defer file.close(s.io);
        const buf = try a.alloc(u8, want);
        const got = try file.readPositionalAll(s.io, buf, f.log_size);
        const data = buf[0..got];
        const last_nl = std.mem.lastIndexOfScalar(u8, data, '\n') orelse {
            // A line longer than the read window would otherwise be re-read every tick forever: skip it.
            if (got >= 4 << 20) f.log_size += got;
            return &.{};
        };
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

    /// Remember `info` for `name` when the document is still the version it was computed from.
    pub fn storeInfo(s: *Server, name: []const u8, mtime_ns: i96, size: u64, info: DocInfo) void {
        s.scan_mu.lockUncancelable(s.io);
        defer s.scan_mu.unlock(s.io);
        const f = s.findState(name) orelse return;
        if (f.info != null or f.mtime_ns != mtime_ns or f.size != size) return;
        const id = s.gpa.dupe(u8, info.id) catch return;
        const title = s.gpa.dupe(u8, info.title) catch {
            s.gpa.free(id);
            return;
        };
        var copy = info;
        copy.id = id;
        copy.title = title;
        f.info = copy;
    }

    // ---- documents ----

    const DocRead = struct { bytes: []u8, mtime_ns: i96, size: u64 };

    pub fn readDoc(s: *Server, a: Allocator, name: []const u8) !DocRead {
        // O_NOFOLLOW: a symlink planted in the folder (an agent can create one) is not a document (V-10).
        var f = try s.dir.openFile(s.io, name, .{ .follow_symlinks = false });
        defer f.close(s.io);
        const st = try f.stat(s.io);
        if (st.kind != .file) return error.NotRegular;
        var rb: [8192]u8 = undefined;
        var fr = f.reader(s.io, &rb);
        const bytes = try fr.interface.allocRemaining(a, .limited(256 << 20));
        return .{ .bytes = bytes, .mtime_ns = st.mtime.nanoseconds, .size = st.size };
    }

    pub fn isMissing(e: anyerror) bool {
        return e == error.FileNotFound or e == error.SymLinkLoop or e == error.NotRegular or e == error.IsDir or e == error.NotDir;
    }

    /// After the server itself wrote `name` (document and optionally one log line): refresh the scan
    /// state and publish the events with `who`. Must be called with `scan_mu` held.
    /// Append one op-log line; a failure is reported on stderr (the document write already succeeded).
    pub fn appendLogOrWarn(s: *Server, lp: []const u8, line: []const u8) void {
        ws.appendLine(s.io, s.dir, lp, line) catch |e| s.warnLog(lp, e);
    }

    pub fn warnLog(s: *Server, lp: []const u8, e: anyerror) void {
        var b: [512]u8 = undefined;
        var w = Io.File.stderr().writer(s.io, &b);
        w.interface.print("kerf serve: warning: could not append to op log {s}: {s} (the document was written)\n", .{ lp, @errorName(e) }) catch {};
        w.interface.flush() catch {};
    }

    pub fn noteWriteLocked(s: *Server, a: Allocator, name: []const u8, is_new: bool, who: []const u8, log_line: ?[]const u8) void {
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

    /// `"<mtime_ms>-<size>-<hash of the bytes>"`: two same-size edits within one millisecond no longer collide.
    pub fn etagOf(a: Allocator, mtime_ns: i96, size: u64, bytes: []const u8) ![]u8 {
        return a.print("\"{d}-{d}-{x:0>16}\"", .{ @divTrunc(mtime_ns, std.time.ns_per_ms), size, std.hash.Wyhash.hash(0, bytes) });
    }
};

pub fn computeInfo(gpa: Allocator, a: Allocator, text: []const u8) !DocInfo {
    var info = try computeInfoNoHash(gpa, a, text);
    info.hash = std.hash.Wyhash.hash(0, text);
    return info;
}

fn computeInfoNoHash(gpa: Allocator, a: Allocator, text: []const u8) !DocInfo {
    var pe: kerf.json.ParseError = undefined;
    const parsed = (try kerf.json.parse(a, text, &pe)) orelse
        return .{ .id = try gpa.dupe(u8, ""), .title = try gpa.dupe(u8, "(invalid JSON)"), .components = 0, .views = 0, .errors = 1, .warnings = 0 };
    const id = if (parsed.get("id")) |v| (v.str() orelse "") else "";
    const title = if (parsed.get("title")) |v| (v.str() orelse "") else "";
    const comps = if (parsed.get("components")) |v| (if (v.arr()) |x| x.len else 0) else 0;
    const views = if (parsed.get("views")) |v| (if (v.arr()) |x| x.len else 0) else 0;
    var errors: usize = 0;
    var warnings: usize = 0;
    const input = try a.print("{{\"doc\":{s}}}", .{std.mem.trim(u8, text, " \t\r\n")});
    const r = try kerf.call(a, "check", input);
    if (r.ok) {
        if (try kerf.json.parse(a, r.bytes, &pe)) |res| if (res.get("diagnostics")) |dl| if (dl.arr()) |items| for (items) |d| {
            const lv = if (d.get("level")) |l| (l.str() orelse "") else "";
            if (std.mem.eql(u8, lv, "error")) errors += 1 else if (std.mem.eql(u8, lv, "warning")) warnings += 1;
        };
    } else errors += 1;
    return .{ .id = try gpa.dupe(u8, id), .title = try gpa.dupe(u8, title), .components = comps, .views = views, .errors = errors, .warnings = warnings };
}

pub const attachment_ttl_s = 24 * 3600;

pub const serve_usage =
    \\usage: kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --no-token] [--allow-origin URL]
    \\  --dir DIR         folder of *.kerf.json documents (default: current directory)
    \\  --host HOST       bind address; 0.0.0.0 serves the LAN and then requires a token
    \\  --port N          default 7700 (0 = pick a free port)
    \\  --open            open the UI in the default browser
    \\  --token T         require this token (Authorization: Bearer T, or ?token=T); visible in `ps`, prefer:
    \\  --token-file F    read the token from file F (or set KERF_TOKEN); the default is a random token, printed in the URL
    \\  --no-token        no token at all (trusted machine and network only; localhost then also checks Host/Origin)
    \\  --llm-timeout S   longest pause of a proxied LLM call (first byte and between chunks; default 300 s / 120 s)
    \\  --timeouts-ms I,H,W  tuning/testing: keep-alive idle, request head and streaming write deadlines (30000,10000,20000)
    \\  --allow-origin U  also accept browser requests from origin U (dev server, e.g. http://localhost:5173)
    \\  --trust-agents    run agents defined in <dir>/.kerf/agents.json (they execute commands from the folder; without
    \\                    this flag, or a yes at the interactive prompt, they are listed as untrusted and never started)
    \\  --agent-timeout S wall-clock limit of one agent run in seconds (default 1800, 0 = none)
    \\
;

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

fn routeFor(head: []const u8) !Route {
    const req = try http.parseHead(head);
    return routeOf(req, req.path);
}

test "V-5: routes are decided before any body is read, and only body routes have a body cap" {
    try std.testing.expectEqual(Kind.info, (try routeFor("GET /api/info HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.docs_create, (try routeFor("POST /api/docs HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.method_not_allowed, (try routeFor("DELETE /api/docs HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.options, (try routeFor("OPTIONS /api/docs HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.not_found, (try routeFor("POST /api/nope HTTP/1.1")).kind);
    const ap = try routeFor("POST /api/docs/a.kerf.json/apply HTTP/1.1");
    try std.testing.expectEqual(Kind.doc_apply, ap.kind);
    try std.testing.expectEqualStrings("a.kerf.json", ap.file);
    try std.testing.expectEqual(Kind.bad_file, (try routeFor("POST /api/docs/CON.kerf.json/apply HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.not_found, (try routeFor("GET /api/docs/a.kerf.json/apply/x HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.static, (try routeFor("GET /assets/x.js HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.static, (try routeFor("HEAD / HTTP/1.1")).kind);
    try std.testing.expectEqual(Kind.method_not_allowed, (try routeFor("POST /index.html HTTP/1.1")).kind);
    // caps: GET routes take no body; mutating routes have bounded caps
    try std.testing.expectEqual(@as(usize, 0), bodyCap(.docs_list));
    try std.testing.expectEqual(@as(usize, 0), bodyCap(.doc_get));
    try std.testing.expectEqual(@as(usize, 0), bodyCap(.events));
    try std.testing.expect(bodyCap(.agent_stop) <= 4096);
    try std.testing.expect(bodyCap(.docs_create) <= 1 << 20);
    try std.testing.expect(bodyCap(.doc_apply) <= 16 << 20);
    try std.testing.expect(bodyCap(.llm) <= 64 << 20 and bodyCap(.agent_run) <= 64 << 20);
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
    _ = @import("serve/routes.zig");
    _ = ws;
    _ = http;
    _ = events;
    _ = agents;
    _ = proxy;
}
