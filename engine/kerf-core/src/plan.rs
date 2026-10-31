//! Plan views (SPEC 26.3): the model seen from above (looking -Y), cut by the horizontal plane y = `cut_y`.
//!
//! Every prism is an XY profile extruded over [z0, z1], so its plan is a set of rectangles in plan coordinates
//! (x, -z): the x-intervals of the profile times the z range. A plan prism keeps the same shape as a section prism,
//! with model y as its depth (higher is nearer the viewer), so the section engine draws the plan with `cut_y` as its
//! cut plane: strips the plane crosses are cut (hatched), strips below it are outlines hidden by higher ones.

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
/// Adjacent strips of one member whose top surfaces meet at a smaller angle than this merge (arc chords, gentle slopes).
const CREASE_RAD: f64 = 0.17;

/// The plan point of a model point (x, z): plan y is -z, so the section viewer at +z stands at the bottom of the plan.
pub fn plan_pt(x: f64, z: f64) -> V2 {
    V2::init(x, -z)
}

/// The default cut height: 1" below the top of the highest drawn member (fills excluded).
pub fn default_cut_y(prisms: &[Prism]) -> f64 {
    let mut top = f64::NEG_INFINITY;
    for p in prisms.iter().filter(|p| p.kind == Kind::Body && p.role != Role::Soil) {
        for l in &p.loops {
            top = top.max(geom::points_box(l).y1);
        }
    }
    if top.is_finite() { top - 1.0 } else { 0.0 }
}

/// One x-interval of a profile: `lo`..`top` is its y extent (`top` capped at the cut), `tl`/`tr` the top at its ends.
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

/// The y intervals of `flat` on the vertical line through x, as (lo, hi) pairs.
fn column(flat: &[Vec<V2>], x: f64) -> Vec<[f64; 2]> {
    let ys = pathclip::scan(flat, x, true);
    ys.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
}

/// The lowest y and the top (capped at `cap`) of the parts of the column at x below `cap`.
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

/// The plan strips of a flattened profile cut at `cut_y` (`f64::INFINITY`: nothing is cut, every strip is below).
fn strips(flat: &[Vec<V2>], cut_y: f64) -> Vec<Strip> {
    let bx = clip::loops_box(flat);
    if bx.is_empty() || bx.y0 >= cut_y - EPS {
        return Vec::new();
    }
    let mut cuts: Vec<[f64; 2]> = Vec::new();
    if bx.y1 > cut_y + EPS {
        let xs = pathclip::scan(flat, cut_y, false);
        cuts = xs.chunks_exact(2).map(|c| [c[0], c[1]]).filter(|c| c[1] - c[0] > EPS).collect();
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

/// Joins neighbouring strips below the cut whose top surfaces continue without a step or crease (the plan draws a
/// line only where the top surface breaks).
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

/// A plan prism over the strip x0..x1 and the model z range z0..z1, with depth (model y) d0..d1.
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
        ..Prism::default()
    }
}

/// A rebar plan prism: a bar of radius r along the plan polyline `center`, depth d0..d1.
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
        ..Prism::default()
    }
}

/// A dot: a bar or bolt end-on in plan, cut by the plane.
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
        ..Prism::default()
    }
}

fn is_lumber_run_y(scene: &Scene, p: &Prism) -> bool {
    scene.comps.get(p.comp as usize).is_some_and(|c| {
        c.ty.traits.lengthwise_grain && c.node.get("run").and_then(crate::json::Value::str) == Some("y")
    })
}

/// Rebar (and other swept bars) in plan: a bar along z is a line, a bar in the XY plane is a line along x where it
/// lies below the cut and a dot where it crosses the cut.
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

/// Bars bent in the XZ plane in a section (cut plane z = `cut_z`) or an elevation (`f64::INFINITY`): a dot where the
/// centreline crosses the cut, the centreline projected on XY where it runs behind the cut, nothing in front of it.
/// Every other prism passes through unchanged.
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

/// An XZ bar in plan: its centreline at its height, cut when the plane passes through it.
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

/// The plan prisms of the drawn world prisms, cut at `cut_y`. Loose fill (earth, gravel) is not drawn in plan.
pub fn prisms(scene: &Scene, world: &[Prism], cut_y: f64) -> Vec<Prism> {
    let mut out = Vec::new();
    for p in world {
        if p.role == Role::Soil {
            continue;
        }
        if p.xz_bar {
            xz_bar_plan(p, cut_y, &mut out);
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
                for s in strips(&flat, cut_y) {
                    let mut q = rect_prism(p, s.x0, s.x1, p.z0, p.z1, s.lo, s.top);
                    q.kind = Kind::Body;
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
    out
}

/// Joins touching strips regardless of their tops (an outline only: hidden members).
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

/// A membrane crossing the cut is a line along z at each crossing; membranes below the cut lie under what is drawn.
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

/// Plan box of a set of world prisms: the x extent of their profiles times their z range, in plan coordinates.
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
    }
    b
}

/// The plan anchors of one component: per instance its plan box, the boxes of its parts and the z of its centre.
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

/// The scene as a plan view sees it: the same components, with Refs resolved in plan coordinates (box anchors on the
/// plan box of the instance or part, named anchors at their x and the z of the instance's centre).
pub fn scene<'a>(src: &Scene<'a>) -> Scene<'a> {
    let mut s = Scene::new(src.style, src.diags, src.run);
    s.default_z = src.default_z;
    s.instances_total = src.instances_total;
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
