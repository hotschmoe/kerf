//! 3D mesh (SPEC 11): each prism extruded over [z0, z1] with flat-shaded faces and explicit
//! feature edges (sharp edges + profile outlines).

use crate::geom::{self, Pt, V2};
use crate::json;
use crate::math;
use crate::model::{Kind, Prism};
use crate::scene::{Scene, State};
use crate::style::{Role, Style};
use crate::voids;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::f64::consts::PI;

#[derive(Clone, Debug, Default)]
pub struct Part {
    pub src: String,
    pub part: Option<String>,
    pub instance: u32,
    pub material: String,
    pub color: String,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub edges: Vec<f32>,
}

impl Part {
    fn vert(&mut self, x: f64, y: f64, z: f64, nx: f64, ny: f64, nz: f64) -> u32 {
        let idx = (self.positions.len() / 3) as u32;
        self.positions.extend_from_slice(&[x as f32, y as f32, z as f32]);
        self.normals.extend_from_slice(&[nx as f32, ny as f32, nz as f32]);
        idx
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    fn edge(&mut self, a: [f64; 3], b: [f64; 3]) {
        self.edges.extend_from_slice(&[a[0] as f32, a[1] as f32, a[2] as f32, b[0] as f32, b[1] as f32, b[2] as f32]);
    }

    /// Swaps y and z of everything added from vertex `v0`, triangle index `i0` and edge float `e0` on (a solid built
    /// from an (x, z) outline extruded along "z" becomes one extruded along y); the winding flips with the handedness.
    fn swap_yz(&mut self, v0: usize, i0: usize, e0: usize) {
        for xs in [&mut self.positions[v0 * 3..], &mut self.normals[v0 * 3..], &mut self.edges[e0..]] {
            for c in xs.as_chunks_mut::<3>().0 {
                c.swap(1, 2);
            }
        }
        for t in self.indices[i0..].as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
    }
}

fn arc_step_for(r: f64) -> f64 {
    if r < 1.0 { PI / 6.0 } else { PI * 5.0 / 180.0 }
}

fn flatten_for_mesh(lp: &[Pt]) -> Vec<V2> {
    let mut out = Vec::new();
    for (i, p) in lp.iter().enumerate() {
        let q = lp[(i + 1) % lp.len()];
        out.push(p.v());
        if p.b != 0.0 {
            let arc = geom::arc_of(p.v(), q.v(), p.b);
            let n = geom::steps_for_sweep(arc.sweep, arc_step_for(arc.r));
            for k in 1..n {
                out.push(arc.at(k as f64 / n as f64));
            }
        }
    }
    out
}

pub fn ear_clip(pts: &[V2]) -> Vec<u32> {
    let n = pts.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    let mut idx: Vec<u32> = (0..n as u32).collect();
    let ccw = geom::signed_area_v(pts) >= 0.0;
    let mut remaining = n;
    let mut guard = 0usize;
    let mut i = 0usize;
    while remaining > 3 && guard < n * n + 10 {
        guard += 1;
        let ia = idx[(i + remaining - 1) % remaining];
        let ib = idx[i % remaining];
        let ic = idx[(i + 1) % remaining];
        let (pa, pb, pc) = (pts[ia as usize], pts[ib as usize], pts[ic as usize]);
        let cr = pb.sub(pa).cross(pc.sub(pb));
        let mut ear = if ccw { cr > 1e-12 } else { cr < -1e-12 };
        if ear {
            ear = !idx[..remaining].iter().any(|&iv| {
                let v = pts[iv as usize];
                iv != ia
                    && iv != ib
                    && iv != ic
                    && !(v.eql(pa, 1e-12) || v.eql(pb, 1e-12) || v.eql(pc, 1e-12))
                    && point_in_tri(v, pa, pb, pc)
            });
        }
        if ear {
            out.extend_from_slice(&[ia, ib, ic]);
            idx.remove(i % remaining);
            remaining -= 1;
            if i >= remaining {
                i = 0;
            }
        } else {
            i = (i + 1) % remaining;
        }
    }
    if remaining == 3 {
        out.extend_from_slice(&[idx[0], idx[1], idx[2]]);
    }
    out
}

fn point_in_tri(p: V2, a: V2, b: V2, c: V2) -> bool {
    let d1 = b.sub(a).cross(p.sub(a));
    let d2 = c.sub(b).cross(p.sub(b));
    let d3 = a.sub(c).cross(p.sub(c));
    let neg = d1 < -1e-12 || d2 < -1e-12 || d3 < -1e-12;
    let pos = d1 > 1e-12 || d2 > 1e-12 || d3 > 1e-12;
    !(neg && pos)
}

fn turn_angle(prev: V2, cur: V2, next: V2) -> f64 {
    let u = cur.sub(prev);
    let w = next.sub(cur);
    math::atan2(u.cross(w), u.dot(w)).abs()
}

fn clean_loop(loop_in: &[V2]) -> Vec<V2> {
    let mut lp: Vec<V2> = Vec::new();
    for &p in loop_in {
        if lp.last().is_some_and(|l| l.eql(p, 1e-9)) {
            continue;
        }
        lp.push(p);
    }
    if lp.len() > 1 && lp[0].eql(lp[lp.len() - 1], 1e-9) {
        lp.pop();
    }
    lp
}

/// Two segments cross at a point inside both (touching at an end does not count).
fn crosses(a: V2, b: V2, c: V2, d: V2) -> bool {
    let o1 = b.sub(a).cross(c.sub(a));
    let o2 = b.sub(a).cross(d.sub(a));
    let o3 = d.sub(c).cross(a.sub(c));
    let o4 = d.sub(c).cross(b.sub(c));
    ((o1 > 1e-12 && o2 < -1e-12) || (o1 < -1e-12 && o2 > 1e-12))
        && ((o3 > 1e-12 && o4 < -1e-12) || (o3 < -1e-12 && o4 > 1e-12))
}

/// The outer loop with every hole joined to it by a two-way bridge (rightmost hole vertex to the nearest visible outer
/// vertex), so the ear clipper triangulates a face with holes.
fn bridge_holes(outer: &[V2], holes: &[Vec<V2>]) -> Vec<V2> {
    let mut poly = outer.to_vec();
    let right =
        |h: &[V2]| h.iter().enumerate().fold((0, f64::NEG_INFINITY), |b, (i, p)| if p.x > b.1 { (i, p.x) } else { b });
    let mut order: Vec<usize> = (0..holes.len()).collect();
    crate::sort::stable(&mut order, |&x, &y| right(&holes[x]).1 > right(&holes[y]).1);
    for (n, &hi) in order.iter().enumerate() {
        let h = &holes[hi];
        let mi = right(h).0;
        let m = h[mi];
        let blocked = |p: V2, lp: &[V2]| {
            lp.iter().enumerate().any(|(k, &c)| {
                let d = lp[(k + 1) % lp.len()];
                !(c.eql(p, 1e-9) || d.eql(p, 1e-9) || c.eql(m, 1e-9) || d.eql(m, 1e-9)) && crosses(m, p, c, d)
            })
        };
        let mut cand: Vec<usize> = (0..poly.len()).collect();
        crate::sort::stable(&mut cand, |&x, &y| poly[x].dist(m) < poly[y].dist(m));
        let Some(pi) = cand.into_iter().find(|&k| {
            let p = poly[k];
            !blocked(p, &poly) && !order[n..].iter().any(|&o| blocked(p, &holes[o]))
        }) else {
            continue;
        };
        let mut np = Vec::with_capacity(poly.len() + h.len() + 2);
        np.extend_from_slice(&poly[..=pi]);
        for k in 0..=h.len() {
            np.push(h[(mi + k) % h.len()]);
        }
        np.push(poly[pi]);
        np.extend_from_slice(&poly[pi + 1..]);
        poly = np;
    }
    poly
}

/// A prism over [z0, z1] from its loops (outer first, then holes). `seam` lists, per cap (z0, z1), the outline of the
/// touching slab of the same member where a void split it: cap edges along it are not feature edges.
fn extrude(b: &mut Part, loops_in: &[Vec<V2>], z0: f64, z1: f64, smooth_deg: f64, seam: [&[[V2; 2]]; 2]) {
    let Some(first) = loops_in.first() else { return };
    let lp = clean_loop(first);
    if lp.len() < 3 {
        return;
    }
    let ccw = geom::signed_area_v(&lp) >= 0.0;
    let holes: Vec<Vec<V2>> = loops_in[1..]
        .iter()
        .map(|h| {
            let mut h = clean_loop(h);
            if (geom::signed_area_v(&h) >= 0.0) == ccw {
                h.reverse();
            }
            h
        })
        .filter(|h| h.len() >= 3)
        .collect();
    let cap = if holes.is_empty() { lp.clone() } else { bridge_holes(&lp, &holes) };
    let tris = ear_clip(&cap);
    {
        let base_top = (b.positions.len() / 3) as u32;
        for p in &cap {
            b.vert(p.x, p.y, z1, 0.0, 0.0, 1.0);
        }
        let base_bot = (b.positions.len() / 3) as u32;
        for p in &cap {
            b.vert(p.x, p.y, z0, 0.0, 0.0, -1.0);
        }
        for &[i0, i1, i2] in tris.as_chunks::<3>().0 {
            let (t0, t1, t2) = (base_top + i0, base_top + i1, base_top + i2);
            if ccw {
                b.tri(t0, t1, t2);
            } else {
                b.tri(t0, t2, t1);
            }
            let (b0, b1, b2) = (base_bot + i0, base_bot + i1, base_bot + i2);
            if ccw {
                b.tri(b0, b2, b1);
            } else {
                b.tri(b0, b1, b2);
            }
        }
    }
    walls(b, &lp, ccw, z0, z1, smooth_deg, seam);
    for h in &holes {
        walls(b, h, ccw, z0, z1, smooth_deg, seam);
    }
}

fn walls(b: &mut Part, lp: &[V2], ccw: bool, z0: f64, z1: f64, smooth_deg: f64, seam: [&[[V2; 2]]; 2]) {
    let n = lp.len();
    let smooth_rad = smooth_deg * (PI / 180.0);
    for (i, &p) in lp.iter().enumerate() {
        let q = lp[(i + 1) % n];
        let prev = lp[(i + n - 1) % n];
        let next = lp[(i + 2) % n];
        let d = q.sub(p);
        let l = d.len();
        if l < 1e-12 {
            continue;
        }
        let nrm = if ccw { V2::init(d.y / l, -d.x / l) } else { V2::init(-d.y / l, d.x / l) };
        let mut n0 = nrm;
        let mut n1 = nrm;
        if turn_angle(prev, p, q) < smooth_rad {
            let dp = p.sub(prev).norm();
            let np = if ccw { V2::init(dp.y, -dp.x) } else { V2::init(-dp.y, dp.x) };
            n0 = np.add(nrm).norm();
        }
        if turn_angle(p, q, next) < smooth_rad {
            let dq = next.sub(q).norm();
            let nq = if ccw { V2::init(dq.y, -dq.x) } else { V2::init(-dq.y, dq.x) };
            n1 = nq.add(nrm).norm();
        }
        let v00 = b.vert(p.x, p.y, z0, n0.x, n0.y, 0.0);
        let v10 = b.vert(q.x, q.y, z0, n1.x, n1.y, 0.0);
        let v11 = b.vert(q.x, q.y, z1, n1.x, n1.y, 0.0);
        let v01 = b.vert(p.x, p.y, z1, n0.x, n0.y, 0.0);
        if ccw {
            b.tri(v00, v10, v11);
            b.tri(v00, v11, v01);
        } else {
            b.tri(v00, v11, v10);
            b.tri(v00, v01, v11);
        }
        for (k, z) in [z0, z1].into_iter().enumerate() {
            if seam[k].is_empty() {
                b.edge([p.x, p.y, z], [q.x, q.y, z]);
                continue;
            }
            for [t0, t1] in geom::uncovered(p, q, seam[k]) {
                let (a, c) = (p.lerp(q, t0), p.lerp(q, t1));
                b.edge([a.x, a.y, z], [c.x, c.y, z]);
            }
        }
        if turn_angle(prev, p, q) >= 20.0 * (PI / 180.0) {
            b.edge([p.x, p.y, z0], [p.x, p.y, z1]);
        }
    }
}

fn tube(b: &mut Part, center: &[V2], r: f64, zc: f64) {
    let m = center.len();
    if m < 2 {
        return;
    }
    const SEG: usize = 12;
    let mut rings: Vec<u32> = Vec::new();
    for (i, &p) in center.iter().enumerate() {
        let t = if i == 0 {
            center[1].sub(p)
        } else if i + 1 == m {
            p.sub(center[i - 1])
        } else {
            center[i + 1].sub(center[i - 1])
        };
        let nrm = t.norm().perp();
        for k in 0..SEG {
            let phi = 2.0 * PI * k as f64 / SEG as f64;
            let dir = nrm.scale(math::cos(phi));
            let nz = math::sin(phi);
            rings.push(b.vert(p.x + r * dir.x, p.y + r * dir.y, zc + r * nz, dir.x, dir.y, nz));
        }
    }
    for i in 0..m - 1 {
        for k in 0..SEG {
            let k2 = (k + 1) % SEG;
            let a0 = rings[i * SEG + k];
            let a1 = rings[i * SEG + k2];
            let b0 = rings[(i + 1) * SEG + k];
            let b1 = rings[(i + 1) * SEG + k2];
            b.tri(a0, b0, b1);
            b.tri(a0, b1, a1);
        }
    }
    let pos = |b: &Part, ring_vert: u32| -> [f64; 3] {
        let o = ring_vert as usize * 3;
        [f64::from(b.positions[o]), f64::from(b.positions[o + 1]), f64::from(b.positions[o + 2])]
    };
    for ri in [0, m - 1] {
        for k in 0..SEG {
            let p0 = pos(b, rings[ri * SEG + k]);
            let p1 = pos(b, rings[ri * SEG + (k + 1) % SEG]);
            b.edge(p0, p1);
        }
    }
    for q in (0..SEG).step_by(SEG / 4) {
        for j in 0..m - 1 {
            let p0 = pos(b, rings[j * SEG + q]);
            let p1 = pos(b, rings[(j + 1) * SEG + q]);
            b.edge(p0, p1);
        }
    }
}

fn hex_color(st: &Style, material: &str) -> String {
    st.material(material).map_or_else(|| "#A0A0A0".to_string(), |m| m.color3d.clone())
}

#[derive(Clone, Copy, Debug)]
pub struct ZSeg {
    pub z0: f64,
    pub z1: f64,
    pub mortar: bool,
}

pub const CMU_UNIT_LEN: f64 = 15.625;
pub const CMU_JOINT: f64 = 0.375;

pub fn cmu_split(pr: &Prism) -> Vec<ZSeg> {
    let mut out = Vec::new();
    let offset = if pr.course.is_multiple_of(2) { 8.0 } else { 0.0 };
    let mut z = pr.bond_z0.unwrap_or(pr.z0) - offset;
    while z < pr.z1 {
        let a0 = z.max(pr.z0);
        let a1 = (z + CMU_UNIT_LEN).min(pr.z1);
        if a1 - a0 > 1e-6 {
            out.push(ZSeg { z0: a0, z1: a1, mortar: false });
        }
        let j0 = (z + CMU_UNIT_LEN).max(pr.z0);
        let j1 = (z + CMU_UNIT_LEN + CMU_JOINT).min(pr.z1);
        if j1 - j0 > 1e-6 {
            out.push(ZSeg { z0: j0, z1: j1, mortar: true });
        }
        z += CMU_UNIT_LEN + CMU_JOINT;
    }
    out
}

pub fn build(scene: &Scene<'_>, include_fills: bool) -> Vec<Part> {
    let mut parts = Vec::new();
    let st = scene.style;
    for c in &scene.comps {
        if c.state != State::Ok || !c.visible {
            continue;
        }
        for pr in &c.world {
            if (pr.kind == Kind::Ghost && !pr.dashed) || pr.role == Role::Void {
                continue;
            }
            if !include_fills && pr.role == Role::Soil {
                continue;
            }
            let mut part = Part {
                src: c.id.clone(),
                part: (!pr.part.is_empty()).then(|| pr.part.clone()),
                instance: pr.instance,
                material: pr.material.clone(),
                color: hex_color(st, &pr.material),
                ..Part::default()
            };
            if pr.kind == Kind::Line && pr.line_pts.is_empty() {
                continue;
            }
            if pr.centerline.len() >= 2 && pr.sweep_r > 0.0 && !pr.loops.is_empty() && pr.zhalf == 0.0 {
                let cl = geom::flatten_polyline(&pr.centerline, false, 0.02);
                tube(&mut part, &cl, pr.sweep_r, (pr.z0 + pr.z1) / 2.0);
                parts.push(part);
                continue;
            }
            if pr.fp_proxy {
                if pr.footprint.len() >= 3 {
                    extrude(&mut part, core::slice::from_ref(&pr.footprint), pr.fp_y[0], pr.fp_y[1], 20.0, [&[], &[]]);
                    part.swap_yz(0, 0, 0);
                    parts.push(part);
                }
                continue;
            }
            if pr.loops.is_empty() {
                continue;
            }
            let lp: Vec<Vec<V2>> = pr.loops.iter().map(|l| flatten_for_mesh(l)).collect();
            let seams = [false, true].map(|hi| {
                voids::seam_neighbors(&c.world, pr, hi, |q| q.loops.iter().map(|l| flatten_for_mesh(l)).collect())
            });
            let seam_at = |a: f64, b: f64| -> [&[[V2; 2]]; 2] {
                [
                    if (a - pr.z0).abs() < 1e-9 { &seams[0] } else { &[] },
                    if (b - pr.z1).abs() < 1e-9 { &seams[1] } else { &[] },
                ]
            };
            if pr.cmu_unit && pr.role != Role::Grout {
                let offset = if pr.course % 2 == 0 { 8.0 } else { 0.0 };
                let mut z = pr.bond_z0.unwrap_or(pr.z0) - offset;
                let mut joints: Vec<[f64; 2]> = Vec::new();
                while z < pr.z1 {
                    let uu0 = z.max(pr.z0);
                    let uu1 = (z + CMU_UNIT_LEN).min(pr.z1);
                    if uu1 - uu0 > 1e-6 {
                        extrude(&mut part, &lp, uu0, uu1, 20.0, seam_at(uu0, uu1));
                    }
                    let j0 = z + CMU_UNIT_LEN;
                    let j1 = (z + CMU_UNIT_LEN + CMU_JOINT).min(pr.z1);
                    if j1 - j0.max(pr.z0) > 1e-6 && j0 < pr.z1 {
                        joints.push([j0.max(pr.z0), j1]);
                    }
                    z += CMU_UNIT_LEN + CMU_JOINT;
                }
                parts.push(part);
                if !joints.is_empty() {
                    let mut mortar = Part {
                        src: c.id.clone(),
                        part: Some(pr.part.clone()),
                        instance: pr.instance,
                        material: "mortar".to_string(),
                        color: hex_color(st, "mortar"),
                        ..Part::default()
                    };
                    for j in &joints {
                        extrude(&mut mortar, &lp, j[0], j[1], 20.0, seam_at(j[0], j[1]));
                    }
                    parts.push(mortar);
                }
                continue;
            }
            extrude(&mut part, &lp, pr.z0, pr.z1, 20.0, seam_at(pr.z0, pr.z1));
            parts.push(part);
        }
    }
    parts
}

pub fn to_json(parts: &[Part]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"{\"kerf_mesh\":\"0.1\",\"parts\":[\n");
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",\n");
        }
        out.extend_from_slice(b"{\"src\":");
        json::write_string(&mut out, &p.src);
        out.extend_from_slice(b",\"part\":");
        match &p.part {
            Some(x) => json::write_string(&mut out, x),
            None => out.extend_from_slice(b"null"),
        }
        out.extend_from_slice(format!(",\"instance\":{},\"material\":", p.instance).as_bytes());
        json::write_string(&mut out, &p.material);
        out.extend_from_slice(b",\"color\":");
        json::write_string(&mut out, &p.color);
        out.extend_from_slice(b",\"positions\":");
        floats(&mut out, &p.positions);
        out.extend_from_slice(b",\"normals\":");
        floats(&mut out, &p.normals);
        out.extend_from_slice(b",\"indices\":[");
        for (k, x) in p.indices.iter().enumerate() {
            if k > 0 {
                out.push(b',');
            }
            out.extend_from_slice(format!("{x}").as_bytes());
        }
        out.extend_from_slice(b"],\"edges\":");
        floats(&mut out, &p.edges);
        out.push(b'}');
    }
    out.extend_from_slice(b"\n]}\n");
    out
}

fn floats(out: &mut Vec<u8>, xs: &[f32]) {
    out.push(b'[');
    for (i, &x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        json::push_number(out, f64::from(x));
    }
    out.push(b']');
}
