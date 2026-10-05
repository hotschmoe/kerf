//! The document session: current document JSON, revision, op log, undo,
//! diagnostics, and all engine traffic (apply / check / drawing / mesh /
//! export / inspect). UI-framework free so it is unit-tested with a fake
//! engine.
//!
//! Undo model: every successful apply pushes the PREVIOUS document text on
//! an undo stack together with its op-log entry. `undo()` restores the text
//! and marks the entry undone (the log stays append-only; an undone entry is
//! shown struck out in DIFF).

const std = @import("std");
const Allocator = std.mem.Allocator;
const eng = @import("engine.zig");
const docinfo = @import("docinfo.zig");

pub const Who = enum {
    designer,
    claude,
    system,

    pub fn label(self: Who) []const u8 {
        return switch (self) {
            .designer => "DESIGNER",
            .claude => "CLAUDE",
            .system => "SYSTEM",
        };
    }
};

pub const Level = enum {
    @"error",
    warning,
    info,

    pub fn letter(self: Level) u8 {
        return switch (self) {
            .@"error" => 'E',
            .warning => 'W',
            .info => 'I',
        };
    }
};

pub const Diag = struct {
    level: Level,
    code: []const u8,
    /// Component / annotation id the diagnostic is about ("" when general).
    id: []const u8,
    message: []const u8,
};

pub const LogEntry = struct {
    who: Who,
    why: []const u8,
    ops_json: []const u8,
    rev_before: u32,
    rev_after: u32,
    /// Seconds since the unix epoch (0 when unknown).
    time_s: i64,
    undone: bool = false,
};

pub const ApplyOutcome = struct {
    ok: bool,
    /// Engine message for the model/designer: summary + diagnostics, or the error.
    text: []const u8,
    errors: u32 = 0,
    warnings: u32 = 0,
    /// Number of ops in the batch.
    op_count: u32 = 0,
};

pub const Session = struct {
    gpa: Allocator,
    engine: eng.Engine,
    style_json: []const u8,

    doc: []u8 = &.{},
    info: ?docinfo.DocInfo = null,
    rev: u32 = 0,
    /// Compact engine summary text of the current doc.
    summary: []u8 = &.{},
    diag_arena: std.heap.ArenaAllocator,
    diags: []const Diag = &.{},

    log: std.ArrayList(LogEntry) = .empty,
    /// (document text before the edit, index into `log`).
    undo_stack: std.ArrayList(struct { doc: []u8, log_index: usize }) = .empty,
    log_arena: std.heap.ArenaAllocator,

    pub fn init(gpa: Allocator, engine: eng.Engine, style_json: []const u8) Session {
        return .{ .gpa = gpa, .engine = engine, .style_json = style_json, .diag_arena = .init(gpa), .log_arena = .init(gpa) };
    }

    pub fn deinit(self: *Session) void {
        self.gpa.free(self.doc);
        self.gpa.free(self.summary);
        if (self.info) |*i| i.deinit();
        self.diag_arena.deinit();
        self.log_arena.deinit();
        self.log.deinit(self.gpa);
        for (self.undo_stack.items) |u| self.gpa.free(u.doc);
        self.undo_stack.deinit(self.gpa);
    }

    /// The engine uses its embedded kerf-standard style when none is given.
    fn styleOpt(self: *const Session) ?[]const u8 {
        return if (self.style_json.len > 0) self.style_json else null;
    }

    pub fn hasDoc(self: *const Session) bool {
        return self.doc.len > 0;
    }

    /// Replace the whole document (open / sample / new). Clears log + undo.
    /// Returns false (and leaves the session unchanged) when the engine
    /// rejects the document; `err_out` then holds the message (caller frees).
    pub fn load(self: *Session, doc_json: []const u8, err_out: *?[]u8) !bool {
        const fmt_req = try (eng.Request{ .doc = doc_json }).build(self.gpa);
        defer self.gpa.free(fmt_req);
        const res = self.engine.call(self.gpa, "fmt", fmt_req);
        switch (res) {
            .err => |e| {
                err_out.* = e;
                return false;
            },
            .ok => |out| {
                defer self.gpa.free(out);
                const canon = try extractDoc(self.gpa, out);
                errdefer self.gpa.free(canon);
                var info = docinfo.parse(self.gpa, canon) catch {
                    err_out.* = try self.gpa.dupe(u8, "document is not a JSON object");
                    self.gpa.free(canon);
                    return false;
                };
                errdefer info.deinit();
                self.gpa.free(self.doc);
                if (self.info) |*i| i.deinit();
                self.doc = canon;
                self.info = info;
                self.rev = 0;
                _ = self.log_arena.reset(.free_all);
                self.log = .empty;
                for (self.undo_stack.items) |u| self.gpa.free(u.doc);
                self.undo_stack.clearRetainingCapacity();
                try self.refreshCheck();
                return true;
            },
        }
    }

    /// Run `check` and refresh summary + diagnostics.
    pub fn refreshCheck(self: *Session) !void {
        const req = try (eng.Request{ .doc = self.doc, .style = self.styleOpt() }).build(self.gpa);
        defer self.gpa.free(req);
        const res = self.engine.call(self.gpa, "check", req);
        switch (res) {
            .err => |e| {
                defer self.gpa.free(e);
                try self.setDiagsFromText(e);
            },
            .ok => |out| {
                defer self.gpa.free(out);
                try self.ingestDiagnostics(out);
            },
        }
    }

    fn setDiagsFromText(self: *Session, msg: []const u8) !void {
        _ = self.diag_arena.reset(.free_all);
        const a = self.diag_arena.allocator();
        const one = try a.alloc(Diag, 1);
        one[0] = .{ .level = .@"error", .code = "E_ENGINE", .id = "", .message = try a.dupe(u8, msg) };
        self.diags = one;
    }

    /// Parse `{diagnostics:[...], summary:"..."}` from an engine reply.
    fn ingestDiagnostics(self: *Session, out: []const u8) !void {
        _ = self.diag_arena.reset(.free_all);
        const a = self.diag_arena.allocator();
        var parsed = std.json.parseFromSlice(std.json.Value, self.gpa, out, .{}) catch return;
        defer parsed.deinit();
        const root = switch (parsed.value) {
            .object => |o| o,
            else => return,
        };
        if (root.get("summary")) |s| if (s == .string) {
            const dup = try self.gpa.dupe(u8, s.string);
            self.gpa.free(self.summary);
            self.summary = dup;
        };
        var list: std.ArrayList(Diag) = .empty;
        if (root.get("diagnostics")) |dv| if (dv == .array) {
            for (dv.array.items) |item| {
                if (item != .object) continue;
                const o = item.object;
                const lvl_s = if (o.get("level")) |l| (if (l == .string) l.string else "info") else "info";
                const lvl: Level = if (std.mem.eql(u8, lvl_s, "error")) .@"error" else if (std.mem.eql(u8, lvl_s, "warning")) .warning else .info;
                try list.append(a, .{
                    .level = lvl,
                    .code = try a.dupe(u8, jstr(o.get("code"))),
                    .id = try a.dupe(u8, jstr(o.get("id"))),
                    .message = try a.dupe(u8, jstr(o.get("message"))),
                });
            }
        };
        self.diags = list.items;
    }

    pub fn errorCount(self: *const Session) u32 {
        return self.countLevel(.@"error");
    }
    pub fn warnCount(self: *const Session) u32 {
        return self.countLevel(.warning);
    }
    fn countLevel(self: *const Session, lvl: Level) u32 {
        var n: u32 = 0;
        for (self.diags) |d| if (d.level == lvl) {
            n += 1;
        };
        return n;
    }

    /// Apply an ops array (JSON text) atomically. `actor` is "llm" for the
    /// tool path and "designer" for UI edits. On `ok:false` nothing changes.
    /// The returned `text` is owned by `gpa` (caller frees).
    pub fn apply(self: *Session, ops_json: []const u8, why: []const u8, who: Who, time_s: i64) !ApplyOutcome {
        if (!self.hasDoc() and !opsStartWithSetDoc(ops_json)) {
            return .{ .ok = false, .text = try self.gpa.dupe(u8, "no document is loaded: send one {\"op\":\"set\",\"path\":\"doc\",\"value\":{...}} first") };
        }
        const doc_for_req: []const u8 = if (self.hasDoc()) self.doc else "{}";
        const req = try (eng.Request{
            .doc = doc_for_req,
            .style = self.styleOpt(),
            .ops = ops_json,
            .actor = if (who == .designer) "designer" else "llm",
        }).build(self.gpa);
        defer self.gpa.free(req);

        const op_count = countOps(ops_json);
        const res = self.engine.call(self.gpa, "apply", req);
        switch (res) {
            .err => |e| return .{ .ok = false, .text = e, .op_count = op_count },
            .ok => |out| {
                defer self.gpa.free(out);
                var parsed = std.json.parseFromSlice(std.json.Value, self.gpa, out, .{}) catch
                    return .{ .ok = false, .text = try self.gpa.dupe(u8, "engine returned invalid JSON"), .op_count = op_count };
                defer parsed.deinit();
                const root = switch (parsed.value) {
                    .object => |o| o,
                    else => return .{ .ok = false, .text = try self.gpa.dupe(u8, "engine returned a non-object"), .op_count = op_count },
                };
                const ok = if (root.get("ok")) |b| (b == .bool and b.bool) else false;
                try self.ingestDiagnosticsFromParsed(root);
                const text = try self.composeText(root);
                if (!ok) return .{ .ok = false, .text = text, .errors = self.errorCount(), .warnings = self.warnCount(), .op_count = op_count };

                const new_doc_val = root.get("doc") orelse return .{ .ok = false, .text = text, .op_count = op_count };
                const new_doc = try std.json.Stringify.valueAlloc(self.gpa, new_doc_val, .{ .whitespace = .indent_2 });
                errdefer self.gpa.free(new_doc);
                var info = try docinfo.parse(self.gpa, new_doc);
                errdefer info.deinit();

                // Commit.
                const before = self.doc;
                const la = self.log_arena.allocator();
                try self.log.append(self.gpa, .{
                    .who = who,
                    .why = try la.dupe(u8, why),
                    .ops_json = try la.dupe(u8, ops_json),
                    .rev_before = self.rev,
                    .rev_after = self.rev + 1,
                    .time_s = time_s,
                });
                try self.undo_stack.append(self.gpa, .{ .doc = before, .log_index = self.log.items.len - 1 });
                if (self.info) |*i| i.deinit();
                self.info = info;
                self.doc = new_doc;
                self.rev += 1;
                return .{ .ok = true, .text = text, .errors = self.errorCount(), .warnings = self.warnCount(), .op_count = op_count };
            },
        }
    }

    fn ingestDiagnosticsFromParsed(self: *Session, root: std.json.ObjectMap) !void {
        const s = try std.json.Stringify.valueAlloc(self.gpa, std.json.Value{ .object = root }, .{});
        defer self.gpa.free(s);
        // Re-parse path keeps one implementation; replies are small.
        try self.ingestDiagnostics(s);
        // The summary in an apply reply refers to the NEW doc only on success;
        // on failure keep it anyway (it describes the unchanged doc).
    }

    /// Text handed back to the LLM / shown to the designer: summary then diagnostics.
    fn composeText(self: *Session, root: std.json.ObjectMap) ![]u8 {
        var out: std.Io.Writer.Allocating = .init(self.gpa);
        errdefer out.deinit();
        const w = &out.writer;
        const ok = if (root.get("ok")) |b| (b == .bool and b.bool) else false;
        try w.print("{s}\n", .{if (ok) "ok" else "FAILED (nothing changed)"});
        if (root.get("summary")) |s| if (s == .string and s.string.len > 0) try w.print("{s}\n", .{s.string});
        for (self.diags) |d| {
            const tag: []const u8 = switch (d.level) {
                .@"error" => "ERROR",
                .warning => "WARN",
                .info => "INFO",
            };
            try w.print("{s} {s}: {s}\n", .{ tag, d.code, d.message });
        }
        return out.toOwnedSlice();
    }

    pub fn canUndo(self: *const Session) bool {
        return self.undo_stack.items.len > 0;
    }

    /// Revert the last successful edit. Returns false if nothing to undo.
    pub fn undo(self: *Session) !bool {
        const top = self.undo_stack.pop() orelse return false;
        var info = try docinfo.parse(self.gpa, top.doc);
        errdefer info.deinit();
        self.gpa.free(self.doc);
        if (self.info) |*i| i.deinit();
        self.doc = top.doc;
        self.info = info;
        self.rev = self.log.items[top.log_index].rev_before;
        self.log.items[top.log_index].undone = true;
        try self.refreshCheck();
        return true;
    }

    // ── Read paths ──────────────────────────────────────────────────

    pub fn drawingJson(self: *Session, view: []const u8) eng.CallResult {
        const req = (eng.Request{ .doc = self.doc, .style = self.styleOpt(), .view = view }).build(self.gpa) catch return .{ .err = self.gpa.dupe(u8, "out of memory") catch &.{} };
        defer self.gpa.free(req);
        return self.engine.call(self.gpa, "drawing", req);
    }

    pub fn meshJson(self: *Session) eng.CallResult {
        const req = (eng.Request{ .doc = self.doc, .style = self.styleOpt() }).build(self.gpa) catch return .{ .err = self.gpa.dupe(u8, "out of memory") catch &.{} };
        defer self.gpa.free(req);
        return self.engine.call(self.gpa, "mesh", req);
    }

    pub fn exportBytes(self: *Session, view: []const u8, format: []const u8, sheet: bool) eng.CallResult {
        const req = (eng.Request{ .doc = self.doc, .style = self.styleOpt(), .view = view, .format = format, .sheet = sheet }).build(self.gpa) catch return .{ .err = self.gpa.dupe(u8, "out of memory") catch &.{} };
        defer self.gpa.free(req);
        return self.engine.call(self.gpa, "export", req);
    }

    /// `query_json` is the raw JSON object for the query, e.g. `{"q":"summary"}`.
    pub fn inspectJson(self: *Session, query_json: []const u8) eng.CallResult {
        const req = (eng.Request{ .doc = self.doc, .style = self.styleOpt(), .query = query_json }).build(self.gpa) catch return .{ .err = self.gpa.dupe(u8, "out of memory") catch &.{} };
        defer self.gpa.free(req);
        return self.engine.call(self.gpa, "inspect", req);
    }
};

fn jstr(v: ?std.json.Value) []const u8 {
    const val = v orelse return "";
    return if (val == .string) val.string else "";
}

fn extractDoc(gpa: Allocator, fmt_out: []const u8) ![]u8 {
    var parsed = try std.json.parseFromSlice(std.json.Value, gpa, fmt_out, .{});
    defer parsed.deinit();
    const root = switch (parsed.value) {
        .object => |o| o,
        else => return error.BadEngineReply,
    };
    const d = root.get("doc") orelse return error.BadEngineReply;
    return std.json.Stringify.valueAlloc(gpa, d, .{ .whitespace = .indent_2 });
}

fn countOps(ops_json: []const u8) u32 {
    var n: u32 = 0;
    var depth: i32 = 0;
    var in_str = false;
    var esc = false;
    for (ops_json) |c| {
        if (in_str) {
            if (esc) esc = false else if (c == '\\') esc = true else if (c == '"') in_str = false;
            continue;
        }
        switch (c) {
            '"' => in_str = true,
            '[', '{' => {
                depth += 1;
                if (c == '{' and depth == 2) n += 1;
            },
            ']', '}' => depth -= 1,
            else => {},
        }
    }
    return n;
}

fn opsStartWithSetDoc(ops_json: []const u8) bool {
    return std.mem.indexOf(u8, ops_json, "\"set\"") != null and std.mem.indexOf(u8, ops_json, "\"doc\"") != null;
}

// ── Tests with a fake engine ────────────────────────────────────────

pub const FakeEngine = struct {
    calls: u32 = 0,
    fail_apply: bool = false,

    pub fn engine(self: *FakeEngine) eng.Engine {
        return .{ .ctx = self, .call_fn = call, .name = "fake" };
    }

    fn call(ctx: *anyopaque, alloc: Allocator, name: []const u8, input: []const u8) eng.CallResult {
        const self: *FakeEngine = @ptrCast(@alignCast(ctx));
        self.calls += 1;
        var parsed = std.json.parseFromSlice(std.json.Value, alloc, input, .{}) catch return .{ .err = alloc.dupe(u8, "bad input") catch &.{} };
        defer parsed.deinit();
        const root = parsed.value.object;
        if (std.mem.eql(u8, name, "fmt")) {
            return .{ .ok = std.json.Stringify.valueAlloc(alloc, .{ .doc = root.get("doc").? }, .{}) catch &.{} };
        }
        if (std.mem.eql(u8, name, "check")) {
            return .{ .ok = alloc.dupe(u8, "{\"summary\":\"DOC fake\",\"diagnostics\":[{\"level\":\"warning\",\"code\":\"W_TEST\",\"id\":\"x\",\"message\":\"m\"}]}") catch &.{} };
        }
        if (std.mem.eql(u8, name, "apply")) {
            if (self.fail_apply) return .{ .ok = alloc.dupe(u8, "{\"ok\":false,\"diagnostics\":[{\"level\":\"error\",\"code\":\"E_PARAM\",\"message\":\"bad\"}]}") catch &.{} };
            // "Apply": replace title with ops[0].value.title when set doc, else echo doc.
            var doc = root.get("doc").?;
            const ops = root.get("ops").?.array.items;
            if (ops.len > 0) {
                const op = ops[0].object;
                if (std.mem.eql(u8, op.get("op").?.string, "set")) doc = op.get("value").?;
            }
            const out = std.json.Stringify.valueAlloc(alloc, .{ .ok = true, .doc = doc, .summary = "DOC fake", .diagnostics = &[_]u8{}, .changed = &[_]u8{} }, .{}) catch return .{ .err = alloc.dupe(u8, "oom") catch &.{} };
            return .{ .ok = out };
        }
        return .{ .err = alloc.dupe(u8, "unsupported") catch &.{} };
    }
};

test "load, apply, undo, log" {
    const a = std.testing.allocator;
    var fe: FakeEngine = .{};
    var s = Session.init(a, fe.engine(), "{}");
    defer s.deinit();
    var err: ?[]u8 = null;
    try std.testing.expect(try s.load("{\"kerf\":\"0.1\",\"id\":\"a\",\"title\":\"A\",\"components\":[],\"views\":[]}", &err));
    try std.testing.expectEqualStrings("a", s.info.?.id);
    try std.testing.expectEqual(@as(u32, 1), s.warnCount());

    const r = try s.apply("[{\"op\":\"set\",\"path\":\"doc\",\"value\":{\"kerf\":\"0.1\",\"id\":\"b\",\"title\":\"B\",\"components\":[],\"views\":[]}}]", "swap", .claude, 100);
    defer a.free(r.text);
    try std.testing.expect(r.ok);
    try std.testing.expectEqual(@as(u32, 1), r.op_count);
    try std.testing.expectEqualStrings("b", s.info.?.id);
    try std.testing.expectEqual(@as(u32, 1), s.rev);
    try std.testing.expectEqual(@as(usize, 1), s.log.items.len);

    try std.testing.expect(try s.undo());
    try std.testing.expectEqualStrings("a", s.info.?.id);
    try std.testing.expect(s.log.items[0].undone);
    try std.testing.expectEqual(@as(u32, 0), s.rev);
}

test "failed apply changes nothing" {
    const a = std.testing.allocator;
    var fe: FakeEngine = .{ .fail_apply = true };
    var s = Session.init(a, fe.engine(), "{}");
    defer s.deinit();
    var err: ?[]u8 = null;
    try std.testing.expect(try s.load("{\"id\":\"a\",\"components\":[],\"views\":[]}", &err));
    const r = try s.apply("[{\"op\":\"remove\",\"path\":\"components/zz\"}]", "x", .designer, 0);
    defer a.free(r.text);
    try std.testing.expect(!r.ok);
    try std.testing.expectEqual(@as(u32, 0), s.rev);
    try std.testing.expectEqual(@as(usize, 0), s.log.items.len);
    try std.testing.expect(std.mem.indexOf(u8, r.text, "FAILED") != null);
}
