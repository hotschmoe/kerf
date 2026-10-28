//! Visible-region shapes and where a label or a note's arrow can land inside them (SPEC 6.3, SPEC 18): pure geometry on
//! polygons with holes, used by both the section and the iso annotation code.

use crate::clip;
use crate::geom::{self, Box, V2};
use crate::sort;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct Shape<'a> {
    pub outer: &'a [V2],
    pub holes: Vec<&'a [V2]>,
}

pub fn shapes_of<L: AsRef<[V2]>>(loops: &[L]) -> Vec<Shape<'_>> {
    let mut out = Vec::new();
    for l in loops {
        let l = l.as_ref();
        if l.len() < 3 || geom::signed_area_v(l) <= 0.0 {
            continue;
        }
        let mut holes = Vec::new();
        for h in loops {
            let h = h.as_ref();
            if h.len() >= 3 && geom::signed_area_v(h) < 0.0 && geom::point_in_loop_eo(h[0], l) {
                holes.push(h);
            }
        }
        out.push(Shape { outer: l, holes });
    }
    out
}

fn shape_area(s: &Shape) -> f64 {
    let mut a = geom::signed_area_v(s.outer);
    for h in &s.holes {
        a += geom::signed_area_v(h);
    }
    a
}

fn shape_contains(s: &Shape, p: V2) -> bool {
    geom::point_in_loop_eo(p, s.outer) && !s.holes.iter().any(|h| geom::point_in_loop_eo(p, h))
}

pub fn label_point(shapes: &[Shape]) -> Option<V2> {
    let mut best: Option<&Shape> = None;
    let mut best_area = -1.0;
    for s in shapes {
        let ar = shape_area(s);
        if ar > best_area {
            best_area = ar;
            best = Some(s);
        }
    }
    let b = best?;
    let c = geom::centroid_v(b.outer);
    if shape_contains(b, c) {
        return Some(c);
    }
    let mut xs = Vec::new();
    crossings(&mut xs, b.outer, c.y);
    for h in &b.holes {
        crossings(&mut xs, h, c.y);
    }
    sort::stable(&mut xs, |a, b| a < b);
    let mut best_mid: Option<(f64, f64)> = None;
    for pair in xs.chunks_exact(2) {
        let (x0, x1) = (pair[0], pair[1]);
        let mid = (x0 + x1) * 0.5;
        let d = if c.x >= x0 && c.x <= x1 { 0.0 } else { (c.x - mid).abs() };
        if best_mid.is_none_or(|(bd, _)| d < bd) {
            best_mid = Some((d, mid));
        }
    }
    match best_mid {
        Some((_, m)) => Some(V2::init(m, c.y)),
        None => Some(c),
    }
}

fn crossings(xs: &mut Vec<f64>, cont: &[V2], y: f64) {
    for (i, p) in cont.iter().enumerate() {
        let q = cont[(i + 1) % cont.len()];
        if (p.y > y) != (q.y > y) {
            xs.push(p.x + (y - p.y) / (q.y - p.y) * (q.x - p.x));
        }
    }
}

fn contours<'a>(s: &'a Shape) -> impl Iterator<Item = &'a [V2]> {
    core::iter::once(s.outer).chain(s.holes.iter().copied())
}

fn shape_depth(s: &Shape, p: V2) -> f64 {
    let mut d = f64::INFINITY;
    for c in contours(s) {
        for (i, &q) in c.iter().enumerate() {
            d = d.min(geom::dist_point_seg(p, q, c[(i + 1) % c.len()]));
        }
    }
    d
}

fn on_crop_edge(crop: &Box, p: V2, q: V2) -> bool {
    let e = 1e-6;
    ((p.x - crop.x0).abs() < e && (q.x - crop.x0).abs() < e)
        || ((p.x - crop.x1).abs() < e && (q.x - crop.x1).abs() < e)
        || ((p.y - crop.y0).abs() < e && (q.y - crop.y0).abs() < e)
        || ((p.y - crop.y1).abs() < e && (q.y - crop.y1).abs() < e)
}

fn shape_depth_no_crop(s: &Shape, p: V2, crop: &Box) -> f64 {
    let mut d = f64::INFINITY;
    for c in contours(s) {
        for (i, &q) in c.iter().enumerate() {
            let r = c[(i + 1) % c.len()];
            if !on_crop_edge(crop, q, r) {
                d = d.min(geom::dist_point_seg(p, q, r));
            }
        }
    }
    d
}

pub struct Banded {
    pub p: V2,
    pub inset: Box,
}

pub fn banded_label_point(shapes: &[Shape], crop: Box, h: f64) -> Option<Banded> {
    let prim = label_point(shapes)?;
    let band = 2.0 * h;
    let inset = crop.expand(-band);
    if inset.x1 <= inset.x0 || inset.y1 <= inset.y0 {
        return Some(Banded { p: prim, inset: crop });
    }
    if inset.contains(prim) {
        return Some(Banded { p: prim, inset });
    }
    let mut bb = Box::default();
    for x in shapes {
        bb.add_box(clip::loops_box(&[x.outer]));
    }
    let Some(step) = grid_step(&bb, h, 6.0) else {
        return Some(Banded { p: prim, inset });
    };
    let mut best = prim;
    let mut best_score = -1.0;
    let mut best_d = f64::INFINITY;
    let mut x = bb.x0 + step * 0.5;
    while x < bb.x1 {
        let mut y = bb.y0 + step * 0.5;
        while y < bb.y1 {
            let q = V2::init(x, y);
            if let Some(sh) = shapes.iter().find(|sh| shape_contains(sh, q)) {
                let dc = (x - crop.x0).min(crop.x1 - x).min((y - crop.y0).min(crop.y1 - y));
                let score = shape_depth_no_crop(sh, q, &crop).min(dc.min(band));
                let dp = q.dist(prim);
                if score > best_score + 1e-9 || (score > best_score - 1e-9 && dp < best_d) {
                    best_score = score;
                    best_d = dp;
                    best = q;
                }
            }
            y += step;
        }
        x += step;
    }
    Some(Banded { p: best, inset })
}

fn grid_step(bb: &Box, h: f64, div: f64) -> Option<f64> {
    if !(h > 0.0) || !h.is_finite() || !bb.width().is_finite() || !bb.height().is_finite() {
        return None;
    }
    let mut step = h.min(bb.width().min(bb.height()) / div).max(h / 8.0);
    let mut guard = 0u32;
    while (bb.width() / step + 1.0) * (bb.height() / step + 1.0) > 2500.0 && guard < 200 {
        step *= 1.5;
        guard += 1;
    }
    Some(step)
}

fn far_from_all(list: &[V2], q: V2, d: f64) -> bool {
    list.iter().all(|o| o.dist(q) >= d)
}

pub fn candidates_for(shapes: &[Shape], inset: Box, primary: V2, h: f64) -> Vec<V2> {
    let mut out = alloc::vec![primary];
    let mut bb = Box::default();
    for s in shapes {
        bb.add_box(clip::loops_box(&[s.outer]));
    }
    if bb.is_empty() {
        return out;
    }
    let Some(step) = grid_step(&bb, h, 3.0) else {
        return out;
    };
    let mut pts: Vec<V2> = Vec::new();
    let mut depth: Vec<f64> = Vec::new();
    let mut maxd: f64 = 0.0;
    let mut x = bb.x0 + step * 0.5;
    while x < bb.x1 {
        let mut y = bb.y0 + step * 0.5;
        while y < bb.y1 {
            let q = V2::init(x, y);
            if inset.contains(q) {
                if let Some(s) = shapes.iter().find(|s| shape_contains(s, q)) {
                    let d = shape_depth(s, q);
                    pts.push(q);
                    depth.push(d);
                    maxd = maxd.max(d);
                }
            }
            y += step;
        }
        x += step;
    }
    let thr = (0.5 * h).min(0.6 * maxd);
    let keep: Vec<V2> = pts.iter().zip(&depth).filter(|(_, d)| **d >= thr).map(|(q, _)| *q).collect();
    for _ in 0..4 {
        let mut best: Option<V2> = None;
        let mut bd = f64::INFINITY;
        for &q in &keep {
            if !far_from_all(&out, q, h) {
                continue;
            }
            let d = q.dist(primary);
            if d < bd {
                bd = d;
                best = Some(q);
            }
        }
        match best {
            Some(q) => out.push(q),
            None => break,
        }
    }
    let dirs = [
        V2::init(-1.0, 0.0), V2::init(1.0, 0.0), V2::init(0.0, 1.0), V2::init(0.0, -1.0),
        V2::init(-1.0, 1.0), V2::init(1.0, 1.0), V2::init(-1.0, -1.0), V2::init(1.0, -1.0),
    ];
    for dv in dirs {
        let mut best: Option<V2> = None;
        let mut bp = f64::NEG_INFINITY;
        for &q in &keep {
            let pr = q.dot(dv);
            if pr > bp + 1e-9 {
                bp = pr;
                best = Some(q);
            }
        }
        if let Some(q) = best {
            if far_from_all(&out, q, 0.5 * h) {
                out.push(q);
            }
        }
    }
    out
}
