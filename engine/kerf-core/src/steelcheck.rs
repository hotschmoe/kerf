//! Steel connection checks over a compiled scene (warnings, never errors): bolt edge distance against AISC 360 Table J3.4
//! (W_BOLT_EDGE), bolt spacing against J3.3 (W_BOLT_SPACING) and fillet weld size against the thinner joined part, J2.2b
//! and Table J2.4 (W_WELD_SIZE); anchor bolts against ACI 318-19 17.9: clear distance from the shaft to the concrete or
//! masonry edge (W_BOLT_EDGE) and spacing between anchors (W_BOLT_SPACING).

use crate::builders::steel_shapes;
use crate::catalog::Type;
use crate::clip::{self, Op};
use crate::geom::{self, FLAT_TOL, Loc, V2};
use crate::json::Value;
use crate::model::{Diags, Kind, Level, Prism};
use crate::scene::{Comp, Scene, State};
use crate::units::{self, fmt_ft_in};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// AISC 360-16 Table J3.4: minimum edge distance from the center of a standard hole, by bolt diameter.
const EDGE_MIN: [(f64, f64); 7] =
    [(0.5, 0.75), (0.625, 0.875), (0.75, 1.0), (0.875, 1.125), (1.0, 1.25), (1.125, 1.5), (1.25, 1.625)];

/// Minimum edge distance for a bolt of diameter `d` (the next tabulated size up; 1.25 d above 1 1/4").
pub fn edge_min(d: f64) -> f64 {
    EDGE_MIN.iter().find(|(dt, _)| d <= dt + 1e-9).map_or(1.25 * d, |e| e.1)
}

fn len_of(node: &Value, key: &str, default: f64) -> f64 {
    node.get(key).and_then(units::parse_length).unwrap_or(default)
}

pub fn run(scene: &Scene<'_>, doc: &Value, diags: &Diags) {
    weld_sizes(scene, doc, diags);
    for b in scene.comps.iter().filter(|c| c.state == State::Ok && c.ty.ty == Type::Bolt) {
        let d = len_of(b.node, "diameter", 0.75);
        let count = b.node.get("count").and_then(Value::num).unwrap_or(1.0);
        let spacing = len_of(b.node, "spacing", 3.0);
        if count > 1.0 && spacing < 8.0 / 3.0 * d - 1e-9 {
            diags.add_fix(
                Level::Warning,
                "W_BOLT_SPACING",
                Some(&b.id),
                Some(&format!("components/{}/spacing", b.id)),
                format!(
                    "bolt '{}': spacing {} is below the AISC 360 J3.3 minimum of 2 2/3 d = {} for {} bolts (3d = {} preferred)",
                    b.id,
                    fmt_ft_in(spacing),
                    fmt_ft_in(8.0 / 3.0 * d),
                    fmt_ft_in(d),
                    fmt_ft_in(3.0 * d)
                ),
                format!("set \"spacing\" to at least {} (e.g. {})", fmt_ft_in(8.0 / 3.0 * d), fmt_ft_in(3.0 * d)),
            );
        }
        edge_distance(scene, b, d, diags);
    }
    anchors(scene, diags);
}

/// ACI 318-19 17.9.2 / 20.5.1.3: an anchor's shaft keeps at least this clear distance to the concrete or masonry edge.
const ANCHOR_EDGE_CLEAR: f64 = 1.5;

/// The straight embedded shaft of one anchor bolt instance: from the top of its centerline to the bend (or its end).
struct Shaft<'a> {
    c: &'a Comp<'a>,
    instance: u32,
    a: V2,
    b: V2,
    zc: f64,
    d: f64,
    post: bool,
}

fn shafts<'a>(scene: &'a Scene<'a>) -> Vec<Shaft<'a>> {
    let mut out = Vec::new();
    for c in scene.comps.iter().filter(|c| c.state == State::Ok && c.ty.ty == Type::AnchorBolt && c.visible && !c.dashed) {
        let d = len_of(c.node, "diameter", 0.5);
        let post = matches!(c.node.get("hook").and_then(Value::str), Some("wedge" | "screw"));
        for p in c.world.iter().filter(|p| p.part == "shank" && p.centerline.len() >= 2) {
            out.push(Shaft {
                c,
                instance: p.instance,
                a: p.centerline[0].v(),
                b: p.centerline[1].v(),
                zc: (p.z0 + p.z1) / 2.0,
                d,
                post,
            });
        }
    }
    out
}

fn anchor_name(s: &Shaft<'_>) -> String {
    if s.c.arr_count > 1 { format!("'{}' (instance {})", s.c.id, s.instance) } else { format!("'{}'", s.c.id) }
}

fn anchors(scene: &Scene<'_>, diags: &Diags) {
    let list = shafts(scene);
    let mut reported: Vec<u32> = Vec::new();
    for s in &list {
        if reported.contains(&s.c.index) {
            continue;
        }
        let hosts: Vec<(&Comp<'_>, Vec<Vec<V2>>)> = scene
            .comps
            .iter()
            .filter(|h| h.state == State::Ok && h.visible && !h.dashed)
            .flat_map(|h| {
                h.world
                    .iter()
                    .filter(|p| {
                        p.role.is_masonry() && p.kind == Kind::Body && !p.embedded && p.z0 <= s.zc && p.z1 >= s.zc
                    })
                    .map(move |p| (h, clip::from_region(&p.loops, FLAT_TOL)))
            })
            .collect();
        if hosts.is_empty() {
            continue;
        }
        let mut region: Vec<Vec<V2>> = Vec::new();
        for (_, f) in &hosts {
            region = if region.is_empty() { f.clone() } else { clip::boolean(&region, f, Op::Unite) };
        }
        let perp = s.b.sub(s.a).norm().perp();
        let steps = 32;
        let mut worst: Option<(f64, V2)> = None;
        for i in 0..=steps {
            let m = s.a.lerp(s.b, i as f64 / steps as f64);
            if geom::locate(m, &region, 0.0) != Loc::Inside {
                continue;
            }
            let e = ray_to_edge(m, perp, &region).min(ray_to_edge(m, perp.scale(-1.0), &region));
            if worst.is_none_or(|w| e < w.0) {
                worst = Some((e, m));
            }
        }
        let Some((e, m)) = worst else { continue };
        let clear = e - s.d / 2.0;
        if clear >= ANCHOR_EDGE_CLEAR - 1.0 / 32.0 {
            continue;
        }
        let host = hosts
            .iter()
            .find(|(_, f)| geom::locate(m, f, 1e-9) != Loc::Outside)
            .map_or("the concrete", |(h, _)| h.id.as_str());
        reported.push(s.c.index);
        diags.add_fix(
            Level::Warning,
            "W_BOLT_EDGE",
            Some(&s.c.id),
            Some(&format!("components/{}", s.c.id)),
            format!(
                "anchor bolt {}: the shaft is {} clear of the edge of '{host}' ({} from its center) near x {}, y {}, below the {} clear cover ACI 318-19 17.9.2 requires for an anchor (20.5.1.3)",
                anchor_name(s),
                fmt_ft_in(clear),
                fmt_ft_in(e),
                fmt_ft_in(m.x),
                fmt_ft_in(m.y),
                fmt_ft_in(ANCHOR_EDGE_CLEAR)
            ),
            format!(
                "move the anchor at least {} into '{host}' (shaft center {} from the edge), or acknowledge W_BOLT_EDGE if a product evaluation (ESR) allows less",
                fmt_ft_in(ANCHOR_EDGE_CLEAR - clear),
                fmt_ft_in(ANCHOR_EDGE_CLEAR + s.d / 2.0)
            ),
        );
    }
    let mut pairs: Vec<[u32; 2]> = Vec::new();
    for (i, s) in list.iter().enumerate() {
        for t in &list[i + 1..] {
            if s.c.index == t.c.index && s.instance == t.instance {
                continue;
            }
            let plane = seg_seg(s.a, s.b, t.a, t.b);
            let dz = s.zc - t.zc;
            let dist = crate::math::sqrt(plane * plane + dz * dz);
            let d = s.d.max(t.d);
            let k = if s.post || t.post { 6.0 } else { 4.0 };
            if dist >= k * d - 1.0 / 32.0 {
                continue;
            }
            let key = [s.c.index.min(t.c.index), s.c.index.max(t.c.index)];
            if pairs.contains(&key) {
                continue;
            }
            pairs.push(key);
            diags.add_fix(
                Level::Warning,
                "W_BOLT_SPACING",
                Some(&s.c.id),
                Some(&format!("components/{}", s.c.id)),
                format!(
                    "anchor bolts {} and {} are {} apart (center to center, {} in the drawing plane, {} along z), below the ACI 318-19 17.9.2 minimum of {}d = {} for {} anchors",
                    anchor_name(s),
                    anchor_name(t),
                    fmt_ft_in(dist),
                    fmt_ft_in(plane),
                    fmt_ft_in(dz.abs()),
                    if k == 6.0 { 6 } else { 4 },
                    fmt_ft_in(k * d),
                    if k == 6.0 { "post-installed" } else { "cast-in" }
                ),
                format!("space them at least {} apart (move one in x/y, or in z with \"z\")", fmt_ft_in(k * d)),
            );
        }
    }
}

fn seg_seg(p: V2, q: V2, r: V2, s: V2) -> f64 {
    let mut ta = [0.0; 2];
    let mut tb = [0.0; 2];
    if geom::seg_seg(p, q, r, s, &mut ta, &mut tb) > 0 {
        return 0.0;
    }
    geom::dist_point_seg(p, r, s)
        .min(geom::dist_point_seg(q, r, s))
        .min(geom::dist_point_seg(r, p, q).min(geom::dist_point_seg(s, p, q)))
}

/// One bolt: its axis segment through the grip (side) or its center (end), and the grip's z range.
struct Bolt {
    k: usize,
    instance: u32,
    side: bool,
    a: V2,
    b: V2,
    z0: f64,
    z1: f64,
}

fn bolts_of(c: &Comp<'_>, grip: f64) -> Vec<Bolt> {
    let side = c.node.get("orient").and_then(Value::str) != Some("end");
    let mut out = Vec::new();
    let mut per_inst: Vec<(u32, usize)> = Vec::new();
    let heads: Vec<&Prism> = c.world.iter().filter(|p| p.part == "head").collect();
    for p in c.world.iter().filter(|p| p.part == "shank") {
        let k = match per_inst.iter_mut().find(|e| e.0 == p.instance) {
            Some(e) => {
                e.1 += 1;
                e.1
            }
            None => {
                per_inst.push((p.instance, 1));
                1
            }
        };
        if side {
            let (Some(top), Some(bot)) = (p.centerline.first(), p.centerline.last()) else { continue };
            let dir = bot.v().sub(top.v()).norm();
            let a = top.v();
            let zc = (p.z0 + p.z1) / 2.0;
            out.push(Bolt { k, instance: p.instance, side, a, b: a.add(dir.scale(grip)), z0: zc, z1: zc });
        } else {
            let c0 = clip::loops_box(&clip::from_region(&p.loops, FLAT_TOL)).center();
            let bearing = heads.iter().find(|h| h.instance == p.instance).map_or(p.z1, |h| h.z0);
            out.push(Bolt { k, instance: p.instance, side, a: c0, b: c0, z0: bearing - grip, z1: bearing });
        }
    }
    out
}

/// Distance from `m` along `dir` to the first crossing of the loops' boundary.
fn ray_to_edge(m: V2, dir: V2, loops: &[Vec<V2>]) -> f64 {
    let mut best = f64::INFINITY;
    for lp in loops {
        for (i, &p) in lp.iter().enumerate() {
            let q = lp[(i + 1) % lp.len()];
            let e = q.sub(p);
            let den = dir.cross(e);
            if den.abs() < 1e-12 {
                continue;
            }
            let w = p.sub(m);
            let t = w.cross(e) / den;
            let s = w.cross(dir) / den;
            if t > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&s) {
                best = best.min(t);
            }
        }
    }
    best
}

fn dist_to_boundary(m: V2, loops: &[Vec<V2>]) -> f64 {
    let mut best = f64::INFINITY;
    for lp in loops {
        for (i, &p) in lp.iter().enumerate() {
            best = best.min(geom::dist_point_seg(m, p, lp[(i + 1) % lp.len()]));
        }
    }
    best
}

fn edge_distance(scene: &Scene<'_>, bc: &Comp<'_>, d: f64, diags: &Diags) {
    let grip = len_of(bc.node, "grip", 1.0);
    let need = edge_min(d);
    let bolts = bolts_of(bc, grip);
    for s in scene.comps.iter().filter(|c| c.state == State::Ok && c.ty.ty == Type::Steel && !c.dashed) {
        let mut worst: Option<(f64, &Bolt, &'static str)> = None;
        let n_inst = s.world.iter().map(|p| p.instance + 1).max().unwrap_or(0);
        for bolt in &bolts {
            for inst in 0..n_inst {
                let hits: Vec<&Prism> = s
                    .world
                    .iter()
                    .filter(|p| {
                        p.instance == inst && p.kind == Kind::Body && p.z0 <= bolt.z1 + 1e-9 && p.z1 >= bolt.z0 - 1e-9
                    })
                    .collect();
                if hits.is_empty() {
                    continue;
                }
                let mut region: Vec<Vec<V2>> = Vec::new();
                for p in &hits {
                    let f = clip::from_region(&p.loops, FLAT_TOL);
                    region = if region.is_empty() { f } else { clip::boolean(&region, &f, Op::Unite) };
                }
                let (dist, how) = if bolt.side {
                    let steps = 32;
                    let inside: Vec<f64> = (0..=steps)
                        .map(|i| i as f64 / steps as f64)
                        .filter(|t| geom::locate(bolt.a.lerp(bolt.b, *t), &region, 0.0) == Loc::Inside)
                        .collect();
                    let (Some(t0), Some(t1)) = (inside.first(), inside.last()) else { continue };
                    let m = bolt.a.lerp(bolt.b, (t0 + t1) / 2.0);
                    let perp = bolt.b.sub(bolt.a).norm().perp();
                    let in_plane = ray_to_edge(m, perp, &region).min(ray_to_edge(m, perp.scale(-1.0), &region));
                    let zc = bolt.z0;
                    let in_z = hits
                        .iter()
                        .filter(|p| geom::locate(m, &clip::from_region(&p.loops, FLAT_TOL), 1e-9) != Loc::Outside)
                        .map(|p| (zc - p.z0).min(p.z1 - zc))
                        .fold(f64::INFINITY, f64::min);
                    if in_z < in_plane {
                        (in_z, "along z (into the drawing)")
                    } else {
                        (in_plane, "in the drawing plane")
                    }
                } else {
                    if geom::locate(bolt.a, &region, 0.0) != Loc::Inside {
                        continue;
                    }
                    (dist_to_boundary(bolt.a, &region), "in the drawing plane")
                };
                if dist < need - 1.0 / 32.0 && worst.as_ref().is_none_or(|w| dist < w.0) {
                    worst = Some((dist, bolt, how));
                }
            }
        }
        if let Some((dist, bolt, how)) = worst {
            let which = if bc.arr_count > 1 || bolt.instance > 0 {
                format!("bolt {} of instance {}", bolt.k, bolt.instance)
            } else {
                format!("bolt {}", bolt.k)
            };
            diags.add_fix(
                Level::Warning,
                "W_BOLT_EDGE",
                Some(&bc.id),
                Some(&format!("components/{}", bc.id)),
                format!(
                    "bolt '{}' ({which}): edge distance to steel '{}' is {} {how}, below the AISC 360 Table J3.4 minimum of {} for a {} bolt",
                    bc.id,
                    s.id,
                    fmt_ft_in(dist),
                    fmt_ft_in(need),
                    fmt_ft_in(d)
                ),
                format!(
                    "move the bolts (at/offset, spacing) or enlarge '{}' so every bolt center is at least {} from its edge",
                    s.id,
                    fmt_ft_in(need)
                ),
            );
        }
    }
}

/// A thickness as a sixteenth fraction when it is one (`1/4"`), else three decimals (`0.24"`).
fn thk(x: f64) -> alloc::string::String {
    let n = crate::math::round(x * 16.0);
    if (x * 16.0 - n).abs() < 1e-6 { fmt_ft_in(x) } else { format!("{}\"", crate::model::fmt_num(x)) }
}

/// AISC 360-16 Table J2.4: minimum fillet weld size by the thinner part joined.
fn fillet_min(t: f64) -> f64 {
    match t {
        t if t <= 0.25 + 1e-9 => 0.125,
        t if t <= 0.5 + 1e-9 => 0.1875,
        t if t <= 0.75 + 1e-9 => 0.25,
        _ => 0.3125,
    }
}

/// AISC 360-16 J2.2b: maximum fillet along an edge of material t thick.
fn fillet_max(t: f64) -> f64 {
    if t < 0.25 { t } else { t - 0.0625 }
}

/// `members` of a weld must name two steel components: anything else is silently left out of W_WELD_SIZE otherwise.
fn weld_members(scene: &Scene<'_>, vid: &str, id: &str, an: &Value, diags: &Diags) {
    let Some(mv) = an.get("members").filter(|m| !m.is_null()) else { return };
    let path = format!("views/{vid}/annotations/{id}/members");
    let entries = mv.arr().unwrap_or(&[]);
    if mv.arr().is_none() || entries.len() != 2 || entries.iter().any(|e| e.str().is_none()) {
        diags.add_fix(
            Level::Warning,
            "W_PARAM",
            Some(id),
            Some(&path),
            format!("weld '{id}' in view {vid}: \"members\" must be two component ids (got {})", crate::model::kind_or_text(mv)),
            "e.g. \"members\": [\"col\", \"base_plate\"], or remove it".to_string(),
        );
        return;
    }
    for m in entries.iter().filter_map(Value::str) {
        match scene.find(m) {
            None => diags.add_fix(
                Level::Warning,
                "W_PARAM",
                Some(id),
                Some(&path),
                format!("weld '{id}' in view {vid}: member '{m}' is not a component, so W_WELD_SIZE cannot check it"),
                format!("use a component id (components: {})", crate::scene::join_ids(&scene.comp_ids())),
            ),
            Some(c) if c.ty.ty != Type::Steel => diags.add_fix(
                Level::Warning,
                "W_PARAM",
                Some(id),
                Some(&path),
                format!(
                    "weld '{id}' in view {vid}: member '{m}' is {}, not steel; a weld joins steel parts and W_WELD_SIZE checks only steel members",
                    c.ty.name()
                ),
                format!("list the two steel parts the weld joins, or remove '{m}' from \"members\""),
            ),
            Some(_) => {}
        }
    }
}

fn weld_sizes(scene: &Scene<'_>, doc: &Value, diags: &Diags) {
    for v in doc.get("views").and_then(Value::arr).unwrap_or(&[]) {
        let vid = v.get("id").and_then(Value::str).unwrap_or("");
        for an in v.get("annotations").and_then(Value::arr).unwrap_or(&[]) {
            if an.get("type").and_then(Value::str) != Some("weld") {
                continue;
            }
            if let Some(id) = an.get("id").and_then(Value::str) {
                weld_members(scene, vid, id, an, diags);
            }
            if an.get("weld").and_then(Value::str).is_some_and(|k| k != "fillet") {
                continue;
            }
            let (Some(id), Some(size)) =
                (an.get("id").and_then(Value::str), an.get("size").and_then(units::parse_length))
            else {
                continue;
            };
            let mut thin: Option<(f64, &str)> = None;
            for m in an.get("members").and_then(Value::arr).unwrap_or(&[]).iter().filter_map(Value::str) {
                let Some(c) = scene.find(m).filter(|c| c.state == State::Ok && c.ty.ty == Type::Steel) else {
                    continue;
                };
                let Some(t) = c.node.get("shape").and_then(Value::str).and_then(|s| steel_shapes::lookup(s).ok())
                else {
                    continue;
                };
                if thin.is_none_or(|(tt, _)| t.t_min < tt) {
                    thin = Some((t.t_min, m));
                }
            }
            let Some((t, part)) = thin else { continue };
            let (lo, hi) = (fillet_min(t), fillet_max(t));
            let problem = if size > hi + 1e-6 {
                format!(
                    "is larger than the {} maximum along the edge of '{part}' ({} thick, AISC 360 J2.2b)",
                    thk(hi),
                    thk(t)
                )
            } else if size < lo - 1e-6 {
                format!(
                    "is smaller than the {} minimum for a {} thinner part (AISC 360 Table J2.4)",
                    fmt_ft_in(lo),
                    thk(t)
                )
            } else {
                continue;
            };
            diags.add_fix(
                Level::Warning,
                "W_WELD_SIZE",
                Some(id),
                Some(&format!("views/{vid}/annotations/{id}/size")),
                format!("weld '{id}': fillet size {} {problem}", fmt_ft_in(size)),
                format!("use a fillet from {} to {}, or check the joined parts in \"members\"", fmt_ft_in(lo), thk(hi)),
            );
        }
    }
}
