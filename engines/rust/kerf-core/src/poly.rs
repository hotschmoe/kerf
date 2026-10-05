//! Polygon booleans on flattened loops (via i_overlay) and rect clipping of regions.

use crate::geom::*;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

pub type Contours = Vec<Vec<[f64; 2]>>;
pub type Shape = Vec<Vec<Pt>>; // outer first, then holes

pub fn to_c(p: &[Pt]) -> Vec<[f64; 2]> {
    p.iter().map(|q| [q.x, q.y]).collect()
}

fn from_shapes(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Shape> {
    shapes.into_iter().map(|sh| sh.into_iter().map(|c| c.into_iter().map(|p| pt(p[0], p[1])).collect()).collect()).collect()
}

/// Flatten a region to contour lists (outer, holes).
pub fn region_contours(r: &Region, tol: f64) -> Shape {
    let mut out = vec![flatten_loop(&r.outer, tol)];
    for h in &r.holes {
        out.push(flatten_loop(h, tol));
    }
    out
}

pub fn run(subj: &[Vec<Pt>], clip: &[Vec<Pt>], rule: OverlayRule) -> Vec<Shape> {
    let s: Contours = subj.iter().map(|c| to_c(c)).collect();
    let c: Contours = clip.iter().map(|c| to_c(c)).collect();
    from_shapes(s.overlay(&c, rule, FillRule::NonZero))
}

pub fn difference(subj: &[Vec<Pt>], clip: &[Vec<Pt>]) -> Vec<Shape> {
    if clip.is_empty() {
        return vec![subj.to_vec()];
    }
    run(subj, clip, OverlayRule::Difference)
}

pub fn intersect(subj: &[Vec<Pt>], clip: &[Vec<Pt>]) -> Vec<Shape> {
    run(subj, clip, OverlayRule::Intersect)
}

pub fn union(subj: &[Vec<Pt>]) -> Vec<Shape> {
    let empty: Vec<Vec<Pt>> = vec![];
    run(subj, &empty, OverlayRule::Subject)
}

pub fn shape_area(sh: &Shape) -> f64 {
    let mut a = 0.0;
    for (i, c) in sh.iter().enumerate() {
        let ar = poly_area(c).abs();
        if i == 0 { a += ar } else { a -= ar }
    }
    a
}

pub fn shape_to_region(sh: &Shape) -> Region {
    let to_loop = |c: &Vec<Pt>| -> Loop { c.iter().map(|p| v(p.x, p.y)).collect() };
    let mut it = sh.iter();
    let outer = it.next().map(to_loop).unwrap_or_default();
    Region { outer: make_ccw(&outer), holes: it.map(|h| make_cw(&to_loop(h))).collect() }
}

/// Region clipped to a rect: unchanged (exact arcs) when inside, empty when outside, flattened otherwise.
pub fn clip_region_rect(r: &Region, rect: &Rect) -> Vec<Region> {
    let bb = region_bbox(r);
    if !bb.overlaps(rect, 0.0) {
        return vec![];
    }
    if bb.x0 >= rect.x0 - 1e-9 && bb.x1 <= rect.x1 + 1e-9 && bb.y0 >= rect.y0 - 1e-9 && bb.y1 <= rect.y1 + 1e-9 {
        return vec![r.clone()];
    }
    let subj = region_contours(r, 0.002);
    let clip = vec![rect.corners().to_vec()];
    intersect(&subj, &clip).iter().map(shape_to_region).collect()
}
