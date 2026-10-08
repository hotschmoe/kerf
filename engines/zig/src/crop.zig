//! W_CROP_STALE (SPEC 21): a section view with an explicit `crop` that a non-fill component mostly falls outside of.
//! In `apply` the document state before the edit is known, so the warning fires when the edit caused it: the component
//! was mostly inside the crop before (or the view auto-fitted), or it is new and no view shows most of it. A member the
//! crop already cut before the edit stays quiet. Without history (`kerf check`, or a view that is new) it fires only for
//! a member that no view shows at all: the reference details crop one model several ways and cut studs on purpose, so
//! "more than 25% outside" alone would flag them (recorded in NOTES.md).

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const scene_mod = @import("scene.zig");
const section = @import("section.zig");
const units = @import("units.zig");
const view_mod = @import("view.zig");
const Allocator = std.mem.Allocator;
const Scene = scene_mod.Scene;

/// A component is stale in a view when more than this fraction of its extent is outside the crop.
const stale_fraction = 0.25;

/// The state of the document before an edit.
pub const Before = struct {
    scene: *const Scene,
    doc: json.Value,
};

/// Bounding box of what a section view at `cut_z` shows of `c`: non-fill prisms that are cut or lie beyond the plane.
fn visibleExtent(c: *const scene_mod.Comp, cut_z: f64) geom.Box {
    var b = geom.Box{};
    for (c.world) |p| {
        if (p.role == .soil) continue;
        const is_cut = p.z0 < cut_z - 1e-9 and p.z1 > cut_z + 1e-9;
        if (!is_cut and p.z1 > cut_z + 1e-9) continue; // above the cut plane: dropped
        for (p.loops) |l| b.addBox(geom.pointsBox(l));
        b.addBox(geom.pointsBox(p.line_pts));
        b.addBox(geom.pointsBox(p.centerline));
    }
    return b;
}

/// Fraction (0..1) of the box that lies outside the crop. A degenerate dimension counts as inside or outside as a whole.
fn outsideFraction(b: geom.Box, crop: geom.Box) f64 {
    const eps = 1e-6;
    const fx = inside1(b.x0, b.x1, crop.x0, crop.x1, eps);
    const fy = inside1(b.y0, b.y1, crop.y0, crop.y1, eps);
    return 1 - fx * fy;
}

fn inside1(lo: f64, hi: f64, c0: f64, c1: f64, eps: f64) f64 {
    if (hi - lo < eps) return if (lo >= c0 - eps and lo <= c1 + eps) 1 else 0;
    return @max(0, @min(hi, c1) - @max(lo, c0)) / (hi - lo);
}

/// The explicit crop of section view `id` in `doc`, or null (no such view, auto crop, iso).
fn explicitCrop(a: Allocator, doc: json.Value, id: []const u8) Allocator.Error!?geom.Box {
    const node = view_mod.findView(doc, id) orelse return null;
    var scratch = model.Diags.init(a);
    const spec = (try view_mod.parse(a, node, 0, &scratch)) orelse return null;
    if (spec.kind != .section or !spec.has_crop) return null;
    return spec.crop;
}

fn fmtIn(a: Allocator, x: f64) []const u8 {
    return units.fmtFtIn(a, x) catch "?";
}

/// True when some view of the document shows `c`: an iso view, a section view without a crop (auto-fit), or a section
/// view whose explicit crop leaves at most `max_outside` (a fraction) of it outside.
fn shownSomewhere(a: Allocator, c: *const scene_mod.Comp, vs: []const json.Value, max_outside: f64) Allocator.Error!bool {
    for (vs, 0..) |vnode, vi| {
        var scratch = model.Diags.init(a);
        const spec = (try view_mod.parse(a, vnode, vi, &scratch)) orelse continue;
        if (spec.kind != .section or !spec.has_crop) return true;
        const ext = visibleExtent(c, spec.cut_z);
        if (!ext.isEmpty() and outsideFraction(ext, spec.crop) <= max_outside) return true;
    }
    return false;
}

/// What the document before the edit says about component `c` in view `spec`.
const Prior = enum {
    /// No history (`kerf check`), or the view is new.
    none,
    /// The component did not exist before the edit.
    added,
    /// It was mostly inside the view's crop (or the view auto-fitted).
    inside,
    /// It was already mostly outside the same view's crop.
    cut,
};

fn prior(a: Allocator, before: ?Before, c: *const scene_mod.Comp, spec: *const view_mod.ViewSpec) Allocator.Error!Prior {
    const bf = before orelse return .none;
    const old = bf.scene.find(c.id) orelse return .added;
    const old_ext = visibleExtent(old, spec.cut_z);
    if (old_ext.isEmpty() or view_mod.findView(bf.doc, spec.id) == null) return .none;
    const old_crop = (try explicitCrop(a, bf.doc, spec.id)) orelse return .inside;
    return if (outsideFraction(old_ext, old_crop) > stale_fraction) .cut else .inside;
}

pub fn run(a: Allocator, scene: *Scene, doc: json.Value, before: ?Before, diags: *model.Diags) Allocator.Error!void {
    const vs = (doc.get("views") orelse return).arr() orelse return;
    const reported = try a.alloc(bool, scene.comps.len);
    @memset(reported, false);
    for (vs, 0..) |vnode, vi| {
        var scratch = model.Diags.init(a);
        const spec = (try view_mod.parse(a, vnode, vi, &scratch)) orelse continue;
        if (spec.kind != .section or !spec.has_crop) continue;
        for (scene.comps) |*c| {
            if (c.state != .ok) continue;
            var omitted = false;
            for (spec.omit) |o| if (std.mem.eql(u8, o, c.id)) {
                omitted = true;
            };
            if (omitted) continue;
            const ext = visibleExtent(c, spec.cut_z);
            if (ext.isEmpty()) continue;
            const frac = outsideFraction(ext, spec.crop);
            if (frac <= stale_fraction) continue;
            switch (try prior(a, before, c, &spec)) {
                .cut => continue, // the crop already cut it before the edit: deliberate
                .inside => {}, // the edit caused it
                // reported once per component, and only when no view shows it (a new one: no view shows most of it)
                .added, .none => |p| if (reported[c.index] or try shownSomewhere(a, c, vs, if (p == .added) stale_fraction else 1 - 1e-6)) continue,
            }
            reported[c.index] = true;
            var need = spec.crop;
            need.addBox(ext);
            const fit = try a.print("{{\"x\":[{s},{s}],\"y\":[{s},{s}]}}", .{ try json.fmtNumberAlloc(a, @floor(need.x0)), try json.fmtNumberAlloc(a, @ceil(need.x1)), try json.fmtNumberAlloc(a, @floor(need.y0)), try json.fmtNumberAlloc(a, @ceil(need.y1)) });
            diags.addFix(.warning, "W_CROP_STALE", c.id, try a.print("views/{s}/crop", .{spec.id}), "{s}'{s}' has {d:.0}% of its extent outside the crop of view {s}: it spans x {s}..{s}, y {s}..{s}, the crop is x {s}..{s}, y {s}..{s}", .{
                if (before != null) "after this edit, " else "",
                c.id,
                frac * 100,
                spec.id,
                fmtIn(a, ext.x0),
                fmtIn(a, ext.x1),
                fmtIn(a, ext.y0),
                fmtIn(a, ext.y1),
                fmtIn(a, spec.crop.x0),
                fmtIn(a, spec.crop.x1),
                fmtIn(a, spec.crop.y0),
                fmtIn(a, spec.crop.y1),
            }, try a.print("remove \"crop\" (and \"scale\", if you set one) from view {s}: the engine then re-fits the view to all non-fill components + 6\"; or set \"crop\" to {s} to contain '{s}'", .{ spec.id, fit, c.id }));
        }
    }
}
