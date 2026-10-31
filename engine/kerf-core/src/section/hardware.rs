//! Thin hardware: minimum metal thickness on paper, path bars and face-on ties.

use super::occlusion::{Occ, visible_open};
use super::{Section, Target};
use crate::clip;
use crate::drawing::FillItem;
use crate::geom::{self, FLAT_TOL, V2};
use crate::model::{self, Kind, Prism};
use crate::num;
use crate::pathgeom;
use crate::pen::Pen;
use crate::style::Role;
use alloc::vec::Vec;

pub const MIN_METAL_PAPER_IN: f64 = 0.022;

/// Metal sections thinner than the minimum paper thickness get a ribbon of that thickness around their centerline.
pub fn thicken_thin_hardware(prisms: &[Prism], scale: f64) -> Vec<Prism> {
    let mut out = prisms.to_vec();
    let min_t = MIN_METAL_PAPER_IN * scale;
    for (i, p) in prisms.iter().enumerate() {
        if p.kind != Kind::Body || p.face_tie || p.sweep_r > 0.0 || p.centerline.len() < 2 || p.loops.len() != 1 {
            continue;
        }
        if !p.role.is_metal() || (p.embedded && p.role != Role::Steel) {
            continue;
        }
        let flat = geom::flatten_polyline(&p.loops[0], true, FLAT_TOL);
        let cl = geom::flatten_polyline(&p.centerline, false, FLAT_TOL);
        if cl.len() < 2 {
            continue;
        }
        let mut len = 0.0;
        for (k, q) in cl[..cl.len() - 1].iter().enumerate() {
            len += q.dist(cl[k + 1]);
        }
        if len < 1e-9 {
            continue;
        }
        let t = geom::signed_area_v(&flat).abs() / len;
        if t >= min_t - 1e-9 {
            continue;
        }
        let d = cl[1].sub(cl[0]).norm();
        let m = cl[0].add(cl[1]).scale(0.5);
        let probe = t.min(0.5 * cl[0].dist(cl[1])) * 0.5;
        let on_left = geom::point_in_loop_eo(m.add(d.perp().scale(probe)), &flat);
        let on_right = geom::point_in_loop_eo(m.sub(d.perp().scale(probe)), &flat);
        if !on_left && !on_right {
            continue;
        }
        let both = on_left && on_right;
        let lt = if both {
            min_t / 2.0
        } else if on_left {
            min_t
        } else {
            0.0
        };
        let rt = if both {
            min_t / 2.0
        } else if on_right {
            min_t
        } else {
            0.0
        };
        let rib = pathgeom::ribbon(&p.centerline, lt, rt);
        out[i].loops = model::one_loop(rib);
    }
    out
}

/// Reinforcing bar drawn as one centerline instead of an outline.
pub fn is_path_bar(p: &Prism) -> bool {
    p.kind == Kind::Body && p.centerline.len() >= 2 && p.role == Role::Rebar
}

impl Section<'_> {
    /// Nail or screw dots of a hardware plate seen face-on, where no nearer body hides them.
    pub fn nail_dots(&mut self, nails: &[V2], src: &str) {
        let crop = self.spec.crop;
        let r = (0.012 * self.spec.scale).max(0.04);
        let pen = self.pen_for(Pen::Steel);
        for &c in nails {
            if c.x - r < crop.x0 || c.x + r > crop.x1 || c.y - r < crop.y0 || c.y + r > crop.y1 {
                continue;
            }
            self.fills.push(FillItem {
                layer: self.layer_for(pen),
                src: src.into(),
                loops: model::one_loop(model::circle_loop(c.x, c.y, r)),
            });
        }
    }

    /// Face-on ties sit in front of everything: strokes behind them are cut away.
    pub fn knock_out_face_ties(&mut self) {
        let mut occs: Vec<Occ> = Vec::new();
        for i in 0..self.prisms.len() {
            if self.cls[i] == super::Class::Drop || !self.prisms[i].face_tie {
                continue;
            }
            let f = self.flat_of(i);
            let z1 = self.prisms[i].z1;
            occs.push(Occ { bx: clip::loops_box(&f), loops: f, z1, cut: false, prism: i });
        }
        if occs.is_empty() {
            return;
        }
        let refs: Vec<&Occ> = occs.iter().collect();
        let mut out = Vec::new();
        for st in core::mem::take(&mut self.strokes) {
            if st.rank >= 10.0 {
                out.push(st);
                continue;
            }
            for pc in visible_open(&st.pts, st.closed, &refs) {
                let mut q = st.clone();
                q.pts = pc.pts;
                q.closed = pc.closed;
                out.push(q);
            }
        }
        self.strokes = out;
    }

    /// A strap seen face-on: outline in the steel pen, nail dots along its centerline.
    pub fn draw_face_tie(&mut self, i: usize, p: &Prism) {
        let crop = self.spec.crop;
        let src = self.src_name(p);
        let s = self.spec.scale;
        let pen = self.pen_for(Pen::Steel);
        for l in &p.loops {
            self.add_clipped(Target::Strokes, l, true, pen, &src, true);
        }
        if p.centerline.len() < 2 {
            return;
        }
        let flat = geom::flatten_polyline(&p.centerline, false, FLAT_TOL);
        if flat.len() < 2 {
            return;
        }
        let mut total = 0.0;
        for (k, q) in flat[..flat.len() - 1].iter().enumerate() {
            total += q.dist(flat[k + 1]);
        }
        let f = self.flat_of(i);
        let area0 = f.first().map_or(0.0, |l| geom::signed_area_v(l).abs());
        let strap_w = if total > 0.0 { area0 / total } else { 0.0 };
        let r = (0.016 * s).max(0.05).min(0.15 * strap_w + 0.02);
        let pitch = crate::math::ceil((0.07 * s).max(1.0));
        if total < 0.5 * pitch {
            return;
        }
        let n: usize = num::to_int_clamped(crate::math::floor(total / pitch + 1e-9), 1, 5000);
        let first = (total - (n - 1) as f64 * pitch) / 2.0;
        let mut seg = 0;
        let mut seg_start = 0.0;
        for k in 0..n {
            let d = first + k as f64 * pitch;
            while seg + 2 < flat.len() && d > seg_start + flat[seg].dist(flat[seg + 1]) {
                seg_start += flat[seg].dist(flat[seg + 1]);
                seg += 1;
            }
            let a0 = flat[seg];
            let b0 = flat[seg + 1];
            let sl = a0.dist(b0);
            let c = if sl < 1e-12 { a0 } else { a0.add(b0.sub(a0).scale((d - seg_start) / sl)) };
            if c.x - r < crop.x0 || c.x + r > crop.x1 || c.y - r < crop.y0 || c.y + r > crop.y1 {
                continue;
            }
            self.fills.push(FillItem {
                layer: self.layer_for(pen),
                src: src.clone(),
                loops: model::one_loop(model::circle_loop(c.x, c.y, r)),
            });
        }
    }
}
