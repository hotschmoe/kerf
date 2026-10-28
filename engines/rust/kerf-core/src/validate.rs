//! Geometry checks over a compiled scene: overlaps, floating members, untreated wood on masonry, rebar cover,
//! near-miss gaps, short sloped sheets and the citation tally.

use crate::builders::world_box;
use crate::clip::{self, Op};
use crate::geom::{self, Loc, Pt, V2, Xf};
use crate::json::Value;
use crate::math;
use crate::model::{Cover, Diags, HostInfo, Kind, Level, Prism};
use crate::scene::{Comp, Scene, State, join_ids};
use crate::style::Role;
use crate::units::{self, ftin};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::f64::consts::PI;

const FLAT_TOL: f64 = geom::FLAT_TOL;

struct Pf<'a> {
    prism: &'a Prism,
    flat: Vec<Vec<V2>>,
    box_: geom::Box,
    comp: &'a Comp<'a>,
}

fn box_text(b: geom::Box) -> String {
    format!("x {}..{}, y {}..{}", ftin(b.x0), ftin(b.x1), ftin(b.y0), ftin(b.y1))
}

fn two_places(x: f64) -> String {
    let n = crate::num::to_int_clamped::<u64>(math::round(x.abs() * 100.0), 0, u64::MAX);
    format!("{}{}.{:02}", if x < 0.0 && n != 0 { "-" } else { "" }, n / 100, n % 100)
}

fn seg_dist(p: V2, q: V2, r: V2, s: V2) -> f64 {
    let mut ta = [0.0; 2];
    let mut tb = [0.0; 2];
    if geom::seg_seg(p, q, r, s, &mut ta, &mut tb) > 0 {
        return 0.0;
    }
    geom::dist_point_seg(p, r, s)
        .min(geom::dist_point_seg(q, r, s))
        .min(geom::dist_point_seg(r, p, q).min(geom::dist_point_seg(s, p, q)))
}

fn region_dist(a: &[Vec<V2>], b: &[Vec<V2>], limit: f64) -> f64 {
    let mut best = f64::INFINITY;
    for la in a {
        for lb in b {
            for (i, &p) in la.iter().enumerate() {
                let q = la[(i + 1) % la.len()];
                let bx0 = p.x.min(q.x) - limit;
                let bx1 = p.x.max(q.x) + limit;
                let by0 = p.y.min(q.y) - limit;
                let by1 = p.y.max(q.y) + limit;
                for (k, &r) in lb.iter().enumerate() {
                    let s = lb[(k + 1) % lb.len()];
                    if r.x.min(s.x) > bx1 || r.x.max(s.x) < bx0 || r.y.min(s.y) > by1 || r.y.max(s.y) < by0 {
                        continue;
                    }
                    let d = seg_dist(p, q, r, s);
                    if d < best {
                        best = d;
                        if best == 0.0 {
                            return 0.0;
                        }
                    }
                }
            }
        }
    }
    if let (Some(a0), Some(b0)) = (a.first().and_then(|l| l.first()), b.first().and_then(|l| l.first())) {
        if !b.is_empty() && geom::locate(*a0, b, 0.0) == Loc::Inside {
            return 0.0;
        }
        if !a.is_empty() && geom::locate(*b0, a, 0.0) == Loc::Inside {
            return 0.0;
        }
    }
    best
}

fn z_gap(a: &Prism, b: &Prism) -> f64 {
    (a.z0.max(b.z0) - a.z1.min(b.z1)).max(0.0)
}

pub fn run(scene: &Scene<'_>, doc: &Value, diags: &Diags) {
    let mut items: Vec<Pf<'_>> = Vec::new();
    for c in &scene.comps {
        if c.state != State::Ok {
            continue;
        }
        for p in &c.world {
            if p.kind == Kind::Ghost {
                continue;
            }
            let flat = clip::from_region(&p.loops, FLAT_TOL);
            let box_ = clip::loops_box(&flat);
            items.push(Pf { prism: p, flat, box_, comp: c });
        }
    }
    overlaps(&items, diags);
    floating(scene, &items, diags);
    untreated(&items, diags);
    cover(scene, diags);
    near_miss(scene, diags);
    short_slope(scene, &items, doc, diags);
    for c in &scene.comps {
        if c.state == State::Ok && c.ty.traits.escape_hatch {
            let mat = c.world.first().map_or("?", |p| p.material.as_str());
            diags.add(
                Level::Info,
                "I_SOLID_USED",
                Some(&c.id),
                None,
                format!("{} uses the `solid` escape hatch (material {}); a reviewer should confirm no typed component fits.", c.id, mat),
            );
        }
    }
    unverified_count(doc, diags);
}

fn overlaps(items: &[Pf<'_>], diags: &Diags) {
    let mut reported: Vec<[u32; 2]> = Vec::new();
    for (i, p) in items.iter().enumerate() {
        if p.prism.embedded || p.prism.kind != Kind::Body || p.comp.ty.traits.hardware {
            continue;
        }
        for q in &items[i + 1..] {
            if q.prism.embedded || q.prism.kind != Kind::Body || q.comp.ty.traits.hardware {
                continue;
            }
            if p.comp.index == q.comp.index && p.prism.instance == q.prism.instance {
                continue;
            }
            if z_gap(p.prism, q.prism) > 0.0 || (p.prism.z1.min(q.prism.z1) - p.prism.z0.max(q.prism.z0)) < 1e-6 {
                continue;
            }
            if !p.box_.overlaps(q.box_, 0.0) {
                continue;
            }
            let inter = clip::boolean(&p.flat, &q.flat, Op::Intersect);
            let area = clip::total_area(&inter);
            if area <= 0.01 {
                continue;
            }
            let key = [p.comp.index.min(q.comp.index), p.comp.index.max(q.comp.index)];
            if reported.contains(&key) {
                continue;
            }
            reported.push(key);
            let b = clip::loops_box(&inter);
            diags.add_fix(
                Level::Warning,
                "W_OVERLAP",
                Some(&p.comp.id),
                None,
                format!(
                    "'{}' and '{}' overlap by {} sq in ({}) within z {}..{}",
                    p.comp.id,
                    q.comp.id,
                    two_places(area),
                    box_text(b),
                    ftin(p.prism.z0.max(q.prism.z0)),
                    ftin(p.prism.z1.min(q.prism.z1)),
                ),
                "move or resize one of them so they only touch, or mark the inner one embedded:true if it is reinforcement or hardware".to_string(),
            );
        }
    }
}

fn floating(scene: &Scene<'_>, items: &[Pf<'_>], diags: &Diags) {
    let gap = 1.0 / 32.0;
    let n_comps = scene.comps.iter().filter(|c| c.state == State::Ok && !c.dashed).count();
    if n_comps < 2 {
        return;
    }
    for c in &scene.comps {
        if c.state != State::Ok || c.dashed {
            continue;
        }
        let touches = items.iter().filter(|p| p.comp.index == c.index).any(|p| {
            items.iter().any(|q| {
                q.comp.index != c.index
                    && z_gap(p.prism, q.prism) <= gap
                    && p.box_.expand(gap).overlaps(q.box_, 0.0)
                    && region_dist(&p.flat, &q.flat, gap) <= gap
            })
        });
        if !touches {
            diags.add_fix(
                Level::Warning,
                "W_FLOATING",
                Some(&c.id),
                None,
                format!("component '{}' touches nothing (gap > 1/32\" to every other component, in plan and in z); it may be mis-placed", c.id),
                "check its `at` anchor/offset, or `z` range if it should bear on another member".to_string(),
            );
        }
    }
}

fn untreated(items: &[Pf<'_>], diags: &Diags) {
    let mut reported: Vec<[u32; 2]> = Vec::new();
    let gap = 1.0 / 32.0;
    for p in items {
        if p.prism.role != Role::Wood {
            continue;
        }
        for q in items {
            if !q.prism.role.is_masonry() || z_gap(p.prism, q.prism) > gap {
                continue;
            }
            if !p.box_.expand(gap).overlaps(q.box_, 0.0) || region_dist(&p.flat, &q.flat, gap) > gap {
                continue;
            }
            let key = [p.comp.index, q.comp.index];
            if reported.contains(&key) {
                continue;
            }
            reported.push(key);
            diags.add_fix(
                Level::Warning,
                "W_UNTREATED_CONTACT",
                Some(&p.comp.id),
                None,
                format!(
                    "untreated wood '{}' touches {} '{}' (IRC R317.1 requires preservative-treated wood or a barrier where wood contacts concrete or masonry)",
                    p.comp.id, q.prism.material, q.comp.id
                ),
                "set \"treated\": true on the wood member (or add a sill seal/barrier and note it)".to_string(),
            );
        }
    }
}

struct ZoneBox<'a> {
    name: &'a str,
    box_: geom::Box,
}

#[derive(Clone, Copy)]
struct Edge {
    a: V2,
    b: V2,
    class: usize,
}

fn host_edges(outline: &[Pt], xf: &Xf) -> Vec<Edge> {
    let world = xf.apply_loop(outline);
    let flat = geom::flatten_loop(&world, FLAT_TOL);
    let ccw = geom::signed_area_v(&flat) >= 0.0;
    let mut out = Vec::new();
    for (i, &p) in flat.iter().enumerate() {
        let q = flat[(i + 1) % flat.len()];
        let d = q.sub(p);
        let l = d.len();
        if l < 1e-9 {
            continue;
        }
        let nrm = if ccw { V2::init(d.y / l, -d.x / l) } else { V2::init(-d.y / l, d.x / l) };
        let class = if nrm.y < -0.5 {
            0
        } else if nrm.y > 0.5 {
            1
        } else {
            2
        };
        out.push(Edge { a: p, b: q, class });
    }
    out
}

fn cover_for(h: &HostInfo, zones_world: &[ZoneBox<'_>], c: V2) -> Cover {
    for pc in &h.part_cover {
        if zones_world.iter().any(|z| z.name == pc.part && z.box_.contains(c)) {
            return pc.cover;
        }
    }
    h.cover
}

struct Piece {
    segs: Vec<[V2; 2]>,
    leg: usize,
    is_bend: bool,
    p0: V2,
    p1: V2,
}

fn line_intersect(p: V2, d: V2, q: V2, e: V2) -> Option<V2> {
    let den = d.cross(e);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = q.sub(p).cross(e) / den;
    Some(p.add(d.scale(t)))
}

fn bar_pieces(cl: &[Pt]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut legs = 0;
    for w in cl.windows(2) {
        let mut fl = alloc::vec![w[0].v()];
        geom::flatten_seg_into(&mut fl, w[0].v(), w[1].v(), w[0].b, FLAT_TOL);
        let segs: Vec<[V2; 2]> = fl.windows(2).map(|s| [s[0], s[1]]).collect();
        let bend = w[0].b != 0.0;
        if !bend {
            legs += 1;
        }
        out.push(Piece { segs, leg: legs, is_bend: bend, p0: w[0].v(), p1: w[1].v() });
    }
    for i in 0..out.len() {
        if out[i].is_bend || out[i].p0.dist(out[i].p1) < 1e-6 {
            continue;
        }
        let d = out[i].p1.sub(out[i].p0).norm();
        if i >= 2 && out[i - 1].is_bend {
            let prev = &out[i - 2];
            if !prev.is_bend
                && prev.p0.dist(prev.p1) > 1e-6
                && let Some(x) = line_intersect(out[i].p0, d, prev.p0, prev.p1.sub(prev.p0).norm())
            {
                out[i].p0 = x;
            }
        }
        if i + 2 < out.len() && out[i + 1].is_bend {
            let next = &out[i + 2];
            if !next.is_bend
                && next.p0.dist(next.p1) > 1e-6
                && let Some(x) = line_intersect(out[i].p0, d, next.p0, next.p1.sub(next.p0).norm())
            {
                out[i].p1 = x;
            }
        }
    }
    out
}

fn piece_text(pieces: &[Piece], k: usize) -> String {
    let Some(pc) = pieces.get(k) else { return "?".to_string() };
    let nlegs = pieces.iter().filter(|q| !q.is_bend).count();
    if pc.is_bend {
        let m = pc.segs.get(pc.segs.len() / 2).map_or(pc.p0, |s| s[0]);
        return format!("the bend after segment {} of {} (near x {}, y {})", pc.leg, nlegs, ftin(m.x), ftin(m.y));
    }
    format!(
        "segment {} of {} (x {}, y {} to x {}, y {})",
        pc.leg,
        nlegs,
        ftin(pc.p0.x),
        ftin(pc.p0.y),
        ftin(pc.p1.x),
        ftin(pc.p1.y)
    )
}

fn cover(scene: &Scene<'_>, diags: &Diags) {
    for bar in &scene.comps {
        if bar.state != State::Ok || !bar.ty.traits.reinforcement {
            continue;
        }
        let r = bar.built.bar_d / 2.0;
        for bp in &bar.world {
            let centre;
            let mut dir = V2::init(0.0, 0.0);
            let mut segs: Vec<[V2; 2]> = Vec::new();
            let mut pieces: Vec<Piece> = Vec::new();
            if bp.centerline.len() >= 2 {
                pieces = bar_pieces(&bp.centerline);
                let flat = geom::flatten_polyline(&bp.centerline, false, FLAT_TOL);
                segs = flat.windows(2).map(|s| [s[0], s[1]]).collect();
                let mut longest = 0;
                let mut ll = -1.0;
                for (i, s) in segs.iter().enumerate() {
                    let l = s[0].dist(s[1]);
                    if l > ll {
                        ll = l;
                        longest = i;
                    }
                }
                let Some(ls) = segs.get(longest) else { continue };
                centre = ls[0].mid(ls[1]);
                dir = ls[1].sub(ls[0]).norm();
            } else {
                let Some(l0) = bp.loops.first() else { continue };
                centre = geom::loop_box(l0).center();
            }
            for h in &scene.comps {
                let Some(host) = h.built.host.as_ref().filter(|_| h.state == State::Ok) else { continue };
                let Some(hz0) = h.zs.first() else { continue };
                if bp.z0 < hz0[0] - 1e-6 && bp.z1 < hz0[0] - 1e-6 {
                    continue;
                }
                let mut found = false;
                for (hi, xf) in h.xfs.iter().enumerate() {
                    let Some(hz) = h.zs.get(hi) else { break };
                    if bp.z1 < hz[0] - 1e-6 || bp.z0 > hz[1] + 1e-6 {
                        continue;
                    }
                    let wl = xf.apply_loop(&host.outline);
                    let fl = geom::flatten_loop(&wl, FLAT_TOL);
                    let Some(&f0) = fl.first() else { continue };
                    if !geom::point_in_loop_eo(centre, &fl) && geom::dist_point_seg(centre, f0, f0) > 0.0 {
                        let onb = fl
                            .iter()
                            .enumerate()
                            .any(|(i, &p)| geom::dist_point_seg(centre, p, fl[(i + 1) % fl.len()]) < 1e-6);
                        if !onb {
                            continue;
                        }
                    }
                    found = true;
                    let edges = host_edges(&host.outline, xf);
                    let zw: Vec<ZoneBox<'_>> =
                        h.built.zones.iter().map(|z| ZoneBox { name: &z.name, box_: world_box(xf, z.box_) }).collect();
                    let req = cover_for(host, &zw, centre);
                    let mut min_clear = [f64::INFINITY; 3];
                    let mut min_piece = [0usize; 3];
                    for e in &edges {
                        let dist;
                        if !segs.is_empty() {
                            let ed = e.b.sub(e.a).norm();
                            if dir.dot(ed.perp()).abs() > 0.7 {
                                continue;
                            }
                            let mut d = f64::INFINITY;
                            let mut best_key = f64::INFINITY;
                            let mut best_pi = 0;
                            for (pi, pc) in pieces.iter().enumerate() {
                                let mut pd = f64::INFINITY;
                                for s in &pc.segs {
                                    pd = pd.min(seg_dist(s[0], s[1], e.a, e.b));
                                }
                                let key = pd + if pc.is_bend { 1e-4 } else { 0.0 };
                                if key < best_key - 1e-12 {
                                    best_key = key;
                                    best_pi = pi;
                                }
                                d = d.min(pd);
                            }
                            dist = d;
                            if dist - r < min_clear[e.class] {
                                min_piece[e.class] = best_pi;
                            }
                        } else {
                            dist = geom::dist_point_seg(centre, e.a, e.b);
                        }
                        let clear = dist - r;
                        if clear < min_clear[e.class] {
                            min_clear[e.class] = clear;
                        }
                    }
                    let reqs = [req.bottom, req.top, req.sides];
                    let names = ["bottom", "top", "sides"];
                    for k in 0..3 {
                        if min_clear[k] < reqs[k] - 1e-3 {
                            let fix = "move the bar inward (increase place.cover / side_cover or the offset) or lower the host's cover requirement".to_string();
                            let message = if !pieces.is_empty() {
                                format!(
                                    "clear cover from {}, {}, to the {} of '{}' is {} < required {}",
                                    bar.built.info,
                                    piece_text(&pieces, min_piece[k]),
                                    names[k],
                                    h.id,
                                    ftin(min_clear[k]),
                                    ftin(reqs[k]),
                                )
                            } else {
                                format!(
                                    "clear cover from {} bar to the {} of '{}' is {} < required {}; bar center at x {}, y {}",
                                    bar.built.info,
                                    names[k],
                                    h.id,
                                    ftin(min_clear[k]),
                                    ftin(reqs[k]),
                                    ftin(centre.x),
                                    ftin(centre.y),
                                )
                            };
                            diags.add_fix(Level::Warning, "W_COVER", Some(&bar.id), None, message, fix);
                        }
                    }
                    break;
                }
                if found {
                    break;
                }
            }
        }
    }
}

fn unverified_count(doc: &Value, diags: &Diags) {
    let mut n = 0usize;
    let mut ids: Vec<String> = Vec::new();
    if let Some(va) = doc.get("views").and_then(Value::arr) {
        for v in va {
            let vid = v.get("id").map_or("?", |x| x.str().unwrap_or("?"));
            let Some(aa) = v.get("annotations").and_then(Value::arr) else { continue };
            for x in aa {
                let mut mine = false;
                if let Some(ca) = x.get("cite").and_then(Value::arr) {
                    for c in ca {
                        let st = c.get("status").map_or("suggested", |s| s.str().unwrap_or("suggested"));
                        if st != "verified" {
                            n += 1;
                            mine = true;
                        }
                    }
                }
                if mine {
                    ids.push(format!("{}/{}", vid, x.get("id").map_or("?", |i| i.str().unwrap_or("?"))));
                }
            }
        }
    }
    if n > 0 {
        diags.add(
            Level::Info,
            "I_UNVERIFIED_CITE",
            None,
            None,
            format!("{} citation(s) await designer verification (notes: {}). Citations print with a trailing * until verified.", n, join_ids(&ids)),
        );
    }
}

#[derive(Clone, Copy)]
struct CompBox<'a> {
    c: &'a Comp<'a>,
    box_: geom::Box,
    z0: f64,
    z1: f64,
}

fn near_miss(scene: &Scene<'_>, diags: &Diags) {
    let mut boxes: Vec<CompBox<'_>> = Vec::new();
    for c in &scene.comps {
        if c.state != State::Ok || !c.ty.traits.near_miss || !axis_aligned(c) {
            continue;
        }
        let mut b = geom::Box::default();
        let mut z0 = f64::INFINITY;
        let mut z1 = f64::NEG_INFINITY;
        for p in &c.world {
            if p.kind == Kind::Ghost {
                continue;
            }
            for l in &p.loops {
                b.add_box(geom::loop_box(l));
            }
            z0 = z0.min(p.z0);
            z1 = z1.max(p.z1);
        }
        if b.is_empty() {
            continue;
        }
        boxes.push(CompBox { c, box_: b, z0, z1 });
    }
    let min_gap = 1.0 / 32.0;
    let max_gap = 3.0;
    for (i, &ca) in boxes.iter().enumerate() {
        for &cb in &boxes[i + 1..] {
            if ca.z1.min(cb.z1) - ca.z0.max(cb.z0) <= 1e-6 {
                continue;
            }
            for on_x in [false, true] {
                let sel = |b: geom::Box| if on_x { [b.x0, b.x1, b.y0, b.y1] } else { [b.y0, b.y1, b.x0, b.x1] };
                let [a0, a1, o0, o1] = sel(ca.box_);
                let [b0, b1, q0, q1] = sel(cb.box_);
                let overlap = o1.min(q1) - o0.max(q0);
                let gap = a0.max(b0) - a1.min(b1);
                if !(overlap > 1e-6 && gap >= min_gap - 1e-9 && gap <= max_gap + 1e-9) {
                    continue;
                }
                let g0 = a1.min(b1);
                let g1 = a0.max(b0);
                let lo = o0.max(q0);
                let hi = o1.min(q1);
                let blocked = boxes.iter().any(|cc| {
                    if cc.c.index == ca.c.index || cc.c.index == cb.c.index {
                        return false;
                    }
                    let [c0, c1, d0, d1] = sel(cc.box_);
                    c1.min(g1) - c0.max(g0) > 1e-6 && d1.min(hi) - d0.max(lo) > 1e-6
                });
                if blocked {
                    continue;
                }
                let a_len = a1 - a0;
                let b_len = b1 - b0;
                let a_across = o1 - o0;
                let b_across = q1 - q0;
                let (grow, other) =
                    if b_len / b_across.max(1e-9) > a_len / a_across.max(1e-9) { (cb, ca) } else { (ca, cb) };
                let grow_is_a = grow.c.index == ca.c.index;
                let grow_len = if grow_is_a { a_len } else { b_len };
                let grow_below_other = if grow_is_a { a1 <= b0 } else { b1 <= a0 };
                let edge_name = if on_x {
                    if grow_below_other { "left" } else { "right" }
                } else if grow_below_other {
                    "bottom"
                } else {
                    "top"
                };
                let anchor = if on_x {
                    if grow_below_other { "middle_left" } else { "middle_right" }
                } else if grow_below_other {
                    "bottom_left"
                } else {
                    "top_left"
                };
                diags.add_fix(
                    Level::Warning,
                    "W_NEAR_MISS",
                    Some(&grow.c.id),
                    None,
                    format!(
                        "'{}' and '{}' leave a {} gap along {} ({}..{}) between facing edges while overlapping along {}; '{}' probably should extend to the {} edge of '{}'",
                        grow.c.id,
                        other.c.id,
                        ftin(gap),
                        if on_x { "x" } else { "y" },
                        ftin(g0),
                        ftin(g1),
                        if on_x { "y" } else { "x" },
                        grow.c.id,
                        edge_name,
                        other.c.id,
                    ),
                    format!(
                        "set the length of '{}' to {}, or use \"until\": \"{}@{}\" so it follows '{}'; ignore this warning if the gap is intended",
                        grow.c.id,
                        ftin(grow_len + gap),
                        other.c.id,
                        anchor,
                        other.c.id
                    ),
                );
            }
        }
    }
}

fn explicit_crop(doc: &Value) -> Option<geom::Box> {
    for v in doc.get("views")?.arr()? {
        if v.get("kind").and_then(Value::str) == Some("iso") {
            continue;
        }
        let Some(cv) = v.get("crop").filter(|c| !c.is_null()) else { continue };
        let Some(xr) = range2(cv.get("x")) else { continue };
        let Some(yr) = range2(cv.get("y")) else { continue };
        return Some(geom::Box { x0: xr[0], x1: xr[1], y0: yr[0], y1: yr[1] });
    }
    None
}

fn range2(v: Option<&Value>) -> Option<[f64; 2]> {
    let arr = v?.arr()?;
    if arr.len() != 2 {
        return None;
    }
    Some([units::parse_length(&arr[0])?, units::parse_length(&arr[1])?])
}

fn host_angle(h: &Comp<'_>) -> Option<f64> {
    h.ty.traits.slope_host?;
    Some(if h.ty.traits.has_pitch { h.angle + h.pitch_angle } else { h.angle })
}

fn same_slope(a1: f64, a2: f64) -> bool {
    let x = a1 - a2;
    let mut d = x % PI;
    if d < 0.0 {
        d += PI;
    }
    if d > PI / 2.0 {
        d = PI - d;
    }
    d < 0.5 * (PI / 180.0)
}

fn proj_extent(items: &[Pf<'_>], c: &Comp<'_>, part: Option<&str>, u: V2) -> Option<[f64; 2]> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for it in items {
        if it.comp.index != c.index || it.prism.instance != 0 {
            continue;
        }
        if part.is_some_and(|pn| it.prism.part != pn) {
            continue;
        }
        for l in &it.flat {
            for v in l {
                let t = v.x * u.x + v.y * u.y;
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
    }
    (lo <= hi).then_some([lo, hi])
}

fn short_slope(scene: &Scene<'_>, items: &[Pf<'_>], doc: &Value, diags: &Diags) {
    let crop = explicit_crop(doc);
    let slack = 0.5;
    let touch = 1.0 / 16.0;
    for c in &scene.comps {
        if c.state != State::Ok || c.dashed || !c.ty.traits.sheet {
            continue;
        }
        let mut u = V2::init(math::cos(c.angle), math::sin(c.angle));
        if u.y.abs() < math::sin(0.5 * (PI / 180.0)) {
            continue;
        }
        if u.y < 0.0 {
            u = V2::init(-u.x, -u.y);
        }
        let mut pmax = f64::NEG_INFINITY;
        let mut vtx = V2::init(0.0, 0.0);
        for it in items {
            if it.comp.index != c.index || it.prism.instance != 0 {
                continue;
            }
            for l in &it.flat {
                for v in l {
                    let t = v.x * u.x + v.y * u.y;
                    if t > pmax {
                        pmax = t;
                        vtx = *v;
                    }
                }
            }
        }
        if pmax == f64::NEG_INFINITY {
            continue;
        }
        let mut best: Option<&Comp<'_>> = None;
        let mut best_end = 0.0;
        for h in &scene.comps {
            if h.index == c.index || h.state != State::Ok || h.dashed {
                continue;
            }
            if c.ty.traits.rigid_sheet && h.ty.traits.rigid_sheet {
                continue;
            }
            let Some(ha) = host_angle(h) else { continue };
            if !same_slope(ha, c.angle) {
                continue;
            }
            let part = h.ty.traits.slope_host.map(|s| s.part).filter(|p| !p.is_empty());
            let mut touching = false;
            for sp in items.iter().filter(|s| s.comp.index == c.index && s.prism.instance == 0) {
                for hp in items.iter().filter(|s| s.comp.index == h.index && s.prism.instance == 0) {
                    if part.is_some_and(|pn| hp.prism.part != pn) {
                        continue;
                    }
                    if z_gap(sp.prism, hp.prism) > touch || !sp.box_.expand(touch).overlaps(hp.box_, 0.0) {
                        continue;
                    }
                    if region_dist(&sp.flat, &hp.flat, touch) <= touch {
                        touching = true;
                    }
                }
            }
            if !touching {
                continue;
            }
            let Some(ext) = proj_extent(items, h, part, u) else { continue };
            if best.is_none() || ext[1] > best_end {
                best = Some(h);
                best_end = ext[1];
            }
        }
        let Some(h) = best else { continue };
        let mut crop_room = f64::INFINITY;
        if let Some(cb) = crop {
            if u.x > 1e-9 {
                crop_room = crop_room.min((cb.x1 - vtx.x) / u.x);
            } else if u.x < -1e-9 {
                crop_room = crop_room.min((cb.x0 - vtx.x) / u.x);
            }
            if u.y > 1e-9 {
                crop_room = crop_room.min((cb.y1 - vtx.y) / u.y);
            }
        }
        let gap_host = best_end - pmax;
        let short = gap_host.min(crop_room);
        if short <= slack {
            continue;
        }
        let by_crop = crop_room < gap_host;
        let end_anchor = h.ty.traits.slope_host.map_or("", |s| s.end_anchor);
        let anchor = if !end_anchor.is_empty() {
            end_anchor
        } else if u.x >= 0.0 {
            "top_right"
        } else {
            "top_left"
        };
        let what = if by_crop {
            format!("the view crop (where '{}' continues)", h.id)
        } else {
            format!("the end of '{}'", h.id)
        };
        diags.add_fix(
            Level::Warning,
            "W_SHORT_SLOPE",
            Some(&c.id),
            None,
            format!("'{}' rests on the sloped '{}' (same slope) but its upper end stops {} short of {}", c.id, h.id, ftin(short), what),
            format!(
                "replace the literal length of '{}' (it does not follow the pitch) with \"until\": \"{}@{}\" (grows along the slope to the member's end; pair it with \"slope\": \"@{}\"), or lengthen it by {}; acknowledge W_SHORT_SLOPE if it should stop there",
                c.id, h.id, anchor, h.id, ftin(short)
            ),
        );
    }
}

fn axis_aligned(c: &Comp<'_>) -> bool {
    for p in &c.world {
        if p.kind == Kind::Ghost {
            continue;
        }
        if p.embedded || p.loops.len() != 1 || p.loops[0].len() != 4 {
            return false;
        }
        let l = &p.loops[0];
        for (i, v) in l.iter().enumerate() {
            let w = l[(i + 1) % 4];
            if v.b != 0.0 {
                return false;
            }
            if (v.x - w.x).abs() > 1e-9 && (v.y - w.y).abs() > 1e-9 {
                return false;
            }
        }
    }
    true
}
