use crate::clip;
use crate::geom::{self, Box, FLAT_TOL, Pt, V2};
use crate::math;
use crate::model::{self, Kind, OutlineMode, Prism};
use crate::pathclip;
use crate::pen::Pen;
use crate::scene::{Comp, PlanInst, Scene};
use crate::sort;
use crate::style::Role;
use alloc::string::String;
use alloc::vec::Vec;

const EPS: f64 = 1e-7;
const CREASE_RAD: f64 = 0.17;

pub fn plan_pt(x: f64, z: f64) -> V2 {
    V2::init(x, -z)
}

pub fn default_cut_y(prisms: &[Prism]) -> f64 {
    let mut top = f64::NEG_INFINITY;
    for p in prisms.iter().filter(|p| p.kind == Kind::Body && p.role != Role::Soil) {
        for l in &p.loops {
            top = top.max(geom::points_box(l).y1);
        }
    }
    if top.is_finite() { top - 1.0 } else { 0.0 }
}

#[derive(Clone, Copy, Debug)]
struct Strip {
    x0: f64,
    x1: f64,
    lo: f64,
    top: f64,
    tl: f64,
    tr: f64,
    cut: bool,
}

fn column(flat: &[Vec<V2>], x: f64) -> Vec<[f64; 2]> {
    let ys = pathclip::scan(flat, x, true);
    ys.as_chunks::<2>().0.to_vec()
}

fn below(flat: &[Vec<V2>], x: f64, cap: f64) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut top = f64::NEG_INFINITY;
    for [a, b] in column(flat, x) {
        if a < cap - EPS {
            lo = lo.min(a);
            top = top.max(b.min(cap));
        }
    }
    (top > lo).then_some((lo, top))
}

fn strips(flat: &[Vec<V2>], cut_y: f64) -> Vec<Strip> {
    let bx = clip::loops_box(flat);
    if bx.is_empty() || bx.y0 >= cut_y - EPS {
        return Vec::new();
    }
    let mut cuts: Vec<[f64; 2]> = Vec::new();
    if bx.y1 > cut_y + EPS {
        let xs = pathclip::scan(flat, cut_y, false);
        cuts = xs.as_chunks::<2>().0.iter().copied().filter(|c| c[1] - c[0] > EPS).collect();
    }
    let mut xs: Vec<f64> = flat.iter().flatten().map(|v| v.x).collect();
    xs.extend(cuts.iter().flatten());
    sort::stable(&mut xs, |a, b| a < b);
    xs.dedup_by(|a, b| (*a - *b).abs() < EPS);
    let mut out: Vec<Strip> = Vec::new();
    for c in &cuts {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for x in [c[0] + EPS, (c[0] + c[1]) / 2.0, c[1] - EPS] {
            for [a, b] in column(flat, x) {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        out.push(Strip {
            x0: c[0],
            x1: c[1],
            lo: lo.min(cut_y - 1e-6),
            top: hi.max(cut_y + 1e-6),
            tl: cut_y,
            tr: cut_y,
            cut: true,
        });
    }
    for w in xs.windows(2) {
        let (xa, xb) = (w[0], w[1]);
        if xb - xa < EPS {
            continue;
        }
        let xm = (xa + xb) / 2.0;
        if cuts.iter().any(|c| xm > c[0] && xm < c[1]) {
            continue;
        }
        let d = ((xb - xa) / 4.0).min(1e-5);
        let samples = [below(flat, xa + d, cut_y), below(flat, xm, cut_y), below(flat, xb - d, cut_y)];
        if samples.iter().all(Option::is_none) {
            continue;
        }
        let lo = samples.iter().flatten().map(|s| s.0).fold(f64::INFINITY, f64::min);
        let top = samples.iter().flatten().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
        let tl = samples[0].map_or(top, |s| s.1);
        let tr = samples[2].map_or(top, |s| s.1);
        out.push(Strip { x0: xa, x1: xb, lo, top, tl, tr, cut: false });
    }
    sort::stable(&mut out, |a, b| a.x0 < b.x0);
    merge(out)
}

fn merge(strips: Vec<Strip>) -> Vec<Strip> {
    let mut out: Vec<Strip> = Vec::new();
    for s in strips {
        if let Some(last) = out.last_mut()
            && !last.cut
            && !s.cut
            && (s.x0 - last.x1).abs() < EPS
            && (s.tl - last.tr).abs() < 1e-4
        {
            let slope = |t: &Strip| math::atan((t.tr - t.tl) / (t.x1 - t.x0).max(EPS));
            if (slope(last) - slope(&s)).abs() < CREASE_RAD {
                last.x1 = s.x1;
                last.lo = last.lo.min(s.lo);
                last.top = last.top.max(s.top);
                last.tr = s.tr;
                continue;
            }
        }
        out.push(s);
    }
    out
}

fn rect_prism(src: &Prism, x0: f64, x1: f64, z0: f64, z1: f64, d0: f64, d1: f64) -> Prism {
    let (a, b) = (plan_pt(x0, z1), plan_pt(x1, z0));
    Prism {
        comp: src.comp,
        part: src.part.clone(),
        instance: src.instance,
        material: src.material.clone(),
        loops: model::one_loop(model::rect_loop(a.x, a.y, b.x, b.y)),
        z0: d0,
        z1: d1,
        embedded: src.embedded,
        kind: src.kind,
        outline: if src.outline == OutlineMode::None { OutlineMode::None } else { OutlineMode::Full },
        pen: src.pen,
        role: src.role,
        blocking: src.blocking,
        dashed: src.dashed,
        existing: src.existing,
        hw_dash: src.hw_dash,
        seam: src.seam,
        ..Prism::default()
    }
}

fn bar_prism(src: &Prism, center: Vec<Pt>, r: f64, d0: f64, d1: f64) -> Prism {
    Prism {
        comp: src.comp,
        part: src.part.clone(),
        instance: src.instance,
        material: src.material.clone(),
        loops: model::one_loop(crate::pathgeom::ribbon(&center, r, r)),
        z0: d0,
        z1: d1,
        embedded: src.embedded,
        role: src.role,
        centerline: center,
        sweep_r: r,
        kind: src.kind,
        pen: src.pen,
        dashed: src.dashed,
        existing: src.existing,
        ..Prism::default()
    }
}

fn dot_prism(src: &Prism, c: V2, r: f64, cut_y: f64) -> Prism {
    Prism {
        comp: src.comp,
        part: src.part.clone(),
        instance: src.instance,
        material: src.material.clone(),
        loops: model::one_loop(model::circle_loop(c.x, c.y, r)),
        z0: cut_y - r.max(1e-3),
        z1: cut_y + r.max(1e-3),
        embedded: src.embedded,
        role: src.role,
        pen: src.pen,
        sweep_r: r,
        kind: src.kind,
        dashed: src.dashed,
        existing: src.existing,
        ..Prism::default()
    }
}

fn is_lumber_run_y(scene: &Scene, p: &Prism) -> bool {
    scene.comps.get(p.comp as usize).is_some_and(|c| {
        c.ty.traits.lengthwise_grain && c.node.get("run").and_then(crate::json::Value::str) == Some("y")
    })
}

fn bar_plan(p: &Prism, cut_y: f64, out: &mut Vec<Prism>) {
    let r = p.sweep_r;
    if p.centerline.len() < 2 {
        let c = clip::loops_box(&clip::from_region(&p.loops, FLAT_TOL)).center();
        if c.y - r >= cut_y {
            return;
        }
        let center = alloc::vec![Pt::at(plan_pt(c.x, p.z0), 0.0), Pt::at(plan_pt(c.x, p.z1), 0.0)];
        let (d0, d1) = if c.y + r > cut_y { (cut_y - 1e-6, cut_y + 1e-6) } else { (c.y - r, c.y + r) };
        out.push(bar_prism(p, center, r, d0, d1));
        return;
    }
    let zc = (p.z0 + p.z1) / 2.0;
    let pts = geom::flatten_polyline(&p.centerline, false, FLAT_TOL);
    let mut run: Option<(f64, f64, f64, f64)> = None;
    let flush = |run: &mut Option<(f64, f64, f64, f64)>, out: &mut Vec<Prism>| {
        if let Some((x0, x1, y0, y1)) = run.take()
            && x1 - x0 > EPS
        {
            let center = alloc::vec![Pt::at(plan_pt(x0, zc), 0.0), Pt::at(plan_pt(x1, zc), 0.0)];
            let d1 = (y1 + r).min(cut_y);
            out.push(bar_prism(p, center, r, (y0 - r).min(d1 - 1e-6), d1));
        }
    };
    let mut dots: Vec<f64> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ya, yb) = (a.y < cut_y, b.y < cut_y);
        if ya != yb {
            let t = (cut_y - a.y) / (b.y - a.y);
            let x = a.x + (b.x - a.x) * t;
            if !dots.iter().any(|&d| (d - x).abs() < r) {
                dots.push(x);
                out.push(dot_prism(p, plan_pt(x, zc), r, cut_y));
            }
        }
        if ya && yb && (b.x - a.x).abs() < EPS && (b.y - a.y).abs() > r && !dots.iter().any(|&d| (d - a.x).abs() < r) {
            dots.push(a.x);
            out.push(dot_prism(p, plan_pt(a.x, zc), r, cut_y));
        }
        let (lo, hi) = match (ya, yb) {
            (true, true) => (a, b),
            (true, false) | (false, true) => {
                let t = (cut_y - a.y) / (b.y - a.y);
                let m = a.lerp(b, t);
                if ya { (a, m) } else { (m, b) }
            }
            (false, false) => {
                flush(&mut run, out);
                continue;
            }
        };
        let (x0, x1) = (lo.x.min(hi.x), lo.x.max(hi.x));
        let (y0, y1) = (lo.y.min(hi.y), lo.y.max(hi.y));
        run = Some(match run {
            Some((rx0, rx1, ry0, ry1)) => (rx0.min(x0), rx1.max(x1), ry0.min(y0), ry1.max(y1)),
            None => (x0, x1, y0, y1),
        });
        if !(ya && yb) {
            flush(&mut run, out);
        }
    }
    flush(&mut run, out);
}

pub fn xz_bars_in_section(world: Vec<Prism>, cut_z: f64) -> Vec<Prism> {
    let mut out = Vec::with_capacity(world.len());
    for p in world {
        if !p.xz_bar {
            out.push(p);
            continue;
        }
        let r = p.sweep_r;
        let mut run: Vec<[f64; 3]> = Vec::new();
        let flush = |run: &mut Vec<[f64; 3]>, out: &mut Vec<Prism>| {
            let mut center: Vec<Pt> = Vec::new();
            let (mut zlo, mut zhi) = (f64::INFINITY, f64::NEG_INFINITY);
            for c in run.drain(..) {
                zlo = zlo.min(c[2]);
                zhi = zhi.max(c[2]);
                let v = V2::init(c[0], c[1]);
                if center.last().is_none_or(|l| l.v().dist(v) > EPS) {
                    center.push(Pt::at(v, 0.0));
                }
            }
            if center.len() >= 2 {
                let d1 = (zhi + r).min(cut_z);
                out.push(bar_prism(&p, center, r, (zlo - r).min(d1 - 1e-6), d1));
            }
        };
        for w in p.path3.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (ba, bb) = (a[2] < cut_z, b[2] < cut_z);
            if ba != bb {
                let t = (cut_z - a[2]) / (b[2] - a[2]);
                let m = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, cut_z];
                out.push(dot_prism(&p, V2::init(m[0], m[1]), r, cut_z));
                if ba {
                    run.extend([a, m]);
                    flush(&mut run, &mut out);
                } else {
                    run.extend([m, b]);
                }
            } else if ba {
                run.extend([a, b]);
            } else {
                flush(&mut run, &mut out);
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

fn xz_bar_plan(p: &Prism, cut_y: f64, out: &mut Vec<Prism>) {
    let Some(first) = p.path3.first() else { return };
    let (y, r) = (first[1], p.sweep_r);
    if y - r >= cut_y {
        return;
    }
    let center: Vec<Pt> = p.path3.iter().map(|c| Pt::at(plan_pt(c[0], c[2]), 0.0)).collect();
    let (d0, d1) = if y + r > cut_y { (cut_y - 1e-6, cut_y + 1e-6) } else { (y - r, y + r) };
    out.push(bar_prism(p, center, r, d0, d1));
}

fn footprint_prism(src: &Prism, lp: Vec<Pt>, z0: f64, z1: f64) -> Prism {
    Prism {
        comp: src.comp,
        part: src.part.clone(),
        instance: src.instance,
        material: src.material.clone(),
        loops: model::one_loop(lp),
        z0,
        z1,
        kind: src.kind,
        outline: src.outline,
        pen: src.pen,
        role: src.role,
        dashed: src.dashed,
        existing: src.existing,
        ..Prism::default()
    }
}

fn footprint_plan(p: &Prism, cut_y: f64, out: &mut Vec<Prism>) {
    let [y0, y1] = p.fp_y;
    if p.footprint.len() < 3 || y0 >= cut_y - EPS {
        return;
    }
    let mut lp: Vec<Pt> = p.footprint.iter().map(|v| Pt::at(plan_pt(v.x, v.y), 0.0)).collect();
    lp.reverse();
    out.push(footprint_prism(p, lp, y0, y1));
}

pub fn footprints_in_section(world: Vec<Prism>, cut_z: f64) -> Vec<Prism> {
    if !world.iter().any(|p| p.fp_proxy) {
        return world;
    }
    let mut out = Vec::with_capacity(world.len());
    for p in world {
        if !p.fp_proxy {
            out.push(p);
            continue;
        }
        if p.footprint.len() < 3 {
            continue;
        }
        let [y0, y1] = p.fp_y;
        for s in strips(core::slice::from_ref(&p.footprint), cut_z) {
            out.push(footprint_prism(&p, model::rect_loop(s.x0, y0, s.x1, y1), s.lo, s.top));
        }
    }
    out
}

const CMU_JOINT: f64 = 0.375;
const CMU_WEB: f64 = 1.25;
const CMU_CELL_R: f64 = 0.75;

fn plain_rect(p: &Prism) -> Option<Box> {
    let l = p.loops.first().filter(|l| p.loops.len() == 1 && l.len() == 4 && l.iter().all(|q| q.b == 0.0))?;
    let b = geom::points_box(l);
    let flat: Vec<V2> = l.iter().map(|q| q.v()).collect();
    ((geom::signed_area_v(&flat).abs() - b.width() * b.height()).abs() < 1e-6).then_some(b)
}

fn round_rect(b: Box, ccw: bool) -> Vec<Pt> {
    let r = CMU_CELL_R.min(b.width() / 3.0).min(b.height() / 3.0);
    let k = math::tan(core::f64::consts::PI / 8.0);
    let mut pts = alloc::vec![
        Pt { x: b.x0 + r, y: b.y0, b: 0.0 },
        Pt { x: b.x1 - r, y: b.y0, b: k },
        Pt { x: b.x1, y: b.y0 + r, b: 0.0 },
        Pt { x: b.x1, y: b.y1 - r, b: k },
        Pt { x: b.x1 - r, y: b.y1, b: 0.0 },
        Pt { x: b.x0 + r, y: b.y1, b: k },
        Pt { x: b.x0, y: b.y1 - r, b: 0.0 },
        Pt { x: b.x0, y: b.y0 + r, b: k },
    ];
    if !ccw {
        let n = pts.len();
        let bulges: Vec<f64> = pts.iter().map(|p| p.b).collect();
        pts.reverse();
        for (i, p) in pts.iter_mut().enumerate() {
            p.b = -bulges[(2 * n - 2 - i) % n];
        }
    }
    pts
}

/// A cut wall whose webs the document draws itself as `cmu` pieces of other components (SPEC 32.7).
pub struct OwnCells {
    pub wall: u32,
    pub piece: u32,
    pub count: usize,
}

/// A CMU wall the plan cuts shows its units: face shells, cross webs, cells and head joints (SPEC 32.7).
/// Returns which world prisms it drew.
fn cmu_units(scene: &Scene, world: &[Prism], cut_y: f64, out: &mut Vec<Prism>, own: &mut Vec<OwnCells>) -> Vec<bool> {
    let mut done = alloc::vec![false; world.len()];
    let wall = |p: &Prism| scene.comps.get(p.comp as usize).filter(|c| c.ty.ty == crate::catalog::Type::CmuWall);
    let same = |a: &Prism, b: &Prism| a.comp == b.comp && a.instance == b.instance;
    let cuts = |b: &Box| b.y0 < cut_y - EPS && b.y1 > cut_y + EPS;
    for i in 0..world.len() {
        let p = &world[i];
        let Some(comp) = wall(p).filter(|_| !done[i] && p.kind == Kind::Body && p.material == "cmu") else {
            continue;
        };
        let Some(b) = plain_rect(p).filter(cuts) else { continue };
        let along = |q: &Prism, qb: &Box| if q.face_on { [qb.x0, qb.x1] } else { [q.z0, q.z1] };
        let across = |q: &Prism, qb: &Box| if q.face_on { [q.z0, q.z1] } else { [qb.x0, qb.x1] };
        let run = along(p, &b);
        let pair = (i + 1..world.len()).find_map(|j| {
            let q = &world[j];
            let ok = !done[j] && same(p, q) && q.kind == Kind::Body && q.material == "cmu" && q.face_on == p.face_on;
            let qb = plain_rect(q).filter(|qb| ok && cuts(qb))?;
            let r = along(q, &qb);
            ((r[0] - run[0]).abs() < 1e-6 && (r[1] - run[1]).abs() < 1e-6).then_some((j, qb))
        });
        let Some((j, qb)) = pair else { continue };
        let (ta, tb) = (across(p, &b), across(&world[j], &qb));
        let shell = ta[1] - ta[0];
        let (t0, t1) = (ta[0].min(tb[0]), ta[1].max(tb[1]));
        if t1 - t0 < 2.0 * shell + 1.0 {
            continue;
        }
        let in_core = |q: &&Prism| {
            let qb = q.loops.first().map(|l| geom::points_box(l)).filter(cuts);
            qb.is_some_and(|qb| {
                let (s, t) = if p.face_on { ([qb.x0, qb.x1], [q.z0, q.z1]) } else { ([q.z0, q.z1], [qb.x0, qb.x1]) };
                q.kind == Kind::Body
                    && q.material == "cmu"
                    && wall(q).is_none()
                    && s[1] > run[0] + EPS
                    && s[0] < run[1] - EPS
                    && t[1] > t0 + shell + EPS
                    && t[0] < t1 - shell - EPS
            })
        };
        let mut pieces: Vec<u32> = world.iter().filter(in_core).map(|q| q.comp).collect();
        pieces.dedup();
        if let Some(&piece) = pieces.first() {
            if !own.iter().any(|o| o.wall == p.comp) {
                own.push(OwnCells { wall: p.comp, piece, count: pieces.len() });
            }
            continue;
        }
        let base = world
            .iter()
            .filter(|q| same(p, q) && q.material == "cmu")
            .filter_map(|q| q.loops.first().map(|l| geom::points_box(l).y0))
            .fold(f64::INFINITY, f64::min);
        let course = crate::num::to_int_clamped::<usize>(math::floor((cut_y - base) / 8.0), 0, 10_000);
        if crate::builders::cmu_wall::is_bond_beam(comp.node, course + 1) {
            for (k, q) in world.iter().enumerate() {
                let below = q.loops.first().is_some_and(|l| geom::points_box(l).y1 <= cut_y + EPS);
                if same(p, q) && below {
                    done[k] = true;
                }
            }
            continue;
        }
        let mut joints: Vec<f64> = if p.face_on {
            p.face_lines
                .iter()
                .filter(|s| (s[0].x - s[1].x).abs() < 1e-9 && s[0].y.min(s[1].y) < cut_y && s[0].y.max(s[1].y) > cut_y)
                .map(|s| s[0].x)
                .collect()
        } else {
            let first = run[0] + if course % 2 == 1 { 8.0 } else { 16.0 } - CMU_JOINT / 2.0;
            (0..2000).map(|k| first + 16.0 * f64::from(k)).take_while(|s| *s < run[1] - 1e-6).collect()
        };
        sort::stable(&mut joints, |a, b| a < b);
        joints.retain(|s| *s > run[0] + CMU_JOINT && *s < run[1] - CMU_JOINT);
        let solid_grout = world.iter().find(|q| {
            same(p, q)
                && q.kind == Kind::Body
                && q.part == "grout"
                && q.loops.first().is_some_and(|l| cuts(&geom::points_box(l)))
        });
        let by_bars = comp.node.get("grout").and_then(|v| v.str()).is_none_or(|g| g == "reinforced");
        let bars: Vec<V2> = if by_bars {
            world
                .iter()
                .filter(|q| q.role == Role::Rebar && !q.xz_bar && q.z1 - q.z0 < 2.0 && !q.loops.is_empty())
                .flat_map(|q| {
                    let flat = clip::from_region(&q.loops, FLAT_TOL);
                    let z = (q.z0 + q.z1) / 2.0;
                    let xs = pathclip::scan(&flat, cut_y, false);
                    xs.as_chunks::<2>().0.iter().map(|c| V2::init((c[0] + c[1]) / 2.0, z)).collect::<Vec<_>>()
                })
                .collect()
        } else {
            Vec::new()
        };
        let to_plan = |s: [f64; 2], t: [f64; 2]| -> Box {
            if p.face_on {
                Box { x0: s[0], y0: -t[1], x1: s[1], y1: -t[0] }
            } else {
                Box { x0: t[0], y0: -s[1], x1: t[1], y1: -s[0] }
            }
        };
        let mut edges: Vec<f64> = alloc::vec![run[0]];
        for s in &joints {
            edges.push(s - CMU_JOINT / 2.0);
            edges.push(s + CMU_JOINT / 2.0);
        }
        edges.push(run[1]);
        let cell_t = [t0 + shell, t1 - shell];
        for u in edges.as_chunks::<2>().0 {
            let len = u[1] - u[0];
            if len < 1e-6 {
                continue;
            }
            let cells: Vec<[f64; 2]> = if len >= 12.0 {
                let cl = (len - 3.0 * CMU_WEB) / 2.0;
                alloc::vec![[u[0] + CMU_WEB, u[0] + CMU_WEB + cl], [u[1] - CMU_WEB - cl, u[1] - CMU_WEB]]
            } else if len >= 4.0 {
                alloc::vec![[u[0] + CMU_WEB, u[1] - CMU_WEB]]
            } else {
                Vec::new()
            };
            let mut unit = rect_prism(p, 0.0, 1.0, 0.0, 1.0, b.y0, b.y1);
            let ob = to_plan(*u, [t0, t1]);
            unit.loops = alloc::vec![model::rect_loop(ob.x0, ob.y0, ob.x1, ob.y1)];
            for c in &cells {
                let cb = to_plan(*c, cell_t);
                unit.loops.push(round_rect(cb, false));
                let has_bar = bars.iter().any(|v| {
                    let (s, t) = if p.face_on { (v.x, v.y) } else { (v.y, v.x) };
                    s > c[0] && s < c[1] && t > cell_t[0] && t < cell_t[1]
                });
                let grouted =
                    if by_bars && !(p.face_on && solid_grout.is_some()) { has_bar } else { solid_grout.is_some() };
                if grouted {
                    let mut g = rect_prism(solid_grout.unwrap_or(p), 0.0, 1.0, 0.0, 1.0, b.y0, b.y1);
                    g.part = String::from("grout");
                    g.material = String::from("grout");
                    g.role = Role::Grout;
                    g.loops = alloc::vec![round_rect(cb, true)];
                    out.push(g);
                }
            }
            out.push(unit);
        }
        for (k, q) in world.iter().enumerate() {
            let Some(qb) = q.loops.first().map(|l| geom::points_box(l)).filter(|_| same(p, q)) else { continue };
            let r = along(q, &qb);
            let under = r[0] < run[1] - EPS && r[1] > run[0] + EPS;
            let hidden = qb.y1 <= cut_y + EPS || (cuts(&qb) && (q.part == "grout" || q.kind == Kind::Ghost));
            if k == i || k == j || (under && hidden) {
                done[k] = true;
            }
        }
    }
    done
}

pub fn prisms(scene: &Scene, world: &[Prism], cut_y: f64) -> (Vec<Prism>, Vec<OwnCells>) {
    let mut out = Vec::new();
    let mut own = Vec::new();
    let drawn = cmu_units(scene, world, cut_y, &mut out, &mut own);
    for (i, p) in world.iter().enumerate() {
        if p.role == Role::Soil || drawn[i] {
            continue;
        }
        if p.xz_bar {
            xz_bar_plan(p, cut_y, &mut out);
            continue;
        }
        if p.fp_proxy {
            footprint_plan(p, cut_y, &mut out);
            continue;
        }
        match p.kind {
            Kind::Line => line_plan(p, cut_y, &mut out),
            _ if p.role == Role::Rebar && p.sweep_r > 0.0 => bar_plan(p, cut_y, &mut out),
            Kind::Ghost => {
                let flat = clip::from_region(&p.loops, FLAT_TOL);
                for s in merge_all(strips(&flat, f64::INFINITY)) {
                    let d1 = s.top.min(cut_y);
                    let mut q = rect_prism(p, s.x0, s.x1, p.z0, p.z1, s.lo.min(d1 - 1e-6), d1);
                    q.pen = Some(p.pen.unwrap_or(Pen::Hidden));
                    out.push(q);
                }
            }
            Kind::Body | Kind::Batt => {
                if p.loops.is_empty() {
                    continue;
                }
                let flat = clip::from_region(&p.loops, FLAT_TOL);
                let end_grain = p.quads.is_empty() && is_lumber_run_y(scene, p);
                let ss = strips(&flat, cut_y);
                let bx = clip::loops_box(&flat);
                let concrete =
                    scene.comps.get(p.comp as usize).is_some_and(|c| c.ty.ty == crate::catalog::Type::Concrete);
                let recess_floor = (bx.y0 + bx.y1) / 2.0;
                let has_cut = ss.iter().any(|s| s.cut);
                for s in ss {
                    let recess = concrete && has_cut && !s.cut && s.top > recess_floor;
                    let top = if recess { s.top.max(cut_y + 1e-6) } else { s.top };
                    let mut q = rect_prism(p, s.x0, s.x1, p.z0, p.z1, s.lo, top);
                    q.kind = Kind::Body;
                    q.recess = recess;
                    if s.cut && p.embedded && ((s.x1 - s.x0) - (p.z1 - p.z0)).abs() <= 0.05 * (s.x1 - s.x0) {
                        let c = plan_pt((s.x0 + s.x1) / 2.0, (p.z0 + p.z1) / 2.0);
                        out.push(dot_prism(p, c, (s.x1 - s.x0) / 2.0, cut_y));
                        continue;
                    }
                    if s.cut && end_grain {
                        let (a, b) = (plan_pt(s.x0, p.z1), plan_pt(s.x1, p.z0));
                        q.quads = alloc::vec![[
                            V2::init(a.x, a.y),
                            V2::init(b.x, a.y),
                            V2::init(b.x, b.y),
                            V2::init(a.x, b.y)
                        ]];
                    }
                    out.push(q);
                }
            }
        }
    }
    (out, own)
}

fn merge_all(strips: Vec<Strip>) -> Vec<Strip> {
    let mut out: Vec<Strip> = Vec::new();
    for s in strips {
        if let Some(last) = out.last_mut()
            && (s.x0 - last.x1).abs() < EPS
        {
            last.x1 = s.x1;
            last.lo = last.lo.min(s.lo);
            last.top = last.top.max(s.top);
            continue;
        }
        out.push(s);
    }
    out
}

fn line_plan(p: &Prism, cut_y: f64, out: &mut Vec<Prism>) {
    let pts = geom::flatten_polyline(&p.line_pts, false, FLAT_TOL);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a.y < cut_y) == (b.y < cut_y) {
            continue;
        }
        let x = a.x + (b.x - a.x) * (cut_y - a.y) / (b.y - a.y);
        out.push(Prism {
            comp: p.comp,
            part: p.part.clone(),
            instance: p.instance,
            material: p.material.clone(),
            kind: Kind::Line,
            line_pts: alloc::vec![Pt::at(plan_pt(x, p.z0), 0.0), Pt::at(plan_pt(x, p.z1), 0.0)],
            z0: cut_y - 1e-6,
            z1: cut_y + 1e-6,
            role: p.role,
            pen: p.pen,
            embedded: p.embedded,
            ..Prism::default()
        });
    }
}

fn plan_box<'p>(ps: impl Iterator<Item = &'p Prism>) -> Box {
    let mut b = Box::default();
    for p in ps {
        let mut xb = Box::default();
        for l in &p.loops {
            xb.add_box(geom::points_box(l));
        }
        xb.add_box(geom::points_box(&p.line_pts));
        if xb.is_empty() {
            continue;
        }
        b.add_point(xb.x0, -p.z1);
        b.add_point(xb.x1, -p.z0);
        for v in &p.footprint {
            b.add_point(v.x, -v.y);
        }
    }
    b
}

fn plan_insts(c: &Comp) -> Vec<PlanInst> {
    (0..c.xfs.len())
        .map(|k| {
            let own = || c.world.iter().filter(move |p| p.instance as usize == k);
            let bx = plan_box(own());
            let zc = c.zs.get(k).map_or((bx.y0 + bx.y1) / -2.0, |z| (z[0] + z[1]) / 2.0);
            let mut parts: Vec<(String, Box)> = Vec::new();
            for p in own() {
                if p.part.is_empty() || parts.iter().any(|(n, _)| *n == p.part) {
                    continue;
                }
                parts.push((p.part.clone(), plan_box(own().filter(|q| q.part == p.part))));
            }
            for zn in &c.built.zones {
                if parts.iter().any(|(n, _)| *n == zn.name) {
                    continue;
                }
                let (Some(xf), Some(z)) = (c.xfs.get(k), c.zs.get(k)) else { continue };
                let mut b = Box::default();
                for corner in [V2::init(zn.box_.x0, zn.box_.y0), V2::init(zn.box_.x1, zn.box_.y1)] {
                    let w = xf.apply(corner);
                    b.add_point(w.x, -z[1]);
                    b.add_point(w.x, -z[0]);
                }
                parts.push((zn.name.clone(), b));
            }
            PlanInst { bx, zc, parts }
        })
        .collect()
}

pub fn scene<'a>(src: &Scene<'a>) -> Scene<'a> {
    let mut s = Scene::new(src.style, src.diags, src.run);
    s.default_z = src.default_z;
    s.instances_total = src.instances_total;
    s.views = src.views.clone();
    s.comps = src
        .comps
        .iter()
        .map(|c| {
            let mut c2 = c.clone();
            c2.plan = plan_insts(c);
            c2
        })
        .collect();
    s
}
