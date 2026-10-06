//! Layout repair (SPEC 20): re-rendering the dimension labels and the repair moves for a dimension or label that sits on a note leader.

const std = @import("std");
const drawing = @import("../drawing.zig");
const geom = @import("../geom.zig");
const route = @import("../route.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Box = geom.Box;
const Item = drawing.Item;
const DimSpec = @import("dims.zig").DimSpec;
const labelItems = @import("dims.zig").labelItems;
const baseHits = @import("dims.zig").baseHits;
const stackDims = @import("dims.zig").stackDims;
const Obstacle = @import("notes.zig").Obstacle;
const Meta = @import("notes.zig").Meta;
const LabelSpec = @import("notes.zig").LabelSpec;
const clearOfLeaders = @import("notes.zig").clearOfLeaders;
const annot = @import("../annot.zig");
const Env = annot.Env;
const textPoly = annot.textPoly;
const itemsBox = annot.itemsBox;
const polysOverlap = annot.polysOverlap;

/// One rendering of the dimensions and labels (stacked, with the repair pushes applied) plus what the router needs.
pub const Render = struct {
    per: []std.ArrayList(Item),
    eff: []f64,
    obstacles: []Obstacle,
    soft: []const [2]V2,
    knock: []const [4]V2,
    ext: Box,
    meta: []Meta,
};

pub const DimLab = struct {
    dspecs: []const DimSpec,
    lspecs: []const LabelSpec,
    base_segs: []const [2]V2,
};

pub fn renderDimLabels(env: *Env, dl: DimLab, per_in: []const std.ArrayList(Item), meta_in: []const Meta, dpush: []const f64, lpush: []const V2) Allocator.Error!Render {
    const a = env.a;
    const per = try a.dupe(std.ArrayList(Item), per_in);
    const meta = try a.dupe(Meta, meta_in);
    const eff = try a.alloc(f64, dl.dspecs.len);
    const ditems = try a.alloc(std.ArrayList(Item), dl.dspecs.len);
    var label_polys: std.ArrayList([4]V2) = .empty;
    for (dl.lspecs, 0..) |l, i| {
        var its: std.ArrayList(Item) = .empty;
        if (l.base) |p| try labelItems(env, l.id, l.text, p.add(lpush[i]), &its);
        per[l.k] = its;
        meta[l.k].off = l.off.x + lpush[i].x;
        meta[l.k].off2 = l.off.y + lpush[i].y;
        for (its.items) |it| if (it == .text) try label_polys.append(a, textPoly(env.font, it.text, 0.02 * env.S));
    }
    try stackDims(env, dl.dspecs, dpush, dl.base_segs, label_polys.items, ditems, eff);
    for (dl.dspecs, 0..) |d, i| {
        per[d.k] = ditems[i];
        meta[d.k].off = eff[i];
    }
    var ext = env.crop;
    var obstacles: std.ArrayList(Obstacle) = .empty;
    var soft: std.ArrayList([2]V2) = .empty;
    var knock: std.ArrayList([4]V2) = .empty;
    for (per, 0..) |its, k| {
        ext.addBox(itemsBox(env.font, its.items));
        const m = meta[k];
        for (its.items) |it| {
            if (it == .text) {
                try knock.append(a, textPoly(env.font, it.text, 0.02 * env.S));
                try obstacles.append(a, .{
                    .poly = textPoly(env.font, it.text, 0),
                    .id = it.text.src,
                    .kind = if (m.kind == .label) .label else .dim,
                    .text = it.text.s,
                    .owner = m.owner,
                    .axis = m.axis,
                    .off = m.off,
                    .off2 = m.off2,
                });
            } else if (it == .path and m.kind == .dim and it.path.pen == .dim and it.path.pts.len == 2) {
                try soft.append(a, .{ it.path.pts[0].v(), it.path.pts[1].v() });
            }
        }
    }
    return .{ .per = per, .eff = eff, .obstacles = obstacles.items, .soft = soft.items, .knock = knock.items, .ext = ext, .meta = meta };
}

/// Push a dimension out by whole dimension spacings (0.25 paper inch) until its text clears every leader.
pub fn repairDim(env: *Env, o: Obstacle, leaders: []const [3]V2, h: f64, dpush: []f64, dspecs: []const DimSpec) bool {
    const step = 0.25 * env.S;
    const cap = 8.0 * step;
    if (dpush[o.owner] + step > cap + 1e-9) return false;
    const sgn = dspecs[o.owner].sgn();
    var chosen: f64 = 1;
    var k: f64 = 1;
    while (k <= 6) : (k += 1) {
        if (dpush[o.owner] + k * step > cap + 1e-9) break;
        if (clearOfLeaders(leaders, o.poly, o.axis.scale(sgn * k * step), h)) {
            chosen = k;
            break;
        }
    }
    dpush[o.owner] += chosen * step;
    return true;
}

/// Move a label (smallest move first, up/down before sideways) to where no leader comes within a text height
/// and it overprints no other label or dimension text; prefers a spot that does not sit on drawn outlines.
pub fn repairLabel(o: Obstacle, obsts: []const Obstacle, leaders: []const [3]V2, h: f64, lpush: []V2, base_segs: []const [2]V2) bool {
    const dirs = [8]V2{ V2.init(0, 1), V2.init(0, -1), V2.init(1, 0), V2.init(-1, 0), V2.init(1, 1), V2.init(-1, 1), V2.init(1, -1), V2.init(-1, -1) };
    const cap = 12.0 * h;
    var relax: usize = 0;
    while (relax < 2) : (relax += 1) {
        var k: f64 = 1;
        while (k * 0.5 * h <= cap) : (k += 1) {
            const delta = @ceil(k * 0.5 * h * 4.0) / 4.0;
            for (dirs) |dv| {
                const dn = dv.norm().scale(delta);
                if (@abs(lpush[o.owner].x + dn.x) > cap or @abs(lpush[o.owner].y + dn.y) > cap) continue;
                if (!clearOfLeaders(leaders, o.poly, dn, h)) continue;
                var q = o.poly;
                for (&q) |*pt| pt.* = pt.add(dn);
                var ok = true;
                for (obsts) |ob| {
                    if (ob.kind == .label and ob.owner == o.owner) continue;
                    if (polysOverlap(&q, &ob.poly)) {
                        ok = false;
                        break;
                    }
                }
                if (!ok) continue;
                if (relax == 0 and baseHits(base_segs, &q) > 0) continue;
                lpush[o.owner] = lpush[o.owner].add(dn);
                return true;
            }
        }
    }
    return false;
}

pub const Best = struct { r: route.Layout, rd: Render };
