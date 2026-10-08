//! Routing (spec/SERVE.md): which handler a request is for (decided before any body byte is read), the per-route body caps, and the request dispatcher.

const std = @import("std");
const kerf = @import("kerf");
const ws = @import("../workspace.zig");
const http = @import("../http.zig");
const events = @import("../events.zig");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const static = @import("ui.zig").static;
const apiAgentRun = @import("agent.zig").apiAgentRun;
const apiAgentStop = @import("agent.zig").apiAgentStop;
const apiEvents = @import("stream.zig").apiEvents;
const apiLlm = @import("stream.zig").apiLlm;
const apiInfo = @import("docs.zig").apiInfo;
const apiList = @import("docs.zig").apiList;
const apiGetDoc = @import("docs.zig").apiGetDoc;
const apiCreate = @import("docs.zig").apiCreate;
const apiApply = @import("docs.zig").apiApply;
const apiLog = @import("docs.zig").apiLog;
const apiExport = @import("docs.zig").apiExport;
const busy_ms = @import("conn.zig").busy_ms;
const checkAccess = @import("conn.zig").checkAccess;
const serve = @import("../serve.zig");
const Server = serve.Server;
const arm = serve.arm;

pub const Kind = enum { options, info, docs_list, docs_create, doc_get, doc_apply, doc_log, doc_export, events, llm, agent_run, agent_stop, bad_file, method_not_allowed, not_found, static };

pub const Route = struct { kind: Kind, file: []const u8 = "", api: bool = false };

/// The handler of each method for one path (null: the method is not allowed there).
const Methods = struct { get: ?Kind = null, post: ?Kind = null };

/// The API route table: `/api` + path. `/api/docs/<file>[/<action>]` is `doc_actions`.
const endpoints = [_]struct { path: []const u8, methods: Methods }{
    .{ .path = "/info", .methods = .{ .get = .info } },
    .{ .path = "/docs", .methods = .{ .get = .docs_list, .post = .docs_create } },
    .{ .path = "/events", .methods = .{ .get = .events } },
    .{ .path = "/llm", .methods = .{ .post = .llm } },
    .{ .path = "/agent/run", .methods = .{ .post = .agent_run } },
    .{ .path = "/agent/stop", .methods = .{ .post = .agent_stop } },
};

/// Actions on one document, `/api/docs/<file>/<action>` ("" is the document itself).
const doc_actions = [_]struct { action: []const u8, methods: Methods }{
    .{ .action = "", .methods = .{ .get = .doc_get } },
    .{ .action = "apply", .methods = .{ .post = .doc_apply } },
    .{ .action = "log", .methods = .{ .get = .doc_log } },
    .{ .action = "export", .methods = .{ .get = .doc_export } },
};

/// Which handler a request is for. Pure: no I/O, so it runs before any body byte is read.
pub fn routeOf(req: http.Request, path: []const u8) Route {
    const is_get = std.mem.eql(u8, req.method, "GET");
    const is_post = std.mem.eql(u8, req.method, "POST");
    if (!(std.mem.eql(u8, path, "/api") or std.mem.startsWith(u8, path, "/api/"))) {
        return .{ .kind = if (is_get or std.mem.eql(u8, req.method, "HEAD")) .static else .method_not_allowed };
    }
    if (std.mem.eql(u8, req.method, "OPTIONS")) return .{ .kind = .options, .api = true };
    const rest = path["/api".len..]; // "" or "/..."
    const mna: Route = .{ .kind = .method_not_allowed, .api = true };
    for (endpoints) |ep| {
        if (!std.mem.eql(u8, rest, ep.path)) continue;
        const kind = (if (is_get) ep.methods.get else if (is_post) ep.methods.post else null) orelse return mna;
        return .{ .kind = kind, .api = true };
    }
    if (std.mem.startsWith(u8, rest, "/docs/")) {
        const tail = rest["/docs/".len..];
        var parts = std.mem.splitScalar(u8, tail, '/');
        const file = parts.next().?;
        const action = parts.next() orelse "";
        if (parts.next() != null) return .{ .kind = .not_found, .api = true };
        if (!ws.validDocFile(file)) return .{ .kind = .bad_file, .api = true };
        for (doc_actions) |da| {
            if (!std.mem.eql(u8, action, da.action)) continue;
            const kind = (if (is_get) da.methods.get else if (is_post) da.methods.post else null) orelse return mna;
            return .{ .kind = kind, .file = file, .api = true };
        }
        return .{ .kind = .not_found, .api = true };
    }
    return .{ .kind = .not_found, .api = true };
}

/// Per-route request body cap (V-5). Routes that take no body have 0.
pub fn bodyCap(kind: Kind) usize {
    return switch (kind) {
        .agent_stop => 4 << 10,
        .docs_create => 64 << 10,
        .doc_apply => 16 << 20,
        .llm => 32 << 20,
        .agent_run => 64 << 20, // images: up to 8 x 12 MB decoded, base64
        else => 0,
    };
}

/// Route first, then read the body, then handle: nothing is read from an unauthenticated or unroutable request.
/// Every answer given before the body was read closes the connection (the unread body would otherwise be parsed as the
/// next request).
pub fn handleRequest(s: *Server, a: Allocator, req: *http.Request, r: *Io.Reader, w: *Io.Writer) !bool {
    const path = try http.percentDecode(a, req.path, false);
    const route = routeOf(req.*, path);
    const body_pending = req.chunked or req.content_length != 0;
    const ka = req.keep_alive and !body_pending; // for answers sent before the body is read
    const extra = if (route.api) try s.cors(a, req.*) else "";

    if (route.kind == .options) {
        try http.send(w, .{ .status = 204, .keep_alive = ka, .extra = extra }, "");
        return ka;
    }
    if (route.api) {
        if (checkAccess(s, a, req.*)) |rej| {
            if (route.kind == .info and rej.status == 401) {
                // Let the UI discover that a token is needed.
                const body = try a.print("{{\"version\":\"{s}\",\"token_required\":true,\"authenticated\":false}}\n", .{kerf.version});
                try http.sendJson(w, 200, ka, extra, body);
                return ka;
            }
            try http.sendError(a, w, rej.status, ka, extra, rej.code, rej.message);
            return ka;
        }
    }
    switch (route.kind) {
        .method_not_allowed => {
            try http.sendError(a, w, 405, ka, extra, "E_METHOD", if (route.api) "method not allowed for this path" else "method not allowed");
            return ka;
        },
        .not_found => {
            try http.sendError(a, w, 404, ka, extra, "E_NOT_FOUND", "no such endpoint");
            return ka;
        },
        .bad_file => {
            try http.sendError(a, w, 400, ka, extra, "E_FILE", "file must be a plain NAME.kerf.json (no directories)");
            return ka;
        },
        else => {},
    }
    // Body.
    const cap = bodyCap(route.kind);
    if (body_pending) {
        if (cap == 0) {
            try http.sendError(a, w, 400, false, extra, "E_BODY", "this endpoint takes no request body");
            return false;
        }
        arm(@as(u64, s.cfg.timeouts.head_ms) * 3 + @as(u64, @min(req.content_length, 256 << 20) / (512 << 10)) * 1000);
        http.readBody(a, r, w, req, cap) catch |e| {
            switch (e) {
                error.TooLarge => http.sendError(a, w, 413, false, extra, "E_TOO_LARGE", try a.print("request body over {d} bytes for this endpoint", .{cap})) catch {},
                error.BadChunk, error.ShortBody => http.sendError(a, w, 400, false, extra, "E_HTTP", "malformed request body") catch {},
                else => {},
            }
            return false;
        };
        arm(busy_ms);
    }
    const rq = req.*;
    return switch (route.kind) {
        .info => apiInfo(s, a, rq, w, extra),
        .docs_list => apiList(s, a, rq, w, extra),
        .docs_create => apiCreate(s, a, rq, w, extra),
        .doc_get => apiGetDoc(s, a, rq, w, extra, route.file),
        .doc_apply => apiApply(s, a, rq, w, extra, route.file),
        .doc_log => apiLog(s, a, rq, w, extra, route.file),
        .doc_export => apiExport(s, a, rq, w, extra, route.file),
        .events => apiEvents(s, a, w, extra),
        .llm => apiLlm(s, a, rq, w, extra),
        .agent_run => apiAgentRun(s, a, rq, w, extra),
        .agent_stop => apiAgentStop(s, a, rq, w, extra),
        .static => static(s, a, rq, w, path),
        else => unreachable,
    };
}

pub fn badRequest(a: Allocator, req: http.Request, w: *Io.Writer, extra: []const u8, code: []const u8, msg: []const u8) !bool {
    try http.sendError(a, w, 400, req.keep_alive, extra, code, msg);
    return req.keep_alive;
}

pub fn parseBody(a: Allocator, req: http.Request) ?kerf.json.Value {
    var pe: kerf.json.ParseError = undefined;
    return (kerf.json.parse(a, req.body, &pe) catch return null) orelse null;
}

test "every handler kind is reachable through the route table" {
    inline for (std.meta.tags(Kind)) |k| {
        switch (k) {
            // answered without a handler of their own
            .options, .bad_file, .method_not_allowed, .not_found, .static => {},
            else => {
                var found = false;
                for (endpoints) |ep| {
                    if (ep.methods.get == k or ep.methods.post == k) found = true;
                }
                for (doc_actions) |da| {
                    if (da.methods.get == k or da.methods.post == k) found = true;
                }
                if (!found) {
                    std.debug.print("route kind .{s} is not in the route table\n", .{@tagName(k)});
                    return error.TestUnexpectedResult;
                }
            },
        }
    }
}
