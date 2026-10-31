//! Break lines where cut members, and steel seen lengthwise, run off the crop.

use super::Class;
use super::Section;
use crate::drawing::PathItem;
use crate::geom::{self, Pt, V2};
use crate::model::Kind;
use crate::pathclip;
use crate::pen::Pen;
use crate::sort;
use crate::style::Role;
use alloc::string::ToString;
use alloc::vec::Vec;

#[derive(Clone, Copy)]
pub struct Edge {
    vertical: bool,
    c: f64,
    lo: f64,
    hi: f64,
    out_sign: f64,
}

#[derive(Clone, Copy)]
struct Iv {
    a: f64,
    b: f64,
    comp: u32,
}

impl Section<'_> {
    pub fn break_lines(&mut self) {
        let crop = self.spec.crop;
        let edges = [
            Edge { vertical: true, c: crop.x0, lo: crop.y0, hi: crop.y1, out_sign: -1.0 },
            Edge { vertical: true, c: crop.x1, lo: crop.y0, hi: crop.y1, out_sign: 1.0 },
            Edge { vertical: false, c: crop.y0, lo: crop.x0, hi: crop.x1, out_sign: -1.0 },
            Edge { vertical: false, c: crop.y1, lo: crop.x0, hi: crop.x1, out_sign: 1.0 },
        ];
        for e in edges {
            let mut ivs: Vec<Iv> = Vec::new();
            for i in 0..self.prisms.len() {
                let (kind, embedded, role, comp, lengthwise) = {
                    let p = &self.prisms[i];
                    (p.kind, p.embedded, p.role, p.comp, p.lengthwise)
                };
                if kind != Kind::Body || embedded {
                    continue;
                }
                let seen_lengthwise = lengthwise && self.cls[i] == Class::Beyond;
                if !seen_lengthwise
                    && (self.cls[i] != Class::Cut
                        || role == Role::Soil
                        || self.style.material(&self.prisms[i].material).is_some_and(|m| m.fill))
                {
                    continue;
                }
                let f = self.flat_of(i);
                let bx = crate::clip::loops_box(&f);
                if e.vertical {
                    if bx.x0 > e.c + 1e-9 || bx.x1 < e.c - 1e-9 {
                        continue;
                    }
                } else if bx.y0 > e.c + 1e-9 || bx.y1 < e.c - 1e-9 {
                    continue;
                }
                let xs = pathclip::scan(&f, e.c, e.vertical);
                for &[x0, x1] in xs.as_chunks::<2>().0 {
                    let lo = x0.max(e.lo);
                    let hi = x1.min(e.hi);
                    if hi - lo < 1e-6 {
                        continue;
                    }
                    let mid = (lo + hi) / 2.0;
                    let probe = if e.vertical {
                        V2::init(e.c + e.out_sign * 1e-6, mid)
                    } else {
                        V2::init(mid, e.c + e.out_sign * 1e-6)
                    };
                    if geom::locate_even_odd(probe, &f, 0.0) != geom::Loc::Inside {
                        continue;
                    }
                    ivs.push(Iv { a: lo, b: hi, comp });
                }
            }
            if ivs.is_empty() {
                continue;
            }
            sort::stable(&mut ivs, |x, y| if x.a != y.a { x.a < y.a } else { x.comp < y.comp });
            let mut merged: Vec<Iv> = Vec::new();
            for iv in ivs {
                match merged.last_mut() {
                    Some(last) if iv.a <= last.b + 1e-3 => {
                        last.b = last.b.max(iv.b);
                        last.comp = last.comp.min(iv.comp);
                    }
                    _ => merged.push(iv),
                }
            }
            for m in merged {
                let pts = self.break_polyline(e, m.a, m.b);
                self.breaks.push(PathItem {
                    layer: self.layer_for(Pen::Break),
                    pen: Pen::Break,
                    src: "crop".to_string(),
                    closed: false,
                    pts,
                });
            }
        }
    }

    pub fn break_polyline(&self, e: Edge, a0: f64, b0: f64) -> Vec<Pt> {
        let s = self.spec.scale;
        let over = self.style.break_overshoot_in * s;
        let a_ = a0 - over;
        let b_ = b0 + over;
        let len = b_ - a_;
        let mid = (a_ + b_) / 2.0;
        let half = (self.style.break_period_in * s / 2.0).min(len * 0.35);
        let zig = (self.style.break_zig_in * s).min(len * 0.2);
        let uv = [
            [a_, 0.0],
            [mid - half, 0.0],
            [mid - half * 0.4, zig],
            [mid + half * 0.4, -zig],
            [mid + half, 0.0],
            [b_, 0.0],
        ];
        uv.iter()
            .map(
                |q| {
                    if e.vertical {
                        Pt { x: e.c + q[1], y: q[0], b: 0.0 }
                    } else {
                        Pt { x: q[0], y: e.c + q[1], b: 0.0 }
                    }
                },
            )
            .collect()
    }
}
