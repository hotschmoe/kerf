//! Document endpoints (spec/SERVE.md): server info, list, read, create, apply with optimistic concurrency, edit log and export.

const std = @import("std");
const builtin = @import("builtin");
const kerf = @import("kerf");
const fsx = @import("../fsx.zig");
const ws = @import("../workspace.zig");
const http = @import("../http.zig");
const agents = @import("../agents.zig");
const proxy = @import("../proxy.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const serve = @import("../serve.zig");
const Server = serve.Server;
const DocInfo = serve.DocInfo;
const computeInfo = serve.computeInfo;
const badRequest = serve.badRequest;
const parseBody = serve.parseBody;

pub fn apiInfo(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    var out: std.ArrayList(u8) = .empty;
    try out.print(a, "{{\"version\":\"{s}\",\"dir\":", .{kerf.version});
    try http.jsonString(&out, a, s.dir_abs);
    try out.print(a, ",\"token_required\":{s},\"authenticated\":true,\"proxy\":true,\"agents\":[", .{if (s.cfg.token != null) "true" else "false"});
    const templates = try agents.loadTemplates(a, s.io, s.dir, s.cfg.trust_agents);
    for (templates, 0..) |t, i| {
        if (i > 0) try out.append(a, ',');
        const d = s.agent_mgr.detectCached(s.io, a, t);
        try out.appendSlice(a, "{\"id\":");
        try http.jsonString(&out, a, t.id);
        try out.appendSlice(a, ",\"name\":");
        try http.jsonString(&out, a, t.name);
        try out.print(a, ",\"available\":{s},\"source\":\"{s}\"", .{ if (d.available) "true" else "false", if (t.from_workspace) "workspace" else "builtin" });
        if (t.untrusted) try out.appendSlice(a, ",\"untrusted\":true");
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

pub fn apiList(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
    const Row = struct { name: []const u8, mtime_ns: i96, size: u64, info: ?DocInfo, bytes_hash: u64 = 0 };
    var rows: std.ArrayList(Row) = .empty;
    {
        // Snapshot under the lock; the engine `check` below runs outside it (it can take seconds on a big folder).
        s.scan_mu.lockUncancelable(s.io);
        defer s.scan_mu.unlock(s.io);
        s.scanLocked(true);
        for (s.files.items) |f| {
            var info: ?DocInfo = null;
            if (f.info) |i| {
                var c = i;
                c.id = try a.dupe(u8, i.id);
                c.title = try a.dupe(u8, i.title);
                info = c;
            }
            try rows.append(a, .{ .name = try a.dupe(u8, f.name), .mtime_ns = f.mtime_ns, .size = f.size, .info = info });
        }
    }
    var out: std.ArrayList(u8) = .empty;
    try out.append(a, '[');
    var first = true;
    for (rows.items) |*r| {
        if (r.info == null) {
            const rd = s.readDoc(a, r.name) catch continue;
            var info = computeInfo(a, a, rd.bytes) catch continue;
            info.hash = std.hash.Wyhash.hash(0, rd.bytes);
            r.mtime_ns = rd.mtime_ns;
            r.size = rd.size;
            r.info = info;
            s.storeInfo(r.name, rd.mtime_ns, rd.size, info);
        }
        const info = r.info.?;
        if (!first) try out.append(a, ',');
        first = false;
        try out.appendSlice(a, "{\"file\":");
        try http.jsonString(&out, a, r.name);
        try out.appendSlice(a, ",\"id\":");
        try http.jsonString(&out, a, info.id);
        try out.appendSlice(a, ",\"title\":");
        try http.jsonString(&out, a, info.title);
        try out.print(a, ",\"mtime_ms\":{d},\"size\":{d},\"etag\":\"\\\"{d}-{d}-{x:0>16}\\\"\",\"components\":{d},\"views\":{d},\"errors\":{d},\"warnings\":{d}}}", .{
            @divTrunc(r.mtime_ns, std.time.ns_per_ms), r.size, @divTrunc(r.mtime_ns, std.time.ns_per_ms), r.size, info.hash, info.components, info.views, info.errors, info.warnings,
        });
    }
    try out.appendSlice(a, "]\n");
    try http.sendJson(w, 200, req.keep_alive, extra, out.items);
    return req.keep_alive;
}

pub fn apiGetDoc(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const rd = s.readDoc(a, file) catch |e| {
        if (Server.isMissing(e)) {
            try http.sendError(a, w, 404, req.keep_alive, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
            return req.keep_alive;
        }
        return e;
    };
    const etag = try Server.etagOf(a, rd.mtime_ns, rd.size, rd.bytes);
    const hdr = try std.fmt.allocPrint(a, "ETag: {s}\r\n{s}", .{ etag, extra });
    if (req.header("if-none-match")) |inm| if (std.mem.eql(u8, inm, etag)) {
        try http.send(w, .{ .status = 304, .keep_alive = req.keep_alive, .extra = hdr }, "");
        return req.keep_alive;
    };
    try http.sendJson(w, 200, req.keep_alive, hdr, rd.bytes);
    return req.keep_alive;
}

pub fn apiCreate(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8) !bool {
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
    s.appendLogOrWarn(lp, line);
    s.noteWriteLocked(a, file, true, "designer", line);
    const st = s.dir.statFile(s.io, file, .{ .follow_symlinks = false }) catch return error.StatFailed;
    const etag = try Server.etagOf(a, st.mtime.nanoseconds, st.size, text);
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

pub fn apiApply(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const ka = req.keep_alive;
    const body = parseBody(a, req) orelse return badRequest(a, req, w, extra, "E_JSON", "body must be JSON: { ops, why, actor, if_match? }");
    // lenient (SPEC 21): the body may itself be the ops array or a single op; otherwise it is { ops, why, actor, if_match? }
    const bare_ops = body == .array or (body == .object and body.get("ops") == null and body.get("op") != null);
    if (body != .object and body != .array) return badRequest(a, req, w, extra, "E_INPUT", "body must be a JSON object");
    const ops_v = if (bare_ops) body else (body.get("ops") orelse return badRequest(a, req, w, extra, "E_INPUT", "missing \"ops\": an array of ops (see SPEC 14)"));
    var ops_diags = kerf.model.Diags.init(a);
    const norm = (try kerf.ops.normalize(a, ops_v, &ops_diags)) orelse {
        const d = ops_diags.list.items[0];
        return badRequest(a, req, w, extra, d.code, d.message);
    };
    const why = if (body.get("why")) |v| (v.str() orelse "") else norm.why orelse "";
    var who: []const u8 = "designer";
    if (body.get("actor")) |v| if (v.str()) |ac| {
        var ok = ac.len > 0 and ac.len <= 32;
        for (ac) |c| if (!(std.ascii.isAlphanumeric(c) or c == '-' or c == '_')) {
            ok = false;
        };
        if (ok) who = ac;
    };
    // `if_match` must be a string (or null/absent): anything else used to be treated as absent, i.e. an unconditional write.
    var if_match: ?[]const u8 = null;
    if (body.get("if_match")) |v| {
        if (v.str()) |im| if_match = im else if (!v.isNull()) return badRequest(a, req, w, extra, "E_INPUT", "\"if_match\" must be the ETag string of the version the ops were made against (or omitted)");
    }

    s.write_mu.lockUncancelable(s.io);
    defer s.write_mu.unlock(s.io);
    // The same advisory lock `kerf apply -w` takes: no other process can interleave a read-modify-write with ours.
    const lp = try ws.logPath(a, file);
    const doc_lock = ws.DocLock.acquire(s.io, s.dir, lp) catch |e| {
        try http.sendError(a, w, 500, ka, extra, "E_WRITE", try std.fmt.allocPrint(a, "cannot open or lock the op log {s}: {s}", .{ lp, @errorName(e) }));
        return ka;
    };
    defer doc_lock.release(s.io);
    const rd = s.readDoc(a, file) catch |e| {
        if (Server.isMissing(e)) {
            try http.sendError(a, w, 404, ka, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
            return ka;
        }
        return e;
    };
    const etag = try Server.etagOf(a, rd.mtime_ns, rd.size, rd.bytes);
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
    try kerf.json.writeCompact(&ops_text, a, norm.ops);
    // The engine only distinguishes designer edits (verified citations stay verified) from model edits.
    const engine_actor = if (std.mem.eql(u8, who, "designer")) "designer" else "llm";
    const input = try std.fmt.allocPrint(a, "{{\"doc\":{s},\"ops\":{s},\"actor\":\"{s}\"}}", .{ std.mem.trim(u8, rd.bytes, " \t\r\n"), ops_text.items, engine_actor });
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
        s.scan_mu.lockUncancelable(s.io);
        defer s.scan_mu.unlock(s.io);
        ws.writeFileAtomic(s.io, s.dir, file, text) catch |e| {
            try http.sendError(a, w, 500, ka, extra, "E_WRITE", try std.fmt.allocPrint(a, "could not write {s}: {s}", .{ file, @errorName(e) }));
            return ka;
        };
        doc_lock.appendLine(s.io, line) catch |e| s.warnLog(lp, e);
        s.noteWriteLocked(a, file, false, who, line);
        if (s.dir.statFile(s.io, file, .{ .follow_symlinks = false })) |st| final_etag = try Server.etagOf(a, st.mtime.nanoseconds, st.size, text) else |_| {}
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

/// Whole file at `path` (a regular file only: a symlink is "missing"), at most `limit` bytes.
pub fn readNoFollow(s: *Server, a: Allocator, path: []const u8, limit: usize) ?[]u8 {
    var f = fsx.openFileNoFollow(s.io, s.dir, path) catch return null;
    defer f.close(s.io);
    const st = f.stat(s.io) catch return null;
    if (st.kind != .file) return null;
    var rb: [8192]u8 = undefined;
    var fr = f.reader(s.io, &rb);
    return fr.interface.allocRemaining(a, .limited(limit)) catch null;
}

pub fn apiLog(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
    const since: usize = if (try req.param(a, "since")) |sv| (std.fmt.parseInt(usize, sv, 10) catch return badRequest(a, req, w, extra, "E_INPUT", "since must be a line index (integer)")) else 0;
    const dst = s.dir.statFile(s.io, file, .{ .follow_symlinks = false }) catch null;
    if (dst == null or dst.?.kind != .file) {
        try http.sendError(a, w, 404, req.keep_alive, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
        return req.keep_alive;
    }
    const lp = try ws.logPath(a, file);
    const data: []u8 = readNoFollow(s, a, lp, 64 << 20) orelse &.{};
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

pub fn apiExport(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, file: []const u8) !bool {
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
    const rd = s.readDoc(a, file) catch |e| {
        if (Server.isMissing(e)) {
            try http.sendError(a, w, 404, ka, extra, "E_NOT_FOUND", try std.fmt.allocPrint(a, "{s} does not exist in the served folder", .{file}));
            return ka;
        }
        return e;
    };
    try input.appendSlice(a, "{\"doc\":");
    try input.appendSlice(a, std.mem.trim(u8, rd.bytes, " \t\r\n"));
    try input.appendSlice(a, ",\"view\":");
    try http.jsonString(&input, a, view);
    try input.print(a, ",\"format\":\"{s}\",\"sheet\":{s}", .{ f.name, if (sheet) "true" else "false" });
    if (try req.param(a, "px")) |px| {
        const px_n = std.fmt.parseInt(u32, px, 10) catch return badRequest(a, req, w, extra, "E_INPUT", "px must be an integer (output width in pixels)");
        try input.print(a, ",\"px\":{d}", .{px_n}); // the parsed number, never the raw text ("+5" made invalid JSON)
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
