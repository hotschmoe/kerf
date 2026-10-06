//! Dimensions (SPEC 6.5): the dimension line, extension lines, terminators and text of one dimension, and the stacking of dimensions that would collide.

const std = @import("std");
const geom = @import("../geom.zig");
const model = @import("../model.zig");
const drawing = @import("../drawing.zig");
const units = @import("../units.zig");
const route = @import("../route.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Box = geom.Box;
const Item = drawing.Item;
const textItem = @import("text.zig").textItem;
const pathItem = @import("text.zig").pathItem;
const upperIf = @import("text.zig").upperIf;
const annot = @import("../annot.zig");
const Env = annot.Env;
const asciiFold = annot.asciiFold;
const textPoly = annot.textPoly;
const DimDir = annot.DimDir;
const dim_variants = annot.dim_variants;

/// A parsed dimension. Its drawn offset is `off0` (authored) pushed outward by `push` (layout repair) and by
/// the automatic stacking (SPEC 20).
pub const DimSpec = struct {
    k: usize,
    id: []const u8,
    from: V2,
    to: V2,
    dir: DimDir,
    off0: f64,
    text: ?[]const u8,

    pub fn sgn(self: DimSpec) f64 {
        return if (self.off0 >= 0) 1 else -1;
    }

    /// Length measured by the dimension (model units).
    pub fn span(self: DimSpec) f64 {
        return switch (self.dir) {
            .v => @abs(self.to.y - self.from.y),
            .aligned => self.from.dist(self.to),
            .h => @abs(self.to.x - self.from.x),
        };
    }

    /// Unit vector along which the line moves when |offset| grows (before the sign of the offset).
    pub fn axis(self: DimSpec) V2 {
        return switch (self.dir) {
            .v => V2.init(1, 0),
            .aligned => self.to.sub(self.from).norm().perp(),
            .h => V2.init(0, 1),
        };
    }
};

/// The drawn pieces of one dimension, for conflict tests (SPEC 20 stacking and outside text).
pub const DimShape = struct {
    segs: [6][2]V2 = undefined,
    n: usize = 0,
    /// The dimension line proper (between the extension lines).
    line: [2]V2 = undefined,
    /// The text box (padded).
    text: [4]V2 = undefined,
    fits: bool = true,

    pub fn add(self: *DimShape, a: V2, b: V2) void {
        self.segs[self.n] = .{ a, b };
        self.n += 1;
    }
};

/// Build one dimension. `offset` is the signed dimension-line offset (SPEC 16). When the text does not fit
/// between the extension lines it goes outside (SPEC 20), never smaller and never dropped, joined to the
/// dimension line by a short leader. `variant` picks where: 0/1 on the axis beyond the end of the line, 2-5
/// raised out of the axis (outward / inward of the object) at the far / near end.
pub fn dimBuild(env: *Env, d: DimSpec, offset: f64, variant: usize, out: *std.ArrayList(Item)) Allocator.Error!DimShape {
    const a = env.a;
    const st = env.style;
    const S = env.S;
    const gap = st.ext_gap_in * S;
    const over = st.ext_over_in * S;
    const tick = st.tick_len_in * S;
    const th = st.text_height_in * S;
    const tgap = st.dim_text_gap_in * S;
    const from = d.from;
    const to = d.to;
    var pa: V2 = undefined;
    var pb: V2 = undefined;
    var la: V2 = undefined; // dimension line endpoints (ordered along u)
    var lb: V2 = undefined;
    var u: V2 = undefined;
    var nrm: V2 = undefined;
    switch (d.dir) {
        .v => {
            const x = if (offset >= 0) @max(from.x, to.x) + offset else @min(from.x, to.x) + offset;
            pa = V2.init(x, from.y);
            pb = V2.init(x, to.y);
            if (from.y <= to.y) {
                la = pa;
                lb = pb;
            } else {
                la = pb;
                lb = pa;
            }
            u = V2.init(0, 1);
            nrm = V2.init(-1, 0);
        },
        .aligned => {
            const dd = to.sub(from).norm();
            const n = dd.perp();
            pa = from.add(n.scale(offset));
            pb = to.add(n.scale(offset));
            la = pa;
            lb = pb;
            u = dd;
            nrm = n;
        },
        .h => {
            const y = if (offset >= 0) @max(from.y, to.y) + offset else @min(from.y, to.y) + offset;
            pa = V2.init(from.x, y);
            pb = V2.init(to.x, y);
            if (from.x <= to.x) {
                la = pa;
                lb = pb;
            } else {
                la = pb;
                lb = pa;
            }
            u = V2.init(1, 0);
            nrm = V2.init(0, 1);
        },
    }
    var sh = DimShape{};
    const pairs = [2][2]V2{ .{ from, pa }, .{ to, pb } };
    for (pairs) |pq| {
        const dv = pq[1].sub(pq[0]);
        if (dv.len() < 1e-9) continue;
        const dn = dv.norm();
        const e0 = pq[0].add(dn.scale(gap));
        const e1 = pq[1].add(dn.scale(over));
        try out.append(a, try pathItem(env, .dim, d.id, &.{ e0, e1 }, false));
        sh.add(e0, e1);
    }
    const dist = d.span();
    const label: []const u8 = d.text orelse try units.fmtFtIn(a, dist);
    const label_f = try asciiFold(a, label);
    const tw = env.font.width(label_f, th);
    const fits = tw + 2.0 * tgap <= lb.sub(la).len() - tick;
    sh.fits = fits;
    sh.line = .{ la, lb };
    try out.append(a, try pathItem(env, .dim, d.id, &.{ la, lb }, false));
    sh.add(la, lb);
    const tdir = u.add(nrm).norm();
    for ([2]V2{ la, lb }) |p| {
        const t0 = p.sub(tdir.scale(tick * 0.5));
        const t1 = p.add(tdir.scale(tick * 0.5));
        try out.append(a, try pathItem(env, .profile, d.id, &.{ t0, t1 }, false));
        sh.add(t0, t1);
    }
    var ang = std.math.radiansToDegrees(std.math.atan2(u.y, u.x));
    if (ang > 90.0 + 1e-9 or ang <= -90.0 + 1e-9) ang += 180.0;
    if (d.dir == .v) ang = 90.0;
    const tn = V2.init(-@sin(std.math.degreesToRadians(ang)), @cos(std.math.degreesToRadians(ang)));
    var center: V2 = undefined;
    var valign: drawing.VAlign = .baseline;
    if (fits) {
        center = V2.mid(la, lb).add(tn.scale(tgap));
    } else {
        // outside: a short leader from the end of the dimension line to the text
        valign = .middle;
        const at_far = variant == 0 or variant == 2 or variant == 4;
        const e = if (at_far) lb else la;
        const s: f64 = if (at_far) 1 else -1;
        const outward = nrm.scale(d.sgn());
        const lift: f64 = switch (variant) {
            2, 3 => th + tgap,
            4, 5 => -(th + tgap),
            else => 0,
        };
        const q = e.add(u.scale(s * 2.0 * tick)).add(outward.scale(lift));
        try out.append(a, try pathItem(env, .dim, d.id, &.{ e, q }, false));
        sh.add(e, q);
        center = q.add(u.scale(s * (tgap + tw * 0.5)));
    }
    const ti = try textItem(env, .dims, .dim, d.id, label, center.x, center.y, th, ang, .center, valign);
    try out.append(a, ti);
    sh.text = textPoly(env.font, ti.text, 0.015 * S);
    return sh;
}

pub fn labelItems(env: *Env, id: []const u8, text: []const u8, at: V2, out: *std.ArrayList(Item)) Allocator.Error!void {
    const t = try upperIf(env, text);
    try out.append(env.a, try textItem(env, .notes, .anno, id, t, at.x, at.y, env.style.label_height_in * env.S, 0, .center, .middle));
}

pub fn polyHitsSeg(poly: *const [4]V2, a: V2, b: V2) bool {
    if (geom.pointInLoopEO(a, poly) or geom.pointInLoopEO(b, poly)) return true;
    for (0..4) |i| if (route.segSegDist(a, b, poly[i], poly[(i + 1) % 4]) <= 1e-9) return true;
    return false;
}

pub fn polysOverlap(p: *const [4]V2, q: *const [4]V2) bool {
    for (0..4) |i| if (polyHitsSeg(q, p[i], p[(i + 1) % 4])) return true;
    for (0..4) |i| if (geom.pointInLoopEO(q[i], p)) return true;
    return false;
}

/// Parallel dimension lines that run on top of each other (within `tol`, with a shared stretch).
pub fn linesCollide(a: [2]V2, b: [2]V2, tol: f64) bool {
    const da = a[1].sub(a[0]);
    const db = b[1].sub(b[0]);
    const la = da.len();
    const lb = db.len();
    if (la < 1e-9 or lb < 1e-9) return false;
    const ua = da.scale(1.0 / la);
    if (@abs(ua.cross(db.scale(1.0 / lb))) > 0.02) return false;
    const t0 = b[0].sub(a[0]).dot(ua);
    const t1 = b[1].sub(a[0]).dot(ua);
    const ov = @min(la, @max(t0, t1)) - @max(0.0, @min(t0, t1));
    if (ov <= 1e-6) return false;
    const dperp = @abs(ua.cross(b[0].sub(a[0])));
    return dperp < tol;
}

pub fn dimConflicts(x: *const DimShape, y: *const DimShape, tol: f64) usize {
    var c: usize = 0;
    if (linesCollide(x.line, y.line, tol)) c += 1;
    if (polysOverlap(&x.text, &y.text)) c += 1;
    for (y.segs[0..y.n]) |sg| if (polyHitsSeg(&x.text, sg[0], sg[1])) {
        c += 1;
        break;
    };
    for (x.segs[0..x.n]) |sg| if (polyHitsSeg(&y.text, sg[0], sg[1])) {
        c += 1;
        break;
    };
    return c;
}

pub fn baseHits(segs: []const [2]V2, poly: *const [4]V2) usize {
    var bb = Box{};
    for (poly) |p| bb.addPoint(p.x, p.y);
    var n: usize = 0;
    for (segs) |sg| {
        if (@max(sg[0].x, sg[1].x) < bb.x0 or @min(sg[0].x, sg[1].x) > bb.x1 or @max(sg[0].y, sg[1].y) < bb.y0 or @min(sg[0].y, sg[1].y) > bb.y1) continue;
        if (polyHitsSeg(poly, sg[0], sg[1])) n += 1;
    }
    return n;
}

pub const DimPlaced = struct { offset: f64, shape: DimShape, items: std.ArrayList(Item) };

/// SPEC 20 dimension stacking: dims are placed shortest first; a dimension whose line would overlap another
/// one, or whose text would overprint another dimension's text or lines, moves out in steps of 0.25 paper
/// inch (outside text first tries the other places before the dimension line moves). `pushes` is the extra
/// outward distance chosen by the layout repair. Returns the effective offsets and fills `items`.
pub fn stackDims(env: *Env, specs: []const DimSpec, pushes: []const f64, base_segs: []const [2]V2, label_polys: []const [4]V2, items: []std.ArrayList(Item), eff: []f64) Allocator.Error!void {
    const a = env.a;
    const S = env.S;
    const step = 0.25 * S;
    const order = try a.alloc(usize, specs.len);
    for (order, 0..) |*o, i| o.* = i;
    std.mem.sort(usize, order, specs, struct {
        fn lt(sp: []const DimSpec, x: usize, y: usize) bool {
            const sx = sp[x].span();
            const sy = sp[y].span();
            if (@abs(sx - sy) > 1e-9) return sx < sy;
            return x < y;
        }
    }.lt);
    var placed: std.ArrayList(DimShape) = .empty;
    for (order) |i| {
        const d = specs[i];
        const start = d.off0 + d.sgn() * pushes[i];
        var best: ?DimPlaced = null;
        var best_c: usize = std.math.maxInt(usize);
        var k: usize = 0;
        search: while (k <= 12) : (k += 1) {
            const off = start + d.sgn() * @as(f64, @floatFromInt(k)) * step;
            var kbest: ?DimPlaced = null;
            var kbest_hits: usize = std.math.maxInt(usize);
            var kbest_c: usize = std.math.maxInt(usize);
            var v: usize = 0;
            while (v < dim_variants) : (v += 1) {
                var its: std.ArrayList(Item) = .empty;
                const sh = try dimBuild(env, d, off, v, &its);
                var c: usize = 0;
                for (placed.items) |*o| c += dimConflicts(&sh, o, 0.5 * step);
                for (label_polys) |*lp| {
                    if (polysOverlap(&sh.text, lp)) c += 1;
                    for (sh.segs[0..sh.n]) |sg| if (polyHitsSeg(lp, sg[0], sg[1])) {
                        c += 1;
                        break;
                    };
                }
                const bh: usize = if (sh.fits) 0 else baseHits(base_segs, &sh.text);
                if (c < kbest_c or (c == kbest_c and bh < kbest_hits)) {
                    kbest_c = c;
                    kbest_hits = bh;
                    kbest = .{ .offset = off, .shape = sh, .items = its };
                }
                if (sh.fits) break;
            }
            if (kbest_c < best_c) {
                best_c = kbest_c;
                best = kbest;
            }
            if (kbest_c == 0) break :search;
        }
        const pick = best.?;
        eff[i] = pick.offset;
        items[i] = pick.items;
        try placed.append(a, pick.shape);
    }
}
