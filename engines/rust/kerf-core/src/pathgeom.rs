//! Polyline helpers for thickened members: fillets, offsets, ribbons.

use crate::geom::{self, Pt, V2};
use crate::math;
use alloc::vec::Vec;

/// Reverse an open bulge polyline (the last vertex carries bulge 0).
pub fn reverse_open(pts: &[Pt]) -> Vec<Pt> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let src = n - 1 - i;
            let b = if src > 0 { -pts[src - 1].b } else { 0.0 };
            Pt { x: pts[src].x, y: pts[src].y, b: if i + 1 == n { 0.0 } else { b } }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct SegOff {
    a: V2,
    b: V2,
    bulge: f64,
}

fn offset_seg(p0: V2, p1: V2, bulge: f64, d: f64) -> SegOff {
    if bulge == 0.0 {
        let dir = p1.sub(p0).norm();
        let n = dir.perp().scale(d);
        return SegOff { a: p0.add(n), b: p1.add(n), bulge: 0.0 };
    }
    let arc = geom::arc_of(p0, p1, bulge);
    let r2 = if bulge > 0.0 { arc.r - d } else { arc.r + d };
    let rr = r2.max(1e-9);
    let k = rr / arc.r;
    SegOff { a: arc.c.add(p0.sub(arc.c).scale(k)), b: arc.c.add(p1.sub(arc.c).scale(k)), bulge }
}

fn line_intersect(a0: V2, a1: V2, b0: V2, b1: V2) -> Option<V2> {
    let r = a1.sub(a0);
    let s = b1.sub(b0);
    let den = r.cross(s);
    if den.abs() < 1e-12 * (r.len() * s.len()).max(1.0) {
        return None;
    }
    let t = b0.sub(a0).cross(s) / den;
    Some(a0.add(r.scale(t)))
}

/// Offset an open bulge polyline to its left by `d` (negative = right). Line-line joints are mitered.
pub fn offset_open(pts: &[Pt], d: f64) -> Vec<Pt> {
    let nseg = pts.len().saturating_sub(1);
    if nseg == 0 {
        return pts.to_vec();
    }
    let mut segs: Vec<SegOff> = (0..nseg).map(|i| offset_seg(pts[i].v(), pts[i + 1].v(), pts[i].b, d)).collect();
    for i in 0..nseg - 1 {
        let (s0, s1) = (segs[i], segs[i + 1]);
        if s0.b.eql(s1.a, 1e-9) {
            continue;
        }
        if s0.bulge == 0.0 && s1.bulge == 0.0 {
            if let Some(m) = line_intersect(s0.a, s0.b, s1.a, s1.b) {
                let lim = 4.0 * d.abs().max(1e-9);
                if m.dist(pts[i + 1].v()) <= lim {
                    segs[i].b = m;
                    segs[i + 1].a = m;
                }
            }
        }
    }
    let mut out = Vec::new();
    for i in 0..nseg {
        out.push(Pt { x: segs[i].a.x, y: segs[i].a.y, b: segs[i].bulge });
        if i + 1 < nseg && !segs[i].b.eql(segs[i + 1].a, 1e-9) {
            out.push(Pt { x: segs[i].b.x, y: segs[i].b.y, b: 0.0 });
        }
    }
    out.push(Pt { x: segs[nseg - 1].b.x, y: segs[nseg - 1].b.y, b: 0.0 });
    out
}

/// Closed ribbon around `center`: `left` to the left side, `right` to the right side, flat end caps.
pub fn ribbon(center: &[Pt], left: f64, right: f64) -> Vec<Pt> {
    let l = offset_open(center, left);
    let r = reverse_open(&offset_open(center, -right));
    let mut all = l;
    all.extend_from_slice(&r);
    let n = all.len();
    let mut cleaned: Vec<Pt> = Vec::new();
    for (i, &p) in all.iter().enumerate() {
        let prev = all[(i + n - 1) % n];
        if !cleaned.is_empty() && p.v().eql(prev.v(), 1e-12) && prev.b == 0.0 {
            continue;
        }
        cleaned.push(p);
    }
    if let (Some(first), Some(last)) = (cleaned.first(), cleaned.last()) {
        if cleaned.len() > 1 && first.v().eql(last.v(), 1e-12) && last.b == 0.0 {
            cleaned.pop();
        }
    }
    cleaned
}

/// Fillet the interior vertices of a polyline with radius `r_fillet` (reduced where segments are short).
/// Output is an open bulge polyline.
pub fn fillet(pts: &[V2], r_fillet: f64) -> Vec<Pt> {
    if pts.len() < 3 || r_fillet <= 0.0 {
        return pts.iter().map(|&p| Pt::at(p, 0.0)).collect();
    }
    let mut out = Vec::new();
    out.push(Pt::at(pts[0], 0.0));
    for i in 1..pts.len() - 1 {
        let p = pts[i];
        let u = p.sub(pts[i - 1]);
        let w = pts[i + 1].sub(p);
        let lu = u.len();
        let lw = w.len();
        if lu < 1e-12 || lw < 1e-12 {
            continue;
        }
        let ud = u.scale(1.0 / lu);
        let wd = w.scale(1.0 / lw);
        let cr = ud.cross(wd);
        let dt = ud.dot(wd);
        let phi = math::atan2(cr, dt);
        if phi.abs() < 1e-9 {
            out.push(Pt::at(p, 0.0));
            continue;
        }
        let mut r = r_fillet;
        let t_need = r * math::tan(phi.abs() / 2.0);
        let lim_u = if i == 1 { lu } else { lu / 2.0 };
        let lim_w = if i + 2 == pts.len() { lw } else { lw / 2.0 };
        let lim = lim_u.min(lim_w);
        if t_need > lim {
            r = lim / math::tan(phi.abs() / 2.0);
        }
        let t = r * math::tan(phi.abs() / 2.0);
        out.push(Pt::at(p.sub(ud.scale(t)), geom::bulge_from_sweep(phi)));
        out.push(Pt::at(p.add(wd.scale(t)), 0.0));
    }
    out.push(Pt::at(pts[pts.len() - 1], 0.0));
    out
}
