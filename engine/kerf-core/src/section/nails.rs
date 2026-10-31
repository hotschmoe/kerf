//! Fasteners of `nailing` components in section, plan and elevation views (SPEC 27.4): a shank with a head bar where the
//! nail lies in the drawing plane, a dot where it is seen end-on.

use super::{Section, Target};
use crate::drawing::FillItem;
use crate::geom::{self, Loc, Pt, V2};
use crate::model::{self, Nail};
use crate::pen::Pen;
use crate::view::Kind as ViewKind;
use alloc::string::String;
use alloc::vec::Vec;

/// A `nailing` component that has fasteners inside the crop but draws none of them in this view.
pub struct NailsHidden {
    pub id: String,
    /// The z (y in a plan) just in front of its members, where a cut would show the nails.
    pub clear: f64,
}

const EPS: f64 = 1e-6;

impl Section<'_> {
    /// View coordinates of a world point: the drawing point and its depth toward the viewer.
    fn nail_view(&self, p: [f64; 3]) -> (V2, f64) {
        if self.spec.kind == ViewKind::Plan { (V2::init(p[0], -p[2]), p[1]) } else { (V2::init(p[0], p[1]), p[2]) }
    }

    /// A cut solid covers the point, or a beyond solid that is not one of the nailing's members lies wholly in front of
    /// depth `w`. Embedded members (bolts, bars) never hide a nail.
    fn nail_hidden(&self, p: V2, w: f64, stack: &[u32]) -> bool {
        self.occs.iter().any(|o| {
            if !o.bx.contains(p) {
                return false;
            }
            let pr = &self.prisms[o.prism];
            if pr.embedded {
                return false;
            }
            if !o.cut && (stack.contains(&pr.comp) || pr.z0 < w - EPS) {
                return false;
            }
            geom::locate_even_odd(p, &o.loops, 1e-9) == Loc::Inside
        })
    }

    pub fn draw_nails(&mut self) {
        let scene = self.scene;
        let elevation = self.spec.kind == ViewKind::Elevation;
        let cut = self.spec.cut_z;
        let s = self.spec.scale;
        let crop = self.spec.crop;
        let pen = self.pen_for(Pen::Steel);
        let mut dots: Vec<V2> = Vec::new();
        for c in &scene.comps {
            if !scene.nails_shown(c, &self.spec.members) {
                continue;
            }
            let stack = &c.built.nail_stack;
            let mut in_crop = 0usize;
            let mut drawn = 0usize;
            for n in &c.built.nails {
                let (h, hw) = self.nail_view(n.head);
                let (t, tw) = self.nail_view(n.tip);
                let (wmin, wmax) = (hw.min(tw), hw.max(tw));
                let row_w = if self.spec.kind == ViewKind::Plan { n.row[1] } else { n.row[2] };
                let band = if row_w.abs() > 0.7 { n.pitch / 2.0 } else { 0.0 };
                let len = h.dist(t);
                let full = (0..3).map(|k| (n.tip[k] - n.head[k]) * (n.tip[k] - n.head[k])).sum::<f64>();
                let end_on = len < 0.3 * crate::math::sqrt(full);
                let probe = if end_on { h } else { h.mid(t) };
                if !crop.contains(probe) {
                    continue;
                }
                in_crop += 1;
                let is_cut = !elevation && wmin - band <= cut + EPS && wmax + band >= cut - EPS;
                if !elevation && !is_cut && wmax + band > cut {
                    continue;
                }
                if !is_cut && self.nail_hidden(probe, wmax, stack) {
                    continue;
                }
                drawn += 1;
                self.nail_marks.push((c.index, probe));
                if end_on {
                    if dots.iter().any(|d| d.eql(h, EPS)) {
                        continue;
                    }
                    dots.push(h);
                    self.nail_dot(n, h, s, pen, &c.id);
                } else {
                    self.nail_shank(n, h, t, s, pen, &c.id);
                }
            }
            if in_crop > 0 && drawn == 0 {
                let mut clear = f64::NEG_INFINITY;
                for o in &scene.comps {
                    if stack.contains(&o.index) {
                        for p in &o.world {
                            clear = clear.max(if self.spec.kind == ViewKind::Plan {
                                p.loops.iter().flat_map(|l| l.iter().map(|q| q.y)).fold(f64::NEG_INFINITY, f64::max)
                            } else {
                                p.z1
                            });
                        }
                    }
                }
                self.nails_hidden.push(NailsHidden { id: c.id.clone(), clear: crate::math::ceil(clear + 0.5) });
            }
        }
    }

    fn nail_dot(&mut self, n: &Nail, h: V2, s: f64, pen: Pen, src: &str) {
        if n.screw {
            let r = 0.03 * s;
            self.add_stroke(Target::Strokes, model::circle_loop(h.x, h.y, r), true, pen, src, true);
            let d = r * 0.7071;
            for (a, b) in [(V2::init(-d, -d), V2::init(d, d)), (V2::init(-d, d), V2::init(d, -d))] {
                self.add_stroke(
                    Target::Strokes,
                    alloc::vec![Pt::at(h.add(a), 0.0), Pt::at(h.add(b), 0.0)],
                    false,
                    pen,
                    src,
                    true,
                );
            }
            return;
        }
        let r = 0.016 * s;
        self.fills.push(FillItem {
            layer: self.layer_for(pen),
            src: String::from(src),
            loops: model::one_loop(model::circle_loop(h.x, h.y, r)),
        });
    }

    fn nail_shank(&mut self, n: &Nail, h: V2, t: V2, s: f64, pen: Pen, src: &str) {
        let d = t.sub(h).norm();
        let p = d.perp();
        let len = h.dist(t);
        let hb = (0.04 * s).min(0.4 * len);
        let h = h.sub(d.scale(0.5 * hb));
        let len = h.dist(t);
        self.add_clipped(Target::Strokes, &[Pt::at(h, 0.0), Pt::at(t, 0.0)], false, pen, src, true);
        self.add_clipped(
            Target::Strokes,
            &[Pt::at(h.add(p.scale(hb)), 0.0), Pt::at(h.sub(p.scale(hb)), 0.0)],
            false,
            pen,
            src,
            true,
        );
        if n.screw {
            let tk = 0.6 * hb;
            for f in [0.45, 0.6, 0.75, 0.9] {
                let c = h.add(d.scale(f * len));
                let a = c.add(p.scale(tk)).sub(d.scale(tk * 0.5));
                let b = c.sub(p.scale(tk)).add(d.scale(tk * 0.5));
                self.add_clipped(Target::Strokes, &[Pt::at(a, 0.0), Pt::at(b, 0.0)], false, pen, src, true);
            }
        }
    }
}
