//! Steel connection checks over a compiled scene (warnings, never errors): bolt edge distance against AISC 360 Table J3.4
//! (W_BOLT_EDGE) and bolt spacing against J3.3 (W_BOLT_SPACING).

use crate::catalog::Type;
use crate::clip::{self, Op};
use crate::geom::{self, FLAT_TOL, Loc, V2};
use crate::json::Value;
use crate::model::{Diags, Kind, Level, Prism};
use crate::scene::{Comp, Scene, State};
use crate::units::{self, fmt_ft_in};
use alloc::format;
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

pub fn run(scene: &Scene<'_>, diags: &Diags) {
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
