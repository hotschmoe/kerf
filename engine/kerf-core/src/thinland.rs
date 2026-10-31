//! Where a note can land on a thin member (membrane, flashing, thin panel) so the leader hits the member and not what is beside it.

use crate::catalog::Traits;
use crate::geom::{self, Box, V2};
use crate::model::Kind;
use crate::num;
use crate::scene::Comp;
use crate::section::{Class, Section};
use crate::sort;
use crate::units;
use alloc::rc::Rc;
use alloc::vec::Vec;

const THIN_PANEL: f64 = 0.5;

struct Skeleton {
    pts: Vec<V2>,
    half_width: f64,
    gap: f64,
    prism: usize,
    stroke: bool,
    exposed: bool,
}

fn poly_len(pts: &[V2]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).fold(0.0, |a, b| a + b)
}

fn quad_midline(lp: &[V2]) -> Option<Vec<V2>> {
    if lp.len() != 4 {
        return None;
    }
    let e0 = lp[0].dist(lp[1]);
    let e1 = lp[1].dist(lp[2]);
    let mid = |p: V2, q: V2| V2::init((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
    if e0 >= e1 {
        return Some(alloc::vec![mid(lp[1], lp[2]), mid(lp[3], lp[0])]);
    }
    Some(alloc::vec![mid(lp[0], lp[1]), mid(lp[2], lp[3])])
}

fn skeleton_of(sec: &mut Section<'_>, i: usize, traits: &Traits, panel_thickness: f64) -> Option<Skeleton> {
    let prisms = Rc::clone(&sec.prisms);
    let p = &prisms[i];
    if traits.line_like {
        if p.centerline.len() < 2 {
            return None;
        }
        let drawn =
            if p.kind == Kind::Line && p.line_pts.len() >= 2 { sec.line_points(p) } else { p.centerline.clone() };
        let v: Vec<V2> = drawn.iter().map(|q| q.v()).collect();
        let gap = if p.kind == Kind::Line { nearest_on_poly(&v, p.centerline[0].v()).dist } else { 0.0 };
        return Some(Skeleton {
            half_width: 0.5 * (p.sweep_r * 2.0).max(0.0625),
            pts: v,
            gap,
            prism: i,
            stroke: p.kind == Kind::Line,
            exposed: !(p.embedded || p.face_tie),
        });
    }
    if traits.rigid_sheet && panel_thickness < THIN_PANEL {
        let flat = sec.flat_of(i);
        let mid = quad_midline(flat.first()?)?;
        return Some(Skeleton {
            pts: mid,
            half_width: 0.5 * panel_thickness,
            gap: 0.0,
            prism: i,
            stroke: false,
            exposed: !p.embedded,
        });
    }
    None
}

struct Sample {
    p: V2,
    score: f64,
    d: f64,
}

/// Up to six landing points for a note on a thin component, best first, or `None` when the component is not thin or the
/// preferred point already hits it.
pub fn candidates(
    sec: &mut Section<'_>,
    comp: &Comp<'_>,
    inst: Option<u32>,
    part: Option<&str>,
    inset: Box,
    text_h: f64,
    prefer: V2,
) -> Option<Vec<V2>> {
    let face_on = comp.node.get("lay").and_then(crate::json::Value::str) == Some("face")
        && sec.spec.kind != crate::view::Kind::Plan;
    let panel_t = if face_on {
        1.0
    } else {
        comp.node.get("thickness").map_or(1.0, |t| units::parse_length(t).unwrap_or(1.0))
    };
    let prisms = Rc::clone(&sec.prisms);
    let mut skels: Vec<Skeleton> = Vec::new();
    for (i, p) in prisms.iter().enumerate() {
        if p.comp != comp.index || inst.is_some_and(|k| p.instance != k) || part.is_some_and(|pn| p.part != pn) {
            continue;
        }
        if sec.cls[i] == Class::Drop {
            continue;
        }
        skels.push(skeleton_of(sec, i, &comp.ty.traits, panel_t)?);
    }
    if skels.is_empty() {
        return None;
    }

    let mut edges: Vec<[V2; 2]> = Vec::new();
    let mut bodies: Vec<crate::section::Flat> = Vec::new();
    for (i, p) in prisms.iter().enumerate() {
        if p.comp == comp.index || sec.cls[i] == Class::Drop || p.kind != Kind::Body {
            continue;
        }
        let flat = sec.flat_of(i);
        for lp in flat.iter() {
            for (k, &q) in lp.iter().enumerate() {
                edges.push([q, lp[(k + 1) % lp.len()]]);
            }
        }
        if sec.cls[i] == Class::Cut && !p.embedded {
            bodies.push(flat);
        }
    }

    for sk in &skels {
        let near = nearest_on_poly(&sk.pts, prefer);
        if near.dist > sk.half_width * 1.5 + 0.02
            || clearance(&edges, prefer, near.dir, sk.half_width + sk.gap) < text_h
        {
            continue;
        }
        if sk.exposed && inside_body(&bodies, prefer, sk.half_width + sk.gap) {
            continue;
        }
        return None;
    }

    let mut samples: Vec<Sample> = Vec::new();
    let step = (text_h * 0.5).max(0.05);
    for sk in &skels {
        let region = sec.visible_region(sk.prism);
        let total = poly_len(&sk.pts);
        let n: usize = num::to_int_clamped(crate::math::ceil(total / step), 2, 400);
        for k in 0..=n {
            let at = total * k as f64 / n as f64;
            let q = point_at(&sk.pts, at);
            if !inset.contains(q.p) {
                continue;
            }
            if !sk.stroke && !shown(&region, q.p, sk.half_width * 1.5 + 0.02) {
                continue;
            }
            if sk.exposed && inside_body(&bodies, q.p, sk.half_width + sk.gap) {
                continue;
            }
            let score = clearance(&edges, q.p, q.dir, sk.half_width + sk.gap).min(2.0 * text_h);
            samples.push(Sample { p: q.p, score, d: q.p.dist(prefer) });
        }
    }
    if samples.is_empty() {
        return None;
    }
    sort::stable(&mut samples, |x, y| if (x.score - y.score).abs() > 1e-6 { x.score > y.score } else { x.d < y.d });
    let mut out: Vec<V2> = alloc::vec![samples[0].p];
    for s in &samples[1..] {
        if out.len() >= 6 {
            break;
        }
        if out.iter().all(|o| o.dist(s.p) >= text_h) {
            out.push(s.p);
        }
    }
    Some(out)
}

fn clearance(edges: &[[V2; 2]], p: V2, dir: V2, half_width: f64) -> f64 {
    let mut clear = f64::INFINITY;
    for e in edges {
        let d = geom::dist_point_seg(p, e[0], e[1]);
        if d >= clear {
            continue;
        }
        let ev = e[1].sub(e[0]);
        let el = ev.len();
        if el > 1e-9 && d < half_width * 2.0 + 0.05 {
            let cross = (ev.x * dir.y - ev.y * dir.x).abs() / el;
            if cross < 0.15 {
                continue;
            }
        }
        clear = d;
    }
    clear
}

struct Nearest {
    dist: f64,
    dir: V2,
}

fn nearest_on_poly(pts: &[V2], p: V2) -> Nearest {
    let mut best = Nearest { dist: f64::INFINITY, dir: V2::init(1.0, 0.0) };
    for w in pts.windows(2) {
        let d = geom::dist_point_seg(p, w[0], w[1]);
        if d < best.dist {
            let v = w[1].sub(w[0]);
            let l = v.len();
            best = Nearest { dist: d, dir: if l > 1e-12 { V2::init(v.x / l, v.y / l) } else { V2::init(1.0, 0.0) } };
        }
    }
    best
}

struct OnLine {
    p: V2,
    dir: V2,
}

fn point_at(pts: &[V2], at: f64) -> OnLine {
    let mut rem = at;
    for i in 0..pts.len().saturating_sub(1) {
        let l = pts[i].dist(pts[i + 1]);
        if rem <= l || i + 2 == pts.len() {
            let t = if l > 1e-12 { (rem / l).min(1.0) } else { 0.0 };
            let d = pts[i + 1].sub(pts[i]);
            return OnLine {
                p: V2::init(pts[i].x + d.x * t, pts[i].y + d.y * t),
                dir: if l > 1e-12 { V2::init(d.x / l, d.y / l) } else { V2::init(1.0, 0.0) },
            };
        }
        rem -= l;
    }
    OnLine { p: pts.first().copied().unwrap_or(V2::init(0.0, 0.0)), dir: V2::init(1.0, 0.0) }
}

fn inside_body(bodies: &[crate::section::Flat], p: V2, half_width: f64) -> bool {
    let tol = half_width * 2.0 + 0.05;
    for b in bodies {
        if geom::locate_even_odd(p, &b[..], 1e-9) != geom::Loc::Inside {
            continue;
        }
        let mut depth = f64::INFINITY;
        for lp in b.iter() {
            for (k, &q) in lp.iter().enumerate() {
                depth = depth.min(geom::dist_point_seg(p, q, lp[(k + 1) % lp.len()]));
            }
        }
        if depth > tol {
            return true;
        }
    }
    false
}

fn shown(region: &[Vec<V2>], p: V2, tol: f64) -> bool {
    if geom::locate_even_odd(p, region, 1e-9) != geom::Loc::Outside {
        return true;
    }
    region
        .iter()
        .any(|lp| lp.iter().enumerate().any(|(k, &q)| geom::dist_point_seg(p, q, lp[(k + 1) % lp.len()]) <= tol))
}
