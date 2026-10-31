//! Beyond prisms: outlines hidden by nearer bodies, visible regions, top edges of `top` outlines.

use super::cut::group_region;
use super::hardware::is_path_bar;
use super::occlusion::{Occ, PieceOut, hidden_pieces, visible_open, visible_pieces};
use super::{Class, Section, Target};
use crate::clip::{self, Op};
use crate::drawing::HatchItem;
use crate::geom::{self, Pt, V2};
use crate::hatch;
use crate::model::{Kind, OutlineMode, Prism};
use crate::pen::Pen;
use crate::style::{CutMark, Role};
use crate::view::Kind as ViewKind;
use alloc::string::ToString;
use alloc::vec::Vec;

impl Section<'_> {
    pub fn draw_beyond(&mut self, i: usize, p: &Prism) {
        if p.kind != Kind::Body {
            return;
        }
        if p.face_tie {
            return self.draw_face_tie(i, p);
        }
        let crop = self.spec.crop;
        let f = self.flat_of(i);
        if !clip::loops_box(&f).overlaps(crop, 1e-6) {
            return;
        }
        let src = self.src_name(p);
        if is_path_bar(p) {
            let pen = self.pen_for(Pen::Rebar);
            return self.add_clipped(Target::Strokes, &p.centerline, false, pen, &src, true);
        }
        let mat = self.style.material(&p.material);
        let elevation = self.spec.kind == ViewKind::Elevation;
        let pen = if mat.is_some_and(|m| m.fill) {
            self.pen_for_role(p.role)
        } else if elevation {
            self.pen_for(Pen::Profile)
        } else {
            Pen::Beyond
        };
        let top_only = elevation && p.outline == OutlineMode::Top;
        if elevation && p.role == Role::Soil {
            self.soil_hatch(i, p, &src);
        }
        if elevation && p.outline == OutlineMode::None {
            return;
        }
        let paths: Vec<(Vec<Pt>, bool)> = if top_only {
            p.loops.iter().flat_map(|l| top_runs(l)).map(|r| (r, false)).collect()
        } else {
            p.loops.iter().map(|l| (l.clone(), true)).collect()
        };
        let mut pieces = Vec::new();
        let mut hidden = Vec::new();
        {
            let occ: Vec<&Occ> = if p.embedded {
                Vec::new()
            } else {
                self.occs.iter().filter(|o| o.prism != i && (o.cut || o.z1 > p.z1 + 1e-9)).collect()
            };
            for (pts, closed) in &paths {
                if *closed {
                    pieces.extend(visible_pieces(pts, &occ));
                } else {
                    pieces.extend(visible_open(pts, false, &occ));
                }
                if self.spec.hidden_lines && !occ.is_empty() {
                    hidden.extend(hidden_pieces(pts, *closed, &occ));
                }
            }
        }
        if p.lengthwise && !elevation {
            pieces = self.off_nearer_slabs(i, p, pieces);
        }
        for pc in pieces {
            self.add_clipped(Target::Strokes, &pc.pts, pc.closed, pen, &src, p.embedded);
        }
        let hidden_pen = self.pen_for(Pen::Hidden);
        for pc in hidden {
            self.add_clipped(Target::Ghosts, &pc.pts, pc.closed, hidden_pen, &src, false);
        }
        if elevation {
            self.end_grain_marks(i, p, &src);
        }
    }

    /// Elevation: members seen end-on (lumber with a cut mark, e.g. joists running toward the viewer) keep their X or
    /// diagonal, cut away where nearer members hide it.
    fn end_grain_marks(&mut self, i: usize, p: &Prism, src: &str) {
        let st = self.style;
        let Some(m) = st.material(&p.material) else { return };
        let mark = if p.blocking && m.cut_mark != CutMark::None { CutMark::Diagonal } else { m.cut_mark };
        if mark == CutMark::None || p.quads.is_empty() {
            return;
        }
        let mut segs: Vec<Vec<Pt>> = Vec::new();
        for q in &p.quads {
            segs.push(alloc::vec![Pt::at(q[0], 0.0), Pt::at(q[2], 0.0)]);
            if mark == CutMark::X {
                segs.push(alloc::vec![Pt::at(q[1], 0.0), Pt::at(q[3], 0.0)]);
            }
        }
        let occ: Vec<&Occ> = self.occs.iter().filter(|o| o.prism != i && o.z1 > p.z1 + 1e-9).collect();
        let mut out = Vec::new();
        for s in &segs {
            out.extend(visible_open(s, false, &occ));
        }
        for pc in out {
            self.add_clipped(Target::Strokes, &pc.pts, false, Pen::Beyond, src, false);
        }
    }

    /// Elevation: earth and other loose fill keep their hatch on the part no nearer member hides.
    fn soil_hatch(&mut self, i: usize, p: &Prism, src: &str) {
        let st = self.style;
        let Some(m) = st.material(&p.material) else { return };
        if m.hatch.is_empty() {
            return;
        }
        let region = self.visible_region(i);
        if region.is_empty() {
            return;
        }
        for g in group_region(&region, None) {
            for hs in &m.hatch {
                let Some(pat) = st.pattern(&hs.pattern) else { continue };
                let res = hatch::generate(&g.flat, pat, self.spec.scale * hs.scale, hs.angle);
                if res.truncated {
                    self.truncated_hatch = true;
                }
                self.hatches.push(HatchItem {
                    layer: self.layer_for(Pen::Hatch),
                    pen: Pen::Hatch,
                    src: src.to_string(),
                    pattern: hs.pattern.clone(),
                    scale: hs.scale,
                    angle: hs.angle,
                    loops: g.loops.clone(),
                    lines: res.lines,
                });
            }
        }
    }

    /// A member seen lengthwise is a stack of slabs: an edge of a slab that lies on the outline of a nearer slab of the
    /// same member is that slab's edge, drawn once in the member's pen (not as a shared edge in the thin pen).
    fn off_nearer_slabs(&mut self, i: usize, p: &Prism, pieces: Vec<PieceOut>) -> Vec<PieceOut> {
        let mut edges: Vec<[V2; 2]> = Vec::new();
        let prisms = alloc::rc::Rc::clone(&self.prisms);
        for (k, q) in prisms.iter().enumerate() {
            if k == i || !q.lengthwise || q.comp != p.comp || q.instance != p.instance || q.z1 <= p.z1 + 1e-9 {
                continue;
            }
            for lp in self.flat_of(k).iter() {
                for (j, a) in lp.iter().enumerate() {
                    edges.push([*a, lp[(j + 1) % lp.len()]]);
                }
            }
        }
        if edges.is_empty() {
            return pieces;
        }
        let mut out = Vec::new();
        for pc in pieces {
            let n = pc.pts.len();
            let nseg = if pc.closed { n } else { n.saturating_sub(1) };
            for k in 0..nseg {
                let (a, b) = (pc.pts[k], pc.pts[(k + 1) % n]);
                if a.b != 0.0 {
                    out.push(PieceOut { pts: alloc::vec![a, Pt::at(b.v(), 0.0)], closed: false });
                    continue;
                }
                for iv in uncovered(a.v(), b.v(), &edges) {
                    let ends = [a.v().lerp(b.v(), iv[0]), a.v().lerp(b.v(), iv[1])];
                    out.push(PieceOut { pts: ends.iter().map(|&v| Pt::at(v, 0.0)).collect(), closed: false });
                }
            }
        }
        out
    }

    pub fn visible_region(&mut self, i: usize) -> Vec<Vec<V2>> {
        let (cls, kind, dashed, embedded, z1) = {
            let p = &self.prisms[i];
            (self.cls[i], p.kind, p.dashed, p.embedded, p.z1)
        };
        if cls == Class::Drop || (kind == Kind::Ghost && !dashed) {
            return Vec::new();
        }
        let f = self.flat_of(i);
        let mut region = clip::boolean(&f, core::slice::from_ref(&self.crop_loop), Op::Intersect);
        if cls == Class::Beyond && !embedded && !dashed {
            for o in &self.occs {
                if o.prism == i || !(o.cut || o.z1 > z1 + 1e-9) {
                    continue;
                }
                if !o.bx.overlaps(clip::loops_box(&region), 1e-9) {
                    continue;
                }
                region = clip::boolean(&region, &o.loops, Op::Diff);
            }
        }
        region
    }

    pub fn top_edges(&mut self, lp: &[Pt], pen: Pen, src: &str) {
        for run in top_runs(lp) {
            self.add_clipped(Target::Strokes, &run, false, pen, src, false);
        }
    }
}

/// The runs of a loop's edges whose outward normal points up the page, as open polylines.
pub fn top_runs(lp: &[Pt]) -> Vec<Vec<Pt>> {
    let ccw = geom::signed_area(lp) >= 0.0;
    let mut out = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let n = lp.len();
    let mut start = 0;
    for k in 0..n {
        if !edge_is_top(lp, (k + n - 1) % n, ccw) {
            start = k;
            break;
        }
    }
    for idx in 0..n {
        let i = (start + idx) % n;
        let q = lp[(i + 1) % n];
        if edge_is_top(lp, i, ccw) {
            match cur.last_mut() {
                None => cur.push(lp[i]),
                Some(last) => last.b = lp[i].b,
            }
            cur.push(Pt::at(q.v(), 0.0));
        } else if !cur.is_empty() {
            out.push(core::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// An outline edge whose outward normal points up the page.
pub fn edge_is_top(lp: &[Pt], i: usize, ccw: bool) -> bool {
    let p = lp[i];
    let q = lp[(i + 1) % lp.len()];
    let d = q.v().sub(p.v());
    let l = d.len();
    if l < 1e-12 {
        return false;
    }
    let n = if ccw { V2::init(d.y / l, -d.x / l) } else { V2::init(-d.y / l, d.x / l) };
    n.y > 0.01
}

/// Parameter intervals of the straight segment a-b not lying on any of `edges` (collinear overlap within 1e-6).
fn uncovered(a: V2, b: V2, edges: &[[V2; 2]]) -> Vec<[f64; 2]> {
    let d = b.sub(a);
    let l = d.len();
    if l < 1e-12 {
        return Vec::new();
    }
    let u = d.scale(1.0 / l);
    let mut covered: Vec<[f64; 2]> = Vec::new();
    for e in edges {
        if u.cross(e[0].sub(a)).abs() > 1e-6 || u.cross(e[1].sub(a)).abs() > 1e-6 {
            continue;
        }
        let t0 = e[0].sub(a).dot(u) / l;
        let t1 = e[1].sub(a).dot(u) / l;
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
        if hi - lo > 1e-9 {
            covered.push([lo, hi]);
        }
    }
    crate::sort::stable(&mut covered, |x, y| x[0] < y[0]);
    let mut keep = Vec::new();
    let mut t = 0.0;
    for c in covered {
        if c[0] > t + 1e-9 {
            keep.push([t, c[0]]);
        }
        t = t.max(c[1]);
    }
    if t < 1.0 - 1e-9 {
        keep.push([t, 1.0]);
    }
    keep
}
