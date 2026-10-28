//! Iso view with hidden-line removal on planar faces (SPEC 8.2).
//!
//! Projection (isometric drawing, axes at true length): for camera quadrant (sx, sz)
//!   u = (sz*x - sx*z) * cos30,   v = y - (sx*x + sz*z) * sin30,   depth = sx*x + y + sz*z
//! (larger depth = nearer the viewer). Prisms are tessellated into planar faces (two caps and one
//! side face per profile segment); edges are tested against front-facing faces on a uniform grid.

use crate::clip;
use crate::compile;
use crate::drawing::{HatchItem, Item, PathItem, RegionItem};
use crate::geom::{self, Box, Pt, V2};
use crate::hatch;
use crate::math;
use crate::mesh;
use crate::model::{Kind, OutlineMode};
use crate::num;
use crate::pen::Pen;
use crate::scene::{Comp, Scene, State};
use crate::shape::{self, Shape};
use crate::sort;
use crate::style::{Role, Style};
use crate::view::{self, ViewSpec};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const C30: f64 = 0.8660254037844386;
const S30: f64 = 0.5;

fn proj(sx: f64, sz: f64, p: [f64; 3]) -> V2 {
    V2::init((sz * p[0] - sx * p[2]) * C30, p[1] - (sx * p[0] + sz * p[2]) * S30)
}

fn depth_of(sx: f64, sz: f64, p: [f64; 3]) -> f64 {
    sx * p[0] + p[1] + sz * p[2]
}

struct Face {
    n: [f64; 3],
    p0: [f64; 3],
    outer: Vec<V2>,
    holes: Vec<Vec<V2>>,
    bbox: Box,
    prism: usize,
    front: bool,
    nd: f64,
}

struct Edge {
    a: [f64; 3],
    b: [f64; 3],
    pen: Pen,
    prism: usize,
    f1: usize,
    f2: usize,
    chain: usize,
}

struct IsoPrism {
    src: String,
    instance: u32,
    comp: u32,
    part: String,
    loops: Vec<Vec<V2>>,
    z0: f64,
    z1: f64,
    cap_cut: bool,
    pen: Option<Pen>,
    material: String,
    embedded: bool,
    is_fill: bool,
    outline: OutlineMode,
}

/// A visible face: outer loop and holes in paper-projected model units.
#[derive(Clone, Debug)]
pub struct OwnedShape {
    pub outer: Vec<V2>,
    pub holes: Vec<Vec<V2>>,
}

impl OwnedShape {
    pub fn as_shape(&self) -> Shape<'_> {
        Shape { outer: &self.outer, holes: self.holes.iter().map(|h| h.as_slice()).collect() }
    }
}

#[derive(Clone, Debug)]
pub struct Vis {
    pub comp: u32,
    pub part: String,
    pub instance: u32,
    pub shapes: Vec<OwnedShape>,
    pub src: String,
    pub cut: bool,
}

pub struct Iso {
    pub sx: f64,
    pub sz: f64,
    pub cut_z: f64,
    faces: Vec<Face>,
    edges: Vec<Edge>,
    prisms: Vec<IsoPrism>,
    pub vis: Vec<Vis>,
    gx0: f64,
    gy0: f64,
    gw: f64,
    gh: f64,
    gn: usize,
    cells: Vec<Vec<u32>>,
    eps: f64,
}

impl Default for Iso {
    fn default() -> Iso {
        Iso {
            sx: 1.0,
            sz: 1.0,
            cut_z: 0.0,
            faces: Vec::new(),
            edges: Vec::new(),
            prisms: Vec::new(),
            vis: Vec::new(),
            gx0: 0.0,
            gy0: 0.0,
            gw: 1.0,
            gh: 1.0,
            gn: 0,
            cells: Vec::new(),
            eps: 1e-3,
        }
    }
}

impl Iso {
    /// Label point of the visible faces of `comp` (None: nothing of it is visible).
    pub fn landing(&self, comp: &Comp, inst: Option<u32>, part: Option<&str>) -> Option<V2> {
        let mut shapes: Vec<Shape> = Vec::new();
        for v in &self.vis {
            if v.comp != comp.index {
                continue;
            }
            if inst.is_some_and(|k| v.instance != k) {
                continue;
            }
            if part.is_some_and(|p| v.part != p) {
                continue;
            }
            shapes.extend(v.shapes.iter().map(OwnedShape::as_shape));
        }
        shape::label_point(&shapes)
    }

    /// Non-drawn picking regions: the visible face chosen per prism (cut cap first).
    pub fn region_items(&self) -> Vec<Item> {
        let mut out = Vec::new();
        for v in &self.vis {
            for sh in &v.shapes {
                let lift = |l: &[V2]| l.iter().map(|&q| Pt::at(q, 0.0)).collect::<Vec<Pt>>();
                let mut loops = vec![lift(&sh.outer)];
                loops.extend(sh.holes.iter().map(|h| lift(h)));
                out.push(Item::Region(RegionItem {
                    src: v.src.clone(),
                    part: if v.part.is_empty() { None } else { Some(v.part.clone()) },
                    instance: v.instance,
                    cut: v.cut,
                    loops,
                    layer: String::new(),
                }));
            }
        }
        out
    }

    pub fn project(&self, p: V2) -> Option<V2> {
        Some(proj(self.sx, self.sz, [p.x, p.y, self.cut_z]))
    }

    fn cell_range(&self, b: Box) -> [usize; 4] {
        let gn = self.gn;
        let cl = |v: f64, o: f64, w: f64| num::to_int_clamped::<usize>(math::floor((v - o) / w), 0, gn.saturating_sub(1));
        [cl(b.x0, self.gx0, self.gw), cl(b.x1, self.gx0, self.gw), cl(b.y0, self.gy0, self.gh), cl(b.y1, self.gy0, self.gh)]
    }

    fn build_grid(&mut self) {
        let mut b = Box::default();
        for f in &self.faces {
            b.add_box(f.bbox);
        }
        if b.is_empty() {
            return;
        }
        self.gn = 24;
        self.gx0 = b.x0;
        self.gy0 = b.y0;
        self.gw = (b.width() / 24.0).max(1e-6);
        self.gh = (b.height() / 24.0).max(1e-6);
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); 24 * 24];
        for (i, f) in self.faces.iter().enumerate() {
            if !f.front || f.nd.abs() < 1e-9 {
                continue;
            }
            let r = self.cell_range(f.bbox);
            for row in r[2]..=r[3] {
                for col in r[0]..=r[1] {
                    lists[row * 24 + col].push(i as u32);
                }
            }
        }
        self.cells = lists;
    }

    fn candidates(&self, b: Box, out: &mut Vec<u32>) {
        out.clear();
        if self.cells.is_empty() {
            return;
        }
        let r = self.cell_range(b);
        for row in r[2]..=r[3] {
            for col in r[0]..=r[1] {
                if let Some(c) = self.cells.get(row * self.gn + col) {
                    out.extend_from_slice(c);
                }
            }
        }
        sort::stable(out, |a, b| a < b);
        out.dedup();
    }

    fn face_depth(&self, f: &Face, m: V2) -> f64 {
        let (sx, sz) = (self.sx, self.sz);
        let uu = m.x / C30;
        let vv = -m.y / S30;
        let x = (sz * uu + sx * vv) * 0.5;
        let z = (-sx * uu + sz * vv) * 0.5;
        let pb = [x, 0.0, z];
        let d = [sx, 1.0, sz];
        let t = (f.n[0] * (f.p0[0] - pb[0]) + f.n[1] * (f.p0[1] - pb[1]) + f.n[2] * (f.p0[2] - pb[2])) / f.nd;
        let p = [pb[0] + t * d[0], pb[1] + t * d[1], pb[2] + t * d[2]];
        depth_of(sx, sz, p)
    }

    fn occluded_at(&self, m: V2, dm: f64, skip: &[usize], cand: &[u32]) -> bool {
        cand.iter().any(|&fi| {
            let fi = fi as usize;
            if skip.contains(&fi) {
                return false;
            }
            let Some(f) = self.faces.get(fi) else { return false };
            face_contains(f, m) && self.face_depth(f, m) > dm + self.eps
        })
    }

    /// Visible parameter intervals of the screen segment a->b whose depth runs da -> db.
    fn visible(&self, a: V2, b: V2, da: f64, db: f64, skip: &[usize]) -> Vec<[f64; 2]> {
        let mut bb = Box::default();
        bb.add_point(a.x, a.y);
        bb.add_point(b.x, b.y);
        let bb = bb.expand(1e-9);
        let mut cand = Vec::new();
        self.candidates(bb, &mut cand);
        let keep: Vec<u32> = cand
            .into_iter()
            .filter(|&fi| !skip.contains(&(fi as usize)) && self.faces.get(fi as usize).is_some_and(|f| f.bbox.overlaps(bb, 0.0)))
            .collect();
        let mut out: Vec<[f64; 2]> = Vec::new();
        if keep.is_empty() {
            out.push([0.0, 1.0]);
            return out;
        }
        let mut ts = vec![0.0, 1.0];
        for &fi in &keep {
            let Some(f) = self.faces.get(fi as usize) else { continue };
            add_contour_cuts(&mut ts, a, b, &f.outer);
            for h in &f.holes {
                add_contour_cuts(&mut ts, a, b, h);
            }
        }
        sort::stable(&mut ts, |x, y| x < y);
        for w in ts.windows(2) {
            let (t0, t1) = (w[0], w[1]);
            if t1 - t0 < 1e-9 {
                continue;
            }
            let tm = (t0 + t1) * 0.5;
            let m = V2::lerp(a, b, tm);
            let dm = da + (db - da) * tm;
            if self.occluded_at(m, dm, skip, &keep) {
                continue;
            }
            match out.last_mut() {
                Some(last) if (last[1] - t0).abs() < 1e-9 => last[1] = t1,
                _ => out.push([t0, t1]),
            }
        }
        out
    }
}

fn face_contains(f: &Face, m: V2) -> bool {
    f.bbox.contains(m) && geom::point_in_loop_eo(m, &f.outer) && !f.holes.iter().any(|h| geom::point_in_loop_eo(m, h))
}

fn add_contour_cuts(ts: &mut Vec<f64>, p: V2, q: V2, contour: &[V2]) {
    for (i, &c0) in contour.iter().enumerate() {
        let c1 = contour[(i + 1) % contour.len()];
        let mut ta = [0.0; 2];
        let mut tb = [0.0; 2];
        let n = geom::seg_seg(p, q, c0, c1, &mut ta, &mut tb);
        for &t in ta.iter().take(n) {
            if t > 1e-9 && t < 1.0 - 1e-9 {
                ts.push(t);
            }
        }
    }
}

// ---- tessellation ---------------------------------------------------------------------------------------

struct FLoop {
    pts: Vec<V2>,
    smooth: Vec<bool>,
}

fn flatten_sharp(lp: &[V2]) -> FLoop {
    let mut pts: Vec<V2> = Vec::new();
    for &p in lp {
        pts.push(p);
    }
    let n = pts.len();
    let mut sm = Vec::with_capacity(n);
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        let u = cur.sub(prev);
        let w = next.sub(cur);
        let ang = math::atan2(u.cross(w), u.dot(w)).abs();
        sm.push(ang < 20.0 * geom::RAD_PER_DEG);
    }
    FLoop { pts, smooth: sm }
}

fn quadrant(from: view::From) -> [f64; 2] {
    match from {
        view::From::FrontRight => [1.0, 1.0],
        view::From::FrontLeft => [-1.0, 1.0],
        view::From::BackRight => [1.0, -1.0],
        view::From::BackLeft => [-1.0, -1.0],
    }
}

fn src_of(c: &Comp, instance: u32) -> String {
    if c.arr_count > 1 {
        let per = (c.xfs.len() / c.arr_count).max(1) as u32;
        return format!("{}#{}", c.id, instance / per);
    }
    c.id.clone()
}

fn gather(scene: &Scene, spec: &ViewSpec, crop: Option<Box>, out: &mut Vec<IsoPrism>) {
    let crop_loop: Option<Vec<V2>> = crop.map(|c| vec![V2::init(c.x0, c.y0), V2::init(c.x1, c.y0), V2::init(c.x1, c.y1), V2::init(c.x0, c.y1)]);
    for c in &scene.comps {
        if c.state != State::Ok || !c.visible {
            continue;
        }
        if compile::is_omitted(&spec.omit, &c.id) {
            continue;
        }
        for p in &c.world {
            if p.kind == Kind::Ghost || p.role == Role::Void {
                continue;
            }
            let fill_mat = p.role == Role::Soil;
            if fill_mat && !spec.cutaway {
                continue;
            }
            let segs = if p.cmu_unit && p.role != Role::Grout {
                mesh::cmu_split(p)
            } else {
                vec![mesh::ZSeg { z0: p.z0, z1: p.z1, mortar: false }]
            };
            for sg in segs {
                let mat_name = if sg.mortar { "mortar" } else { p.material.as_str() };
                let z0 = sg.z0;
                let mut z1 = sg.z1;
                let mut cap = false;
                if spec.cutaway {
                    if z0 >= spec.cut_z {
                        continue;
                    }
                    if z1 > spec.cut_z {
                        z1 = spec.cut_z;
                        cap = true;
                    }
                }
                if z1 - z0 < 1e-9 {
                    continue;
                }
                if fill_mat && !cap {
                    continue;
                }
                let mut region = clip::from_region(&p.loops, 0.003);
                if let Some(cl) = &crop_loop {
                    region = clip::boolean(&region, core::slice::from_ref(cl), clip::Op::Intersect);
                }
                for (oi, outer) in region.iter().enumerate() {
                    if geom::signed_area_v(outer) <= 0.0 {
                        continue;
                    }
                    let mut loops: Vec<Vec<V2>> = vec![outer.clone()];
                    for h in &region {
                        if geom::signed_area_v(h) < 0.0 && h.first().is_some_and(|&h0| geom::point_in_loop_eo(h0, &region[oi])) {
                            loops.push(h.clone());
                        }
                    }
                    out.push(IsoPrism {
                        src: src_of(c, p.instance),
                        instance: p.instance,
                        comp: c.index,
                        part: p.part.clone(),
                        loops,
                        z0,
                        z1,
                        cap_cut: cap,
                        pen: if p.kind == Kind::Ghost { p.pen } else { None },
                        material: mat_name.into(),
                        embedded: p.embedded,
                        is_fill: fill_mat,
                        outline: p.outline,
                    });
                }
            }
        }
    }
}

fn mk_face(iso: &mut Iso, n: [f64; 3], verts: &[[f64; 3]], holes3: &[Vec<[f64; 3]>], pi: usize) -> usize {
    let mut bb = Box::default();
    let outer: Vec<V2> = verts
        .iter()
        .map(|&v| {
            let q = proj(iso.sx, iso.sz, v);
            bb.add_point(q.x, q.y);
            q
        })
        .collect();
    let holes = holes3.iter().map(|h| h.iter().map(|&v| proj(iso.sx, iso.sz, v)).collect()).collect();
    let nd = n[0] * iso.sx + n[1] + n[2] * iso.sz;
    iso.faces.push(Face { n, p0: verts.first().copied().unwrap_or([0.0; 3]), outer, holes, bbox: bb, prism: pi, front: nd > 1e-9, nd });
    iso.faces.len() - 1
}

fn add_prism(iso: &mut Iso, pi: usize) {
    let Some(ip) = iso.prisms.get(pi) else { return };
    let (z0, z1, cap_cut, ip_pen) = (ip.z0, ip.z1, ip.cap_cut, ip.pen);
    let loops: Vec<FLoop> = ip.loops.iter().map(|l| flatten_sharp(l)).collect();
    let Some(first) = loops.first() else { return };
    let lift = |pts: &[V2], z: f64| -> Vec<[f64; 3]> { pts.iter().map(|p| [p.x, p.y, z]).collect() };
    let top_v = lift(&first.pts, z1);
    let bot_v = lift(&first.pts, z0);
    let top_h: Vec<Vec<[f64; 3]>> = loops[1..].iter().map(|fl| lift(&fl.pts, z1)).collect();
    let bot_h: Vec<Vec<[f64; 3]>> = loops[1..].iter().map(|fl| lift(&fl.pts, z0)).collect();
    let ftop = mk_face(iso, [0.0, 0.0, 1.0], &top_v, &top_h, pi);
    let fbot = mk_face(iso, [0.0, 0.0, -1.0], &bot_v, &bot_h, pi);
    for (li, fl) in loops.iter().enumerate() {
        let n = fl.pts.len();
        let mut side_faces = Vec::with_capacity(n);
        for i in 0..n {
            let p = fl.pts[i];
            let q = fl.pts[(i + 1) % n];
            let d = q.sub(p).norm();
            let nn = [d.y, -d.x, 0.0];
            let verts = [[p.x, p.y, z0], [q.x, q.y, z0], [q.x, q.y, z1], [p.x, p.y, z1]];
            side_faces.push(mk_face(iso, nn, &verts, &[], pi));
        }
        let chain = pi * 16 + li;
        for i in 0..n {
            let p = fl.pts[i];
            let q = fl.pts[(i + 1) % n];
            let sf = side_faces[i];
            let top_pen = pen_of(iso, ip_pen, ftop, sf, cap_cut);
            iso.edges.push(Edge { a: [p.x, p.y, z1], b: [q.x, q.y, z1], pen: top_pen, prism: pi, f1: ftop, f2: sf, chain });
            let bot_pen = pen_of(iso, ip_pen, fbot, sf, false);
            iso.edges.push(Edge { a: [p.x, p.y, z0], b: [q.x, q.y, z0], pen: bot_pen, prism: pi, f1: fbot, f2: sf, chain: chain + 8 });
        }
        for i in 0..n {
            let p = fl.pts[i];
            let prev = side_faces[(i + n - 1) % n];
            let cur = side_faces[i];
            let fp = iso.faces[prev].front;
            let fc = iso.faces[cur].front;
            if fl.smooth[i] && fp == fc {
                continue;
            }
            let pen = ip_pen.unwrap_or(if fp != fc { Pen::Profile } else { Pen::Beyond });
            let chain = usize::MAX.saturating_sub(pi * 4096 + li * 1024 + i);
            iso.edges.push(Edge { a: [p.x, p.y, z0], b: [p.x, p.y, z1], pen, prism: pi, f1: prev, f2: cur, chain });
        }
    }
}

fn pen_of(iso: &Iso, pen: Option<Pen>, f1: usize, f2: usize, cut_cap_edge: bool) -> Pen {
    if let Some(p) = pen {
        return p;
    }
    if cut_cap_edge {
        return Pen::Cut;
    }
    if iso.faces[f1].front != iso.faces[f2].front { Pen::Profile } else { Pen::Beyond }
}

pub struct Result {
    pub items: Vec<Item>,
    pub scale: f64,
    pub crop: Box,
}

type EdgeKey = [i64; 6];

fn edge_key(ea: [f64; 3], eb: [f64; 3]) -> EdgeKey {
    let r = |x: f64| num::to_int_clamped::<i64>(math::round(x * 2000.0), i64::MIN, i64::MAX);
    let ka = [r(ea[0]), r(ea[1]), r(ea[2])];
    let kb = [r(eb[0]), r(eb[1]), r(eb[2])];
    let swap = ka[0] > kb[0] || (ka[0] == kb[0] && (ka[1] > kb[1] || (ka[1] == kb[1] && ka[2] > kb[2])));
    if swap { [kb[0], kb[1], kb[2], ka[0], ka[1], ka[2]] } else { [ka[0], ka[1], ka[2], kb[0], kb[1], kb[2]] }
}

/// Build the iso view. `iso` is initialised here and stays alive for annotation landing/projection.
pub fn build(iso: &mut Iso, scene: &Scene, spec: &ViewSpec, style: &Style, scale_override: f64) -> Result {
    let q = quadrant(spec.from);
    iso.sx = q[0];
    iso.sz = q[1];
    iso.cut_z = spec.cut_z;
    let mut crop = if spec.has_crop { Some(spec.crop) } else { None };
    if crop.is_none() {
        let mut b = Box::default();
        for c in &scene.comps {
            if c.state == State::Ok && c.visible {
                for p in &c.world {
                    for l in &p.loops {
                        b.add_box(geom::loop_box(l));
                    }
                }
            }
        }
        if !b.is_empty() {
            crop = Some(b);
        }
    }
    let mut prisms = Vec::new();
    gather(scene, spec, crop, &mut prisms);
    iso.prisms = prisms;
    for i in 0..iso.prisms.len() {
        if !iso.prisms[i].is_fill {
            add_prism(iso, i);
        }
    }
    let mut sb = Box::default();
    for f in &iso.faces {
        sb.add_box(f.bbox);
    }
    if sb.is_empty() {
        return Result { items: Vec::new(), scale: 12.0, crop: Box { x0: 0.0, y0: 0.0, x1: 12.0, y1: 12.0 } };
    }
    iso.eps = 1e-4 * sb.width().max(sb.height()).max(1.0);
    iso.build_grid();
    let s = if scale_override > 0.0 {
        scale_override
    } else if spec.scale > 0.0 {
        spec.scale
    } else {
        (math::ceil(sb.width().max(sb.height()) / 5.5 * 2.0) / 2.0).max(0.5)
    };
    let (sx, sz) = (iso.sx, iso.sz);

    // dedupe identical 3D edges (heavier pen wins)
    let ne = iso.edges.len();
    let mut keep = vec![true; ne];
    {
        let mut ents: Vec<(EdgeKey, usize)> = iso.edges.iter().enumerate().map(|(i, e)| (edge_key(e.a, e.b), i)).collect();
        sort::stable(&mut ents, |x, y| x.0 < y.0 || (x.0 == y.0 && x.1 < y.1));
        let mut i = 0;
        while i < ne {
            let mut j = i + 1;
            while j < ne && ents[i].0 == ents[j].0 {
                j += 1;
            }
            let mut best = ents[i].1;
            for en in &ents[i + 1..j] {
                if style.pen_width_mm(iso.edges[en.1].pen) > style.pen_width_mm(iso.edges[best].pen) {
                    best = en.1;
                }
            }
            for en in &ents[i..j] {
                if en.1 != best {
                    keep[en.1] = false;
                }
            }
            i = j;
        }
    }

    struct Piece {
        edge: usize,
        segs: Vec<[V2; 2]>,
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for (ei, e) in iso.edges.iter().enumerate() {
        if !keep[ei] {
            continue;
        }
        if !iso.faces[e.f1].front && !iso.faces[e.f2].front {
            continue;
        }
        let pa = proj(sx, sz, e.a);
        let pb = proj(sx, sz, e.b);
        if pa.dist(pb) < 1e-9 {
            continue;
        }
        let vis = iso.visible(pa, pb, depth_of(sx, sz, e.a), depth_of(sx, sz, e.b), &[e.f1, e.f2]);
        if vis.is_empty() {
            continue;
        }
        let segs = vis.iter().map(|v| [V2::lerp(pa, pb, v[0]), V2::lerp(pa, pb, v[1])]).collect();
        pieces.push(Piece { edge: ei, segs });
    }

    let mut items: Vec<Item> = Vec::new();
    // hatch on cutaway caps (front-facing only)
    if spec.cutaway && sz > 0.0 {
        let mut cand: Vec<u32> = Vec::new();
        for ip in &iso.prisms {
            if !ip.cap_cut || ip.embedded {
                continue;
            }
            let Some(mat) = style.material(&ip.material) else { continue };
            for hs in &mat.hatch {
                let Some(pat) = style.pattern(&hs.pattern) else { continue };
                let res = hatch::generate(&ip.loops, pat, s * hs.scale, hs.angle);
                let mut out_lines: Vec<[f64; 4]> = Vec::new();
                for l in &res.lines {
                    let pa3 = [l[0], l[1], ip.z1];
                    let pb3 = [l[2], l[3], ip.z1];
                    let pa = proj(sx, sz, pa3);
                    let pb = proj(sx, sz, pb3);
                    let da = depth_of(sx, sz, pa3);
                    let db = depth_of(sx, sz, pb3);
                    if V2::eql(pa, pb, 1e-12) {
                        let mut bb = Box::default();
                        bb.add_point(pa.x, pa.y);
                        iso.candidates(bb.expand(1e-9), &mut cand);
                        if !iso.occluded_at(pa, da, &[], &cand) {
                            out_lines.push([pa.x, pa.y, pa.x, pa.y]);
                        }
                        continue;
                    }
                    for v in iso.visible(pa, pb, da, db, &[]) {
                        let p0 = V2::lerp(pa, pb, v[0]);
                        let p1 = V2::lerp(pa, pb, v[1]);
                        out_lines.push([p0.x, p0.y, p1.x, p1.y]);
                    }
                }
                let loops = ip.loops.iter().map(|l| l.iter().map(|v| Pt::at(proj(sx, sz, [v.x, v.y, ip.z1]), 0.0)).collect()).collect();
                items.push(Item::Hatch(HatchItem {
                    layer: style.layer_for_pen(Pen::Hatch).into(),
                    pen: Pen::Hatch,
                    src: ip.src.clone(),
                    pattern: hs.pattern.clone(),
                    scale: hs.scale,
                    angle: hs.angle,
                    loops,
                    lines: out_lines,
                }));
            }
            if ip.is_fill && ip.outline != OutlineMode::None {
                fill_outline(&mut items, ip, style, sx, sz);
            }
        }
    }

    // chain visible pieces into paths, lighter pens first
    let mut pens: Vec<Pen> = Vec::new();
    for pc in &pieces {
        let pen = iso.edges[pc.edge].pen;
        if !pens.contains(&pen) {
            pens.push(pen);
        }
    }
    sort::stable(&mut pens, |&x, &y| {
        let (wx, wy) = (style.pen_width_mm(x), style.pen_width_mm(y));
        if wx != wy { wx < wy } else { x.name() < y.name() }
    });
    for &pen in &pens {
        let layer = style.layer_for_pen(pen);
        let mut cur: Vec<Pt> = Vec::new();
        let mut cur_src: &str = "";
        let mut cur_chain = usize::MAX;
        let flush = |cur: &mut Vec<Pt>, src: &str, items: &mut Vec<Item>| {
            if cur.len() >= 2 {
                items.push(Item::Path(PathItem { layer: layer.into(), pen, src: src.into(), closed: false, pts: core::mem::take(cur) }));
            }
            cur.clear();
        };
        for pc in &pieces {
            let e = &iso.edges[pc.edge];
            if e.pen != pen {
                continue;
            }
            let src = iso.prisms[e.prism].src.as_str();
            for sg in &pc.segs {
                let continues = cur_chain == e.chain && cur_src == src && cur.last().is_some_and(|l| V2::eql(l.v(), sg[0], 1e-7));
                if !continues {
                    flush(&mut cur, cur_src, &mut items);
                    cur.push(Pt::at(sg[0], 0.0));
                }
                cur.push(Pt::at(sg[1], 0.0));
                cur_src = src;
                cur_chain = e.chain;
            }
        }
        flush(&mut cur, cur_src, &mut items);
    }

    // visible shapes per prism for note landing points
    let mut cand: Vec<u32> = Vec::new();
    let mut vis_out: Vec<Vis> = Vec::new();
    for (pi, ip) in iso.prisms.iter().enumerate() {
        if ip.is_fill {
            let lp: Vec<V2> = ip.loops.first().map(|l| l.iter().map(|v| proj(sx, sz, [v.x, v.y, ip.z1])).collect()).unwrap_or_default();
            vis_out.push(Vis {
                comp: ip.comp,
                part: ip.part.clone(),
                instance: ip.instance,
                shapes: vec![OwnedShape { outer: lp, holes: Vec::new() }],
                src: ip.src.clone(),
                cut: ip.cap_cut,
            });
            continue;
        }
        let mut faces: Vec<usize> = (0..iso.faces.len()).filter(|&i| iso.faces[i].prism == pi && iso.faces[i].front).collect();
        sort::stable(&mut faces, |&x, &y| {
            let (fx, fy) = (&iso.faces[x], &iso.faces[y]);
            let cx = fx.n[2] > 0.5 && ip.cap_cut;
            let cy = fy.n[2] > 0.5 && ip.cap_cut;
            if cx != cy {
                return cx;
            }
            geom::signed_area_v(&fx.outer).abs() > geom::signed_area_v(&fy.outer).abs()
        });
        let mut shapes: Vec<OwnedShape> = Vec::new();
        for &fi in &faces {
            let f = &iso.faces[fi];
            let sh = Shape { outer: &f.outer, holes: f.holes.iter().map(|h| h.as_slice()).collect() };
            if let Some(lp) = shape::label_point(core::slice::from_ref(&sh)) {
                let mut bb = Box::default();
                bb.add_point(lp.x, lp.y);
                iso.candidates(bb.expand(1e-9), &mut cand);
                if !iso.occluded_at(lp, iso.face_depth(f, lp), &[fi], &cand) {
                    shapes.push(OwnedShape { outer: f.outer.clone(), holes: f.holes.clone() });
                    break;
                }
            }
        }
        if shapes.is_empty() {
            for &fi in faces.iter().take(6) {
                let f = &iso.faces[fi];
                let c = geom::centroid_v(&f.outer);
                let mut best: Option<V2> = None;
                let mut bd = f64::INFINITY;
                for gx in 0..7 {
                    for gy in 0..7 {
                        let qp = V2::init(
                            f.bbox.x0 + f.bbox.width() * (gx as f64 + 0.5) / 7.0,
                            f.bbox.y0 + f.bbox.height() * (gy as f64 + 0.5) / 7.0,
                        );
                        if !face_contains(f, qp) {
                            continue;
                        }
                        let mut bb = Box::default();
                        bb.add_point(qp.x, qp.y);
                        iso.candidates(bb.expand(1e-9), &mut cand);
                        if iso.occluded_at(qp, iso.face_depth(f, qp), &[fi], &cand) {
                            continue;
                        }
                        let d = qp.dist(c);
                        if d < bd {
                            bd = d;
                            best = Some(qp);
                        }
                    }
                }
                if let Some(qp) = best {
                    let e = 1e-3;
                    let tri = vec![V2::init(qp.x - e, qp.y - e), V2::init(qp.x + e, qp.y - e), V2::init(qp.x, qp.y + e)];
                    shapes.push(OwnedShape { outer: tri, holes: Vec::new() });
                    break;
                }
            }
        }
        vis_out.push(Vis { comp: ip.comp, part: ip.part.clone(), instance: ip.instance, shapes, src: ip.src.clone(), cut: ip.cap_cut });
    }
    iso.vis = vis_out;

    let mut cropb = Box::default();
    for it in &items {
        match it {
            Item::Path(p) => cropb.add_box(geom::points_box(&p.pts)),
            Item::Hatch(h) => {
                for l in &h.loops {
                    cropb.add_box(geom::points_box(l));
                }
            }
            _ => {}
        }
    }
    if cropb.is_empty() {
        cropb = sb;
    }
    Result { items, scale: s, crop: cropb }
}

/// Grade-line chain (or full outline) of a fill's cut face.
fn fill_outline(items: &mut Vec<Item>, ip: &IsoPrism, style: &Style, sx: f64, sz: f64) {
    let layer = style.layer_for_pen(Pen::Cut);
    let path = |pts: Vec<Pt>| Item::Path(PathItem { layer: layer.into(), pen: Pen::Cut, src: ip.src.clone(), closed: false, pts });
    for l in &ip.loops {
        let mut cur: Vec<Pt> = Vec::new();
        let n = l.len();
        let ccw = geom::signed_area_v(l) >= 0.0;
        let mut start = 0;
        if ip.outline == OutlineMode::Top {
            for k in 0..n {
                if !edge_top(l, (k + n - 1) % n, ccw) {
                    start = k;
                    break;
                }
            }
        }
        for idx in 0..n {
            let i = (start + idx) % n;
            let p = l[i];
            let q = l[(i + 1) % n];
            if ip.outline == OutlineMode::Full || edge_top(l, i, ccw) {
                let pp = proj(sx, sz, [p.x, p.y, ip.z1]);
                let qq = proj(sx, sz, [q.x, q.y, ip.z1]);
                if cur.is_empty() {
                    cur.push(Pt::at(pp, 0.0));
                }
                cur.push(Pt::at(qq, 0.0));
            } else if !cur.is_empty() {
                items.push(path(core::mem::take(&mut cur)));
            }
        }
        if !cur.is_empty() {
            items.push(path(cur));
        }
    }
}

fn edge_top(l: &[V2], i: usize, ccw: bool) -> bool {
    let p = l[i];
    let q = l[(i + 1) % l.len()];
    let d = q.sub(p);
    let len = d.len();
    if len < 1e-12 {
        return false;
    }
    let ny = if ccw { -d.x / len } else { d.x / len };
    ny > 0.01
}
