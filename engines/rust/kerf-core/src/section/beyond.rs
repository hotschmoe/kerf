//! Beyond prisms: outlines hidden by nearer bodies, visible regions, top edges of `top` outlines.

use super::hardware::is_path_bar;
use super::occlusion::{Occ, visible_pieces};
use super::{Class, Section, Target};
use crate::clip::{self, Op};
use crate::geom::{self, Pt, V2};
use crate::model::{Kind, Prism};
use crate::pen::Pen;
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
        let pen = if mat.is_some_and(|m| m.fill) { self.pen_for_role(p.role) } else { Pen::Beyond };
        let mut pieces = Vec::new();
        {
            let occ: Vec<&Occ> = if p.embedded {
                Vec::new()
            } else {
                self.occs.iter().filter(|o| o.prism != i && (o.cut || o.z1 > p.z1 + 1e-9)).collect()
            };
            for lp in &p.loops {
                pieces.extend(visible_pieces(lp, &occ));
            }
        }
        for pc in pieces {
            self.add_clipped(Target::Strokes, &pc.pts, pc.closed, pen, &src, p.embedded);
        }
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
        let ccw = geom::signed_area(lp) >= 0.0;
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
                self.add_clipped(Target::Strokes, &cur, false, pen, src, false);
                cur.clear();
            }
        }
        if !cur.is_empty() {
            self.add_clipped(Target::Strokes, &cur, false, pen, src, false);
        }
    }
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
