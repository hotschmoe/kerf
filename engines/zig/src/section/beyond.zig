//! Beyond prisms (SPEC 8.1): outlines of bodies behind the cut plane clipped against what is in front, the visible region of a prism, and grade-line top edges.

const std = @import("std");
const model = @import("../model.zig");
const geom = @import("../geom.zig");
const clip = @import("../clip.zig");
const Pen = @import("../pen.zig").Pen;
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Prism = model.Prism;
const isPathBar = @import("hardware.zig").isPathBar;
const Stroke = @import("strokes.zig").Stroke;
const Occ = @import("occlusion.zig").Occ;
const visiblePieces = @import("occlusion.zig").visiblePieces;
const section = @import("../section.zig");
const Section = section.Section;

pub fn drawBeyond(self: *Section, i: usize, p: Prism) Allocator.Error!void {
    if (p.kind != .body) return;
    if (p.face_tie) return self.drawFaceTie(i, p);
    const crop = self.spec.crop;
    const f = try self.flatOf(i);
    if (!clip.loopsBox(f).overlaps(crop, 1e-6)) return;
    const src = self.srcName(p);
    if (isPathBar(p)) return self.addClipped(&self.strokes, p.centerline, false, self.penFor(.rebar), src, true);
    const mat = self.style.material(p.material);
    const pen: Pen = if (mat != null and mat.?.fill) self.penForRole(p.role) else .beyond;
    // occluders
    var occ: std.ArrayList(*const Occ) = .empty;
    if (!p.embedded) {
        for (self.occs.items) |*o| {
            if (o.prism == i) continue;
            if (o.cut or o.z1 > p.z1 + 1e-9) try occ.append(self.a, o);
        }
    }
    for (p.loops) |loop| {
        const pieces = try visiblePieces(self.a, loop, occ.items);
        for (pieces) |pc| try self.addClipped(&self.strokes, pc.pts, pc.closed, pen, src, p.embedded);
    }
}

/// Visible (non-occluded, in-crop) region polygons of prism `i` (for note landing points).
pub fn visibleRegion(self: *Section, i: usize) Allocator.Error![]const []const V2 {
    const p = self.prisms[i];
    if (self.cls[i] == .drop or (p.kind == .ghost and !p.dashed)) return &.{};
    var region: []const []const V2 = try self.flatOf(i);
    region = try clip.boolean(self.a, region, &.{self.crop_loop}, .intersect);
    if (self.cls[i] == .beyond and !p.embedded and !p.dashed) {
        for (self.occs.items) |o| {
            if (o.prism == i) continue;
            if (!(o.cut or o.z1 > p.z1 + 1e-9)) continue;
            if (!o.box.overlaps(clip.loopsBox(region), 1e-9)) continue;
            region = try clip.boolean(self.a, region, o.loops, .diff);
        }
    }
    return region;
}

/// Stroke only the edges whose outward normal points up (grade lines). Loops are CCW.
pub fn topEdges(self: *Section, loop: []const Pt, pen: Pen, src: []const u8) Allocator.Error!void {
    const ccw = geom.signedArea(loop) >= 0;
    var cur: std.ArrayList(Pt) = .empty;
    const n = loop.len;
    // find a start vertex where the previous edge does not qualify so runs are not split
    var start: usize = 0;
    var k: usize = 0;
    while (k < n) : (k += 1) {
        if (!edgeIsTop(loop, (k + n - 1) % n, ccw)) {
            start = k;
            break;
        }
    }
    var idx: usize = 0;
    while (idx < n) : (idx += 1) {
        const i = (start + idx) % n;
        const q = loop[(i + 1) % n];
        if (edgeIsTop(loop, i, ccw)) {
            if (cur.items.len == 0) {
                try cur.append(self.a, loop[i]);
            } else cur.items[cur.items.len - 1].b = loop[i].b;
            try cur.append(self.a, Pt.at(q.v(), 0));
        } else if (cur.items.len > 0) {
            try self.addClipped(&self.strokes, cur.items, false, pen, src, false);
            cur = .empty;
        }
    }
    if (cur.items.len > 0) try self.addClipped(&self.strokes, cur.items, false, pen, src, false);
}

/// Whether the outward normal of edge `i` points up. Arcs use their chord: good enough for grade lines.
pub fn edgeIsTop(loop: []const Pt, i: usize, ccw: bool) bool {
    const p = loop[i];
    const q = loop[(i + 1) % loop.len];
    const d = q.v().sub(p.v());
    const l = d.len();
    if (l < 1e-12) return false;
    const n = if (ccw) V2.init(d.y / l, -d.x / l) else V2.init(-d.y / l, d.x / l);
    return n.y > 0.01;
}
