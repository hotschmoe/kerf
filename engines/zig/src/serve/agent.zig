//! Agent bridge endpoints (spec/SERVE.md): start a run (message, images, session), stop it, and the attachment files a run gets.

const std = @import("std");
const kerf = @import("kerf");
const ws = @import("../workspace.zig");
const http = @import("../http.zig");
const agents = @import("../agents.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const serve = @import("../serve.zig");
const Server = serve.Server;
const badRequest = serve.badRequest;
const parseBody = serve.parseBody;
const attachment_ttl_s = serve.attachment_ttl_s;

/// The message is one argv element; Linux caps a single argument at 131072 bytes (`MAX_ARG_STRLEN`) and the server adds
/// a fixed prefix and the attachment paths, so stay well under it (V-6).
pub const max_message_bytes = 100_000;

pub fn apiAgentRun(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const ka = req.keep_alive;
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { agent, message, session_id?, file? }");
    const agent_id = (if (body.get("agent")) |v| v.str() else null) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"agent\" (claude, grok, codex, or an id from .kerf/agents.json)");
    const message = (if (body.get("message")) |v| v.str() else null) orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"message\"");
    if (std.mem.trim(u8, message, " \t\r\n").len == 0 or message.len > max_message_bytes) return badRequest(a, req, w, extra, "E_INPUT", "\"message\" must be 1..100000 bytes (it is one command-line argument; the OS limit is 128 KiB)");
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
    var message_full: []const u8 = message;
    if (body.get("images")) |iv| if (!iv.isNull()) {
        const paths = saveImages(s, a, iv) catch |e| switch (e) {
            error.BadImages => return badRequest(a, req, w, extra, "E_IMAGES", "images must be [{ name, data_base64 }] (png/jpg/gif/webp, at most 8, 12 MB each)"),
            else => return e,
        };
        if (paths.len > 0) {
            var m: std.ArrayList(u8) = .empty;
            try m.appendSlice(a, message);
            try m.appendSlice(a, "\n\nThe designer attached these images (open them with your file-reading tool):\n");
            for (paths) |ip| try m.print(a, "- {s}\n", .{ip});
            message_full = m.items;
        }
    };
    const templates = try agents.loadTemplates(a, s.io, s.dir, s.cfg.trust_agents);
    var tmpl: ?agents.Template = null;
    for (templates) |t| if (std.mem.eql(u8, t.id, agent_id)) {
        tmpl = t;
    };
    const t = tmpl orelse {
        try http.sendError(a, w, 404, ka, extra, "E_AGENT", try std.fmt.allocPrint(a, "unknown agent '{s}'; GET /api/info lists them", .{agent_id}));
        return ka;
    };
    const res = try agents.start(&s.agent_mgr, s.io, a, &s.group, .{ .template = t, .message = message_full, .session_id = session, .file = file });
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
            .too_long => |why| try http.sendError(a, w, 400, ka, extra, "E_INPUT", why),
            .unavailable => |why| try http.sendError(a, w, 409, ka, extra, if (t.untrusted) "E_UNTRUSTED" else "E_UNAVAILABLE", try std.fmt.allocPrint(a, "agent '{s}' is not available: {s}", .{ agent_id, why })),
            .spawn_failed => |why| try http.sendError(a, w, 500, ka, extra, "E_SPAWN", why),
        },
    }
    return ka;
}

/// `path` (relative to the served folder) exists as a real directory: created when missing, refused when it is a
/// symlink (an agent could have pointed `.kerf/attachments` somewhere else).
pub fn ensureRealDir(s: *Server, path: []const u8) !void {
    s.dir.createDir(s.io, path, .default_dir) catch |e| switch (e) {
        error.PathAlreadyExists => {},
        else => return e,
    };
    const st = try s.dir.statFile(s.io, path, .{ .follow_symlinks = false });
    if (st.kind != .directory) return error.BadImages;
}

/// Attached images are only needed while their run lasts (at most `--agent-timeout`); drop anything older than a day.
pub fn cleanupAttachments(s: *Server) void {
    var d = s.dir.openDir(s.io, ".kerf/attachments", .{ .iterate = true, .follow_symlinks = false }) catch return;
    defer d.close(s.io);
    const now_ns = Io.Timestamp.now(s.io, .real).nanoseconds;
    var it = d.iterate();
    while (it.next(s.io) catch null) |e| {
        if (e.kind != .file) continue;
        const st = d.statFile(s.io, e.name, .{ .follow_symlinks = false }) catch continue;
        if (now_ns - st.mtime.nanoseconds > @as(i96, attachment_ttl_s) * std.time.ns_per_s) d.deleteFile(s.io, e.name) catch {};
    }
}

/// Save `[{name, data_base64}]` under `<dir>/.kerf/attachments/` and return the absolute paths.
pub fn saveImages(s: *Server, a: Allocator, iv: kerf.json.Value) ![]const []const u8 {
    const items = iv.arr() orelse return error.BadImages;
    if (items.len > 8) return error.BadImages;
    var out: std.ArrayList([]const u8) = .empty;
    if (items.len == 0) return out.items;
    try ensureRealDir(s, ".kerf");
    try ensureRealDir(s, ".kerf/attachments");
    cleanupAttachments(s);
    for (items) |it| {
        const name_in = (if (it.get("name")) |v| v.str() else null) orelse return error.BadImages;
        var data = (if (it.get("data_base64")) |v| v.str() else null) orelse return error.BadImages;
        if (std.mem.startsWith(u8, data, "data:")) {
            const comma = std.mem.indexOfScalar(u8, data, ',') orelse return error.BadImages;
            data = data[comma + 1 ..];
        }
        var safe: std.ArrayList(u8) = .empty;
        const base = std.fs.path.basename(name_in);
        for (base[0..@min(base.len, 60)]) |c| try safe.append(a, if (std.ascii.isAlphanumeric(c) or c == '.' or c == '-' or c == '_') c else '_');
        const ext = std.fs.path.extension(safe.items);
        const okext = [_][]const u8{ ".png", ".jpg", ".jpeg", ".gif", ".webp" };
        var good = false;
        for (okext) |e| if (std.ascii.eqlIgnoreCase(ext, e)) {
            good = true;
        };
        if (!good) try safe.appendSlice(a, ".png");
        const dec = std.base64.standard.Decoder;
        const n = dec.calcSizeForSlice(data) catch return error.BadImages;
        if (n > 12 << 20) return error.BadImages;
        const bytes = try a.alloc(u8, n);
        dec.decode(bytes, data) catch return error.BadImages;
        var rnd: [4]u8 = undefined;
        s.io.random(&rnd);
        const rel = try std.fmt.allocPrint(a, ".kerf/attachments/{x}-{s}", .{ &rnd, safe.items });
        // exclusive create: never write through a name an agent pre-planted (symlink)
        var af = try s.dir.createFile(s.io, rel, .{ .exclusive = true });
        defer af.close(s.io);
        try af.writeStreamingAll(s.io, bytes);
        try out.append(a, try std.fs.path.join(a, &.{ s.dir_abs, rel }));
    }
    return out.items;
}

pub fn apiAgentStop(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
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
