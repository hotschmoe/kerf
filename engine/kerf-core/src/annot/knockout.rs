//! Hatch knockout (SPEC 18): hatch lines are cut where a note, dimension text or label sits on them.

use crate::drawing::Item;
use crate::geom::{self, V2};
use crate::sort;
use alloc::vec::Vec;

pub fn convex_interval(a: V2, b: V2, poly: &[V2; 4]) -> Option<[f64; 2]> {
    let area = geom::signed_area_v(poly);
    let sgn = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    let d = b.sub(a);
    for (i, &p) in poly.iter().enumerate() {
        let q = poly[(i + 1) % 4];
        let e = q.sub(p);
        let num = e.cross(a.sub(p)) * sgn;
        let den = e.cross(d) * sgn;
        if den.abs() < 1e-15 {
            if num < 0.0 {
                return None;
            }
        } else {
            let t = -num / den;
            if den > 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
        if t0 > t1 {
            return None;
        }
    }
    Some([t0, t1])
}

pub fn knock_hatch(items: &mut [Item], boxes: &[[V2; 4]]) {
    if boxes.is_empty() {
        return;
    }
    for it in items {
        let Item::Hatch(h) = it else { continue };
        let mut out: Vec<[f64; 4]> = Vec::new();
        for l in &h.lines {
            let p = V2::init(l[0], l[1]);
            let q = V2::init(l[2], l[3]);
            let mut cuts: Vec<[f64; 2]> = boxes.iter().filter_map(|bx| convex_interval(p, q, bx)).collect();
            if cuts.is_empty() {
                out.push(*l);
                continue;
            }
            if p.eql(q, 1e-12) {
                continue;
            }
            sort::stable(&mut cuts, |x, y| x[0] < y[0]);
            let mut t = 0.0f64;
            for c in &cuts {
                if c[0] > t + 1e-9 {
                    let s0 = p.lerp(q, t);
                    let s1 = p.lerp(q, c[0]);
                    out.push([s0.x, s0.y, s1.x, s1.y]);
                }
                t = t.max(c[1]);
            }
            if t < 1.0 - 1e-9 {
                let s0 = p.lerp(q, t);
                out.push([s0.x, s0.y, q.x, q.y]);
            }
        }
        h.lines = out;
    }
}
