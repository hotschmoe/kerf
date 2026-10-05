//! Polyline offsetting and bar stroking (fillets with exact arcs).

use crate::geom::*;

/// Left offset of an open polyline by `d` with mitered joins.
pub fn offset_poly(pts: &[Pt], d: f64) -> Vec<Pt> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    let mut dirs: Vec<Pt> = vec![];
    for i in 0..n - 1 {
        dirs.push((pts[i + 1] - pts[i]).norm());
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let nl = if i == 0 {
            dirs[0].perp()
        } else if i == n - 1 {
            dirs[n - 2].perp()
        } else {
            let a = dirs[i - 1].perp();
            let b = dirs[i].perp();
            let s = a + b;
            let k = 1.0 + a.dot(b);
            if k < 0.2 {
                // very sharp turn: fall back to the incoming normal
                a
            } else {
                s * (1.0 / k)
            }
        };
        out.push(pts[i] + nl * d);
    }
    out
}

/// Strip of thickness `t` grown to one side of the polyline (left if t > 0).
pub fn thick_poly(pts: &[Pt], t: f64) -> Loop {
    let off = offset_poly(pts, t);
    let mut l: Loop = pts.iter().map(|p| v(p.x, p.y)).collect();
    for p in off.iter().rev() {
        l.push(v(p.x, p.y));
    }
    dedupe_loop(&l)
}

/// Strip of total width `w` centered on the polyline.
pub fn centered_strip(pts: &[Pt], w: f64) -> Loop {
    let a = offset_poly(pts, w * 0.5);
    let b = offset_poly(pts, -w * 0.5);
    let mut l: Loop = b.iter().map(|p| v(p.x, p.y)).collect();
    for p in a.iter().rev() {
        l.push(v(p.x, p.y));
    }
    dedupe_loop(&l)
}

#[derive(Clone, Copy, Debug)]
enum El {
    L(Pt, Pt),
    A { c: Pt, r: f64, a0: f64, sw: f64 },
}

fn fillet(pts: &[Pt], r: f64) -> Vec<El> {
    let n = pts.len();
    let mut els = vec![];
    let mut cur = pts[0];
    for i in 1..n - 1 {
        let p = pts[i];
        let d1 = (p - pts[i - 1]).norm();
        let d2 = (pts[i + 1] - p).norm();
        let cr = d1.cross(d2);
        let th = cr.abs().atan2(d1.dot(d2));
        if th < 1e-6 || r <= 1e-9 {
            els.push(El::L(cur, p));
            cur = p;
            continue;
        }
        let l1 = (p - cur).len();
        let l2 = (pts[i + 1] - p).len();
        let mut t = r * (th * 0.5).tan();
        let tmax = (l1.min(l2)) * if i + 2 == n { 0.99 } else { 0.5 };
        let mut reff = r;
        if t > tmax {
            t = tmax;
            reff = t / (th * 0.5).tan();
        }
        let a = p - d1 * t;
        let b = p + d2 * t;
        let left = cr > 0.0;
        let c = if left { a + d1.perp() * reff } else { a - d1.perp() * reff };
        let a0 = (a.y - c.y).atan2(a.x - c.x);
        let sw = if left { th } else { -th };
        if cur.dist(a) > 1e-9 {
            els.push(El::L(cur, a));
        }
        els.push(El::A { c, r: reff, a0, sw });
        cur = b;
    }
    if cur.dist(pts[n - 1]) > 1e-9 {
        els.push(El::L(cur, pts[n - 1]));
    }
    els
}

fn off_el(e: &El, d: f64) -> (Pt, Pt, f64) {
    // d > 0: left of travel. returns (start, end, bulge)
    match *e {
        El::L(a, b) => {
            let nl = (b - a).perp().norm();
            (a + nl * d, b + nl * d, 0.0)
        }
        El::A { c, r, a0, sw } => {
            let ro = if sw >= 0.0 { r - d } else { r + d };
            let s = pt(c.x + ro * a0.cos(), c.y + ro * a0.sin());
            let e2 = pt(c.x + ro * (a0 + sw).cos(), c.y + ro * (a0 + sw).sin());
            (s, e2, (sw / 4.0).tan())
        }
    }
}

/// Outline loop of a bar of diameter `dia` along `pts`, with fillet radius `bend_r` (centerline) at bends.
pub fn stroke_bar(pts: &[Pt], dia: f64, bend_r: f64) -> Loop {
    let mut clean: Vec<Pt> = vec![];
    for p in pts {
        if clean.last().map_or(true, |q| q.dist(*p) > 1e-9) {
            clean.push(*p);
        }
    }
    if clean.len() < 2 {
        return vec![];
    }
    let hw = dia * 0.5;
    let els = fillet(&clean, bend_r.max(hw * 1.01));
    let left: Vec<(Pt, Pt, f64)> = els.iter().map(|e| off_el(e, hw)).collect();
    let right: Vec<(Pt, Pt, f64)> = els.iter().map(|e| off_el(e, -hw)).collect();
    let mut l: Loop = vec![];
    for &(s, _, b) in &left {
        l.push(vb(s.x, s.y, b));
    }
    let last_l = left.last().unwrap().1;
    l.push(v(last_l.x, last_l.y));
    for &(_, e, b) in right.iter().rev() {
        l.push(vb(e.x, e.y, -b));
    }
    let first_r = right[0].0;
    l.push(v(first_r.x, first_r.y));
    dedupe_loop(&l)
}

/// Centerline sample (for 3D sweeps): the filleted path flattened.
pub fn bar_centerline(pts: &[Pt], bend_r: f64, tol: f64) -> Vec<Pt> {
    let mut clean: Vec<Pt> = vec![];
    for p in pts {
        if clean.last().map_or(true, |q| q.dist(*p) > 1e-9) {
            clean.push(*p);
        }
    }
    if clean.len() < 2 {
        return clean;
    }
    let els = fillet(&clean, bend_r);
    let mut out = vec![clean[0]];
    for e in els {
        match e {
            El::L(_, b) => out.push(b),
            El::A { c, r, a0, sw } => {
                let p0 = pt(c.x + r * a0.cos(), c.y + r * a0.sin());
                let p1 = pt(c.x + r * (a0 + sw).cos(), c.y + r * (a0 + sw).sin());
                let s = Seg::Arc { p0, p1, c, r, a0, sw };
                flatten_seg_pts(&s, tol, &mut out);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn straight_bar() {
        let l = stroke_bar(&[pt(0.0, 0.0), pt(0.0, 10.0)], 0.5, 1.5);
        assert_eq!(l.len(), 4);
        assert!((loop_area(&l).abs() - 5.0).abs() < 1e-9);
    }
    #[test]
    fn bent_bar() {
        let l = stroke_bar(&[pt(0.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0)], 0.5, 1.5);
        let a = loop_area(&l).abs();
        // approx length 10 + 10 - bend savings
        assert!(a > 8.0 && a < 11.0, "{}", a);
    }
    #[test]
    fn j_hook_area() {
        // d = 0.5: shaft 10 long then a 180 degree bend (inside radius 0.75) and a 2 inch return leg
        let d = 0.5;
        let l = stroke_bar(&[pt(0.0, 10.0), pt(0.0, 0.25), pt(2.0, 0.25), pt(2.0, 2.0)], d, 1.0);
        let a = loop_area(&l).abs();
        // centerline length: 9.75 + pi*1.0 + (2.0-0.25-1.0) -> area = length * d
        let len = 8.75 + std::f64::consts::PI * 1.0 + 0.75;
        assert!((a - len * d).abs() < 0.05, "{} vs {}", a, len * d);
    }

    #[test]
    fn strip() {
        let l = thick_poly(&[pt(0.0, 0.0), pt(10.0, 0.0)], 1.0);
        assert!((loop_area(&l).abs() - 10.0).abs() < 1e-9);
    }
}
