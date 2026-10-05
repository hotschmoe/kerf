//! The compiled scene: components (with builder output and per-instance transforms), reference
//! resolution (`comp[#k][.part]@anchor`), and zone lookups.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const catalog = @import("catalog.zig");
const style_mod = @import("style.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;

pub const State = enum { pending, ok, failed };

pub const Comp = struct {
    index: u32,
    id: []const u8,
    ty: *const catalog.Entry,
    node: json.Value,
    label: ?[]const u8 = null,
    state: State = .pending,
    /// Builder output in local coordinates (after `mirror`).
    built: model.Built = undefined,
    /// One transform per instance (array expansion, cover-based rebar placement).
    xfs: []const geom.Xf = &.{},
    /// z range per instance.
    zs: []const [2]f64 = &.{},
    embedded: bool = false,
    visible: bool = true,
    /// Resolved placement point (world) and rotation, for reporting.
    place_pt: V2 = .{ .x = 0, .y = 0 },
    /// Prisms of every instance, world coordinates, in builder order per instance.
    world: []const model.Prism = &.{},

    pub fn instances(self: *const Comp) usize {
        return self.xfs.len;
    }
};

pub const RefParts = struct {
    comp: []const u8 = "",
    inst: u32 = 0,
    part: ?[]const u8 = null,
    anchor: []const u8 = "",
    origin: bool = false,
};

pub const RefError = error{BadRef};

/// Parse `<component>[#k][.part]@<anchor>` or `@origin`.
pub fn parseRef(s: []const u8) RefError!RefParts {
    const at = std.mem.indexOfScalar(u8, s, '@') orelse return error.BadRef;
    const head = s[0..at];
    const anchor = s[at + 1 ..];
    if (anchor.len == 0) return error.BadRef;
    if (head.len == 0) {
        if (std.mem.eql(u8, anchor, "origin")) return .{ .origin = true };
        return error.BadRef;
    }
    var r = RefParts{ .anchor = anchor };
    var comp = head;
    if (std.mem.indexOfScalar(u8, head, '.')) |d| {
        comp = head[0..d];
        r.part = head[d + 1 ..];
        if (r.part.?.len == 0) return error.BadRef;
    }
    if (std.mem.indexOfScalar(u8, comp, '#')) |h| {
        r.inst = std.fmt.parseInt(u32, comp[h + 1 ..], 10) catch return error.BadRef;
        comp = comp[0..h];
    }
    if (comp.len == 0) return error.BadRef;
    r.comp = comp;
    return r;
}

pub const Scene = struct {
    a: Allocator,
    style: *const style_mod.Style,
    comps: []Comp,
    diags: *model.Diags,
    run: [2]f64,

    pub fn find(self: *const Scene, id: []const u8) ?*Comp {
        for (self.comps) |*c| if (std.mem.eql(u8, c.id, id)) return c;
        return null;
    }

    pub fn compIds(self: *const Scene, a: Allocator) Allocator.Error![]const []const u8 {
        const out = try a.alloc([]const u8, self.comps.len);
        for (self.comps, 0..) |c, i| out[i] = c.id;
        return out;
    }

    /// Local box of a part (zone box, else the bbox of that part's prisms).
    pub fn partBox(c: *const Comp, part: []const u8) ?geom.Box {
        for (c.built.zones) |z| if (std.mem.eql(u8, z.name, part)) return z.box;
        var b = geom.Box{};
        for (c.built.prisms) |p| if (std.mem.eql(u8, p.part, part)) {
            for (p.loops) |l| b.addBox(geom.loopBox(l));
        };
        return if (b.isEmpty()) null else b;
    }

    pub fn partNames(self: *const Scene, c: *const Comp) Allocator.Error![]const []const u8 {
        var out: std.ArrayList([]const u8) = .empty;
        for (c.built.zones) |z| try out.append(self.a, z.name);
        for (c.built.prisms) |p| {
            if (p.part.len == 0) continue;
            var seen = false;
            for (out.items) |o| if (std.mem.eql(u8, o, p.part)) {
                seen = true;
                break;
            };
            if (!seen) try out.append(self.a, p.part);
        }
        return out.items;
    }

    pub fn anchorNames(self: *const Scene, c: *const Comp) Allocator.Error![]const []const u8 {
        var out: std.ArrayList([]const u8) = .empty;
        for (model.box_anchor_names) |n| try out.append(self.a, n);
        for (c.built.anchors) |n| try out.append(self.a, n.name);
        return out.items;
    }

    /// World position of an anchor of component `c` instance `inst` (optionally of a part).
    pub fn anchorPoint(c: *const Comp, inst: usize, part: ?[]const u8, name: []const u8) ?V2 {
        if (inst >= c.xfs.len) return null;
        const xf = c.xfs[inst];
        if (part) |p| {
            const b = partBox(c, p) orelse return null;
            const lp = model.boxAnchor(b, name) orelse return null;
            return xf.apply(lp);
        }
        if (model.boxAnchor(c.built.box, name)) |lp| return xf.apply(lp);
        for (c.built.anchors) |n| if (std.mem.eql(u8, n.name, name)) return xf.apply(n.p);
        return null;
    }

    /// Resolve a Ref string to a world point, reporting E_REF_UNKNOWN / E_ANCHOR_UNKNOWN.
    pub fn resolveRefStr(self: *Scene, ref: []const u8, from_id: []const u8, path: []const u8) ?V2 {
        const r = parseRef(ref) catch {
            self.diags.addFix(.@"error", "E_PARAM", from_id, path, "'{s}' is not a valid Ref; use \"<component>@<anchor>\", \"<component>.<part>@<anchor>\" or \"@origin\"", .{ref}, "e.g. \"sill_plate@top_left\"");
            return null;
        };
        if (r.origin) return V2.init(0, 0);
        const c = self.find(r.comp) orelse {
            const ids = self.compIds(self.a) catch &[_][]const u8{};
            if (model.nearest(self.a, r.comp, ids)) |n| {
                self.diags.addFix(.@"error", "E_REF_UNKNOWN", from_id, path, "reference '{s}': no component with id '{s}'; did you mean '{s}'?", .{ ref, r.comp, n }, n);
            } else {
                self.diags.add(.@"error", "E_REF_UNKNOWN", from_id, path, "reference '{s}': no component with id '{s}'. Known ids: {s}", .{ ref, r.comp, joinIds(self.a, ids) });
            }
            return null;
        };
        if (c.state != .ok) {
            self.diags.add(.@"error", "E_REF_UNKNOWN", from_id, path, "reference '{s}': component '{s}' did not build (see its own errors), so its anchors are unavailable", .{ ref, r.comp });
            return null;
        }
        if (r.inst >= c.xfs.len) {
            self.diags.add(.@"error", "E_REF_UNKNOWN", from_id, path, "reference '{s}': component '{s}' has {d} instance(s); instance #{d} does not exist", .{ ref, r.comp, c.xfs.len, r.inst });
            return null;
        }
        if (r.part) |p| {
            if (partBox(c, p) == null) {
                const parts = self.partNames(c) catch &[_][]const u8{};
                self.diags.add(.@"error", "E_REF_UNKNOWN", from_id, path, "reference '{s}': component '{s}' has no part '{s}'. Parts: {s}", .{ ref, r.comp, p, if (parts.len == 0) "(none)" else joinIds(self.a, parts) });
                return null;
            }
        }
        if (anchorPoint(c, r.inst, r.part, r.anchor)) |pt| return pt;
        const names = self.anchorNames(c) catch &[_][]const u8{};
        self.diags.add(.@"error", "E_ANCHOR_UNKNOWN", from_id, path, "reference '{s}': {s} '{s}' has no anchor '{s}'. Anchors: {s}", .{
            ref,
            if (r.part != null) "part of" else "component",
            r.comp,
            r.anchor,
            if (r.part != null) box_names_text else joinIds(self.a, names),
        });
        return null;
    }
};

const box_names_text = "top_left, top_center, top_right, middle_left, center, middle_right, bottom_left, bottom_center, bottom_right";

pub fn joinIds(a: Allocator, ids: []const []const u8) []const u8 {
    var out: std.ArrayList(u8) = .empty;
    for (ids, 0..) |id, i| {
        if (i > 0) out.appendSlice(a, ", ") catch {};
        out.appendSlice(a, id) catch {};
    }
    return out.items;
}

test "parse refs" {
    const r1 = try parseRef("sill_plate@top_left");
    try std.testing.expectEqualStrings("sill_plate", r1.comp);
    try std.testing.expectEqualStrings("top_left", r1.anchor);
    const r2 = try parseRef("truss#1.top_chord@top_right");
    try std.testing.expectEqualStrings("truss", r2.comp);
    try std.testing.expectEqual(@as(u32, 1), r2.inst);
    try std.testing.expectEqualStrings("top_chord", r2.part.?);
    try std.testing.expect((try parseRef("@origin")).origin);
    try std.testing.expectError(error.BadRef, parseRef("nope"));
    try std.testing.expectError(error.BadRef, parseRef("a@"));
    try std.testing.expectError(error.BadRef, parseRef("@x"));
}
