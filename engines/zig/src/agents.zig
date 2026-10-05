//! Agent bridge: run a local coding-agent CLI (Claude Code, Grok Build, Codex, …) headless inside the
//! served folder and stream its stdout to SSE `agent` events. Agents are command templates, so a new
//! CLI is configuration (`<dir>/.kerf/agents.json`), not code.
//!
//! Template shape (also the built-ins):
//!   { "id": "claude", "name": "Claude Code",
//!     "detect": ["claude", "--version"],
//!     "argv":   ["claude", "-p", "{message}", "--output-format", "stream-json", "--verbose", "{resume}"],
//!     "resume": ["--resume", "{session_id}"] }
//! Placeholders: {message} (workspace prefix + the user's text), {session_id}, {dir}, {file}. The single
//! element "{resume}" expands to the `resume` list when a session_id was given and to nothing otherwise.
//! Session ids are picked up generically from any top-level `session_id` / `sessionId` / `thread_id` string
//! in the agent's JSON lines (Claude: system/init + result; Grok: end; Codex: thread.started).

const std = @import("std");
const builtin = @import("builtin");
const kerf = @import("kerf");
const http = @import("http.zig");
const events = @import("events.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const Template = struct {
    id: []const u8,
    name: []const u8,
    detect: []const []const u8,
    argv: []const []const u8,
    resume_args: []const []const u8 = &.{},
};

/// Verified against the vendors' docs (see NOTES.md, "Agent CLI research").
pub const builtin_templates = [_]Template{
    .{
        .id = "claude",
        .name = "Claude Code",
        .detect = &.{ "claude", "--version" },
        .argv = &.{
            "claude",                 "-p",                "{message}",
            "--output-format",        "stream-json",       "--verbose",
            "{resume}",               "--permission-mode", "acceptEdits",
            "--allowedTools",         "Bash(kerf:*)",      "Bash(kerf *)",
            "Read",                   "Write",             "Edit",
        },
        .resume_args = &.{ "--resume", "{session_id}" },
    },
    .{
        .id = "grok",
        .name = "Grok Build",
        .detect = &.{ "grok", "--version" },
        .argv = &.{ "grok", "-p", "{message}", "--output-format", "streaming-json", "--always-approve", "--cwd", "{dir}", "{resume}" },
        .resume_args = &.{ "--resume", "{session_id}" },
    },
    .{
        .id = "codex",
        .name = "Codex CLI",
        .detect = &.{ "codex", "--version" },
        // `codex exec resume` does not take --sandbox/--cd itself, so the shared options come before the subcommand.
        .argv = &.{ "codex", "exec", "--sandbox", "workspace-write", "--skip-git-repo-check", "--cd", "{dir}", "--json", "{resume}", "{message}" },
        .resume_args = &.{ "resume", "{session_id}" },
    },
};

pub const prefix_fmt = "You are working in a Kerf details library. Run 'kerf guide' first if you haven't this session.";

pub fn contextMessage(a: Allocator, message: []const u8, file: ?[]const u8) Allocator.Error![]u8 {
    if (file) |f| return std.fmt.allocPrint(a, "{s} Current document: {s}.\n\n{s}", .{ prefix_fmt, f, message });
    return std.fmt.allocPrint(a, "{s}\n\n{s}", .{ prefix_fmt, message });
}

/// Built-ins, with `<dir>/.kerf/agents.json` entries added (new id) or replacing (same id).
/// The file is either `{ "agents": [...] }` or a bare array. Bad entries are skipped.
pub fn loadTemplates(a: Allocator, io: Io, dir: Io.Dir) Allocator.Error![]Template {
    var list: std.ArrayList(Template) = .empty;
    try list.appendSlice(a, &builtin_templates);
    const text = dir.readFileAlloc(io, ".kerf/agents.json", a, .limited(1 << 20)) catch return list.items;
    var perr: kerf.json.ParseError = undefined;
    const root = (try kerf.json.parse(a, text, &perr)) orelse return list.items;
    const arr: []kerf.json.Value = (if (root == .array) root.arr() else if (root.get("agents")) |v| v.arr() else null) orelse return list.items;
    for (arr) |item| {
        const t = (try parseTemplate(a, item)) orelse continue;
        var replaced = false;
        for (list.items) |*ex| if (std.mem.eql(u8, ex.id, t.id)) {
            ex.* = t;
            replaced = true;
        };
        if (!replaced) try list.append(a, t);
    }
    return list.items;
}

fn strList(a: Allocator, v: ?kerf.json.Value) Allocator.Error!?[]const []const u8 {
    const arr = (v orelse return null).arr() orelse return null;
    var out: std.ArrayList([]const u8) = .empty;
    for (arr) |e| try out.append(a, e.str() orelse return null);
    return out.items;
}

fn parseTemplate(a: Allocator, v: kerf.json.Value) Allocator.Error!?Template {
    if (v != .object) return null;
    const id = (if (v.get("id")) |x| x.str() else null) orelse return null;
    if (id.len == 0 or id.len > 40) return null;
    for (id) |c| if (!(std.ascii.isAlphanumeric(c) or c == '-' or c == '_')) return null;
    const argv = (try strList(a, v.get("argv"))) orelse return null;
    if (argv.len == 0) return null;
    const det = (try strList(a, v.get("detect"))) orelse (&[_][]const u8{ argv[0], "--version" });
    if (det.len == 0) return null;
    return .{
        .id = id,
        .name = if (v.get("name")) |n| (n.str() orelse id) else id,
        .detect = det,
        .argv = argv,
        .resume_args = (try strList(a, v.get("resume"))) orelse &.{},
    };
}

/// Expand placeholders. `session_id` null drops the {resume} element.
pub fn expandArgv(a: Allocator, t: Template, message: []const u8, session_id: ?[]const u8, dir: []const u8, file: []const u8) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    for (t.argv) |arg| {
        if (std.mem.eql(u8, arg, "{resume}")) {
            if (session_id) |sid| for (t.resume_args) |ra| try out.append(a, try subst(a, ra, message, sid, dir, file));
            continue;
        }
        try out.append(a, try subst(a, arg, message, session_id orelse "", dir, file));
    }
    return out.items;
}

fn subst(a: Allocator, s: []const u8, message: []const u8, sid: []const u8, dir: []const u8, file: []const u8) Allocator.Error![]const u8 {
    if (std.mem.indexOfScalar(u8, s, '{') == null) return s;
    var cur: []const u8 = s;
    const pairs = [_]struct { []const u8, []const u8 }{
        .{ "{message}", message }, .{ "{session_id}", sid }, .{ "{dir}", dir }, .{ "{file}", file },
    };
    for (pairs) |p| {
        if (std.mem.indexOf(u8, cur, p[0]) == null) continue;
        cur = try std.mem.replaceOwned(u8, a, cur, p[0], p[1]);
    }
    return cur;
}

pub const Detected = struct {
    available: bool,
    version: ?[]const u8 = null,
    reason: ?[]const u8 = null,
};

pub fn detect(gpa: Allocator, io: Io, t: Template, cwd: []const u8) Detected {
    const res = std.process.run(gpa, io, .{
        .argv = t.detect,
        .cwd = .{ .path = cwd },
        .stdout_limit = .limited(8192),
        .stderr_limit = .limited(8192),
        .timeout = .{ .duration = .{ .raw = Io.Duration.fromSeconds(8), .clock = .awake } },
    }) catch |e| {
        const why = switch (e) {
            error.FileNotFound => std.fmt.allocPrint(gpa, "`{s}` was not found on PATH", .{t.detect[0]}) catch "not found on PATH",
            error.Timeout => std.fmt.allocPrint(gpa, "`{s}` did not answer within 8 s", .{t.detect[0]}) catch "timed out",
            else => std.fmt.allocPrint(gpa, "could not run `{s}`: {s}", .{ t.detect[0], @errorName(e) }) catch "could not run",
        };
        return .{ .available = false, .reason = why };
    };
    defer gpa.free(res.stdout);
    defer gpa.free(res.stderr);
    const ok = switch (res.term) {
        .exited => |c| c == 0,
        else => false,
    };
    if (!ok) {
        const why = std.fmt.allocPrint(gpa, "`{s}` exited with an error (try running it once in a terminal to sign in)", .{t.detect[0]}) catch "exited with an error";
        return .{ .available = false, .reason = why };
    }
    const text = if (std.mem.trim(u8, res.stdout, " \t\r\n").len > 0) res.stdout else res.stderr;
    const first = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, text, '\n')) |nl| text[0..nl] else text, " \t\r");
    const ver = gpa.dupe(u8, first[0..@min(first.len, 80)]) catch null;
    return .{ .available = true, .version = ver };
}

const detect_ttl_s = 30;

pub const Manager = struct {
    gpa: Allocator,
    mu: Io.Mutex = .init,
    hub: *events.Hub,
    dir: Io.Dir,
    dir_abs: []const u8,
    environ: *const std.process.Environ.Map,
    /// Directory of the running `kerf` binary: put on the agent's PATH so `kerf` resolves.
    exe_dir: []const u8,
    /// Hook run right before the `exit` event is published (the server scans the folder so the agent's last
    /// edits are reported before the run ends).
    before_exit_ctx: ?*anyopaque = null,
    before_exit: ?*const fn (*anyopaque) void = null,
    next_id: u32 = 1,
    active: ?*Run = null,
    cache: std.ArrayList(CacheEntry) = .empty,

    const CacheEntry = struct { key: []u8, at_s: i64, det: Detected };

    pub const Run = struct {
        id: []u8,
        agent: []u8,
        child: std.process.Child = undefined,
        finished: bool = false,
        stop_requested: bool = false,
        session_id: ?[]u8 = null,
    };

    pub fn deinit(m: *Manager) void {
        for (m.cache.items) |c| m.gpa.free(c.key);
        m.cache.deinit(m.gpa);
    }

    /// Cached detection (30 s) so /api/info stays fast. Strings are owned by the cache; callers copy.
    pub fn detectCached(m: *Manager, io: Io, t: Template) Detected {
        const now_s: i64 = @intCast(@divTrunc(Io.Timestamp.now(io, .awake).nanoseconds, std.time.ns_per_s));
        m.mu.lockUncancelable(io);
        for (m.cache.items) |c| if (std.mem.eql(u8, c.key, t.id) and now_s - c.at_s < detect_ttl_s) {
            const d = c.det;
            m.mu.unlock(io);
            return d;
        };
        m.mu.unlock(io);
        const det = detect(m.gpa, io, t, m.dir_abs);
        m.mu.lockUncancelable(io);
        defer m.mu.unlock(io);
        for (m.cache.items) |*c| if (std.mem.eql(u8, c.key, t.id)) {
            // Old strings leak into the gpa on purpose (a few bytes per refresh, bounded by the TTL).
            c.at_s = now_s;
            c.det = det;
            return det;
        };
        const key = m.gpa.dupe(u8, t.id) catch return det;
        m.cache.append(m.gpa, .{ .key = key, .at_s = now_s, .det = det }) catch {};
        return det;
    }

    pub fn activeInfo(m: *Manager, io: Io, a: Allocator) ?struct { run_id: []u8, agent: []u8 } {
        m.mu.lockUncancelable(io);
        defer m.mu.unlock(io);
        const r = m.active orelse return null;
        if (r.finished) return null;
        return .{ .run_id = a.dupe(u8, r.id) catch return null, .agent = a.dupe(u8, r.agent) catch return null };
    }
};

pub const StartError = union(enum) {
    busy: []const u8, // run id of the active run
    unknown_agent,
    unavailable: []const u8,
    spawn_failed: []const u8,
};

pub const StartResult = union(enum) { started: []const u8, failed: StartError };

pub const StartParams = struct {
    template: Template,
    message: []const u8,
    session_id: ?[]const u8,
    file: ?[]const u8,
};

/// Spawn the agent and a supervisor task (in `group`); returns the run id (owned by `a`).
pub fn start(m: *Manager, io: Io, a: Allocator, group: *Io.Group, p: StartParams) Allocator.Error!StartResult {
    m.mu.lockUncancelable(io);
    if (m.active) |r| if (!r.finished) {
        const id = try a.dupe(u8, r.id);
        m.mu.unlock(io);
        return .{ .failed = .{ .busy = id } };
    };
    m.mu.unlock(io);

    const det = m.detectCached(io, p.template);
    if (!det.available) return .{ .failed = .{ .unavailable = try a.dupe(u8, det.reason orelse "not available") } };

    const full = try contextMessage(a, p.message, p.file);
    const argv = try expandArgv(a, p.template, full, p.session_id, m.dir_abs, p.file orelse "");

    var env = try m.environ.clone(a);
    const old_path = env.get("PATH") orelse env.get("Path") orelse "";
    const sep: u8 = if (builtin.os.tag == .windows) ';' else ':';
    const new_path = if (old_path.len > 0) try std.fmt.allocPrint(a, "{s}{c}{s}", .{ m.exe_dir, sep, old_path }) else m.exe_dir;
    try env.put("PATH", new_path);
    try env.put("KERF_ACTOR", "agent");

    const run = try m.gpa.create(Manager.Run);
    errdefer m.gpa.destroy(run);

    m.mu.lockUncancelable(io);
    const n = m.next_id;
    m.next_id += 1;
    m.mu.unlock(io);
    const run_id = try std.fmt.allocPrint(m.gpa, "r{d}", .{n});
    errdefer m.gpa.free(run_id);
    const agent_id = try m.gpa.dupe(u8, p.template.id);
    errdefer m.gpa.free(agent_id);
    run.* = .{ .id = run_id, .agent = agent_id };

    run.child = std.process.spawn(io, .{
        .argv = argv,
        .cwd = .{ .path = m.dir_abs },
        .environ_map = &env,
        .stdin = .ignore,
        .stdout = .pipe,
        .stderr = .pipe,
        .create_no_window = true,
    }) catch |e| {
        m.gpa.free(agent_id);
        m.gpa.free(run_id);
        m.gpa.destroy(run);
        return .{ .failed = .{ .spawn_failed = try std.fmt.allocPrint(a, "could not start `{s}`: {s}", .{ argv[0], @errorName(e) }) } };
    };

    m.mu.lockUncancelable(io);
    m.active = run;
    m.mu.unlock(io);

    group.concurrent(io, supervise, .{ m, io, run }) catch {
        m.mu.lockUncancelable(io);
        run.finished = true;
        m.mu.unlock(io);
        run.child.kill(io);
        m.mu.lockUncancelable(io);
        m.active = null;
        m.mu.unlock(io);
        m.gpa.free(agent_id);
        m.gpa.free(run_id);
        m.gpa.destroy(run);
        return .{ .failed = .{ .spawn_failed = "no thread available to supervise the agent" } };
    };
    return .{ .started = try a.dupe(u8, run_id) };
}

pub const StopResult = enum { stopped, no_such_run, already_finished };

pub fn stop(m: *Manager, io: Io, run_id: []const u8) StopResult {
    m.mu.lockUncancelable(io);
    defer m.mu.unlock(io);
    const r = m.active orelse return .no_such_run;
    if (!std.mem.eql(u8, r.id, run_id)) return .no_such_run;
    if (r.finished) return .already_finished;
    r.stop_requested = true;
    terminate(&r.child);
    return .stopped;
}

/// Ask the OS to end the process (SIGTERM / TerminateProcess). Never waits; the supervisor reaps it.
fn terminate(child: *std.process.Child) void {
    const id = child.id orelse return;
    if (builtin.os.tag == .windows) {
        _ = std.os.windows.ntdll.NtTerminateProcess(id, @enumFromInt(1));
    } else {
        _ = std.posix.system.kill(id, .TERM);
    }
}

const max_event_bytes = 1 << 20;

fn supervise(m: *Manager, io: Io, run: *Manager.Run) void {
    var pumps: Io.Group = .init;
    const so = run.child.stdout.?;
    const se = run.child.stderr.?;
    var ok_concurrent = true;
    pumps.concurrent(io, pump, .{ m, io, run, so, false }) catch {
        ok_concurrent = false;
    };
    if (ok_concurrent) {
        pumps.concurrent(io, pump, .{ m, io, run, se, true }) catch {
            ok_concurrent = false;
        };
    }
    if (!ok_concurrent) terminate(&run.child);
    pumps.await(io) catch {};

    m.mu.lockUncancelable(io);
    run.finished = true;
    const stopped = run.stop_requested;
    m.mu.unlock(io);

    var code: i32 = -1;
    var signal: ?i32 = null;
    if (run.child.wait(io)) |term| switch (term) {
        .exited => |c| code = c,
        .signal => |s| {
            signal = @intCast(@intFromEnum(s));
            code = 128 + signal.?;
        },
        else => {},
    } else |_| {}

    if (m.before_exit) |f| f(m.before_exit_ctx.?);

    var arena = std.heap.ArenaAllocator.init(m.gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var ev: std.ArrayList(u8) = .empty;
    ev.print(a, "{{\"type\":\"exit\",\"code\":{d}", .{code}) catch {};
    if (stopped) ev.appendSlice(a, ",\"stopped\":true") catch {};
    m.mu.lockUncancelable(io);
    const sid = run.session_id;
    m.mu.unlock(io);
    if (sid) |s| {
        ev.appendSlice(a, ",\"session_id\":") catch {};
        http.jsonString(&ev, a, s) catch {};
    }
    ev.append(a, '}') catch {};
    publishEvent(m, io, a, run.id, ev.items);

    m.mu.lockUncancelable(io);
    m.active = null;
    m.mu.unlock(io);
    if (run.session_id) |s| m.gpa.free(s);
    m.gpa.free(run.id);
    m.gpa.free(run.agent);
    m.gpa.destroy(run);
}

fn publishEvent(m: *Manager, io: Io, a: Allocator, run_id: []const u8, event_json: []const u8) void {
    var out: std.ArrayList(u8) = .empty;
    out.appendSlice(a, "{\"run_id\":") catch return;
    http.jsonString(&out, a, run_id) catch return;
    out.appendSlice(a, ",\"event\":") catch return;
    out.appendSlice(a, event_json) catch return;
    out.append(a, '}') catch return;
    m.hub.publish(io, "agent", out.items);
}

fn pump(m: *Manager, io: Io, run: *Manager.Run, file: Io.File, is_stderr: bool) void {
    var buf: [16 * 1024]u8 = undefined;
    var rd = file.readerStreaming(io, &buf);
    const r = &rd.interface;
    var line: std.ArrayList(u8) = .empty;
    defer line.deinit(m.gpa);
    while (true) {
        line.clearRetainingCapacity();
        var too_long = false;
        // Accumulate one line (bounded); an over-long line is swallowed and replaced by a stub event.
        while (true) {
            const chunk = r.peekGreedy(1) catch {
                if (line.items.len > 0 and !too_long) emitLine(m, io, run, line.items, is_stderr);
                return;
            };
            if (std.mem.indexOfScalar(u8, chunk, '\n')) |nl| {
                if (!too_long) {
                    if (line.items.len + nl > max_event_bytes) too_long = true else line.appendSlice(m.gpa, chunk[0..nl]) catch {};
                }
                r.toss(nl + 1);
                break;
            }
            if (!too_long) {
                if (line.items.len + chunk.len > max_event_bytes) too_long = true else line.appendSlice(m.gpa, chunk) catch {};
            }
            r.toss(chunk.len);
        }
        if (too_long) {
            var arena = std.heap.ArenaAllocator.init(m.gpa);
            defer arena.deinit();
            publishEvent(m, io, arena.allocator(), run.id, "{\"type\":\"truncated\",\"text\":\"(output line over 1 MiB omitted)\"}");
        } else emitLine(m, io, run, line.items, is_stderr);
    }
}

fn emitLine(m: *Manager, io: Io, run: *Manager.Run, raw: []const u8, is_stderr: bool) void {
    const text = std.mem.trimEnd(u8, raw, "\r");
    if (std.mem.trim(u8, text, " \t").len == 0) return;
    var arena = std.heap.ArenaAllocator.init(m.gpa);
    defer arena.deinit();
    const a = arena.allocator();
    if (!is_stderr) {
        var perr: kerf.json.ParseError = undefined;
        if (kerf.json.parse(a, text, &perr) catch null) |v| if (v == .object) {
            captureSession(m, io, run, v);
            publishEvent(m, io, a, run.id, std.mem.trim(u8, text, " \t"));
            return;
        };
    }
    var ev: std.ArrayList(u8) = .empty;
    ev.appendSlice(a, if (is_stderr) "{\"type\":\"stderr\",\"text\":" else "{\"type\":\"text\",\"text\":") catch return;
    http.jsonString(&ev, a, text) catch return;
    ev.append(a, '}') catch return;
    publishEvent(m, io, a, run.id, ev.items);
}

fn captureSession(m: *Manager, io: Io, run: *Manager.Run, v: kerf.json.Value) void {
    for ([_][]const u8{ "session_id", "sessionId", "thread_id" }) |key| {
        const s = (v.get(key) orelse continue).str() orelse continue;
        if (s.len == 0 or s.len > 200) continue;
        const copy = m.gpa.dupe(u8, s) catch return;
        m.mu.lockUncancelable(io);
        defer m.mu.unlock(io);
        if (run.session_id) |old| m.gpa.free(old);
        run.session_id = copy;
        return;
    }
}

test "expandArgv: resume, placeholders" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const claude = builtin_templates[0];
    const fresh = try expandArgv(a, claude, "MSG", null, "/d", "a.kerf.json");
    try std.testing.expectEqualStrings("claude", fresh[0]);
    try std.testing.expectEqualStrings("MSG", fresh[2]);
    for (fresh) |x| try std.testing.expect(!std.mem.eql(u8, x, "--resume"));
    const resumed = try expandArgv(a, claude, "MSG", "sess-1", "/d", "");
    var found = false;
    for (resumed, 0..) |x, i| if (std.mem.eql(u8, x, "--resume")) {
        found = true;
        try std.testing.expectEqualStrings("sess-1", resumed[i + 1]);
    };
    try std.testing.expect(found);
    const codex = try expandArgv(a, builtin_templates[2], "MSG", "t-9", "/work", "");
    try std.testing.expectEqualStrings("exec", codex[1]);
    try std.testing.expectEqualStrings("/work", codex[6]);
    try std.testing.expectEqualStrings("resume", codex[8]);
    try std.testing.expectEqualStrings("t-9", codex[9]);
    try std.testing.expectEqualStrings("MSG", codex[10]);
    const grok = try expandArgv(a, builtin_templates[1], "MSG", null, "/work", "");
    try std.testing.expectEqualStrings("/work", grok[7]);
}

test "contextMessage" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const m = try contextMessage(a, "Add anchors", "truss.kerf.json");
    try std.testing.expect(std.mem.indexOf(u8, m, "Current document: truss.kerf.json.") != null);
    try std.testing.expect(std.mem.endsWith(u8, m, "Add anchors"));
}

test "parseTemplate" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var pe: kerf.json.ParseError = undefined;
    const v = (try kerf.json.parse(a, "{\"id\":\"fake\",\"argv\":[\"node\",\"x.js\",\"{message}\",\"{resume}\"],\"resume\":[\"--r\",\"{session_id}\"]}", &pe)).?;
    const t = (try parseTemplate(a, v)).?;
    try std.testing.expectEqualStrings("node", t.detect[0]);
    try std.testing.expectEqual(@as(usize, 2), t.resume_args.len);
    const bad = (try kerf.json.parse(a, "{\"id\":\"bad id\",\"argv\":[\"x\"]}", &pe)).?;
    try std.testing.expect((try parseTemplate(a, bad)) == null);
}
