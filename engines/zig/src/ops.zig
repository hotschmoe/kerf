//! Document edit ops (SPEC 14): add / update / remove / set, atomic, with citation rules.

const std = @import("std");
const json = @import("json.zig");
const model = @import("model.zig");
const compile_mod = @import("compile.zig");
const scene_mod = @import("scene.zig");
const Allocator = std.mem.Allocator;
const Value = json.Value;
const Member = json.Member;

pub const Actor = enum { llm, designer };

pub const Applied = struct {
    doc: Value,
    changed: []const []const u8,
};

const Ctx = struct {
    a: Allocator,
    diags: *model.Diags,
    actor: Actor,
    doc: Value,
    changed: std.ArrayList([]const u8) = .empty,
    op_index: usize = 0,
    ok: bool = true,

    fn fail(self: *Ctx, code: []const u8, path: []const u8, comptime fmt: []const u8, args: anytype) void {
        self.ok = false;
        const msg = self.a.print("op {d}: " ++ fmt, .{self.op_index} ++ args) catch return;
        self.diags.list.append(self.a, .{ .level = .@"error", .code = code, .path = path, .message = msg }) catch {};
    }

    fn touch(self: *Ctx, id: []const u8) void {
        for (self.changed.items) |c| if (std.mem.eql(u8, c, id)) return;
        self.changed.append(self.a, id) catch {};
    }
};

fn setKey(a: Allocator, obj: Value, key: []const u8, val: Value) Allocator.Error!Value {
    var ms: std.ArrayList(Member) = .empty;
    var done = false;
    if (obj == .object) for (obj.object) |m| {
        if (std.mem.eql(u8, m.key, key)) {
            try ms.append(a, .{ .key = key, .value = val });
            done = true;
        } else try ms.append(a, m);
    };
    if (!done) try ms.append(a, .{ .key = key, .value = val });
    return .{ .object = ms.items };
}

fn idOf(v: Value) ?[]const u8 {
    if (v.get("id")) |x| return x.str();
    return null;
}

fn indexOfId(list: []const Value, id: []const u8) ?usize {
    for (list, 0..) |v, i| if (idOf(v)) |x| if (std.mem.eql(u8, x, id)) return i;
    return null;
}

fn idList(a: Allocator, list: []const Value) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    for (list) |v| if (idOf(v)) |x| try out.append(a, x);
    return out.items;
}

fn unknownId(c: *Ctx, kind: []const u8, id: []const u8, list: []const Value, path: []const u8) Allocator.Error!void {
    const ids = try idList(c.a, list);
    if (model.nearest(c.a, id, ids)) |n| {
        c.fail("E_REF_UNKNOWN", path, "no {s} '{s}'; did you mean '{s}'? {s}s: {s}", .{ kind, id, n, kind, scene_mod.joinIds(c.a, ids) });
    } else c.fail("E_REF_UNKNOWN", path, "no {s} '{s}'. {s}s: {s}", .{ kind, id, kind, scene_mod.joinIds(c.a, ids) });
}

fn getList(c: *Ctx, key: []const u8) []const Value {
    if (c.doc.get(key)) |v| if (v.arr()) |a| return a;
    return &.{};
}

fn withList(c: *Ctx, key: []const u8, list: []const Value) Allocator.Error!void {
    c.doc = try setKey(c.a, c.doc, key, .{ .array = try c.a.dupe(Value, list) });
}

/// Cite handling: LLM edits can never leave a citation `verified`.
fn sanitizeCites(c: *Ctx, ann: Value, force_reset: bool) Allocator.Error!Value {
    const cv = ann.get("cite") orelse return ann;
    const arr = cv.arr() orelse return ann;
    const out = try c.a.alloc(Value, arr.len);
    for (arr, 0..) |ci, i| {
        const st = if (ci.get("status")) |s| (s.str() orelse "suggested") else "suggested";
        const verified = std.mem.eql(u8, st, "verified");
        if (c.actor == .llm and (verified or force_reset)) {
            if (verified) {
                const sec = if (ci.get("section")) |s| (s.str() orelse "") else "";
                const code = if (ci.get("code")) |s| (s.str() orelse "") else "";
                c.diags.add(.info, "I_CITE_DOWNGRADED", idOf(ann), null, "citation {s} {s} on '{s}' was verified; only the designer can verify code citations, so it was reset to suggested", .{ code, sec, idOf(ann) orelse "" });
            }
            out[i] = try setKey(c.a, ci, "status", .{ .string = "suggested" });
        } else out[i] = if (ci.get("status") == null) try setKey(c.a, ci, "status", .{ .string = "suggested" }) else ci;
    }
    return setKey(c.a, ann, "cite", .{ .array = out });
}

fn referencesComp(v: Value, id: []const u8) bool {
    switch (v) {
        .string => |s| {
            const at = std.mem.indexOfScalar(u8, s, '@');
            var head = if (at) |p| s[0..p] else s;
            if (std.mem.indexOfScalar(u8, head, '.')) |d| head = head[0..d];
            if (std.mem.indexOfScalar(u8, head, '#')) |h| head = head[0..h];
            return std.mem.eql(u8, head, id);
        },
        .object => if (v.get("ref")) |r| return referencesComp(r, id),
        else => {},
    }
    return false;
}

/// Does an annotation point at component `id`: through `target` (a component id or `comp.part`), or an `at`/`from`/`to` ref?
fn annotationReferences(ann: Value, id: []const u8) bool {
    if (ann.get("target")) |t| if (t.str()) |s| {
        const head = if (std.mem.indexOfScalar(u8, s, '.')) |d| s[0..d] else s;
        if (std.mem.eql(u8, head, id)) return true;
    };
    inline for (.{ "at", "from", "to" }) |k| {
        if (ann.get(k)) |x| if (referencesComp(x, id)) return true;
    }
    return false;
}

fn dependents(c: *Ctx, id: []const u8) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    for (getList(c, "components")) |comp| {
        const cid = idOf(comp) orelse continue;
        if (std.mem.eql(u8, cid, id)) continue;
        var deps: std.ArrayList([]const u8) = .empty;
        try compile_mod.collectDeps(c.a, comp, &deps);
        for (deps.items) |d| if (std.mem.eql(u8, d, id)) {
            try out.append(c.a, cid);
            break;
        };
    }
    for (getList(c, "views")) |view| {
        const vid = idOf(view) orelse "?";
        if (view.get("annotations")) |an| if (an.arr()) |aa| for (aa) |ann| {
            const aid = idOf(ann) orelse "?";
            const hit = annotationReferences(ann, id);
            if (hit) try out.append(c.a, try c.a.print("views/{s}/annotations/{s}", .{ vid, aid }));
        };
    }
    return out.items;
}

fn splitPath(a: Allocator, path: []const u8) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    var it = std.mem.splitScalar(u8, path, '/');
    while (it.next()) |s| if (s.len > 0) try out.append(a, s);
    return out.items;
}

const Kind = enum { add, update, remove, set };

/// A well-formed op: what to do, where (the path and its segments) and the value. `raw` is the op object (for `before`).
const Op = struct {
    kind: Kind,
    path: []const u8,
    seg: []const []const u8,
    value: ?Value,
    raw: Value,
};

/// Validate the shape of one op, then hand it to the function of its path root.
fn applyOne(c: *Ctx, raw: Value) Allocator.Error!void {
    if (raw != .object) return c.fail("E_OP", "", "each op must be an object like {{\"op\": \"add\", \"path\": \"components\", \"value\": {{...}}}}; got a {s}", .{raw.kindName()});
    const kind_s = (if (raw.get("op")) |x| x.str() else null) orelse return c.fail("E_OP", "", "op needs \"op\": add | update | remove | set", .{});
    const path = (if (raw.get("path")) |x| x.str() else null) orelse return c.fail("E_OP", "", "op needs a string \"path\" such as \"components\", \"components/<id>\", \"views/<id>/annotations\"", .{});
    const value = raw.get("value");
    const kind = std.meta.stringToEnum(Kind, kind_s) orelse return c.fail("E_OP", path, "unknown op \"{s}\"; use add, update, remove or set", .{kind_s});
    if (kind != .remove and (value == null or value.? == .null)) return c.fail("E_OP", path, "op {s} {s} needs a \"value\"", .{ kind_s, path });
    const seg = try splitPath(c.a, path);
    if (seg.len == 0) return c.fail("E_OP", path, "empty path", .{});
    const op = Op{ .kind = kind, .path = path, .seg = seg, .value = value, .raw = raw };
    const root = seg[0];
    if (std.mem.eql(u8, root, "doc")) return applyDoc(c, op);
    if (c.doc != .object) c.doc = .{ .object = &.{} };
    if (std.mem.eql(u8, root, "meta")) return applyMeta(c, op);
    if (std.mem.eql(u8, root, "components")) return applyComponents(c, op);
    if (std.mem.eql(u8, root, "views")) return applyViews(c, op);
    return c.fail("E_OP", path, "unknown path root \"{s}\"; use doc, meta, components, views", .{root});
}

/// `set doc`: replace the whole document (an LLM's citations are sanitized).
fn applyDoc(c: *Ctx, op: Op) Allocator.Error!void {
    const a = c.a;
    if (op.kind != .set or op.seg.len != 1) return c.fail("E_OP", op.path, "only {{\"op\": \"set\", \"path\": \"doc\", \"value\": {{whole document}}}} may target \"doc\"", .{});
    if (op.value.? != .object) return c.fail("E_OP", op.path, "set doc needs the whole document object as value", .{});
    var v = op.value.?;
    // LLM-authored documents cannot carry verified citations
    if (c.actor == .llm) {
        if (v.get("views")) |vs| if (vs.arr()) |va| {
            const nv = try a.alloc(Value, va.len);
            for (va, 0..) |view, vi| {
                var nview = view;
                if (view.get("annotations")) |an| if (an.arr()) |aa| {
                    const na = try a.alloc(Value, aa.len);
                    for (aa, 0..) |ann, k| na[k] = try sanitizeCites(c, ann, false);
                    nview = try setKey(a, view, "annotations", .{ .array = na });
                };
                nv[vi] = nview;
            }
            v = try setKey(a, v, "views", .{ .array = nv });
        };
    }
    c.doc = v;
    c.changed.clearRetainingCapacity();
    for (getList(c, "components")) |comp| if (idOf(comp)) |i| c.touch(i);
    for (getList(c, "views")) |view| if (idOf(view)) |i| c.touch(i);
}

/// `update meta`: merge patch (or `set`).
fn applyMeta(c: *Ctx, op: Op) Allocator.Error!void {
    const a = c.a;
    if (op.seg.len != 1 or op.kind == .add or op.kind == .remove) return c.fail("E_OP", op.path, "meta supports only {{\"op\": \"update\", \"path\": \"meta\", \"value\": {{merge patch}}}}", .{});
    const cur = c.doc.get("meta") orelse Value{ .object = &.{} };
    const nv = if (op.kind == .set) try json.clone(a, op.value.?) else try json.mergePatch(a, cur, op.value.?);
    c.doc = try setKey(a, c.doc, "meta", nv);
    c.touch("meta");
}

/// `components`, `components/<id>`: add (optionally `before`), update, set, remove (refused while referenced).
fn applyComponents(c: *Ctx, op: Op) Allocator.Error!void {
    const a = c.a;
    const eq = std.mem.eql;
    const list = getList(c, "components");
    if (op.seg.len == 1 and op.kind == .add) {
        const v = op.value.?;
        const id = idOf(v) orelse return c.fail("E_PARAM", op.path, "a component needs a string \"id\" matching [a-z][a-z0-9_]*", .{});
        if (indexOfId(list, id) != null) return c.fail("E_DUP_ID", op.path, "component id '{s}' already exists; ids must be unique (use op update to change it)", .{id});
        var nl: std.ArrayList(Value) = .empty;
        var placed = false;
        if (op.raw.get("before")) |b| if (b.str()) |bid| {
            if (indexOfId(list, bid) == null) return unknownId(c, "component", bid, list, op.path);
            for (list) |x| {
                if (!placed and idOf(x) != null and eq(u8, idOf(x).?, bid)) {
                    try nl.append(a, v);
                    placed = true;
                }
                try nl.append(a, x);
            }
        };
        if (!placed) {
            try nl.appendSlice(a, list);
            try nl.append(a, v);
        }
        try withList(c, "components", nl.items);
        c.touch(id);
        return;
    }
    if (op.seg.len == 2) {
        const id = op.seg[1];
        const idx = indexOfId(list, id) orelse return unknownId(c, "component", id, list, op.path);
        if (op.kind == .update or op.kind == .set) {
            if (op.value.?.get("id")) |nid| if (nid.str()) |s| if (!eq(u8, s, id)) return c.fail("E_OP", op.path, "ids cannot be changed by update (got \"{s}\" for '{s}'); remove and re-add the component, then fix references", .{ s, id });
            const nv = if (op.kind == .set) op.value.? else try json.mergePatch(a, list[idx], op.value.?);
            var nl = try a.dupe(Value, list);
            nl[idx] = nv;
            try withList(c, "components", nl);
            c.touch(id);
            return;
        }
        if (op.kind == .remove) {
            const deps = try dependents(c, id);
            if (deps.len > 0) return c.fail("E_REF_UNKNOWN", op.path, "cannot remove '{s}': still referenced by {s}. Remove or re-point those first (or remove them in the same op batch before this op)", .{ id, scene_mod.joinIds(a, deps) });
            var nl: std.ArrayList(Value) = .empty;
            for (list, 0..) |x, i| if (i != idx) try nl.append(a, x);
            try withList(c, "components", nl.items);
            c.touch(id);
            return;
        }
    }
    return c.fail("E_OP", op.path, "unsupported op {s} on path {s}; components paths: \"components\" (add), \"components/<id>\" (update, remove)", .{ @tagName(op.kind), op.path });
}

/// `views`, `views/<id>`, `views/<id>/annotations[/<id>]`.
fn applyViews(c: *Ctx, op: Op) Allocator.Error!void {
    const a = c.a;
    const eq = std.mem.eql;
    const list = getList(c, "views");
    if (op.seg.len == 1 and op.kind == .add) {
        const v = op.value.?;
        const id = idOf(v) orelse return c.fail("E_PARAM", op.path, "a view needs a string \"id\"", .{});
        if (indexOfId(list, id) != null) return c.fail("E_DUP_ID", op.path, "view id '{s}' already exists", .{id});
        var nv = v;
        if (c.actor == .llm) if (v.get("annotations")) |an| if (an.arr()) |aa| {
            const na = try a.alloc(Value, aa.len);
            for (aa, 0..) |ann, k| na[k] = try sanitizeCites(c, ann, false);
            nv = try setKey(a, v, "annotations", .{ .array = na });
        };
        var nl: std.ArrayList(Value) = .empty;
        try nl.appendSlice(a, list);
        try nl.append(a, nv);
        try withList(c, "views", nl.items);
        c.touch(id);
        return;
    }
    if (op.seg.len >= 2) {
        const vid = op.seg[1];
        const vi = indexOfId(list, vid) orelse return unknownId(c, "view", vid, list, op.path);
        const view = list[vi];
        if (op.seg.len == 2) {
            if (op.kind == .update) {
                if (op.value.?.get("annotations") != null) return c.fail("E_OP", op.path, "update views/{s} cannot carry \"annotations\"; use add/update/remove on views/{s}/annotations[/<id>]", .{ vid, vid });
                if (op.value.?.get("id")) |nid| if (nid.str()) |s| if (!eq(u8, s, vid)) return c.fail("E_OP", op.path, "view ids cannot be changed by update", .{});
                var nl = try a.dupe(Value, list);
                nl[vi] = try json.mergePatch(a, view, op.value.?);
                try withList(c, "views", nl);
                c.touch(vid);
                return;
            }
            if (op.kind == .remove) {
                var nl: std.ArrayList(Value) = .empty;
                for (list, 0..) |x, i| if (i != vi) try nl.append(a, x);
                try withList(c, "views", nl.items);
                c.touch(vid);
                return;
            }
        }
        if (op.seg.len >= 3 and eq(u8, op.seg[2], "annotations")) {
            const anns: []const Value = if (view.get("annotations")) |x| (x.arr() orelse &.{}) else &.{};
            var new_anns: std.ArrayList(Value) = .empty;
            if (op.seg.len == 3 and op.kind == .add) {
                const v = op.value.?;
                const aid = idOf(v) orelse return c.fail("E_PARAM", op.path, "an annotation needs a string \"id\"", .{});
                if (indexOfId(anns, aid) != null) return c.fail("E_DUP_ID", op.path, "annotation id '{s}' already exists in view '{s}'", .{ aid, vid });
                try new_anns.appendSlice(a, anns);
                try new_anns.append(a, try sanitizeCites(c, v, false));
                c.touch(aid);
            } else if (op.seg.len == 4) {
                const aid = op.seg[3];
                const ai = indexOfId(anns, aid) orelse return unknownId(c, "annotation", aid, anns, op.path);
                if (op.kind == .update) {
                    const patch = op.value.?;
                    if (patch.get("id")) |nid| if (nid.str()) |s| if (!eq(u8, s, aid)) return c.fail("E_OP", op.path, "annotation ids cannot be changed by update", .{});
                    var merged = try json.mergePatch(a, anns[ai], patch);
                    const reset = patch.get("text") != null or patch.get("cite") != null;
                    merged = try sanitizeCites(c, merged, reset and c.actor == .llm);
                    try new_anns.appendSlice(a, anns);
                    new_anns.items[ai] = merged;
                } else if (op.kind == .remove) {
                    for (anns, 0..) |x, i| if (i != ai) try new_anns.append(a, x);
                } else return c.fail("E_OP", op.path, "unsupported op {s} on annotation path", .{@tagName(op.kind)});
                c.touch(aid);
            } else return c.fail("E_OP", op.path, "annotation paths: \"views/<id>/annotations\" (add), \"views/<id>/annotations/<id>\" (update, remove)", .{});
            var nl = try a.dupe(Value, list);
            nl[vi] = try setKey(a, view, "annotations", .{ .array = new_anns.items });
            try withList(c, "views", nl);
            return;
        }
    }
    return c.fail("E_OP", op.path, "unsupported op {s} on path {s}; view paths: \"views\" (add), \"views/<id>\" (update, remove), \"views/<id>/annotations[/<id>]\"", .{ @tagName(op.kind), op.path });
}

/// The three accepted shapes of the ops input (SPEC 21), quoted in the `E_PARAM` message.
const ops_shapes = "an array of op objects [{\"op\":...}, ...], a single op object {\"op\":...}, or {\"ops\":[...], \"why\":\"...\"}";

pub const Normalized = struct {
    /// Always an array of ops.
    ops: Value,
    /// The reason from the `{"ops":[...],"why":"..."}` envelope, if present.
    why: ?[]const u8 = null,
};

/// One-line description of what an ops input was, for error messages ("an object with keys op, path, value").
fn describeInput(a: Allocator, v: Value) Allocator.Error![]const u8 {
    if (v != .object) return a.print("a {s}", .{v.kindName()});
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "an object with keys ");
    if (v.object.len == 0) try out.appendSlice(a, "(none)");
    for (v.object, 0..) |m, i| {
        if (i > 0) try out.appendSlice(a, ", ");
        try out.appendSlice(a, m.key);
    }
    return out.items;
}

/// Accept an ops array, a single op object, or `{"ops":[...]}` (optionally with `"why"`); null (+ `E_PARAM`) otherwise.
pub fn normalize(a: Allocator, input: Value, diags: *model.Diags) Allocator.Error!?Normalized {
    switch (input) {
        .array => return .{ .ops = input },
        .object => {
            if (input.get("ops")) |inner| {
                if (inner == .array) return .{ .ops = inner, .why = if (input.get("why")) |w| w.str() else null };
                if (inner == .object and inner.get("op") != null) return .{ .ops = .{ .array = try a.dupe(Value, &.{inner}) }, .why = if (input.get("why")) |w| w.str() else null };
            } else if (input.get("op") != null) {
                return .{ .ops = .{ .array = try a.dupe(Value, &.{input}) } };
            }
        },
        else => {},
    }
    const got = try describeInput(a, input);
    const fix: []const u8 = if (input == .object) "wrap a single op in [ ], or put the ops under an \"ops\" key" else "pass the op list as a JSON array";
    diags.addFix(.@"error", "E_PARAM", null, "ops", "the ops input must be {s}; got {s}", .{ ops_shapes, got }, fix);
    return null;
}

/// Apply `ops` (see `normalize` for the accepted shapes) to `doc`. Returns null when any op fails (diagnostics explain).
pub fn apply(a: Allocator, doc: Value, ops_input: Value, actor: Actor, diags: *model.Diags) Allocator.Error!?Applied {
    var c = Ctx{ .a = a, .diags = diags, .actor = actor, .doc = doc };
    const norm = (try normalize(a, ops_input, diags)) orelse return null;
    for (norm.ops.array, 0..) |op, i| {
        c.op_index = i;
        try applyOne(&c, op);
        if (!c.ok) return null;
    }
    return .{ .doc = c.doc, .changed = c.changed.items };
}
