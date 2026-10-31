//! Subtracted solids (SPEC 26.4). After placement, every concrete or masonry host loses what its `voids` entries and the
//! sleeves passing through it occupy: the world prisms are cut once, split into slabs along z where a void is shorter
//! than the host, so sections, iso, the mesh and the checks all see the same hole. Fills are clipped per view instead,
//! by the solids the view's plane cuts with them (`clip_fill`).

use crate::clip::{self, Op};
use crate::geom::{self, Box, Pt, V2};
use crate::json::{self, Value};
use crate::model::{self, Kind, Level, Prism};
use crate::scene::{Comp, Scene, State, join_ids};
use crate::section::group_region;
use crate::style::Role;
use crate::units::{self, fmt_ft_in};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const EPS: f64 = 1e-9;
const FLAT_TOL: f64 = geom::FLAT_TOL;
/// The z range of a void shape without `z`: through the whole host.
const Z_ALL: [f64; 2] = [-1e12, 1e12];
const SHAPE_KEYS: [&str; 6] = ["rect", "circle", "points", "at", "anchor", "z"];

/// One solid a void takes away: its outer loops (CCW, flattened), the exact loop when it is a single loop, its box and
/// z range.
#[derive(Clone, Debug)]
pub struct Cutter {
    pub loops: Vec<Vec<V2>>,
    pub exact: Option<Vec<Pt>>,
    pub bx: Box,
    pub z: [f64; 2],
}

struct Void {
    host: usize,
    owner: usize,
    /// How messages name the void: `'col'`, `voids[2] (rect)`, `sleeve 'sl'`.
    what: String,
    /// How the host's summary line names it: `col`, `rect`, `sleeve sl`.
    name: String,
    path: String,
    /// The entry of the host's `voids` this void came from (None: a sleeve's `through` or a notch).
    entry: Option<usize>,
    /// The entry of the host's `cuts` this void came from (a lumber or steel notch `by`, SPEC 27.3).
    notch: Option<usize>,
    cutters: Vec<Cutter>,
    removed: bool,
}

/// Cut every host's world prisms by its voids, report voids that remove nothing (W_VOID_MISS) and keep the cutters on the
/// host for W_COVER.
pub fn apply(scene: &mut Scene) {
    check_except(scene);
    let mut voids = collect(scene);
    if voids.is_empty() {
        return;
    }
    let mut hosts: Vec<usize> = voids.iter().map(|v| v.host).collect();
    crate::sort::stable(&mut hosts, |a, b| a < b);
    hosts.dedup();
    let mut worlds: Vec<(usize, Vec<Prism>)> = Vec::new();
    for &h in &hosts {
        let mine: Vec<usize> = (0..voids.len()).filter(|&i| voids[i].host == h).collect();
        let mut out: Vec<Prism> = Vec::new();
        for p in &scene.comps[h].world {
            out.extend(split(p, &mut voids, &mine));
        }
        worlds.push((h, out));
    }
    for v in &voids {
        if !v.removed {
            miss(scene, v);
        }
    }
    for (h, w) in worlds {
        let mine: Vec<&Void> = voids.iter().filter(|v| v.host == h).collect();
        let cutters: Vec<Cutter> = mine.iter().flat_map(|v| v.cutters.iter().cloned()).collect();
        let names: Vec<&str> = mine.iter().filter(|v| v.removed).map(|v| v.name.as_str()).collect();
        let c = &mut scene.comps[h];
        c.world = w;
        c.voids = cutters;
        if !names.is_empty() {
            c.built.info.push_str(&format!(" minus {}", names.join(", ")));
        }
    }
}

fn err(scene: &Scene, code: &'static str, id: &str, path: &str, msg: String, fix: Option<String>) {
    match fix {
        Some(f) => scene.diags.add_fix(Level::Error, code, Some(id), Some(path), msg, f),
        None => scene.diags.add(Level::Error, code, Some(id), Some(path), msg),
    }
}

fn unknown_id(scene: &Scene, from: &str, path: &str, what: &str, id: &str) {
    let ids = scene.comp_ids();
    match model::nearest(id, &ids) {
        Some(n) => err(
            scene,
            "E_REF_UNKNOWN",
            from,
            path,
            format!("{what}: no component with id '{id}'; did you mean '{n}'?"),
            Some(n.to_string()),
        ),
        None => err(
            scene,
            "E_REF_UNKNOWN",
            from,
            path,
            format!("{what}: no component with id '{id}'. Known ids: {}", join_ids(&ids)),
            None,
        ),
    }
}

/// `fill.except` must name components.
fn check_except(scene: &Scene) {
    for c in scene.comps.iter().filter(|c| c.state == State::Ok) {
        let Some(ev) = c.node.get("except").filter(|v| !v.is_null()) else { continue };
        if c.ty.traits.takes_voids || !c.world.iter().any(|p| p.role == Role::Soil) {
            continue;
        }
        let path = format!("components/{}/except", c.id);
        let Some(list) = ev.arr() else {
            err(
                scene,
                "E_PARAM",
                &c.id,
                &path,
                format!("fill '{}': except must be a list of component ids, e.g. [\"pipe\"]", c.id),
                None,
            );
            continue;
        };
        for (k, e) in list.iter().enumerate() {
            let p = format!("{path}/{k}");
            match e.str() {
                Some(s) if scene.find(s).is_some() => {}
                Some(s) => unknown_id(scene, &c.id, &p, &format!("fill '{}' except", c.id), s),
                None => err(
                    scene,
                    "E_PARAM",
                    &c.id,
                    &p,
                    format!("fill '{}': except entries are component ids (got {})", c.id, model::kind_or_text(e)),
                    None,
                ),
            }
        }
    }
}

fn collect(scene: &Scene) -> Vec<Void> {
    let mut out = Vec::new();
    for (hi, c) in scene.comps.iter().enumerate() {
        if c.state != State::Ok {
            continue;
        }
        if c.ty.traits.takes_voids
            && let Some(v) = c.node.get("voids").filter(|v| !v.is_null())
        {
            let path = format!("components/{}/voids", c.id);
            match v.arr() {
                Some(list) => {
                    for (k, e) in list.iter().enumerate() {
                        let p = format!("{path}/{k}");
                        let void = match e {
                            Value::String(s) => comp_void(scene, hi, s, &p, "voids"),
                            Value::Object(_) => shape_void(scene, hi, k, e, &p),
                            _ => {
                                err(
                                    scene,
                                    "E_PARAM",
                                    &c.id,
                                    &p,
                                    format!(
                                        "'{}' voids[{k}] must be a component id or a shape (got {})",
                                        c.id,
                                        model::kind_or_text(e)
                                    ),
                                    Some(VOIDS_FIX.to_string()),
                                );
                                None
                            }
                        };
                        out.extend(void.map(|cutters| Void {
                            host: hi,
                            owner: hi,
                            what: match e {
                                Value::String(s) => format!("'{s}'"),
                                _ => format!("voids[{k}] ({})", shape_kind(e)),
                            },
                            name: match e {
                                Value::String(s) => s.clone(),
                                _ => shape_kind(e).to_string(),
                            },
                            path: p,
                            entry: Some(k),
                            notch: None,
                            cutters,
                            removed: false,
                        }));
                    }
                }
                None => err(
                    scene,
                    "E_PARAM",
                    &c.id,
                    &path,
                    format!("'{}' voids must be a list (got {})", c.id, model::kind_or_text(v)),
                    Some(VOIDS_FIX.to_string()),
                ),
            }
        }
        if c.ty.traits.passes_through {
            out.extend(through(scene, hi));
        }
        for (k, by) in crate::cuts::notch_bys(c) {
            let p = format!("components/{}/cuts/{k}", c.id);
            if let Some(cutters) = comp_void(scene, hi, by, &p, &format!("cuts[{k}] notch by")) {
                out.push(Void {
                    host: hi,
                    owner: hi,
                    what: format!("notch by '{by}'"),
                    name: format!("notch {by}"),
                    path: p,
                    entry: None,
                    notch: Some(k),
                    cutters,
                    removed: false,
                });
            }
        }
    }
    out
}

const VOIDS_FIX: &str = "\"voids\": [\"column\", \"base_plate.plate\", {\"rect\": [6, 8], \"at\": \"wall@center\"}, {\"circle\": 4, \"at\": [12, 30], \"z\": [-6, 6]}]";

fn shape_kind(e: &Value) -> &'static str {
    ["rect", "circle", "points"].into_iter().find(|k| e.get(k).is_some()).unwrap_or("shape")
}

/// A sleeve's `through`: the hosts it is subtracted from.
fn through(scene: &Scene, si: usize) -> Vec<Void> {
    let c = &scene.comps[si];
    let Some(tv) = c.node.get("through").filter(|v| !v.is_null()) else { return Vec::new() };
    let path = format!("components/{}/through", c.id);
    let ids: Vec<(String, &Value)> = match tv {
        Value::String(_) => vec![(path.clone(), tv)],
        Value::Array(a) => a.iter().enumerate().map(|(k, e)| (format!("{path}/{k}"), e)).collect(),
        _ => {
            err(
                scene,
                "E_PARAM",
                &c.id,
                &path,
                format!(
                    "sleeve '{}': through must be a host id or a list of them (got {})",
                    c.id,
                    model::kind_or_text(tv)
                ),
                Some("\"through\": \"stem_wall\"".into()),
            );
            return Vec::new();
        }
    };
    let cutters = envelope(c, None);
    let mut out = Vec::new();
    for (p, e) in ids {
        let Some(hid) = e.str() else {
            err(
                scene,
                "E_PARAM",
                &c.id,
                &p,
                format!("sleeve '{}': through entries are component ids (got {})", c.id, model::kind_or_text(e)),
                None,
            );
            continue;
        };
        let Some(hi) = scene.comps.iter().position(|h| h.id == hid) else {
            unknown_id(scene, &c.id, &p, &format!("sleeve '{}' through", c.id), hid);
            continue;
        };
        let h = &scene.comps[hi];
        if !h.ty.traits.takes_voids {
            err(
                scene,
                "E_PARAM",
                &c.id,
                &p,
                format!(
                    "sleeve '{}' through '{hid}': '{hid}' is a {}; only concrete and cmu_wall take a sleeve or void",
                    c.id,
                    h.ty.name()
                ),
                Some(format!(
                    "name the concrete or cmu_wall the sleeve passes through, or remove '{hid}' from through"
                )),
            );
            continue;
        }
        if h.state != State::Ok {
            continue;
        }
        out.push(Void {
            host: hi,
            owner: si,
            what: format!("sleeve '{}'", c.id),
            name: format!("sleeve {}", c.id),
            path: p,
            entry: None,
            notch: None,
            cutters: cutters.clone(),
            removed: false,
        });
    }
    out
}

/// A `voids` entry naming a component (`"col"` or `"col.part"`): its envelope per instance.
fn comp_void(scene: &Scene, hi: usize, s: &str, path: &str, key: &str) -> Option<Vec<Cutter>> {
    let host = &scene.comps[hi];
    if s.contains(['@', '#']) {
        err(
            scene,
            "E_PARAM",
            &host.id,
            path,
            format!(
                "'{}' {key} entry '{s}' names a component or a part (\"col\", \"col.part\"), not an anchor or an array instance; every instance is subtracted",
                host.id
            ),
            Some(format!("\"{}\"", s.split(['@', '#']).next().unwrap_or(s))),
        );
        return None;
    }
    let (id, part) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    if id == host.id {
        err(
            scene,
            "E_PARAM",
            &host.id,
            path,
            format!("'{}' cannot void itself", host.id),
            Some(format!("remove \"{s}\" from {key}")),
        );
        return None;
    }
    let Some(c) = scene.find(id) else {
        unknown_id(scene, &host.id, path, &format!("'{}' {key}", host.id), id);
        return None;
    };
    if c.state != State::Ok {
        return None;
    }
    if c.world.iter().all(|p| p.role == Role::Soil) && !c.world.is_empty() {
        err(
            scene,
            "E_PARAM",
            &host.id,
            path,
            format!(
                "'{}' {key} '{id}': '{id}' is a fill; fills are clipped by the solids they surround on their own (SPEC 26.4), they never cut a solid",
                host.id
            ),
            Some(format!("remove \"{s}\" from {key}")),
        );
        return None;
    }
    if let Some(pn) = part
        && !c.world.iter().any(|p| p.part == pn)
    {
        let parts = scene.part_names(c);
        err(
            scene,
            "E_REF_UNKNOWN",
            &host.id,
            path,
            format!(
                "'{}' {key} '{s}': component '{id}' has no part '{pn}'. Parts: {}",
                host.id,
                if parts.is_empty() { "(none)".to_string() } else { join_ids(&parts) }
            ),
            Some(format!("\"{id}\"")),
        );
        return None;
    }
    let cutters = envelope(c, part);
    if cutters.is_empty() {
        err(
            scene,
            "E_PARAM",
            &host.id,
            path,
            format!("'{}' {key} '{s}': '{id}' ({}) has no solid outline to subtract", host.id, c.ty.name()),
            Some(format!(
                "remove \"{s}\" from voids, or subtract a shape: {{\"rect\": [w, h], \"at\": \"{id}@center\"}}"
            )),
        );
        return None;
    }
    Some(cutters)
}

/// The solid a component occupies, per instance: the union of the outer outlines of its bodies (holes filled: concrete
/// does not get inside a tube), over its whole z extent.
fn envelope(c: &Comp, part: Option<&str>) -> Vec<Cutter> {
    let mut out = Vec::new();
    for k in 0..c.xfs.len() as u32 {
        let ps: Vec<&Prism> = c
            .world
            .iter()
            .filter(|p| {
                p.instance == k
                    && (p.kind == Kind::Body || p.dashed)
                    && p.role != Role::Void
                    && !p.loops.is_empty()
                    && part.is_none_or(|pn| p.part == pn)
            })
            .collect();
        let Some(first) = ps.first() else { continue };
        let mut u: Vec<Vec<V2>> = Vec::new();
        let mut z = [first.z0, first.z1];
        for p in &ps {
            let mut o = geom::flatten_loop(&p.loops[0], FLAT_TOL);
            clip::orient_ccw(&mut o);
            u = if u.is_empty() { vec![o] } else { clip::boolean(&u, &[o], Op::Unite) };
            z = [z[0].min(p.z0), z[1].max(p.z1)];
        }
        u.retain(|l| geom::signed_area_v(l) > 0.0);
        if u.is_empty() {
            continue;
        }
        let exact = (ps.len() == 1).then(|| ccw(&ps[0].loops[0]));
        out.push(Cutter { bx: clip::loops_box(&u), loops: u, exact, z });
    }
    out
}

fn ccw(lp: &[Pt]) -> Vec<Pt> {
    if geom::signed_area(lp) < 0.0 { geom::reverse_loop(lp) } else { lp.to_vec() }
}

fn len_of(v: Option<&Value>) -> Option<f64> {
    v.and_then(units::parse_length)
}

/// A `voids` entry that is a shape: `{"rect": [w, h] | "circle": d | "points": [...], "at": Ref, "anchor", "z": [z0, z1]}`.
fn shape_void(scene: &Scene, hi: usize, k: usize, e: &Value, path: &str) -> Option<Vec<Cutter>> {
    let host = &scene.comps[hi];
    let hid = host.id.as_str();
    let bad = |msg: String| {
        err(scene, "E_PARAM", hid, path, format!("'{hid}' voids[{k}]: {msg}"), Some(VOIDS_FIX.to_string()));
        None
    };
    if let Value::Object(m) = e
        && let Some(u) = m.iter().find(|x| !SHAPE_KEYS.contains(&x.key.as_str()))
    {
        return bad(format!("unknown key '{}'; a void shape takes {}", u.key, SHAPE_KEYS.join(", ")));
    }
    let kinds: Vec<&str> = ["rect", "circle", "points"].into_iter().filter(|k| e.get(k).is_some()).collect();
    if kinds.len() != 1 {
        return bad("give exactly one of rect: [w, h], circle: d or points: [[x, y], ...]".into());
    }
    let at = match e.get("at") {
        Some(av) => Some(scene.point(av, hid, &format!("{path}/at"))?),
        None => None,
    };
    let z = match e.get("z") {
        None => Z_ALL,
        Some(zv) => match zv.arr().filter(|a| a.len() == 2) {
            Some(a) => match (len_of(a.first()), len_of(a.get(1))) {
                (Some(z0), Some(z1)) if z0 < z1 => [z0, z1],
                _ => return bad(format!("z must be [z0, z1] with z0 < z1 (got {})", model::kind_or_text(zv))),
            },
            None => return bad(format!("z must be [z0, z1] with z0 < z1 (got {})", model::kind_or_text(zv))),
        },
    };
    let exact: Vec<Pt> = match kinds[0] {
        "rect" => {
            let wh = e.get("rect").and_then(Value::arr).filter(|a| a.len() == 2);
            let (Some(w), Some(h)) = (len_of(wh.and_then(|a| a.first())), len_of(wh.and_then(|a| a.get(1)))) else {
                return bad("rect must be [width, height], two positive lengths".into());
            };
            if w <= 0.0 || h <= 0.0 {
                return bad("rect must be [width, height], two positive lengths".into());
            }
            let Some(at) = at else {
                return bad("a rect needs \"at\": the point its anchor (default center) lands on".into());
            };
            let anchor = e.get("anchor").and_then(Value::str).unwrap_or("center");
            let Some(a) = model::box_anchor(Box { x0: 0.0, y0: 0.0, x1: w, y1: h }, anchor) else {
                return bad(format!("anchor '{anchor}' is not a box anchor ({})", model::BOX_ANCHOR_NAMES.join(", ")));
            };
            let o = at.sub(a);
            model::rect_loop(o.x, o.y, o.x + w, o.y + h)
        }
        "circle" => {
            let Some(d) = len_of(e.get("circle")).filter(|d| *d > 0.0) else {
                return bad("circle must be a positive diameter".into());
            };
            let Some(at) = at else { return bad("a circle needs \"at\": its center".into()) };
            model::circle_loop(at.x, at.y, d / 2.0)
        }
        _ => {
            let Some(list) = e.get("points").and_then(Value::arr).filter(|a| a.len() >= 3) else {
                return bad("points must list at least 3 points: [x, y] (relative to at, else absolute) or Refs".into());
            };
            let base = at.unwrap_or(V2::init(0.0, 0.0));
            let mut pts = Vec::new();
            for (j, pv) in list.iter().enumerate() {
                let pp = format!("{path}/points/{j}");
                let q = match pv {
                    Value::Array(_) => scene.point(pv, hid, &pp)?.add(base),
                    _ => scene.point(pv, hid, &pp)?,
                };
                pts.push(Pt::at(q, 0.0));
            }
            ccw(&pts)
        }
    };
    let mut flat = geom::flatten_loop(&exact, FLAT_TOL);
    clip::orient_ccw(&mut flat);
    if geom::signed_area_v(&flat).abs() < 1e-9 {
        return bad("the shape has no area".into());
    }
    let loops = vec![flat];
    Some(vec![Cutter { bx: clip::loops_box(&loops), loops, exact: Some(ccw(&exact)), z }])
}

/// The slabs of host prism `p` with the voids `mine` taken away (`p` itself when none of them reaches it).
fn split(p: &Prism, voids: &mut [Void], mine: &[usize]) -> Vec<Prism> {
    if !(p.kind == Kind::Body || p.dashed) || p.loops.is_empty() || p.role == Role::Void || p.role == Role::Soil {
        return vec![p.clone()];
    }
    let flat0 = clip::from_region(&p.loops, FLAT_TOL);
    let pbx = clip::loops_box(&flat0);
    let mut rel: Vec<(usize, &Cutter)> = Vec::new();
    for &vi in mine {
        for c in &voids[vi].cutters {
            if c.z[0] < p.z1 - EPS && c.z[1] > p.z0 + EPS && c.bx.overlaps(pbx, 1e-9) {
                rel.push((vi, c));
            }
        }
    }
    if rel.is_empty() {
        return vec![p.clone()];
    }
    let mut bps = vec![p.z0, p.z1];
    for (_, c) in &rel {
        for zz in c.z {
            if zz > p.z0 + EPS && zz < p.z1 - EPS {
                bps.push(zz);
            }
        }
    }
    crate::sort::stable(&mut bps, |a, b| a < b);
    bps.dedup_by(|a, b| (*a - *b).abs() < EPS);
    let mut slabs: Vec<([f64; 2], Vec<usize>)> = Vec::new();
    for w in bps.windows(2) {
        let active: Vec<usize> =
            (0..rel.len()).filter(|&i| rel[i].1.z[0] <= w[0] + EPS && rel[i].1.z[1] >= w[1] - EPS).collect();
        match slabs.last_mut() {
            Some(last) if last.1 == active => last.0[1] = w[1],
            _ => slabs.push(([w[0], w[1]], active)),
        }
    }
    let mut hit: Vec<usize> = Vec::new();
    let mut cuts: Vec<Option<Cut>> = Vec::new();
    for (_, active) in &slabs {
        let cs: Vec<&Cutter> = active.iter().map(|&i| rel[i].1).collect();
        let (cut, used) = subtract(&flat0, &p.loops, &cs);
        hit.extend(used.into_iter().map(|j| rel[active[j]].0));
        cuts.push(cut);
    }
    for vi in hit {
        voids[vi].removed = true;
    }
    if cuts.iter().all(Option::is_none) {
        return vec![p.clone()];
    }
    let flat_mode = cuts.iter().any(|c| c.as_ref().is_some_and(|c| c.exact.is_none()));
    let whole: Vec<Vec<Vec<Pt>>> = if flat_mode {
        group_region(&flat0, None).into_iter().map(|g| g.loops).collect()
    } else {
        vec![p.loops.clone()]
    };
    let split_z = slabs.len() > 1;
    let mut out = Vec::new();
    for ((zr, _), cut) in slabs.iter().zip(cuts) {
        let pieces: Vec<Vec<Vec<Pt>>> = match cut {
            None => whole.clone(),
            Some(Cut { exact: Some(ex), .. }) => vec![ex],
            Some(Cut { region, .. }) => group_region(&region, None).into_iter().map(|g| g.loops).collect(),
        };
        for loops in pieces {
            let mut q = p.clone();
            q.loops = loops;
            q.z0 = zr[0];
            q.z1 = zr[1];
            q.seam =
                [if zr[0] > p.z0 + EPS { true } else { p.seam[0] }, if zr[1] < p.z1 - EPS { true } else { p.seam[1] }];
            if split_z {
                q.bond_z0 = Some(p.bond_z0.unwrap_or(p.z0));
            }
            out.push(q);
        }
    }
    out
}

struct Cut {
    region: Vec<Vec<V2>>,
    exact: Option<Vec<Vec<Pt>>>,
}

/// `flat` minus the cutters (None: they remove nothing), and the indices of the cutters that removed area. The exact
/// loops survive while every cutter is a single exact loop that lands wholly inside as a new hole.
fn subtract(flat: &[Vec<V2>], exact_in: &[Vec<Pt>], cs: &[&Cutter]) -> (Option<Cut>, Vec<usize>) {
    let mut region = flat.to_vec();
    let mut exact = Some(exact_in.to_vec());
    let mut used = Vec::new();
    for (j, c) in cs.iter().enumerate() {
        let before = clip::total_area(&region);
        let next = clip::boolean(&region, &c.loops, Op::Diff);
        let after = clip::total_area(&next);
        let tol = 1e-9 + 1e-6 * before.abs();
        let removed = before - after;
        let carea = clip::total_area(&c.loops);
        if removed <= tol || after < -tol || removed > carea + tol {
            continue;
        }
        used.push(j);
        exact = match (exact, &c.exact) {
            (Some(mut ex), Some(ce))
                if (removed - carea).abs() <= tol + 1e-6 * carea && next.len() == region.len() + 1 =>
            {
                ex.push(geom::reverse_loop(ce));
                Some(ex)
            }
            _ => None,
        };
        region = next;
    }
    if used.is_empty() {
        return (None, used);
    }
    (Some(Cut { region, exact }), used)
}

fn box_text(b: Box) -> String {
    format!("x {}..{}, y {}..{}", fmt_ft_in(b.x0), fmt_ft_in(b.x1), fmt_ft_in(b.y0), fmt_ft_in(b.y1))
}

fn z_text(z: [f64; 2]) -> String {
    if z == Z_ALL { "through".to_string() } else { format!("z {}..{}", fmt_ft_in(z[0]), fmt_ft_in(z[1])) }
}

/// W_VOID_MISS: a void that takes nothing from its host.
fn miss(scene: &Scene, v: &Void) {
    let host = &scene.comps[v.host];
    let owner = &scene.comps[v.owner];
    let mut hb = Box::default();
    let mut hz = [f64::INFINITY, f64::NEG_INFINITY];
    for p in host.world.iter().filter(|p| p.kind == Kind::Body || p.dashed) {
        for l in &p.loops {
            hb.add_box(geom::loop_box(l));
        }
        hz = [hz[0].min(p.z0), hz[1].max(p.z1)];
    }
    let mut vb = Box::default();
    let mut vz = [f64::INFINITY, f64::NEG_INFINITY];
    for c in &v.cutters {
        vb.add_box(c.bx);
        vz = [vz[0].min(c.z[0]), vz[1].max(c.z[1])];
    }
    let why = if vb.overlaps(hb, 1e-9) {
        format!("their outlines miss each other or their z ranges do not meet ({} vs {})", z_text(vz), z_text(hz))
    } else {
        "their outlines do not overlap".to_string()
    };
    if let Some(k) = v.notch {
        let rest: Vec<Value> = host
            .node
            .get("cuts")
            .and_then(Value::arr)
            .map(|a| a.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, e)| e.clone()).collect())
            .unwrap_or_default();
        let mut buf = Vec::new();
        json::write_compact(&mut buf, &Value::Array(rest));
        scene.diags.add_fix(
            Level::Warning,
            "W_CUT_MISS",
            Some(&host.id),
            Some(&v.path),
            format!(
                "the {} of '{}' removes nothing: the notching member ({}, {}) and '{}' ({}, {}): {why}",
                v.what,
                host.id,
                box_text(vb),
                z_text(vz),
                host.id,
                box_text(hb),
                z_text(hz)
            ),
            format!(
                "move one of them so they overlap, or remove the cut: {{\"op\": \"update\", \"path\": \"components/{}\", \"value\": {{\"cuts\": {}}}}}",
                host.id,
                String::from_utf8_lossy(&buf)
            ),
        );
        return;
    }
    let (msg, fix) = match v.entry {
        Some(k) => {
            let rest: Vec<Value> = host
                .node
                .get("voids")
                .and_then(Value::arr)
                .map(|a| a.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, e)| e.clone()).collect())
                .unwrap_or_default();
            let mut buf = Vec::new();
            json::write_compact(&mut buf, &Value::Array(rest));
            (
                format!(
                    "void {} of '{}' removes nothing: the void ({}, {}) and '{}' ({}, {}): {why}",
                    v.what,
                    host.id,
                    box_text(vb),
                    z_text(vz),
                    host.id,
                    box_text(hb),
                    z_text(hz)
                ),
                format!(
                    "move the void or '{}' so they overlap, or remove the entry: {{\"op\": \"update\", \"path\": \"components/{}\", \"value\": {{\"voids\": {}}}}}",
                    host.id,
                    host.id,
                    String::from_utf8_lossy(&buf)
                ),
            )
        }
        None => (
            format!(
                "{} does not pass through '{}': the sleeve ({}, {}) and '{}' ({}, {}): {why}",
                v.what,
                host.id,
                box_text(vb),
                z_text(vz),
                host.id,
                box_text(hb),
                z_text(hz)
            ),
            format!("move '{}' into '{}' (its \"at\"), or remove '{}' from its through", owner.id, host.id, host.id),
        ),
    };
    scene.diags.add_fix(Level::Warning, "W_VOID_MISS", Some(&owner.id), Some(&v.path), msg, fix);
}

// ---- fills under solids ---------------------------------------------------------------------------------------

/// The solids that clip the fills of a view at plane `z`: per component instance whose bodies reach across the plane,
/// the union of their outer outlines (a tube or a W seen lengthwise clips as its whole silhouette).
pub struct FillCutters {
    z: f64,
    solids: Vec<(u32, Vec<Vec<V2>>, Box)>,
}

pub fn fill_cutters(prisms: &[Prism], z: f64) -> FillCutters {
    struct Group {
        comp: u32,
        inst: u32,
        outers: Vec<Vec<V2>>,
        z: [f64; 2],
    }
    let mut groups: Vec<Group> = Vec::new();
    for q in prisms {
        if q.kind != Kind::Body
            || q.dashed
            || q.embedded
            || q.loops.is_empty()
            || matches!(q.role, Role::Soil | Role::Void)
        {
            continue;
        }
        let mut o = geom::flatten_loop(&q.loops[0], FLAT_TOL);
        clip::orient_ccw(&mut o);
        match groups.iter_mut().find(|g| g.comp == q.comp && g.inst == q.instance) {
            Some(g) => {
                g.outers.push(o);
                g.z = [g.z[0].min(q.z0), g.z[1].max(q.z1)];
            }
            None => groups.push(Group { comp: q.comp, inst: q.instance, outers: vec![o], z: [q.z0, q.z1] }),
        }
    }
    let mut solids = Vec::new();
    for Group { comp, outers, z: zr, .. } in groups {
        if !(zr[0] < z - EPS && zr[1] > z + EPS) {
            continue;
        }
        let mut u: Vec<Vec<V2>> = Vec::new();
        for o in outers {
            u = if u.is_empty() { vec![o] } else { clip::boolean(&u, &[o], Op::Unite) };
        }
        u.retain(|l| geom::signed_area_v(l) > 0.0);
        if u.is_empty() {
            continue;
        }
        let bx = clip::loops_box(&u);
        solids.push((comp, u, bx));
    }
    FillCutters { z, solids }
}

/// A cut fill clipped by the solids it surrounds (minus its `except` components): None when nothing is taken away, else
/// one prism per remaining piece (none when the solids cover it).
pub fn clip_fill(scene: &Scene, fc: &FillCutters, p: &Prism) -> Option<Vec<Prism>> {
    if p.role != Role::Soil
        || p.kind != Kind::Body
        || p.dashed
        || p.loops.is_empty()
        || !(p.z0 < fc.z - EPS && p.z1 > fc.z + EPS)
    {
        return None;
    }
    let except: Vec<&str> = scene
        .comps
        .get(p.comp as usize)
        .and_then(|c| c.node.get("except"))
        .and_then(Value::arr)
        .map(|a| a.iter().filter_map(Value::str).collect())
        .unwrap_or_default();
    let mut region = clip::from_region(&p.loops, FLAT_TOL);
    let bx = clip::loops_box(&region);
    let mut changed = false;
    for (comp, loops, sb) in &fc.solids {
        if *comp == p.comp || !sb.overlaps(bx, 1e-9) {
            continue;
        }
        if scene.comps.get(*comp as usize).is_some_and(|c| except.contains(&c.id.as_str())) {
            continue;
        }
        let before = clip::total_area(&region);
        let next = clip::boolean(&region, loops, Op::Diff);
        let after = clip::total_area(&next);
        let tol = 1e-9 + 1e-6 * before.abs();
        if before - after <= tol || after < -tol || before - after > clip::total_area(loops) + tol {
            continue;
        }
        region = next;
        changed = true;
    }
    if !changed {
        return None;
    }
    Some(
        group_region(&region, None)
            .into_iter()
            .map(|g| {
                let mut q = p.clone();
                q.loops = g.loops;
                q
            })
            .collect(),
    )
}

/// The view's prisms with its cut fills clipped (SPEC 26.4), in order (a fill in pieces gives consecutive prisms).
pub fn clip_fills(scene: &Scene, prisms: &[Prism], z: f64) -> Vec<Prism> {
    if !prisms.iter().any(|p| p.role == Role::Soil) {
        return prisms.to_vec();
    }
    let fc = fill_cutters(prisms, z);
    let mut out = Vec::with_capacity(prisms.len());
    for p in prisms {
        match clip_fill(scene, &fc, p) {
            Some(pieces) => out.extend(pieces),
            None => out.push(p.clone()),
        }
    }
    out
}

// ---- seams ---------------------------------------------------------------------------------------------------

/// The boundary segments of the slabs of `p`'s member (same component, instance and part) that touch `p`'s cap at
/// `z` (`p.z1` for `hi`, else `p.z0`), flattened with `flatten`. Empty when that cap is not a seam.
pub fn seam_neighbors(world: &[Prism], p: &Prism, hi: bool, flatten: impl Fn(&Prism) -> Vec<Vec<V2>>) -> Vec<[V2; 2]> {
    if !p.seam[usize::from(hi)] {
        return Vec::new();
    }
    let z = if hi { p.z1 } else { p.z0 };
    let mut segs = Vec::new();
    for q in world {
        if q.instance != p.instance || q.part != p.part || q.kind != p.kind || q.material != p.material {
            continue;
        }
        let qz = if hi { q.z0 } else { q.z1 };
        if (qz - z).abs() > EPS {
            continue;
        }
        for l in flatten(q) {
            for (i, &a) in l.iter().enumerate() {
                segs.push([a, l[(i + 1) % l.len()]]);
            }
        }
    }
    segs
}
