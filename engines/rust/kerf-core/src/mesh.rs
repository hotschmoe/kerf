//! 3D mesh (SPEC section 11): prisms extruded along Z, flat-shaded faces plus feature edges.

use crate::geom::*;
use crate::json::{arr_nums, num, obj, s};
use crate::model::*;
use crate::style::Style;
use serde_json::Value;

/// Ear-clipping triangulation of a polygon with holes. Returns (vertices, triangles).
pub fn triangulate(outer: &[Pt], holes: &[Vec<Pt>]) -> (Vec<Pt>, Vec<[usize; 3]>) {
    let mut poly: Vec<Pt> = if poly_area(outer) < 0.0 { outer.iter().rev().cloned().collect() } else { outer.to_vec() };
    let mut hs: Vec<Vec<Pt>> = holes.iter().map(|h| if poly_area(h) > 0.0 { h.iter().rev().cloned().collect() } else { h.clone() }).collect();
    hs.sort_by(|a, b| {
        let ma = a.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let mb = b.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        mb.partial_cmp(&ma).unwrap()
    });
    for h in hs {
        if h.len() < 3 {
            continue;
        }
        let (hi, _) = h.iter().enumerate().max_by(|a, b| a.1.x.partial_cmp(&b.1.x).unwrap()).unwrap();
        let hp = h[hi];
        // closest visible polygon vertex
        let mut best: Option<(f64, usize)> = None;
        for (vi, vp) in poly.iter().enumerate() {
            let d = hp.dist(*vp);
            if best.map_or(false, |(bd, _)| d >= bd) {
                continue;
            }
            let sg = Seg::Line(hp, *vp);
            let mut blocked = false;
            let nn = poly.len();
            for i in 0..nn {
                let (a, b) = (poly[i], poly[(i + 1) % nn]);
                if a.near(*vp, 1e-12) || b.near(*vp, 1e-12) {
                    continue;
                }
                if !intersect(&sg, &Seg::Line(a, b)).is_empty() {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                best = Some((d, vi));
            }
        }
        let vi = best.map(|b| b.1).unwrap_or(0);
        let mut np: Vec<Pt> = Vec::with_capacity(poly.len() + h.len() + 2);
        np.extend_from_slice(&poly[..=vi]);
        for k in 0..=h.len() {
            np.push(h[(hi + k) % h.len()]);
        }
        np.push(poly[vi]);
        np.extend_from_slice(&poly[vi + 1..]);
        poly = np;
    }
    let verts = poly.clone();
    let mut idx: Vec<usize> = (0..verts.len()).collect();
    let mut tris = vec![];
    let mut guard = 0;
    while idx.len() > 3 && guard < 100000 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for k in 0..n {
            let (ia, ib, ic) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let (a, b, c) = (verts[ia], verts[ib], verts[ic]);
            if (b - a).cross(c - b) <= 1e-12 {
                continue;
            }
            let mut ear = true;
            for &j in &idx {
                if j == ia || j == ib || j == ic {
                    continue;
                }
                let p = verts[j];
                if p.near(a, 1e-12) || p.near(b, 1e-12) || p.near(c, 1e-12) {
                    continue;
                }
                if in_tri(p, a, b, c) {
                    ear = false;
                    break;
                }
            }
            if ear {
                tris.push([ia, ib, ic]);
                idx.remove(k);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // degenerate remainder: fan it
            let n = idx.len();
            for k in 1..n - 1 {
                tris.push([idx[0], idx[k], idx[k + 1]]);
            }
            idx.clear();
            break;
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    (verts, tris)
}

fn in_tri(p: Pt, a: Pt, b: Pt, c: Pt) -> bool {
    let d1 = (b - a).cross(p - a);
    let d2 = (c - b).cross(p - b);
    let d3 = (a - c).cross(p - c);
    let neg = d1 < -1e-12 || d2 < -1e-12 || d3 < -1e-12;
    let pos = d1 > 1e-12 || d2 > 1e-12 || d3 > 1e-12;
    !(neg && pos) && !(d1.abs() < 1e-12 && d2.abs() < 1e-12 && d3.abs() < 1e-12) && d1 >= -1e-12 && d2 >= -1e-12 && d3 >= -1e-12
}

/// A flattened profile loop with per-vertex sharpness.
pub struct FLoop {
    pub pts: Vec<Pt>,
    pub sharp: Vec<bool>,
}

pub fn flatten_sharp(l: &Loop, max_step: f64, tol: f64) -> FLoop {
    let mut pts = vec![];
    let mut orig = vec![];
    for sg in loop_segs(l) {
        pts.push(sg.start());
        orig.push(true);
        if let Seg::Arc { c, r, a0, sw, .. } = sg {
            let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { max_step };
            let step = step.clamp(0.05, max_step);
            let n = ((sw.abs() / step).ceil() as usize).max(1);
            for i in 1..n {
                let a = a0 + sw * (i as f64 / n as f64);
                pts.push(pt(c.x + r * a.cos(), c.y + r * a.sin()));
                orig.push(false);
            }
        }
    }
    // sharp when the turn angle is large, or an original vertex that is not tangent-continuous
    let n = pts.len();
    let mut sharp = vec![false; n];
    for i in 0..n {
        let a = pts[(i + n - 1) % n];
        let b = pts[i];
        let c = pts[(i + 1) % n];
        let d1 = (b - a).norm();
        let d2 = (c - b).norm();
        let turn = d1.cross(d2).abs().atan2(d1.dot(d2));
        sharp[i] = turn > 0.35 || (orig[i] && turn > 0.12);
    }
    FLoop { pts, sharp }
}

pub struct PrismMesh {
    pub positions: Vec<f64>,
    pub normals: Vec<f64>,
    pub indices: Vec<u32>,
    pub edges: Vec<f64>,
}

fn push_tri(m: &mut PrismMesh, p: [[f64; 3]; 3], n: [f64; 3]) {
    let base = (m.positions.len() / 3) as u32;
    for q in p {
        m.positions.extend_from_slice(&q);
        m.normals.extend_from_slice(&n);
    }
    m.indices.extend_from_slice(&[base, base + 1, base + 2]);
}

fn push_edge(m: &mut PrismMesh, a: [f64; 3], b: [f64; 3]) {
    m.edges.extend_from_slice(&a);
    m.edges.extend_from_slice(&b);
}

pub fn mesh_prism(p: &Prism) -> PrismMesh {
    let mut m = PrismMesh { positions: vec![], normals: vec![], indices: vec![], edges: vec![] };
    let step = if p.bar.is_some() { std::f64::consts::TAU / 12.0 } else { 0.26 };
    let tol = 0.003;
    let outer = flatten_sharp(&p.region.outer, step, tol);
    let holes: Vec<FLoop> = p.region.holes.iter().map(|h| flatten_sharp(h, step, tol)).collect();
    let hpts: Vec<Vec<Pt>> = holes.iter().map(|h| h.pts.clone()).collect();
    let (verts, tris) = triangulate(&outer.pts, &hpts);
    for t in &tris {
        let (a, b, c) = (verts[t[0]], verts[t[1]], verts[t[2]]);
        // top cap (+z), ccw as given
        push_tri(&mut m, [[a.x, a.y, p.z1], [b.x, b.y, p.z1], [c.x, c.y, p.z1]], [0.0, 0.0, 1.0]);
        push_tri(&mut m, [[a.x, a.y, p.z0], [c.x, c.y, p.z0], [b.x, b.y, p.z0]], [0.0, 0.0, -1.0]);
    }
    for fl in std::iter::once(&outer).chain(holes.iter()) {
        let n = fl.pts.len();
        for i in 0..n {
            let (a, b) = (fl.pts[i], fl.pts[(i + 1) % n]);
            let d = (b - a).norm();
            let nn = [d.y, -d.x, 0.0];
            push_tri(&mut m, [[a.x, a.y, p.z0], [b.x, b.y, p.z0], [b.x, b.y, p.z1]], nn);
            push_tri(&mut m, [[a.x, a.y, p.z0], [b.x, b.y, p.z1], [a.x, a.y, p.z1]], nn);
            push_edge(&mut m, [a.x, a.y, p.z0], [b.x, b.y, p.z0]);
            push_edge(&mut m, [a.x, a.y, p.z1], [b.x, b.y, p.z1]);
            if fl.sharp[i] {
                push_edge(&mut m, [a.x, a.y, p.z0], [a.x, a.y, p.z1]);
            }
        }
    }
    m
}

/// Swept circle (12 segments) along an XY centerline at constant z.
pub fn mesh_sweep(center: &[Pt], r: f64, z: f64) -> PrismMesh {
    let mut m = PrismMesh { positions: vec![], normals: vec![], indices: vec![], edges: vec![] };
    let n = center.len();
    if n < 2 {
        return m;
    }
    let seg = 12;
    let mut rings: Vec<Vec<[f64; 3]>> = vec![];
    for i in 0..n {
        let d = if i == 0 { (center[1] - center[0]).norm() } else if i == n - 1 { (center[n - 1] - center[n - 2]).norm() } else { ((center[i] - center[i - 1]).norm() + (center[i + 1] - center[i]).norm()).norm() };
        let nl = d.perp();
        let mut ring = vec![];
        for k in 0..seg {
            let a = std::f64::consts::TAU * k as f64 / seg as f64;
            let off = nl * (r * a.cos());
            ring.push([center[i].x + off.x, center[i].y + off.y, z + r * a.sin()]);
        }
        rings.push(ring);
    }
    for i in 0..n - 1 {
        for k in 0..seg {
            let (a, b) = (rings[i][k], rings[i][(k + 1) % seg]);
            let (c, d) = (rings[i + 1][k], rings[i + 1][(k + 1) % seg]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let mut nn = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt().max(1e-12);
            nn = [nn[0] / l, nn[1] / l, nn[2] / l];
            push_tri(&mut m, [a, b, c], nn);
            push_tri(&mut m, [b, d, c], nn);
        }
    }
    for ring in [&rings[0], &rings[n - 1]] {
        for k in 0..seg {
            push_edge(&mut m, ring[k], ring[(k + 1) % seg]);
        }
    }
    m
}

pub fn mesh_json(model: &Model, style: &Style) -> Value {
    let mut parts = vec![];
    for c in &model.comps {
        if c.failed || !c.visible {
            continue;
        }
        for inst in &c.insts {
            for p in &inst.prisms {
                if p.only == Only::Section {
                    continue;
                }
                let m = if let Some((cl, r)) = &p.sweep { mesh_sweep(cl, *r, (p.z0 + p.z1) * 0.5) } else { mesh_prism(p) };
                let color = style.material(&p.material).color3d;
                parts.push(obj(vec![
                    ("src", s(&c.id)),
                    ("part", p.part.as_ref().map(|x| s(x)).unwrap_or(Value::Null)),
                    ("instance", num(inst.index as f64)),
                    ("material", s(&p.material)),
                    ("color", s(&color)),
                    ("positions", arr_nums(&m.positions)),
                    ("normals", arr_nums(&m.normals)),
                    ("indices", Value::Array(m.indices.iter().map(|&i| num(i as f64)).collect())),
                    ("edges", arr_nums(&m.edges)),
                ]));
            }
        }
    }
    obj(vec![("kerf_mesh", s("0.1")), ("parts", Value::Array(parts))])
}
