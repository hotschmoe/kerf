//! In-memory fake `tools.Engine` for tests and demo mode. It understands just enough of the op
//! grammar (set doc / add / update / remove of components) to produce plausible, deterministic
//! summaries and diagnostics. It is NOT the Kerf engine; the app wires the real one.

const std = @import("std");
const Allocator = std.mem.Allocator;
const tools = @import("tools.zig");
const js = @import("jsonspan.zig");

const Comp = struct { id: []const u8, ty: []const u8 };

/// 1x1 white PNG.
pub const tiny_png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

pub const FakeEngine = struct {
    arena: std.heap.ArenaAllocator,
    comps: std.ArrayList(Comp) = .empty,
    views: std.ArrayList([]const u8) = .empty,
    apply_calls: u32 = 0,
    render_calls: u32 = 0,
    inspect_calls: u32 = 0,
    last_why: []const u8 = "",
    /// When set, the next `apply` fails with this engine error text (then clears).
    fail_next_apply: ?[]const u8 = null,

    pub fn init(gpa: Allocator) FakeEngine {
        return .{ .arena = std.heap.ArenaAllocator.init(gpa) };
    }

    pub fn deinit(self: *FakeEngine) void {
        self.arena.deinit();
    }

    pub fn engine(self: *FakeEngine) tools.Engine {
        return .{ .ctx = self, .apply_fn = applyFn, .inspect_fn = inspectFn, .render_fn = renderFn };
    }

    fn applyFn(ctx: *anyopaque, a: Allocator, ops_json: []const u8, why: []const u8) tools.ApplyOutcome {
        const self: *FakeEngine = @ptrCast(@alignCast(ctx));
        return self.apply(a, ops_json, why) catch .{ .ok = false, .text = "out of memory" };
    }

    fn find(list: []const Comp, id: []const u8) ?usize {
        for (list, 0..) |c, i| if (std.mem.eql(u8, c.id, id)) return i;
        return null;
    }

    fn apply(self: *FakeEngine, a: Allocator, ops_json: []const u8, why: []const u8) Allocator.Error!tools.ApplyOutcome {
        self.apply_calls += 1;
        const pa = self.arena.allocator();
        if (self.fail_next_apply) |msg| {
            self.fail_next_apply = null;
            return .{ .ok = false, .text = msg, .n_err = 1 };
        }
        // Work on copies: atomic.
        var comps: std.ArrayList(Comp) = .empty;
        try comps.appendSlice(pa, self.comps.items);
        var views: std.ArrayList([]const u8) = .empty;
        try views.appendSlice(pa, self.views.items);

        var it = js.ArrIter.init(ops_json) catch return .{ .ok = false, .text = "ops is not an array", .n_err = 1 };
        var idx: usize = 0;
        while (it.next() catch null) |op| : (idx += 1) {
            const kind = (js.getString(pa, op, "op") catch null) orelse "";
            const path = (js.getString(pa, op, "path") catch null) orelse "";
            const value = (js.get(op, "value") catch null) orelse "{}";
            if (std.mem.eql(u8, kind, "set")) {
                if (!std.mem.eql(u8, path, "doc"))
                    return fail(a, "E_PATH: ops[{d}]: `set` only accepts path \"doc\", got \"{s}\".", .{ idx, path });
                comps.clearRetainingCapacity();
                views.clearRetainingCapacity();
                if (js.get(value, "components") catch null) |arr| {
                    var ci = js.ArrIter.init(arr) catch return fail(a, "E_SCHEMA: ops[{d}]: components must be an array.", .{idx});
                    while (ci.next() catch null) |c| {
                        const id = (js.getString(pa, c, "id") catch null) orelse
                            return fail(a, "E_SCHEMA: ops[{d}]: a component has no id.", .{idx});
                        const ty = (js.getString(pa, c, "type") catch null) orelse "?";
                        if (find(comps.items, id) != null) return fail(a, "E_DUP_ID: ops[{d}]: duplicate component id \"{s}\".", .{ idx, id });
                        try comps.append(pa, .{ .id = id, .ty = ty });
                    }
                }
                if (js.get(value, "views") catch null) |arr| {
                    var vi = js.ArrIter.init(arr) catch return fail(a, "E_SCHEMA: ops[{d}]: views must be an array.", .{idx});
                    while (vi.next() catch null) |v| {
                        if ((js.getString(pa, v, "id") catch null)) |id| try views.append(pa, id);
                    }
                }
            } else if (std.mem.eql(u8, kind, "add") and std.mem.eql(u8, path, "components")) {
                const id = (js.getString(pa, value, "id") catch null) orelse
                    return fail(a, "E_SCHEMA: ops[{d}]: component value needs an \"id\".", .{idx});
                const ty = (js.getString(pa, value, "type") catch null) orelse "?";
                if (find(comps.items, id) != null) return fail(a, "E_DUP_ID: ops[{d}]: duplicate component id \"{s}\".", .{ idx, id });
                try comps.append(pa, .{ .id = id, .ty = ty });
            } else if (std.mem.startsWith(u8, path, "components/")) {
                const id = path["components/".len..];
                const at = find(comps.items, id) orelse
                    return fail(a, "E_REF_UNKNOWN: ops[{d}]: no component \"{s}\". Existing ids: {s}.", .{ idx, id, try idList(a, comps.items) });
                if (std.mem.eql(u8, kind, "remove")) {
                    _ = comps.orderedRemove(at);
                } else if (!std.mem.eql(u8, kind, "update")) {
                    return fail(a, "E_PATH: ops[{d}]: `{s}` is not valid on {s}.", .{ idx, kind, path });
                }
            } else if (std.mem.eql(u8, kind, "add") and std.mem.eql(u8, path, "views")) {
                if ((js.getString(pa, value, "id") catch null)) |id| try views.append(pa, id);
            }
            // views/*, meta, annotations: accepted without effect.
        }
        self.comps = comps;
        self.views = views;
        self.last_why = try pa.dupe(u8, why);

        var n_warn: u32 = 0;
        var out = std.ArrayList(u8).empty;
        try out.print(a, "ok\n{d} components, {d} views\n", .{ comps.items.len, views.items.len });
        for (comps.items) |c| try out.print(a, "  {s}  {s}\n", .{ c.id, c.ty });
        for (comps.items) |c| if (std.mem.eql(u8, c.ty, "solid")) {
            n_warn += 1;
            try out.print(a, "W W_SOLID {s}: generic solid flagged for review\n", .{c.id});
        };
        try out.print(a, "diagnostics: 0 ERR {d} WARN", .{n_warn});
        return .{ .ok = true, .text = out.items, .n_err = 0, .n_warn = n_warn };
    }

    fn fail(a: Allocator, comptime fmt: []const u8, args: anytype) Allocator.Error!tools.ApplyOutcome {
        return .{ .ok = false, .text = try std.fmt.allocPrint(a, fmt, args), .n_err = 1 };
    }

    fn idList(a: Allocator, comps: []const Comp) Allocator.Error![]const u8 {
        var out = std.ArrayList(u8).empty;
        for (comps, 0..) |c, i| {
            if (i != 0) try out.appendSlice(a, ", ");
            try out.appendSlice(a, c.id);
        }
        return out.items;
    }

    fn inspectFn(ctx: *anyopaque, a: Allocator, args_json: []const u8) tools.TextOutcome {
        const self: *FakeEngine = @ptrCast(@alignCast(ctx));
        self.inspect_calls += 1;
        return inspectImpl(self, a, args_json) catch .{ .ok = false, .text = "out of memory" };
    }

    fn inspectImpl(self: *FakeEngine, a: Allocator, args_json: []const u8) Allocator.Error!tools.TextOutcome {
        const q = (js.getString(a, args_json, "q") catch null) orelse "summary";
        if (std.mem.eql(u8, q, "component") or std.mem.eql(u8, q, "anchors")) {
            const id = (js.getString(a, args_json, "id") catch null) orelse "";
            const at = find(self.comps.items, id) orelse
                return .{ .ok = false, .text = try std.fmt.allocPrint(a, "E_REF_UNKNOWN: no component \"{s}\". Existing ids: {s}.", .{ id, try idList(a, self.comps.items) }) };
            return .{ .text = try std.fmt.allocPrint(a, "{s} ({s})\nanchors: top_left top_center top_right middle_left center middle_right bottom_left bottom_center bottom_right", .{ id, self.comps.items[at].ty }) };
        }
        var out = std.ArrayList(u8).empty;
        try out.print(a, "{d} components, {d} views\n", .{ self.comps.items.len, self.views.items.len });
        for (self.comps.items) |c| try out.print(a, "  {s}  {s}\n", .{ c.id, c.ty });
        try out.appendSlice(a, "diagnostics: 0 ERR 0 WARN");
        return .{ .text = out.items };
    }

    fn renderFn(ctx: *anyopaque, a: Allocator, view: []const u8, mode: []const u8) tools.RenderOutcome {
        const self: *FakeEngine = @ptrCast(@alignCast(ctx));
        self.render_calls += 1;
        var known = false;
        for (self.views.items) |v| if (std.mem.eql(u8, v, view)) {
            known = true;
        };
        if (!known) return .{ .ok = false, .caption = std.fmt.allocPrint(a, "E_VIEW_UNKNOWN: no view \"{s}\". Existing views: {d}.", .{ view, self.views.items.len }) catch "no such view" };
        const dec = std.base64.standard.Decoder;
        const n = dec.calcSizeForSlice(tiny_png_b64) catch return .{ .ok = false, .caption = "png" };
        const buf = a.alloc(u8, n) catch return .{ .ok = false, .caption = "oom" };
        dec.decode(buf, tiny_png_b64) catch return .{ .ok = false, .caption = "png" };
        return .{
            .png = buf,
            .caption = std.fmt.allocPrint(a, "{s} {s} rendered; {d} components; 0 errors", .{ mode, view, self.comps.items.len }) catch "rendered",
        };
    }
};
