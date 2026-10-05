//! Iso view with hidden-line removal on planar faces (SPEC section 8.2).

use crate::diag::Diag;
use crate::drawing::{Item, layer_name, pen_layer_key};
use crate::geom::*;
use crate::hatch::hatch_lines;
use crate::mesh::{FLoop, flatten_sharp};
use crate::model::*;
use crate::poly;
use crate::section::{ViewBase, VisInfo};
use crate::style::Style;
use crate::view::ViewParams;

const C30: f64 = 0.866_025_403_784_438_6;
const S30: f64 = 0.5;

fn quadrant(from: &str) -> (f64, f64) {
    match from {
        "front_left" => (-1.0, 1.0),
        "back_right" => (1.0, -1.0),
        "back_left" => (-1.0, -1.0),
        _ => (1.0, 1.0),
    }
}

fn proj(sx: f64, sz: f64, p: [f64; 3]) -> Pt {
    pt((sz * p[0] - sx * p[2]) * C30, p[1] - (sx * p[0] + sz * p[2]) * S30)
}

fn depth(sx: f64, sz: f64, p: [f64; 3]) -> f64 {
    sx * p[0] + p[1] + sz * p[2]
}

struct Face {
    n: [f64; 3],
    p0: [f64; 3],
    outer: Vec<Pt>,
    holes: Vec<Vec<Pt>>,
    bbox: Rect,
    prism: usize,
    front: bool,
    nd: f64,
}

struct Edge {
    a: [f64; 3],
    b: [f64; 3],
    pen: String,
    prism: usize,
    f1: usize,
    f2: usize,
    chain: usize, // chaining group id (loop id) and ordering is by insertion
}

struct Scene {
    sx: f64,
    sz: f64,
    faces: Vec<Face>,
    edges: Vec<Edge>,
    // uniform grid over faces
    gx0: f64,
    gy0: f64,
    gw: f64,
    gh: f64,
    cells: Vec<Vec<u32>>,
    gn: usize,
    eps: f64,
}

impl Scene {
    fn build_grid(&mut self) {
        let mut b = Rect::empty();
        for f in &self.faces {
            b.union(&f.bbox);
        }
        if b.is_empty() {
            return;
        }
        let gn = 24usize;
        self.gn = gn;
        self.gx0 = b.x0;
        self.gy0 = b.y0;
        self.gw = (b.w() / gn as f64).max(1e-6);
        self.gh = (b.h() / gn as f64).max(1e-6);
        self.cells = vec![vec![]; gn * gn];
        for (i, f) in self.faces.iter().enumerate() {
            if !f.front || f.nd.abs() < 1e-9 {
                continue;
            }
            let (c0, c1, r0, r1) = self.cell_range(&f.bbox);
            for r in r0..=r1 {
                for c in c0..=c1 {
                    self.cells[r * gn + c].push(i as u32);
                }
            }
        }
    }

    fn cell_range(&self, b: &Rect) -> (usize, usize, usize, usize) {
        let gn = self.gn as f64;
        let cl = |v: f64, o: f64, w: f64| (((v - o) / w).floor().max(0.0)).min(gn - 1.0) as usize;
        (cl(b.x0, self.gx0, self.gw), cl(b.x1, self.gx0, self.gw), cl(b.y0, self.gy0, self.gh), cl(b.y1, self.gy0, self.gh))
    }

    fn candidates(&self, b: &Rect, out: &mut Vec<u32>) {
        out.clear();
        if self.cells.is_empty() {
            return;
        }
        let (c0, c1, r0, r1) = self.cell_range(b);
        for r in r0..=r1 {
            for c in c0..=c1 {
                out.extend_from_slice(&self.cells[r * self.gn + c]);
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Depth of face `f` at screen point m.
    fn face_depth(&self, f: &Face, m: Pt) -> f64 {
        let (sx, sz) = (self.sx, self.sz);
        let uu = m.x / C30;
        let vv = -m.y / S30;
        // base point with y = 0 projecting to m: u/c30 = sz x - sx z ; -v/s30 = sx x + sz z  (y = 0)
        let x = (sz * uu + sx * vv) * 0.5;
        let z = (-sx * uu + sz * vv) * 0.5;
        let pb = [x, 0.0, z];
        let d = [sx, 1.0, sz];
        let t = (f.n[0] * (f.p0[0] - pb[0]) + f.n[1] * (f.p0[1] - pb[1]) + f.n[2] * (f.p0[2] - pb[2])) / f.nd;
        let p = [pb[0] + t * d[0], pb[1] + t * d[1], pb[2] + t * d[2]];
        depth(sx, sz, p)
    }

    fn face_contains(&self, f: &Face, m: Pt) -> bool {
        if !f.bbox.contains(m, 0.0) {
            return false;
        }
        if !poly_contains(&f.outer, m) {
            return false;
        }
        !f.holes.iter().any(|h| poly_contains(h, m))
    }

    fn occluded_at(&self, m: Pt, dm: f64, skip: &[usize], cand: &[u32]) -> bool {
        for &fi in cand {
            let fi = fi as usize;
            if skip.contains(&fi) {
                continue;
            }
            let f = &self.faces[fi];
            if self.face_contains(f, m) && self.face_depth(f, m) > dm + self.eps {
                return true;
            }
        }
        false
    }

    /// Visible parameter intervals of the 2D segment a->b whose depth runs da -> db.
    fn visible(&self, a: Pt, b: Pt, da: f64, db: f64, skip: &[usize]) -> Vec<(f64, f64)> {
        let bb = {
            let mut r = Rect::empty();
            r.add(a);
            r.add(b);
            r.inflate(1e-9)
        };
        let mut cand = vec![];
        self.candidates(&bb, &mut cand);
        cand.retain(|&fi| !skip.contains(&(fi as usize)) && self.faces[fi as usize].bbox.overlaps(&bb, 0.0));
        if cand.is_empty() {
            return vec![(0.0, 1.0)];
        }
        let sg = Seg::Line(a, b);
        let mut ts = vec![0.0, 1.0];
        for &fi in &cand {
            let f = &self.faces[fi as usize];
            for contour in std::iter::once(&f.outer).chain(f.holes.iter()) {
                let n = contour.len();
                for i in 0..n {
                    for (t, _) in intersect(&sg, &Seg::Line(contour[i], contour[(i + 1) % n])) {
                        ts.push(t);
                    }
                }
            }
        }
        ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        let mut out: Vec<(f64, f64)> = vec![];
        for w in ts.windows(2) {
            if w[1] - w[0] < 1e-9 {
                continue;
            }
            let tm = (w[0] + w[1]) * 0.5;
            let m = a.lerp(b, tm);
            let dm = da + (db - da) * tm;
            if self.occluded_at(m, dm, skip, &cand) {
                continue;
            }
            if let Some(l) = out.last_mut() {
                if (l.1 - w[0]).abs() < 1e-9 {
                    l.1 = w[1];
                    continue;
                }
            }
            out.push((w[0], w[1]));
        }
        out
    }
}

struct IsoPrism {
    src: String,
    comp: usize,
    part: Option<String>,
    region: Region,
    z0: f64,
    z1: f64,
    cap_cut: bool,
    pen: Option<String>,
    material: String,
    embedded: bool,
}

fn gather(model: &Model, vp: &ViewParams, crop: &Rect) -> Vec<IsoPrism> {
    let mut out = vec![];
    for c in &model.comps {
        if !c.visible || c.failed {
            continue;
        }
        for inst in &c.insts {
            for p in &inst.prisms {
                if p.only == Only::Section {
                    continue;
                }
                let (mut z0, mut z1, mut cap) = (p.z0, p.z1, false);
                if vp.cutaway {
                    if z0 >= vp.cut_z {
                        continue;
                    }
                    if z1 > vp.cut_z {
                        z1 = vp.cut_z;
                        cap = true;
                    }
                }
                if z1 - z0 < 1e-9 {
                    continue;
                }
                let _ = &mut z0;
                for r in poly::clip_region_rect(&p.region, crop) {
                    out.push(IsoPrism {
                        src: p.src.clone(),
                        comp: p.comp,
                        part: p.part.clone(),
                        region: r,
                        z0,
                        z1,
                        cap_cut: cap,
                        pen: p.pen.clone(),
                        material: p.material.clone(),
                        embedded: p.embedded,
                    });
                }
            }
        }
    }
    out
}

fn add_prism(scene: &mut Scene, pi: usize, ip: &IsoPrism, cut_pen_for_cap: bool) {
    let (sx, sz) = (scene.sx, scene.sz);
    let cam = [sx, 1.0, sz];
    let step = 0.22;
    let tol = 0.004;
    let mut loops: Vec<FLoop> = vec![flatten_sharp(&ip.region.outer, step, tol)];
    for h in &ip.region.holes {
        loops.push(flatten_sharp(h, step, tol));
    }
    let mk_face = |scene: &mut Scene, n: [f64; 3], verts: Vec<[f64; 3]>, holes3: Vec<Vec<[f64; 3]>>| -> usize {
        let outer: Vec<Pt> = verts.iter().map(|&v| proj(sx, sz, v)).collect();
        let holes: Vec<Vec<Pt>> = holes3.iter().map(|h| h.iter().map(|&v| proj(sx, sz, v)).collect()).collect();
        let mut bb = Rect::empty();
        for q in &outer {
            bb.add(*q);
        }
        let nd = n[0] * cam[0] + n[1] * cam[1] + n[2] * cam[2];
        let f = Face { n, p0: verts[0], outer, holes, bbox: bb, prism: pi, front: nd > 1e-9, nd };
        scene.faces.push(f);
        scene.faces.len() - 1
    };
    // caps
    let top_v: Vec<[f64; 3]> = loops[0].pts.iter().map(|p| [p.x, p.y, ip.z1]).collect();
    let top_h: Vec<Vec<[f64; 3]>> = loops[1..].iter().map(|l| l.pts.iter().map(|p| [p.x, p.y, ip.z1]).collect()).collect();
    let bot_v: Vec<[f64; 3]> = loops[0].pts.iter().map(|p| [p.x, p.y, ip.z0]).collect();
    let bot_h: Vec<Vec<[f64; 3]>> = loops[1..].iter().map(|l| l.pts.iter().map(|p| [p.x, p.y, ip.z0]).collect()).collect();
    let ftop = mk_face(scene, [0.0, 0.0, 1.0], top_v, top_h);
    let fbot = mk_face(scene, [0.0, 0.0, -1.0], bot_v, bot_h);
    let base_pen = ip.pen.clone();
    for (li, fl) in loops.iter().enumerate() {
        let n = fl.pts.len();
        let mut side_faces = vec![];
        for i in 0..n {
            let (a, b) = (fl.pts[i], fl.pts[(i + 1) % n]);
            let d = (b - a).norm();
            let nn = [d.y, -d.x, 0.0];
            let verts = vec![[a.x, a.y, ip.z0], [b.x, b.y, ip.z0], [b.x, b.y, ip.z1], [a.x, a.y, ip.z1]];
            side_faces.push(mk_face(scene, nn, verts, vec![]));
        }
        let chain = pi * 16 + li;
        for i in 0..n {
            let (a, b) = (fl.pts[i], fl.pts[(i + 1) % n]);
            let sf = side_faces[i];
            let pen_of = |f1: usize, f2: usize, cut_cap_edge: bool, scene: &Scene| -> String {
                if let Some(p) = &base_pen {
                    return p.clone();
                }
                if cut_cap_edge && cut_pen_for_cap {
                    return "cut".into();
                }
                if scene.faces[f1].front != scene.faces[f2].front { "profile".into() } else { "beyond".into() }
            };
            // top outline: between top cap and side face
            let p_top = pen_of(ftop, sf, ip.cap_cut, scene);
            scene.edges.push(Edge { a: [a.x, a.y, ip.z1], b: [b.x, b.y, ip.z1], pen: p_top, prism: pi, f1: ftop, f2: sf, chain });
            let p_bot = pen_of(fbot, sf, false, scene);
            scene.edges.push(Edge { a: [a.x, a.y, ip.z0], b: [b.x, b.y, ip.z0], pen: p_bot, prism: pi, f1: fbot, f2: sf, chain: chain + 8 });
        }
        for i in 0..n {
            let a = fl.pts[i];
            let prev = side_faces[(i + n - 1) % n];
            let cur = side_faces[i];
            let (fp, fc) = (scene.faces[prev].front, scene.faces[cur].front);
            let pen = if let Some(p) = &base_pen { p.clone() } else if fp != fc { "profile".into() } else { "beyond".into() };
            scene.edges.push(Edge { a: [a.x, a.y, ip.z0], b: [a.x, a.y, ip.z1], pen, prism: pi, f1: prev, f2: cur, chain: usize::MAX - (pi * 4096 + li * 1024 + i) });
            let _ = fl.sharp[i];
        }
    }
}

pub fn build_iso(model: &Model, vp: &ViewParams, style: &Style, _diags: &mut Vec<Diag>) -> ViewBase {
    let (sx, sz) = quadrant(&vp.from);
    let crop_xy = vp.crop.unwrap_or_else(|| {
        let mut r = Rect::empty();
        for c in &model.comps {
            if c.failed || !c.visible {
                continue;
            }
            r.union(&c.bbox());
        }
        if r.is_empty() { Rect::new(0.0, 0.0, 12.0, 12.0) } else { r }
    });
    let prisms = gather(model, vp, &crop_xy);
    let mut scene = Scene { sx, sz, faces: vec![], edges: vec![], gx0: 0.0, gy0: 0.0, gw: 1.0, gh: 1.0, cells: vec![], gn: 0, eps: 1e-3 };
    for (i, p) in prisms.iter().enumerate() {
        add_prism(&mut scene, i, p, true);
    }
    // scene extents
    let mut sb = Rect::empty();
    for f in &scene.faces {
        sb.union(&f.bbox);
    }
    if sb.is_empty() {
        return ViewBase { items: vec![], vis: vec![], crop: Rect::new(0.0, 0.0, 12.0, 12.0), s: 12.0 };
    }
    scene.eps = 1e-4 * sb.w().max(sb.h()).max(1.0);
    scene.build_grid();
    let s = match vp.factor {
        Some(f) => f,
        None => ((sb.w().max(sb.h()) / 5.5) * 2.0).ceil() / 2.0,
    }
    .max(0.5);

    // dedupe identical 3D edges (keep first, heavier pen wins)
    let key = |a: [f64; 3], b: [f64; 3]| -> [i64; 6] {
        let r = |x: f64| (x * 2000.0).round() as i64;
        let (ka, kb) = ([r(a[0]), r(a[1]), r(a[2])], [r(b[0]), r(b[1]), r(b[2])]);
        if ka <= kb { [ka[0], ka[1], ka[2], kb[0], kb[1], kb[2]] } else { [kb[0], kb[1], kb[2], ka[0], ka[1], ka[2]] }
    };
    let mut seen: std::collections::BTreeMap<[i64; 6], usize> = std::collections::BTreeMap::new();
    let mut keep = vec![true; scene.edges.len()];
    let rank = |pen: &str| style.pen(pen).width_mm;
    for i in 0..scene.edges.len() {
        let k = key(scene.edges[i].a, scene.edges[i].b);
        if let Some(&j) = seen.get(&k) {
            if rank(&scene.edges[i].pen) > rank(&scene.edges[j].pen) {
                keep[j] = false;
                seen.insert(k, i);
            } else {
                keep[i] = false;
            }
        } else {
            seen.insert(k, i);
        }
    }
    // candidate rule: sharp edges need >= 1 front face; smooth (beyond) vertical edges need differing facing (already pen=profile)
    let mut pieces: Vec<(usize, Vec<(Pt, Pt)>)> = vec![]; // (edge idx, visible segments)
    for (ei, e) in scene.edges.iter().enumerate() {
        if !keep[ei] {
            continue;
        }
        let (f1, f2) = (&scene.faces[e.f1], &scene.faces[e.f2]);
        if !f1.front && !f2.front {
            continue;
        }
        let a = proj(sx, sz, e.a);
        let b = proj(sx, sz, e.b);
        if a.dist(b) < 1e-9 {
            continue;
        }
        let (da, db) = (depth(sx, sz, e.a), depth(sx, sz, e.b));
        let vis = scene.visible(a, b, da, db, &[e.f1, e.f2]);
        let segs: Vec<(Pt, Pt)> = vis.into_iter().map(|(t0, t1)| (a.lerp(b, t0), a.lerp(b, t1))).collect();
        if !segs.is_empty() {
            pieces.push((ei, segs));
        }
    }

    let mut items: Vec<Item> = vec![];
    // hatch on cutaway caps (front-facing only)
    let mut hatch_items: Vec<Item> = vec![];
    let cap_front = sz > 0.0;
    if vp.cutaway && cap_front {
        for (pi, ip) in prisms.iter().enumerate() {
            if !ip.cap_cut || ip.embedded {
                continue;
            }
            let mst = style.material(&ip.material);
            for spec in &mst.hatch {
                let Some(pat) = style.pattern(&spec.pattern) else { continue };
                let flat: Vec<Vec<Pt>> = ip.region.loops().map(|l| flatten_loop(l, 0.003)).collect();
                let lines = hatch_lines(&flat, pat, spec.scale * s, spec.angle, None);
                let mut out_lines: Vec<[f64; 4]> = vec![];
                let skip_faces: Vec<usize> = vec![];
                for l in lines {
                    let (pa, pb) = ([l[0], l[1], ip.z1], [l[2], l[3], ip.z1]);
                    let (a, b) = (proj(sx, sz, pa), proj(sx, sz, pb));
                    let (da, db) = (depth(sx, sz, pa), depth(sx, sz, pb));
                    if a.near(b, 1e-12) {
                        let mut cand = vec![];
                        let bb = Rect::new(a.x, a.y, a.x, a.y).inflate(1e-9);
                        scene.candidates(&bb, &mut cand);
                        if !scene.occluded_at(a, da, &skip_faces, &cand) {
                            out_lines.push([a.x, a.y, a.x, a.y]);
                        }
                        continue;
                    }
                    for (t0, t1) in scene.visible(a, b, da, db, &skip_faces) {
                        let (p, q) = (a.lerp(b, t0), a.lerp(b, t1));
                        out_lines.push([p.x, p.y, q.x, q.y]);
                    }
                }
                // loops of the projected cap
                let proj_loop = |l: &Loop| -> Vec<V> { l.iter().map(|q| { let r = proj(sx, sz, [q.x, q.y, ip.z1]); v(r.x, r.y) }).collect() };
                let loops: Vec<Vec<V>> = ip.region.loops().map(|l| proj_loop(&flatten_loop(l, 0.003).iter().map(|p| v(p.x, p.y)).collect())).collect();
                hatch_items.push(Item::Hatch {
                    layer: layer_name(style, "hatch"),
                    pen: "hatch".into(),
                    src: ip.src.clone(),
                    pattern: spec.pattern.clone(),
                    scale: spec.scale,
                    angle: spec.angle,
                    loops,
                    lines: out_lines,
                });
            }
            let _ = pi;
        }
    }
    items.extend(hatch_items);

    // chain visible pieces into paths, ordered by pen weight (light first)
    let mut by_pen: Vec<(String, Vec<(usize, Vec<(Pt, Pt)>)>)> = vec![];
    for (ei, segs) in pieces {
        let pen = scene.edges[ei].pen.clone();
        match by_pen.iter_mut().find(|(p, _)| *p == pen) {
            Some((_, v)) => v.push((ei, segs)),
            None => by_pen.push((pen, vec![(ei, segs)])),
        }
    }
    by_pen.sort_by(|a, b| style.pen(&a.0).width_mm.partial_cmp(&style.pen(&b.0).width_mm).unwrap().then(a.0.cmp(&b.0)));
    for (pen, list) in by_pen {
        let mut cur: Vec<V> = vec![];
        let mut cur_src = String::new();
        let mut cur_chain = usize::MAX;
        let mut flush = |cur: &mut Vec<V>, src: &str, items: &mut Vec<Item>| {
            if cur.len() >= 2 {
                items.push(Item::Path { layer: layer_name(style, pen_layer_key(&pen)), pen: pen.clone(), src: src.to_string(), closed: false, pts: std::mem::take(cur) });
            }
            cur.clear();
        };
        for (ei, segs) in list {
            let e = &scene.edges[ei];
            let src = prisms[e.prism].src.clone();
            for (a, b) in segs {
                let continues = cur_chain == e.chain && cur_src == src && cur.last().map_or(false, |l| l.p().near(a, 1e-7));
                if !continues {
                    flush(&mut cur, &cur_src.clone(), &mut items);
                    cur.push(v(a.x, a.y));
                }
                cur.push(v(b.x, b.y));
                cur_src = src.clone();
                cur_chain = e.chain;
            }
        }
        flush(&mut cur, &cur_src.clone(), &mut items);
    }

    // visible shapes for notes: best visible face per prism (cut caps first)
    let mut vis: Vec<VisInfo> = vec![];
    let mut cand = vec![];
    for (pi, ip) in prisms.iter().enumerate() {
        let mut faces: Vec<usize> = scene.faces.iter().enumerate().filter(|(_, f)| f.prism == pi && f.front).map(|(i, _)| i).collect();
        faces.sort_by(|&a, &b| {
            let (fa, fb) = (&scene.faces[a], &scene.faces[b]);
            let capa = (fa.n[2] > 0.5 && ip.cap_cut) as i32;
            let capb = (fb.n[2] > 0.5 && ip.cap_cut) as i32;
            capb.cmp(&capa).then(poly_area(&fb.outer).abs().partial_cmp(&poly_area(&fa.outer).abs()).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut shapes = vec![];
        let faces_all = faces.clone();
        for fi in faces {
            let f = &scene.faces[fi];
            let mut shape = vec![f.outer.clone()];
            shape.extend(f.holes.iter().cloned());
            if let Some(lp) = crate::annot::label_point(&[shape.clone()]) {
                let bb = Rect::new(lp.x, lp.y, lp.x, lp.y).inflate(1e-9);
                scene.candidates(&bb, &mut cand);
                if !scene.occluded_at(lp, scene.face_depth(f, lp), &[fi], &cand) {
                    shapes.push(shape);
                    break;
                }
            }
        }
        if shapes.is_empty() {
            // fallback: any visible sample point inside one of the larger faces
            'fb: for &fi in faces_all.iter().take(6) {
                let f = &scene.faces[fi];
                let c = poly_centroid(&f.outer);
                let mut samples: Vec<(f64, Pt)> = vec![];
                for gx in 0..7 {
                    for gy in 0..7 {
                        let q = pt(f.bbox.x0 + f.bbox.w() * (gx as f64 + 0.5) / 7.0, f.bbox.y0 + f.bbox.h() * (gy as f64 + 0.5) / 7.0);
                        if poly_contains(&f.outer, q) && !f.holes.iter().any(|h| poly_contains(h, q)) {
                            samples.push((q.dist(c), q));
                        }
                    }
                }
                samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                for (_, q) in samples {
                    let bb = Rect::new(q.x, q.y, q.x, q.y).inflate(1e-9);
                    scene.candidates(&bb, &mut cand);
                    if !scene.occluded_at(q, scene.face_depth(f, q), &[fi], &cand) {
                        // a tiny triangle around the sample so label_point returns it
                        let e = 1e-3;
                        shapes.push(vec![vec![pt(q.x - e, q.y - e), pt(q.x + e, q.y - e), pt(q.x, q.y + e)]]);
                        break 'fb;
                    }
                }
            }
        }
        vis.push(VisInfo { comp: ip.comp, src: ip.src.clone(), part: ip.part.clone(), shapes, cut: ip.cap_cut, embedded: ip.embedded });
    }

    // crop = extent of everything drawn
    let mut crop = crate::drawing::items_bounds(&items);
    if crop.is_empty() {
        crop = sb;
    }
    ViewBase { items, vis, crop, s }
}

pub fn iso_landing(target: &str, vis: &[VisInfo], model: &Model, _crop: &Rect) -> Option<Pt> {
    let (cid, part) = match target.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (target, None),
    };
    let cid = cid.split('#').next().unwrap_or(cid);
    let comp = model.comps.iter().find(|c| c.id == cid)?;
    let mut shapes = vec![];
    for vi in vis.iter().filter(|v| v.comp == comp.idx) {
        if let Some(p) = part {
            if vi.part.as_deref() != Some(p) {
                continue;
            }
        }
        shapes.extend(vi.shapes.iter().cloned());
    }
    crate::annot::label_point(&shapes)
}

pub fn project_point(vp: &ViewParams, _model: &Model, p: Pt) -> Option<Pt> {
    let (sx, sz) = quadrant(&vp.from);
    Some(proj(sx, sz, [p.x, p.y, vp.cut_z]))
}
