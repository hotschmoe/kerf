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
const static = ui_mod.static;
const default_ui_csp = ui_mod.default_ui_csp;
const buildUiCsp = ui_mod.buildUiCsp;
const ui_mod = @import("serve/ui.zig");
const apiAgentRun = agent_mod.apiAgentRun;
const cleanupAttachments = agent_mod.cleanupAttachments;
const apiAgentStop = agent_mod.apiAgentStop;
const agent_mod = @import("serve/agent.zig");
const apiEvents = stream_mod.apiEvents;
const apiLlm = stream_mod.apiLlm;
const stream_mod = @import("serve/stream.zig");
const apiInfo = docs_mod.apiInfo;
const apiList = docs_mod.apiList;
const apiGetDoc = docs_mod.apiGetDoc;
const apiCreate = docs_mod.apiCreate;
const apiApply = docs_mod.apiApply;
const apiLog = docs_mod.apiLog;
const apiExport = docs_mod.apiExport;
const docs_mod = @import("serve/docs.zig");
const max_conns = conn_mod.max_conns;
pub const max_sse_streams = conn_mod.max_sse_streams;
pub const max_proxy_calls = conn_mod.max_proxy_calls;
const busy_ms = conn_mod.busy_ms;
const Timeouts = conn_mod.Timeouts;
const Conn = conn_mod.Conn;
pub const arm = conn_mod.arm;
const connWatchdog = conn_mod.connWatchdog;
const serveConn = conn_mod.serveConn;
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
        return std.fmt.allocPrint(a, "Access-Control-Allow-Origin: {s}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: authorization, content-type, if-match\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Expose-Headers: etag\r\n", .{origin});
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
        std.mem.sort([]const u8, names.items, {}, lessStr);

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
        return std.fmt.allocPrint(a, "\"{d}-{d}-{x:0>16}\"", .{ @divTrunc(mtime_ns, std.time.ns_per_ms), size, std.hash.Wyhash.hash(0, bytes) });
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

// ---- connection limits and deadlines (V-4, V-13) ----

// ---- /api/info ----

// ---- documents ----

// ---- SSE ----

// ---- /api/llm ----

// ---- agents ----

pub const attachment_ttl_s = 24 * 3600;

// ---- static UI ----

// ---------------------------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------------------------

/// Called by the agent bridge just before it publishes `exit`: report the agent's last edits (doc + log
/// events) now instead of up to 500 ms later, so the UI sees them before the run ends.
fn scanForAgent(ctx: *anyopaque) void {
    const s: *Server = @ptrCast(@alignCast(ctx));
    s.scan(true);
}

/// Set by the SIGINT/SIGTERM/SIGHUP handler; the poller notices it, ends the active agent run (and its process tree)
/// and exits, so no agent is reparented to init when the server is stopped.
var shutdown_signal = std.atomic.Value(u8).init(0);

fn onSignal(sig: std.posix.SIG) callconv(.c) void {
    shutdown_signal.store(@intCast(@intFromEnum(sig)), .release);
}

fn installSignalHandlers() void {
    if (builtin.os.tag == .windows or builtin.os.tag == .wasi) return; // Windows: the agent's job object dies with the server
    const act: std.posix.Sigaction = .{ .handler = .{ .handler = onSignal }, .mask = std.posix.sigemptyset(), .flags = 0 };
    for ([_]std.posix.SIG{ .INT, .TERM, .HUP }) |sg| std.posix.sigaction(sg, &act, null);
}

fn pollerTask(s: *Server) void {
    var tick: u32 = 0;
    while (true) {
        s.io.sleep(Io.Duration.fromMilliseconds(poll_ms), .awake) catch return;
        const sig = shutdown_signal.load(.acquire);
        if (sig != 0) {
            s.agent_mgr.shutdown(s.io);
            std.process.exit(128 +| sig);
        }
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
        if (s.active_conns.fetchAdd(1, .acq_rel) >= max_conns) {
            _ = s.active_conns.fetchSub(1, .acq_rel);
            refuse(s, stream);
            continue;
        }
        s.group.concurrent(s.io, serveConn, .{ s, stream }) catch {
            _ = s.active_conns.fetchSub(1, .acq_rel);
            refuse(s, stream);
        };
    }
}

/// Over the connection cap (or no thread to spare): a tiny 503, then close. The reply is far smaller than a socket
/// buffer, so this cannot block the accept loop.
fn refuse(s: *Server, stream: net.Stream) void {
    const msg = "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nRetry-After: 2\r\nContent-Length: 0\r\n\r\n";
    var b: [128]u8 = undefined;
    var sw = stream.writer(s.io, &b);
    sw.interface.writeAll(msg) catch {};
    sw.interface.flush() catch {};
    stream.close(s.io);
}

fn prefetchAgents(s: *Server) void {
    var arena = std.heap.ArenaAllocator.init(s.gpa);
    defer arena.deinit();
    const templates = agents.loadTemplates(arena.allocator(), s.io, s.dir, s.cfg.trust_agents) catch return;
    for (templates) |t| _ = s.agent_mgr.detectCached(s.io, arena.allocator(), t); // untrusted workspace entries are skipped inside
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

/// True when something already accepts connections on host:port (wildcard hosts are probed via loopback).
fn portInUse(io: Io, host: []const u8, port: u16) bool {
    const probe_host = if (std.mem.eql(u8, host, "0.0.0.0")) "127.0.0.1" else if (std.mem.eql(u8, host, "::")) "::1" else host;
    const addr = net.IpAddress.parse(probe_host, port) catch return false;
    const stream = addr.connect(io, .{ .mode = .stream }) catch return false;
    stream.close(io);
    return true;
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

/// V-2: `<dir>/.kerf/agents.json` makes the server run commands from the folder. Without `--trust-agents` that needs an
/// explicit yes on a terminal; anywhere else the entries stay untrusted (listed, never executed).
fn confirmWorkspaceAgents(gpa: Allocator, io: Io, dir: Io.Dir, dir_abs: []const u8, err: *Io.Writer) bool {
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    const templates = agents.loadTemplates(a, io, dir, false) catch return false;
    var n: usize = 0;
    for (templates) |t| {
        if (!t.from_workspace) continue;
        if (n == 0) err.print("kerf serve: {s}/.kerf/agents.json defines custom agents; starting or detecting them runs commands from this folder:\n", .{dir_abs}) catch {};
        n += 1;
        err.print("  - {s}: detect `{s}`, run `{s}`\n", .{ t.id, t.detect[0], t.argv[0] }) catch {};
    }
    if (n == 0) return false;
    const tty = (Io.File.stdin().isTty(io) catch false) and (Io.File.stderr().isTty(io) catch false);
    if (!tty) {
        err.writeAll("  not trusted (no terminal to ask): they are listed as untrusted and never started. Pass --trust-agents if you wrote this file.\n") catch {};
        err.flush() catch {};
        return false;
    }
    err.writeAll("Trust these agents? [y/N] ") catch {};
    err.flush() catch {};
    var rb: [256]u8 = undefined;
    var fr = Io.File.stdin().reader(io, &rb);
    const line = fr.interface.takeDelimiterExclusive('\n') catch return false;
    const ans = std.mem.trim(u8, line, " \t\r");
    const yes = std.ascii.eqlIgnoreCase(ans, "y") or std.ascii.eqlIgnoreCase(ans, "yes");
    if (!yes) err.writeAll("  not trusted: they stay listed as untrusted and never start.\n") catch {};
    err.flush() catch {};
    return yes;
}

pub fn cliMain(gpa: Allocator, io: Io, args: []const []const u8, err: *Io.Writer, environ: *const std.process.Environ.Map) !u8 {
    var cfg: Config = .{};
    var no_token = false;
    var token_file: ?[]const u8 = null;
    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        const a = args[i];
        const takes_value = std.mem.eql(u8, a, "--dir") or std.mem.eql(u8, a, "--host") or std.mem.eql(u8, a, "--port") or std.mem.eql(u8, a, "--token") or std.mem.eql(u8, a, "--allow-origin") or std.mem.eql(u8, a, "--agent-timeout") or std.mem.eql(u8, a, "--token-file") or std.mem.eql(u8, a, "--llm-timeout") or std.mem.eql(u8, a, "--timeouts-ms");
        if (takes_value) {
            if (i + 1 >= args.len) {
                try err.print("kerf serve: {s} needs a value\n{s}", .{ a, serve_usage });
                return 2;
            }
            i += 1;
            const v = args[i];
            if (std.mem.eql(u8, a, "--timeouts-ms")) {
                var it = std.mem.splitScalar(u8, v, ',');
                const t_idle = std.fmt.parseInt(u32, it.next() orelse "", 10) catch 0;
                const t_head = std.fmt.parseInt(u32, it.next() orelse "", 10) catch 0;
                const t_write = std.fmt.parseInt(u32, it.next() orelse "", 10) catch 0;
                if (t_idle == 0 or t_head == 0 or t_write == 0) {
                    try err.print("kerf serve: --timeouts-ms wants IDLE,HEAD,WRITE in milliseconds, got '{s}'\n", .{v});
                    return 2;
                }
                cfg.timeouts = .{ .idle_ms = t_idle, .head_ms = t_head, .write_ms = t_write };
            } else if (std.mem.eql(u8, a, "--llm-timeout")) {
                const secs = std.fmt.parseInt(u32, v, 10) catch {
                    try err.print("kerf serve: --llm-timeout must be a number of seconds, got '{s}'\n", .{v});
                    return 2;
                };
                cfg.llm_limits.idle_ms = @as(u64, secs) * 1000;
                cfg.llm_limits.first_byte_ms = cfg.llm_limits.idle_ms;
            } else if (std.mem.eql(u8, a, "--token-file")) {
                token_file = v;
            } else if (std.mem.eql(u8, a, "--agent-timeout")) {
                cfg.agent_timeout_s = std.fmt.parseInt(u32, v, 10) catch {
                    try err.print("kerf serve: --agent-timeout must be a number of seconds, got '{s}'\n", .{v});
                    return 2;
                };
            } else if (std.mem.eql(u8, a, "--dir")) cfg.dir_path = v else if (std.mem.eql(u8, a, "--host")) cfg.host = v else if (std.mem.eql(u8, a, "--token")) cfg.token = v else if (std.mem.eql(u8, a, "--allow-origin")) cfg.allow_origin = v else cfg.port = std.fmt.parseInt(u16, v, 10) catch {
                try err.print("kerf serve: --port must be 0..65535, got '{s}'\n", .{v});
                return 2;
            };
        } else if (std.mem.eql(u8, a, "--open")) cfg.open = true else if (std.mem.eql(u8, a, "--no-token")) no_token = true else if (std.mem.eql(u8, a, "--trust-agents")) cfg.trust_agents = true else if (std.mem.eql(u8, a, "-h") or std.mem.eql(u8, a, "--help")) {
            try err.writeAll(serve_usage);
            return 0;
        } else {
            try err.print("kerf serve: unknown option {s}\n{s}", .{ a, serve_usage });
            return 2;
        }
    }
    // Token sources, strongest first: --token-file, KERF_TOKEN, --token (visible in `ps`), else a fresh random one.
    // A token is the default everywhere, also on loopback: the Host/Origin checks stop browsers, not other local users
    // or processes, and the agent bridge can run an autonomous coding agent (V-7). `--no-token` opts out.
    if (token_file) |tf| {
        var f = std.Io.Dir.cwd().openFile(io, tf, .{}) catch |e| {
            try err.print("kerf serve: cannot read --token-file '{s}': {s}\n", .{ tf, @errorName(e) });
            return 1;
        };
        defer f.close(io);
        var fb: [512]u8 = undefined;
        var fr = f.reader(io, &fb);
        const raw = fr.interface.allocRemaining(gpa, .limited(4096)) catch {
            try err.print("kerf serve: cannot read --token-file '{s}'\n", .{tf});
            return 1;
        };
        cfg.token = std.mem.trim(u8, raw, " \t\r\n");
    } else if (cfg.token == null) {
        if (environ.get("KERF_TOKEN")) |t| if (t.len > 0) {
            cfg.token = t;
        };
    } else {
        try err.writeAll("kerf serve: note: --token is visible to other users in `ps`; prefer KERF_TOKEN or --token-file\n");
    }
    if (no_token and cfg.token != null) {
        try err.writeAll("kerf serve: use either a token (--token, --token-file, KERF_TOKEN) or --no-token\n");
        return 2;
    }
    if (!no_token and cfg.token == null) {
        var raw: [16]u8 = undefined;
        io.randomSecure(&raw) catch {
            try err.writeAll("kerf serve: no secure random source for the access token; pass --token-file, or --no-token on a trusted machine\n");
            return 1;
        };
        cfg.token = try std.fmt.allocPrint(gpa, "{x}", .{&raw});
    }
    if (cfg.token) |t| if (t.len == 0) {
        try err.writeAll("kerf serve: the token must not be empty\n");
        return 2;
    };
    const loopback = isLoopbackHost(cfg.host);

    var dir = std.Io.Dir.cwd().openDir(io, cfg.dir_path, .{ .iterate = true }) catch |e| {
        try err.print("kerf serve: cannot open --dir '{s}': {s}\n", .{ cfg.dir_path, @errorName(e) });
        return 1;
    };
    defer dir.close(io);
    var abs_buf: [std.fs.max_path_bytes]u8 = undefined;
    const abs_len = dir.realPath(io, &abs_buf) catch 0;
    const dir_abs = try gpa.dupe(u8, if (abs_len > 0) abs_buf[0..abs_len] else cfg.dir_path);
    defer gpa.free(dir_abs);

    if (!cfg.trust_agents) cfg.trust_agents = confirmWorkspaceAgents(gpa, io, dir, dir_abs, err);

    const bind_host = if (std.ascii.eqlIgnoreCase(cfg.host, "localhost")) "127.0.0.1" else cfg.host;
    var addr = net.IpAddress.parse(bind_host, cfg.port) catch {
        try err.print("kerf serve: --host must be an IP address (127.0.0.1, 0.0.0.0, ::1 …), got '{s}'\n", .{cfg.host});
        return 2;
    };
    // std sets SO_REUSEPORT together with SO_REUSEADDR, which would let a second `kerf serve` share the port
    // silently. Probe first so a busy port is an error (we keep SO_REUSEADDR: restarts do not wait out TIME_WAIT).
    if (cfg.port != 0 and portInUse(io, bind_host, cfg.port)) {
        try err.print("kerf serve: port {d} is already in use; try --port {d}\n", .{ cfg.port, cfg.port +% 1 });
        return 1;
    }
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
    server.agent_mgr = .{ .gpa = gpa, .hub = &server.hub, .dir = dir, .dir_abs = dir_abs, .environ = environ, .exe_dir = exe_dir, .before_exit_ctx = &server, .before_exit = scanForAgent, .trust_agents = cfg.trust_agents, .max_run_s = cfg.agent_timeout_s };
    defer server.client.deinit();
    defer server.hub.deinit();
    defer server.agent_mgr.deinit();

    server.scan(false);
    cleanupAttachments(&server);
    installSignalHandlers();

    // Banner (stdout; scripts parse the first line).
    var out_buf: [2048]u8 = undefined;
    var fw = Io.File.stdout().writer(io, &out_buf);
    const o = &fw.interface;
    const local_host = if (loopback) bind_host else "127.0.0.1";
    if (cfg.token) |t| try o.print("kerf serve: http://{s}:{d}/?token={s}  (dir {s})\n", .{ local_host, port, t, dir_abs }) else try o.print("kerf serve: http://{s}:{d}/  (dir {s})\n", .{ local_host, port, dir_abs });
    if (!loopback) {
        const any = std.mem.eql(u8, bind_host, "0.0.0.0") or std.mem.eql(u8, bind_host, "::");
        var lan_buf: [32]u8 = undefined;
        const lan: ?[]const u8 = if (any) (if (lanIp(io)) |b| (std.fmt.bufPrint(&lan_buf, "{d}.{d}.{d}.{d}", .{ b[0], b[1], b[2], b[3] }) catch null) else null) else bind_host;
        const shown = lan orelse "<this-machine-ip>";
        if (cfg.token) |t| try o.print("LAN:  http://{s}:{d}/?token={s}\n", .{ shown, port, t }) else try o.print("LAN:  http://{s}:{d}/   (NO TOKEN: anyone on this network can edit the folder and run agents)\n", .{ shown, port });
    }
    if (ui_assets.files.len == 0) try o.writeAll("note: this binary has no embedded web UI (build with -Dui=<dist dir>); the API is available.\n");
    try o.flush();

    server.ui_csp = buildUiCsp(gpa) catch default_ui_csp;
    server.group.concurrent(io, pollerTask, .{&server}) catch {};
    server.group.concurrent(io, connWatchdog, .{&server}) catch {};
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
    _ = ws;
    _ = http;
    _ = events;
    _ = agents;
    _ = proxy;
}
