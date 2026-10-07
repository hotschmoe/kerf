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
//! in the agent's JSON lines (Claude: system/init + result; Grok: end; Codex: thread.started), plus pi's
//! `{"type":"session","id":...}` first event.

const std = @import("std");
const builtin = @import("builtin");
const kerf = @import("kerf");
const fsx = @import("fsx.zig");
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
    /// Defined by `<dir>/.kerf/agents.json` instead of built in.
    from_workspace: bool = false,
    /// A workspace template the user has not trusted (no `--trust-agents`, no interactive yes): it is listed but
    /// nothing of it is ever executed, not even its detect command.
    untrusted: bool = false,
};

pub const untrusted_reason = "defined by this folder's .kerf/agents.json and not trusted: it would run a command from the folder. Start `kerf serve --trust-agents` (or answer yes at the prompt) if you wrote it";

/// Verified against the vendors' docs (see NOTES.md, "Agent CLI research").
pub const builtin_templates = [_]Template{
    .{
        .id = "claude",
        .name = "Claude Code",
        .detect = &.{ "claude", "--version" },
        .argv = &.{
            "claude",          "-p",                "{message}",
            "--output-format", "stream-json",       "--verbose",
            "{resume}",        "--permission-mode", "acceptEdits",
            "--allowedTools",  "Bash(kerf:*)",      "Bash(kerf *)",
            "Read",            "Write",             "Edit",
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
    .{
        // pi (earendil-works/pi): headless print mode, JSON event lines; uses the model configured in
        // ~/.pi/agent/settings.json (e.g. a self-hosted vLLM endpoint). First event: {"type":"session","id":...}.
        .id = "pi",
        .name = "Pi",
        .detect = &.{ "pi", "--version" },
        .argv = &.{ "pi", "-p", "--mode", "json", "{resume}", "{message}" },
        .resume_args = &.{ "--session-id", "{session_id}" },
    },
};

pub const prefix_fmt = "You are working in a Kerf details library. Run 'kerf guide' first if you haven't this session.";

pub fn contextMessage(a: Allocator, message: []const u8, file: ?[]const u8) Allocator.Error![]u8 {
    if (file) |f| return std.fmt.allocPrint(a, "{s} Current document: {s}.\n\n{s}", .{ prefix_fmt, f, message });
    return std.fmt.allocPrint(a, "{s}\n\n{s}", .{ prefix_fmt, message });
}

/// Built-ins, with `<dir>/.kerf/agents.json` entries added (new id) or, when `trust` is set, replacing (same id).
/// The file is either `{ "agents": [...] }` or a bare array. Bad entries are skipped. Without `trust` a workspace entry
/// can never replace a built-in (it is dropped) and every workspace entry carries `untrusted`: parsing the file is all
/// that happens, nothing is executed.
pub fn loadTemplates(a: Allocator, io: Io, dir: Io.Dir, trust: bool) Allocator.Error![]Template {
    var list: std.ArrayList(Template) = .empty;
    try list.appendSlice(a, &builtin_templates);
    const text = readWorkspaceFile(a, io, dir, ".kerf/agents.json") orelse return list.items;
    var perr: kerf.json.ParseError = undefined;
    const root = (try kerf.json.parse(a, text, &perr)) orelse return list.items;
    const arr: []kerf.json.Value = (if (root == .array) root.arr() else if (root.get("agents")) |v| v.arr() else null) orelse return list.items;
    for (arr) |item| {
        var t = (try parseTemplate(a, item)) orelse continue;
        t.from_workspace = true;
        t.untrusted = !trust;
        var replaced = false;
        for (list.items) |*ex| if (std.mem.eql(u8, ex.id, t.id)) {
            if (trust and !ex.from_workspace) ex.* = t; // an untrusted entry never takes over a built-in id
            replaced = true;
        };
        if (!replaced) try list.append(a, t);
    }
    return list.items;
}

/// Read a small file of the served folder without following a symlink at the last component (a prompt-injected
/// agent can plant `.kerf/agents.json -> /somewhere`).
fn readWorkspaceFile(a: Allocator, io: Io, dir: Io.Dir, sub: []const u8) ?[]u8 {
    var f = fsx.openFileNoFollow(io, dir, sub) catch return null;
    defer f.close(io);
    var rb: [4096]u8 = undefined;
    var fr = f.reader(io, &rb);
    return fr.interface.allocRemaining(a, .limited(1 << 20)) catch null;
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

/// Result of probing a template's binary. Strings are owned by whoever got them (see `detectCached`).
pub const Detected = struct {
    available: bool,
    version: ?[]const u8 = null,
    reason: ?[]const u8 = null,
};

/// Run the template's detect command (`<binary> --version` for every built-in) and describe the outcome; strings are
/// allocated in `a`. Never call this for an untrusted template (it would execute a command from the workspace):
/// it refuses.
pub fn detect(a: Allocator, io: Io, t: Template) Detected {
    if (t.untrusted) return .{ .available = false, .reason = a.dupe(u8, untrusted_reason) catch null };
    const res = std.process.run(a, io, .{
        .argv = t.detect,
        // Not the served folder: a detect command resolves through PATH only.
        .stdout_limit = .limited(8192),
        .stderr_limit = .limited(8192),
        .timeout = .{ .duration = .{ .raw = Io.Duration.fromSeconds(8), .clock = .awake } },
    }) catch |e| {
        const why = switch (e) {
            error.FileNotFound => std.fmt.allocPrint(a, "`{s}` was not found on PATH", .{t.detect[0]}),
            error.Timeout => std.fmt.allocPrint(a, "`{s}` did not answer within 8 s", .{t.detect[0]}),
            else => std.fmt.allocPrint(a, "could not run `{s}`: {s}", .{ t.detect[0], @errorName(e) }),
        } catch null;
        return .{ .available = false, .reason = why };
    };
    defer a.free(res.stdout);
    defer a.free(res.stderr);
    const ok = switch (res.term) {
        .exited => |c| c == 0,
        else => false,
    };
    if (!ok) {
        const why = std.fmt.allocPrint(a, "`{s}` exited with an error (try running it once in a terminal to sign in)", .{t.detect[0]}) catch null;
        return .{ .available = false, .reason = why };
    }
    const text = if (std.mem.trim(u8, res.stdout, " \t\r\n").len > 0) res.stdout else res.stderr;
    const first = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, text, '\n')) |nl| text[0..nl] else text, " \t\r");
    const ver = a.dupe(u8, first[0..@min(first.len, 80)]) catch null;
    return .{ .available = true, .version = ver };
}

const detect_ttl_s = 30;
/// Hard wall-clock limit of one agent run unless the Manager says otherwise.
pub const default_max_run_s: u32 = 30 * 60;
/// SIGTERM to SIGKILL grace for the whole process group.
const kill_grace_ms = 3000;

// ---------------------------------------------------------------------------------------------
// Process groups (POSIX) and job objects (Windows)
// ---------------------------------------------------------------------------------------------

/// Windows: `kernel32` job objects. A job with KILL_ON_JOB_CLOSE ends the agent and everything it spawned, also when
/// the server itself dies. Not in std 0.16, so the few calls are declared here. Cross-compiled in CI but not run on a
/// real Windows host from this repo; when creating or assigning the job fails the run degrades to terminating the
/// agent's own process (grandchildren may survive).
const winjob = if (builtin.os.tag == .windows) struct {
    const w = std.os.windows;
    const BOOL = c_int;
    extern "kernel32" fn CreateJobObjectW(attrs: ?*anyopaque, name: ?[*:0]const u16) callconv(.winapi) ?w.HANDLE;
    extern "kernel32" fn SetInformationJobObject(job: w.HANDLE, class: c_int, info: *const anyopaque, len: u32) callconv(.winapi) BOOL;
    extern "kernel32" fn AssignProcessToJobObject(job: w.HANDLE, process: w.HANDLE) callconv(.winapi) BOOL;
    extern "kernel32" fn TerminateJobObject(job: w.HANDLE, code: u32) callconv(.winapi) BOOL;
    extern "kernel32" fn ResumeThread(thread: w.HANDLE) callconv(.winapi) u32;

    const BasicLimit = extern struct {
        PerProcessUserTimeLimit: i64 = 0,
        PerJobUserTimeLimit: i64 = 0,
        LimitFlags: u32 = 0,
        MinimumWorkingSetSize: usize = 0,
        MaximumWorkingSetSize: usize = 0,
        ActiveProcessLimit: u32 = 0,
        Affinity: usize = 0,
        PriorityClass: u32 = 0,
        SchedulingClass: u32 = 0,
    };
    const ExtLimit = extern struct {
        Basic: BasicLimit = .{},
        Io: [6]u64 = .{ 0, 0, 0, 0, 0, 0 },
        ProcessMemoryLimit: usize = 0,
        JobMemoryLimit: usize = 0,
        PeakProcessMemoryUsed: usize = 0,
        PeakJobMemoryUsed: usize = 0,
    };
    const kill_on_close: u32 = 0x2000;
    const extended_limit_class: c_int = 9;

    fn create() ?w.HANDLE {
        const job = CreateJobObjectW(null, null) orelse return null;
        const info: ExtLimit = .{ .Basic = .{ .LimitFlags = kill_on_close } };
        if (SetInformationJobObject(job, extended_limit_class, &info, @sizeOf(ExtLimit)) == 0) {
            w.CloseHandle(job);
            return null;
        }
        return job;
    }
} else struct {};

/// How to end an agent and everything it started.
const ProcGroup = struct {
    /// POSIX: the process group id (== the agent's pid, it is spawned as a group leader).
    pgid: if (builtin.os.tag == .windows) void else std.posix.pid_t = if (builtin.os.tag == .windows) {} else 0,
    /// Windows: the job object (null = degraded, only the agent's own process is terminated).
    job: if (builtin.os.tag == .windows) ?std.os.windows.HANDLE else void = if (builtin.os.tag == .windows) null else {},
    /// Windows degraded mode: the agent's process handle (valid until `Child.wait`).
    leader: if (builtin.os.tag == .windows) ?std.os.windows.HANDLE else void = if (builtin.os.tag == .windows) null else {},

    /// `hard` = SIGKILL / TerminateJobObject; otherwise SIGTERM (Windows has no soft signal: both are a hard kill).
    fn signal(g: ProcGroup, hard: bool) void {
        if (builtin.os.tag == .windows) {
            if (g.job) |j| {
                _ = winjob.TerminateJobObject(j, 1);
            } else if (g.leader) |h| {
                _ = std.os.windows.ntdll.NtTerminateProcess(h, @enumFromInt(1));
            }
        } else {
            if (g.pgid > 1) _ = std.posix.system.kill(-g.pgid, if (hard) .KILL else .TERM);
        }
    }

    fn close(g: *ProcGroup) void {
        if (builtin.os.tag == .windows) {
            if (g.job) |j| std.os.windows.CloseHandle(j);
            g.job = null;
            g.leader = null;
        }
    }
};

pub const Manager = struct {
    gpa: Allocator,
    mu: Io.Mutex = .init,
    cond: Io.Condition = .init,
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
    /// `.kerf/agents.json` entries are only executed when this is set (V-2).
    trust_agents: bool = false,
    max_run_s: u32 = default_max_run_s,
    next_id: u32 = 1,
    active: ?*Run = null,
    cache: std.ArrayList(CacheEntry) = .empty,

    /// All strings owned by `gpa`; `busy` marks a detection in flight (single flight).
    const CacheEntry = struct { key: []u8, at_s: i64, det: Detected, busy: bool = false };

    pub const Run = struct {
        id: []u8,
        agent: []u8,
        child: std.process.Child = undefined,
        group: ProcGroup = .{},
        /// False while the slot is reserved but the process does not exist yet.
        spawned: bool = false,
        /// The run ended (guarded by `mu`); `active` is cleared right after.
        finished: bool = false,
        stop_requested: bool = false,
        timed_out: bool = false,
        /// The agent's own process exited (so the watchdog has nothing left to escalate).
        leader_done: bool = false,
        /// When SIGTERM went out (awake-clock ns): the watchdog sends SIGKILL `kill_grace_ms` later.
        term_sent_ns: ?i96 = null,
        started_ns: i96 = 0,
        session_id: ?[]u8 = null,
        /// Pump tasks that reached end-of-stream.
        pumps_done: std.atomic.Value(u32) = .init(0),
    };

    pub fn deinit(m: *Manager) void {
        for (m.cache.items) |c| {
            m.gpa.free(c.key);
            freeDetected(m.gpa, c.det);
        }
        m.cache.deinit(m.gpa);
    }

    fn freeDetected(gpa: Allocator, d: Detected) void {
        if (d.version) |v| gpa.free(v);
        if (d.reason) |r| gpa.free(r);
    }

    fn dupDetected(a: Allocator, d: Detected) Detected {
        return .{
            .available = d.available,
            .version = if (d.version) |v| (a.dupe(u8, v) catch null) else null,
            .reason = if (d.reason) |r| (a.dupe(u8, r) catch null) else null,
        };
    }

    /// Cached detection (30 s) so /api/info stays fast; one detection per template at a time (the others wait for its
    /// result). The returned strings are copied into `a`. A template from the workspace that is not trusted is never
    /// executed: it is reported unavailable without touching the cache.
    pub fn detectCached(m: *Manager, io: Io, a: Allocator, t: Template) Detected {
        if (t.untrusted) return .{ .available = false, .reason = a.dupe(u8, untrusted_reason) catch null };
        const now_s: i64 = @intCast(@divTrunc(Io.Timestamp.now(io, .awake).nanoseconds, std.time.ns_per_s));
        // The key covers the command, so a changed agents.json does not serve a stale answer.
        const joined = std.mem.join(a, "\x00", t.detect) catch return .{ .available = false, .reason = "out of memory" };
        const full_key = std.fmt.allocPrint(a, "{s}\x01{s}", .{ t.id, joined }) catch return .{ .available = false, .reason = "out of memory" };
        m.mu.lockUncancelable(io);
        var entry: ?*CacheEntry = null;
        while (true) {
            entry = null;
            for (m.cache.items) |*c| if (std.mem.eql(u8, c.key, full_key)) {
                entry = c;
            };
            if (entry) |c| {
                if (c.busy) {
                    m.cond.waitUncancelable(io, &m.mu);
                    continue;
                }
                if (now_s - c.at_s < detect_ttl_s) {
                    const d = dupDetected(a, c.det);
                    m.mu.unlock(io);
                    return d;
                }
            }
            break;
        }
        if (entry) |c| {
            c.busy = true;
        } else {
            const k = m.gpa.dupe(u8, full_key) catch {
                m.mu.unlock(io);
                return detect(a, io, t);
            };
            m.cache.append(m.gpa, .{ .key = k, .at_s = 0, .det = .{ .available = false }, .busy = true }) catch {
                m.gpa.free(k);
                m.mu.unlock(io);
                return detect(a, io, t);
            };
        }
        m.mu.unlock(io);

        const det = detect(m.gpa, io, t); // owned by gpa, moved into the cache

        m.mu.lockUncancelable(io);
        defer m.mu.unlock(io);
        for (m.cache.items) |*c| if (std.mem.eql(u8, c.key, full_key)) {
            freeDetected(m.gpa, c.det);
            c.det = det;
            c.at_s = now_s;
            c.busy = false;
            break;
        };
        m.cond.broadcast(io);
        return dupDetected(a, det);
    }

    pub fn activeInfo(m: *Manager, io: Io, a: Allocator) ?struct { run_id: []u8, agent: []u8 } {
        m.mu.lockUncancelable(io);
        defer m.mu.unlock(io);
        const r = m.active orelse return null;
        if (r.finished) return null;
        return .{ .run_id = a.dupe(u8, r.id) catch return null, .agent = a.dupe(u8, r.agent) catch return null };
    }

    /// End the active run (if any) and wait until its supervisor has cleaned up, escalating to SIGKILL. Used when the
    /// server itself is going away (SIGINT/SIGTERM) so no agent is left behind.
    pub fn shutdown(m: *Manager, io: Io) void {
        m.mu.lockUncancelable(io);
        if (m.active) |r| if (!r.finished) {
            r.stop_requested = true;
            if (r.spawned and r.term_sent_ns == null) {
                r.group.signal(false);
                r.term_sent_ns = Io.Timestamp.now(io, .awake).nanoseconds;
            }
        };
        m.mu.unlock(io);
        var waited_ms: u32 = 0;
        while (waited_ms < kill_grace_ms + 3000) : (waited_ms += 50) {
            m.mu.lockUncancelable(io);
            const gone = m.active == null;
            m.mu.unlock(io);
            if (gone) return;
            io.sleep(Io.Duration.fromMilliseconds(50), .awake) catch return;
        }
    }
};

pub const StartError = union(enum) {
    busy: []const u8, // run id of the active run
    unknown_agent,
    unavailable: []const u8,
    spawn_failed: []const u8,
    /// The arguments do not fit the OS limit (a client error, not a server fault).
    too_long: []const u8,
};

pub const StartResult = union(enum) { started: []const u8, failed: StartError };

pub const StartParams = struct {
    template: Template,
    message: []const u8,
    session_id: ?[]const u8,
    file: ?[]const u8,
};

/// Spawn the agent and a supervisor task (in `group`); returns the run id (owned by `a`).
/// Order: detection (slow, no lock held) -> reserve the single run slot atomically -> spawn. The slot is taken
/// under `mu` before the process exists, so concurrent requests cannot both start one.
pub fn start(m: *Manager, io: Io, a: Allocator, group: *Io.Group, p: StartParams) Allocator.Error!StartResult {
    if (p.template.untrusted) return .{ .failed = .{ .unavailable = try a.dupe(u8, untrusted_reason) } };

    const det = m.detectCached(io, a, p.template);
    if (!det.available) return .{ .failed = .{ .unavailable = try a.dupe(u8, det.reason orelse "not available") } };

    const full = try contextMessage(a, p.message, p.file);
    const argv = try expandArgv(a, p.template, full, p.session_id, m.dir_abs, p.file orelse "");

    var env = try m.environ.clone(a);
    const old_path = env.get("PATH") orelse env.get("Path") orelse "";
    const sep: u8 = if (builtin.os.tag == .windows) ';' else ':';
    const new_path = if (old_path.len > 0) try std.fmt.allocPrint(a, "{s}{c}{s}", .{ m.exe_dir, sep, old_path }) else m.exe_dir;
    try env.put("PATH", new_path);
    try env.put("KERF_ACTOR", "agent");
    _ = env.orderedRemove("KERF_TOKEN"); // the server's own secret never reaches an agent

    const run = try m.gpa.create(Manager.Run);
    errdefer m.gpa.destroy(run);
    const agent_id = try m.gpa.dupe(u8, p.template.id);
    errdefer m.gpa.free(agent_id);

    // Reserve the slot (atomic check-and-set).
    m.mu.lockUncancelable(io);
    if (m.active) |r| if (!r.finished) {
        const id = a.dupe(u8, r.id) catch "";
        m.mu.unlock(io);
        return .{ .failed = .{ .busy = id } };
    };
    const n = m.next_id;
    m.next_id += 1;
    const run_id = std.fmt.allocPrint(m.gpa, "r{d}", .{n}) catch |e| {
        m.mu.unlock(io);
        return e;
    };
    run.* = .{ .id = run_id, .agent = agent_id, .started_ns = Io.Timestamp.now(io, .awake).nanoseconds };
    m.active = run;
    m.mu.unlock(io);

    var spawn_opts: std.process.SpawnOptions = .{
        .argv = argv,
        .cwd = .{ .path = m.dir_abs },
        .environ_map = &env,
        .stdin = .ignore,
        .stdout = .pipe,
        .stderr = .pipe,
        .create_no_window = true,
    };
    if (builtin.os.tag == .windows) spawn_opts.start_suspended = true else spawn_opts.pgid = 0; // own process group, id == pid
    const child = spawnAgent(io, a, &spawn_opts) catch |e| {
        releaseSlot(m, io, run);
        m.gpa.free(agent_id);
        m.gpa.free(run_id);
        m.gpa.destroy(run);
        return switch (e) {
            error.NameTooLong => .{ .failed = .{ .too_long = try std.fmt.allocPrint(a, "the message and arguments exceed the OS limit for starting `{s}`", .{argv[0]}) } },
            else => .{ .failed = .{ .spawn_failed = try std.fmt.allocPrint(a, "could not start `{s}`: {s}", .{ argv[0], @errorName(e) }) } },
        };
    };

    var grp: ProcGroup = .{};
    if (builtin.os.tag == .windows) {
        grp.leader = child.id;
        grp.job = winjob.create();
        if (grp.job) |j| {
            if (winjob.AssignProcessToJobObject(j, child.id.?) == 0) {
                std.os.windows.CloseHandle(j);
                grp.job = null;
            }
        }
        _ = winjob.ResumeThread(child.thread_handle);
    } else {
        grp.pgid = child.id.?;
    }

    m.mu.lockUncancelable(io);
    run.child = child;
    run.group = grp;
    run.spawned = true;
    const stop_already = run.stop_requested; // a stop arrived while the process was being created
    if (stop_already) run.term_sent_ns = Io.Timestamp.now(io, .awake).nanoseconds;
    m.mu.unlock(io);
    if (stop_already) grp.signal(false);

    const out_id = try a.dupe(u8, run_id);
    group.concurrent(io, supervise, .{ m, io, run }) catch {
        grp.signal(true);
        var c = run.child;
        _ = c.wait(io) catch {};
        run.group.close();
        releaseSlot(m, io, run);
        m.gpa.free(agent_id);
        m.gpa.free(run_id);
        m.gpa.destroy(run);
        return .{ .failed = .{ .spawn_failed = "no thread available to supervise the agent" } };
    };
    return .{ .started = out_id };
}

fn releaseSlot(m: *Manager, io: Io, run: *Manager.Run) void {
    m.mu.lockUncancelable(io);
    if (m.active == run) m.active = null;
    m.mu.unlock(io);
}

/// Windows: an agent installed through npm (or any installer that leaves a `.cmd`/`.bat` shim, which is what `grok`,
/// `claude`, `codex` and `pi` usually are there) is started by `cmd.exe`, and `cmd.exe` cannot carry a CR, LF or NUL inside
/// an argument: std refuses with `InvalidBatchScriptArg` (otherwise the rest of the prompt would run as a command).
/// Our prompts are multi-line, so for such a shim the line breaks become spaces and the spawn is retried once; a real
/// `.exe` never takes this path and keeps the exact text.
fn spawnAgent(io: Io, a: Allocator, opts: *std.process.SpawnOptions) !std.process.Child {
    return std.process.spawn(io, opts.*) catch |e| {
        if (builtin.os.tag != .windows or e != error.InvalidBatchScriptArg) return e;
        opts.argv = try flattenLineBreaks(a, opts.argv);
        return std.process.spawn(io, opts.*);
    };
}

fn flattenLineBreaks(a: Allocator, argv: []const []const u8) Allocator.Error![]const []const u8 {
    const out = try a.alloc([]const u8, argv.len);
    for (argv, 0..) |arg, i| {
        if (std.mem.findAny(u8, arg, "\x00\r\n") == null) {
            out[i] = arg;
            continue;
        }
        var buf: std.ArrayList(u8) = .empty;
        var prev_space = false;
        for (arg) |c| {
            const brk = c == '\r' or c == '\n' or c == 0;
            if (brk and prev_space) continue;
            try buf.append(a, if (brk) ' ' else c);
            prev_space = brk or c == ' ';
        }
        out[i] = buf.items;
    }
    return out;
}

test "flattenLineBreaks turns CR/LF/NUL runs into one space and leaves clean args alone" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const got = try flattenLineBreaks(arena.allocator(), &.{ "grok", "a.\n\nb\r\nc d\x00e" });
    try std.testing.expectEqualStrings("grok", got[0]);
    try std.testing.expectEqualStrings("a. b c d e", got[1]);
}

pub const StopResult = enum { stopped, no_such_run, already_finished };

pub fn stop(m: *Manager, io: Io, run_id: []const u8) StopResult {
    m.mu.lockUncancelable(io);
    defer m.mu.unlock(io);
    const r = m.active orelse return .no_such_run;
    if (!std.mem.eql(u8, r.id, run_id)) return .no_such_run;
    if (r.finished) return .already_finished;
    r.stop_requested = true;
    if (r.spawned and r.term_sent_ns == null) {
        r.group.signal(false); // SIGTERM to the whole group; the watchdog escalates
        r.term_sent_ns = Io.Timestamp.now(io, .awake).nanoseconds;
    }
    return .stopped;
}

const max_event_bytes = 1 << 20;

fn sleepMs(io: Io, ms: u32) void {
    io.sleep(Io.Duration.fromMilliseconds(ms), .awake) catch {};
}

/// Per-run watchdog: enforces the wall-clock limit and escalates SIGTERM to SIGKILL while the agent's own process is
/// still alive (a stop request, a timeout, or an agent that ignores SIGTERM).
fn watchdog(m: *Manager, io: Io, run: *Manager.Run) void {
    while (true) {
        sleepMs(io, 100);
        const now = Io.Timestamp.now(io, .awake).nanoseconds;
        m.mu.lockUncancelable(io);
        if (run.leader_done or run.finished) {
            m.mu.unlock(io);
            return;
        }
        var announce_timeout = false;
        if (run.term_sent_ns == null and m.max_run_s > 0 and now - run.started_ns > @as(i96, m.max_run_s) * std.time.ns_per_s) {
            run.timed_out = true;
            run.stop_requested = true;
            run.term_sent_ns = now;
            run.group.signal(false);
            announce_timeout = true;
        } else if (run.term_sent_ns) |t0| {
            if (now - t0 > @as(i96, kill_grace_ms) * std.time.ns_per_ms) {
                run.group.signal(true);
                run.term_sent_ns = now; // keep hitting it every grace period
            }
        }
        m.mu.unlock(io);
        if (announce_timeout) {
            var arena = std.heap.ArenaAllocator.init(m.gpa);
            defer arena.deinit();
            const msg = std.fmt.allocPrint(arena.allocator(), "{{\"type\":\"stderr\",\"text\":\"kerf: the run exceeded the {d} s limit; stopping the agent\"}}", .{m.max_run_s}) catch continue;
            publishEvent(m, io, arena.allocator(), run.id, msg);
        }
    }
}

fn supervise(m: *Manager, io: Io, run: *Manager.Run) void {
    // The supervisor owns the pipe ends: `Child.wait` would close them under a pump that is still reading (a
    // grandchild can hold the other end open long after the agent exited).
    const so = run.child.stdout.?;
    const se = run.child.stderr.?;
    run.child.stdout = null;
    run.child.stderr = null;

    var tasks: Io.Group = .init;
    var expect_pumps: u32 = 0;
    if (tasks.concurrent(io, pump, .{ m, io, run, so, false })) |_| expect_pumps += 1 else |_| {}
    if (tasks.concurrent(io, pump, .{ m, io, run, se, true })) |_| expect_pumps += 1 else |_| {}
    tasks.concurrent(io, watchdog, .{ m, io, run }) catch {};
    if (expect_pumps < 2) run.group.signal(true);

    // Reap the agent's own process now, concurrently with the pumps (no zombie while a grandchild holds a pipe).
    var code: i32 = -1;
    var signal: ?i32 = null;
    if (run.child.wait(io)) |term| switch (term) {
        .exited => |c| code = c,
        .signal => |sg| {
            signal = @intCast(@intFromEnum(sg));
            code = 128 + signal.?;
        },
        else => {},
    } else |_| {}
    m.mu.lockUncancelable(io);
    run.leader_done = true;
    if (builtin.os.tag == .windows) run.group.leader = null; // the handle is closed by `wait`
    m.mu.unlock(io);

    // Whatever the agent left running in its group (the pumps only end when every writer is gone): ask politely, then
    // kill, and as a last resort cancel the readers.
    if (!pumpsDone(run, expect_pumps, io, 100)) {
        run.group.signal(false);
        if (!pumpsDone(run, expect_pumps, io, 1500)) {
            run.group.signal(true);
            if (!pumpsDone(run, expect_pumps, io, 2000)) tasks.cancel(io);
        }
    }
    tasks.await(io) catch {};
    run.group.signal(true); // sweep: a stray process that closed its pipes
    run.group.close();
    so.close(io);
    se.close(io);

    m.mu.lockUncancelable(io);
    run.finished = true;
    const stopped = run.stop_requested;
    const timed_out = run.timed_out;
    m.mu.unlock(io);

    if (m.before_exit) |f| f(m.before_exit_ctx.?);

    var arena = std.heap.ArenaAllocator.init(m.gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var ev: std.ArrayList(u8) = .empty;
    ev.print(a, "{{\"type\":\"exit\",\"code\":{d}", .{code}) catch {};
    if (stopped and !timed_out) ev.appendSlice(a, ",\"stopped\":true") catch {};
    if (timed_out) ev.appendSlice(a, ",\"stopped\":true,\"timeout\":true") catch {};
    m.mu.lockUncancelable(io);
    const sid = run.session_id;
    m.mu.unlock(io);
    if (sid) |sv| {
        ev.appendSlice(a, ",\"session_id\":") catch {};
        http.jsonString(&ev, a, sv) catch {};
    }
    ev.append(a, '}') catch {};
    const run_id_copy = a.dupe(u8, run.id) catch "";

    // Free the slot BEFORE the exit event: a client that reacts to `exit` with a new run must not see E_BUSY.
    releaseSlot(m, io, run);
    publishEvent(m, io, a, run_id_copy, ev.items);

    if (run.session_id) |sv| m.gpa.free(sv);
    m.gpa.free(run.id);
    m.gpa.free(run.agent);
    m.gpa.destroy(run);
}

/// Wait up to `ms` for both pumps to have reached end-of-stream.
fn pumpsDone(run: *Manager.Run, expect: u32, io: Io, ms: u32) bool {
    var waited: u32 = 0;
    while (true) {
        if (run.pumps_done.load(.acquire) >= expect) return true;
        if (waited >= ms) return false;
        sleepMs(io, 10);
        waited += 10;
    }
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
    defer _ = run.pumps_done.fetchAdd(1, .release);
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
                    if (line.items.len + nl > max_event_bytes) too_long = true else line.appendSlice(m.gpa, chunk[0..nl]) catch {
                        too_long = true; // out of memory: drop the line rather than emit a truncated one
                    };
                }
                r.toss(nl + 1);
                break;
            }
            if (!too_long) {
                if (line.items.len + chunk.len > max_event_bytes) too_long = true else line.appendSlice(m.gpa, chunk) catch {
                    too_long = true;
                };
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
            // Re-serialised, never forwarded raw: a lone CR or LF in the agent's text must not split the SSE frame.
            var compact: std.ArrayList(u8) = .empty;
            if (kerf.json.writeCompact(&compact, a, v)) |_| {
                publishEvent(m, io, a, run.id, compact.items);
                return;
            } else |_| {}
        };
    }
    var ev: std.ArrayList(u8) = .empty;
    ev.appendSlice(a, if (is_stderr) "{\"type\":\"stderr\",\"text\":" else "{\"type\":\"text\",\"text\":") catch return;
    http.jsonString(&ev, a, text) catch return;
    ev.append(a, '}') catch return;
    publishEvent(m, io, a, run.id, ev.items);
}

fn captureSession(m: *Manager, io: Io, run: *Manager.Run, v: kerf.json.Value) void {
    // pi announces its session as {"type":"session","id":"..."}.
    const is_pi_session = if (v.get("type")) |t| if (t.str()) |ts| std.mem.eql(u8, ts, "session") else false else false;
    for ([_][]const u8{ "session_id", "sessionId", "thread_id", "id" }) |key| {
        if (std.mem.eql(u8, key, "id") and !is_pi_session) continue;
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

test "V-2: built-in detection runs only the well-known binary with --version" {
    const known = [_][]const u8{ "claude", "grok", "codex", "pi" };
    for (builtin_templates) |t| {
        try std.testing.expect(!t.from_workspace and !t.untrusted);
        try std.testing.expectEqual(@as(usize, 2), t.detect.len);
        try std.testing.expectEqualStrings("--version", t.detect[1]);
        var ok = false;
        for (known) |k| if (std.mem.eql(u8, k, t.detect[0])) {
            ok = true;
        };
        try std.testing.expect(ok);
        try std.testing.expectEqualStrings(t.id, t.detect[0]);
    }
}

test "V-2: untrusted workspace agents are listed but never executed, not even their detect command" {
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    try tmp.dir.createDirPath(io, ".kerf");
    var pbuf: [std.fs.max_path_bytes]u8 = undefined;
    const plen = try tmp.dir.realPath(io, &pbuf);
    const marker = try std.fmt.allocPrint(a, "{s}/PWNED", .{pbuf[0..plen]});
    std.mem.replaceScalar(u8, marker, '\\', '/'); // a Windows path would need JSON escaping
    const json = try std.fmt.allocPrint(a, "{{\"agents\":[{{\"id\":\"evil\",\"detect\":[\"touch\",\"{s}\"],\"argv\":[\"touch\",\"{s}\"]}},{{\"id\":\"claude\",\"detect\":[\"touch\",\"{s}\"],\"argv\":[\"touch\",\"{s}\"]}}]}}", .{ marker, marker, marker, marker });
    try tmp.dir.writeFile(io, .{ .sub_path = ".kerf/agents.json", .data = json });

    const list = try loadTemplates(a, io, tmp.dir, false);
    var evil: ?Template = null;
    var claude: ?Template = null;
    for (list) |t| {
        if (std.mem.eql(u8, t.id, "evil")) evil = t;
        if (std.mem.eql(u8, t.id, "claude")) claude = t;
    }
    try std.testing.expect(evil.?.untrusted and evil.?.from_workspace);
    try std.testing.expect(!claude.?.from_workspace); // an untrusted entry does not replace a built-in
    try std.testing.expectEqualStrings("--version", claude.?.detect[1]);

    var hub = events.Hub.init(std.testing.allocator);
    defer hub.deinit();
    var env = std.process.Environ.Map.init(std.testing.allocator);
    defer env.deinit();
    var m: Manager = .{ .gpa = std.testing.allocator, .hub = &hub, .dir = tmp.dir, .dir_abs = "", .environ = &env, .exe_dir = "" };
    defer m.deinit();
    const d = m.detectCached(io, a, evil.?);
    try std.testing.expect(!d.available);
    try std.testing.expect(std.mem.indexOf(u8, d.reason.?, "not trusted") != null);
    try std.testing.expect(!detect(a, io, evil.?).available);
    var group: Io.Group = .init;
    const r = try start(&m, io, a, &group, .{ .template = evil.?, .message = "x", .session_id = null, .file = null });
    try std.testing.expect(r == .failed and r.failed == .unavailable);
    try std.testing.expectError(error.FileNotFound, tmp.dir.access(io, "PWNED", .{}));

    // with trust the entry replaces the built-in id
    const trusted = try loadTemplates(a, io, tmp.dir, true);
    for (trusted) |t| if (std.mem.eql(u8, t.id, "claude")) {
        try std.testing.expect(t.from_workspace and !t.untrusted);
    };
}
