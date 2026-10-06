//! kerf serve startup (spec/SERVE.md): argument parsing, the trust prompt for workspace agents, bind and banner, signal handling, the poller and accept loop.

const std = @import("std");
const builtin = @import("builtin");
const kerf = @import("kerf");
const http = @import("../http.zig");
const events = @import("../events.zig");
const agents = @import("../agents.zig");
const ui_assets = @import("ui_assets");
const Io = std.Io;
const net = std.Io.net;
const Allocator = std.mem.Allocator;
const default_ui_csp = @import("ui.zig").default_ui_csp;
const buildUiCsp = @import("ui.zig").buildUiCsp;
const cleanupAttachments = @import("agent.zig").cleanupAttachments;
const max_conns = @import("conn.zig").max_conns;
const connWatchdog = @import("conn.zig").connWatchdog;
const serveConn = @import("conn.zig").serveConn;
const serve = @import("../serve.zig");
const Server = serve.Server;
const poll_ms = serve.poll_ms;
const Config = serve.Config;
const serve_usage = serve.serve_usage;

/// Called by the agent bridge just before it publishes `exit`: report the agent's last edits (doc + log
/// events) now instead of up to 500 ms later, so the UI sees them before the run ends.
pub fn scanForAgent(ctx: *anyopaque) void {
    const s: *Server = @ptrCast(@alignCast(ctx));
    s.scan(true);
}

/// Set by the SIGINT/SIGTERM/SIGHUP handler; the poller notices it, ends the active agent run (and its process tree)
/// and exits, so no agent is reparented to init when the server is stopped.
var shutdown_signal = std.atomic.Value(u8).init(0);

pub fn onSignal(sig: std.posix.SIG) callconv(.c) void {
    shutdown_signal.store(@intCast(@intFromEnum(sig)), .release);
}

pub fn installSignalHandlers() void {
    if (builtin.os.tag == .windows or builtin.os.tag == .wasi) return; // Windows: the agent's job object dies with the server
    const act: std.posix.Sigaction = .{ .handler = .{ .handler = onSignal }, .mask = std.posix.sigemptyset(), .flags = 0 };
    for ([_]std.posix.SIG{ .INT, .TERM, .HUP }) |sg| std.posix.sigaction(sg, &act, null);
}

pub fn pollerTask(s: *Server) void {
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

pub fn acceptLoop(s: *Server, server: *net.Server) void {
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
pub fn refuse(s: *Server, stream: net.Stream) void {
    const msg = "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nRetry-After: 2\r\nContent-Length: 0\r\n\r\n";
    var b: [128]u8 = undefined;
    var sw = stream.writer(s.io, &b);
    sw.interface.writeAll(msg) catch {};
    sw.interface.flush() catch {};
    stream.close(s.io);
}

pub fn prefetchAgents(s: *Server) void {
    var arena = std.heap.ArenaAllocator.init(s.gpa);
    defer arena.deinit();
    const templates = agents.loadTemplates(arena.allocator(), s.io, s.dir, s.cfg.trust_agents) catch return;
    for (templates) |t| _ = s.agent_mgr.detectCached(s.io, arena.allocator(), t); // untrusted workspace entries are skipped inside
}

pub fn openBrowser(s: *Server, url: []const u8) void {
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
pub fn lanIp(io: Io) ?[4]u8 {
    const target: net.IpAddress = .{ .ip4 = net.Ip4Address.parse("192.0.2.1", 9) catch return null };
    const stream = target.connect(io, .{ .mode = .dgram }) catch return null;
    defer stream.close(io);
    return switch (stream.socket.address) {
        .ip4 => |a4| if (std.mem.eql(u8, &a4.bytes, &[_]u8{ 0, 0, 0, 0 })) null else a4.bytes,
        else => null,
    };
}

/// True when something already accepts connections on host:port (wildcard hosts are probed via loopback).
pub fn portInUse(io: Io, host: []const u8, port: u16) bool {
    const probe_host = if (std.mem.eql(u8, host, "0.0.0.0")) "127.0.0.1" else if (std.mem.eql(u8, host, "::")) "::1" else host;
    const addr = net.IpAddress.parse(probe_host, port) catch return false;
    const stream = addr.connect(io, .{ .mode = .stream }) catch return false;
    stream.close(io);
    return true;
}

pub fn isLoopbackHost(host: []const u8) bool {
    return std.ascii.eqlIgnoreCase(host, "localhost") or std.mem.eql(u8, host, "::1") or std.mem.startsWith(u8, host, "127.");
}

/// V-2: `<dir>/.kerf/agents.json` makes the server run commands from the folder. Without `--trust-agents` that needs an
/// explicit yes on a terminal; anywhere else the entries stay untrusted (listed, never executed).
pub fn confirmWorkspaceAgents(gpa: Allocator, io: Io, dir: Io.Dir, dir_abs: []const u8, err: *Io.Writer) bool {
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
