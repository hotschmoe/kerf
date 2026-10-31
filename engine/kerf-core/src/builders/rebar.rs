//! rebar (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, parse_point_list, path_len, world_box};
use crate::clip;
use crate::geom::{self, Box, FLAT_TOL, V2, Xf};
use crate::json::Value;
use crate::model::fmt_num;
use crate::model::{self, Built, Level, NamedAnchor, Prism};
use crate::num;
use crate::params::{param_enum, params};
use crate::pathgeom;
use crate::scene::{Comp, Scene, State, join_ids};
use crate::units;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

pub fn rebar_diameter(size: &str) -> Option<f64> {
    let digits = size.strip_prefix('#').filter(|d| !d.is_empty())?;
    match digits.parse::<u32>().ok()? {
        3 => Some(0.375),
        4 => Some(0.5),
        5 => Some(0.625),
        6 => Some(0.75),
        7 => Some(0.875),
        8 => Some(1.0),
        9 => Some(1.128),
        10 => Some(1.27),
        11 => Some(1.41),
        _ => None,
    }
}

param_enum! { pub enum Mode { AlongZ = "along_z", Path = "path" } }

params! {
    pub struct Params {
        size: Str = "#4" => { desc: "#3 .375, #4 .5, #5 .625, #6 .75, #7 .875, #8 1.0 (diameter in)" },
        mode: Enum(Mode) = Mode::AlongZ => { desc: "along_z (continuous bar seen as a dot) or path (bar in the XY plane)" },
        place: OptRaw => { desc: "cover-based placement (preferred): {in: \"comp[.part]\", face: bottom|top|left|right|center, cover: 3, count: 2, side_cover: cover, axis: x|y, station: in}. bottom/top/left/right: bars at clear `cover` from that face of the host's outline (cut to the part for comp.part; in a polygon, the outermost run of edges facing that way, e.g. the lowest bottom run = a thickened footing), spread evenly along it at `side_cover` from its ends (count 1 centers); a bar centre outside the zone is E_PLACE_OUTSIDE. center: bars centered in the zone on both axes (e.g. a single #4 in the middle of a stem wall); count > 1 spreads along axis x (default) or y at side_cover. station: ONE bar at that offset from the zone's left face (bottom face with axis y; for bottom/top/left/right faces it sets the along-face position)" },
        points: OptRaw => { def: "path: required", desc: "polyline [x,y] or Refs; bends get radius bend_radius, drawn as fillets" },
        bend_radius: OptLenPos => { def: "3*d_b", desc: "inside bend radius for path bars" },
        spacing_note: Str = "" => { def: "null", desc: "e.g. \"#4 @ 16\\\" O.C.\" for summaries and notes" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let rp = ctx.p.parse::<Params>()?;
    let size = rp.size.as_str();
    let Some(d) = rebar_diameter(size) else {
        ctx.p.fail("size", format!("bar size '{size}' is not recognised. Use #3 (.375), #4 (.5), #5 (.625), #6 (.75), #7 (.875) or #8 (1.0)"));
        return None;
    };
    let r = d / 2.0;
    if rp.mode == Mode::AlongZ {
        let lp = model::circle_loop(0.0, 0.0, r);
        let prism = Prism {
            material: "rebar".to_string(),
            loops: model::one_loop(lp),
            embedded: true,
            sweep_r: r,
            ..Prism::default()
        };
        let mut built = Built {
            prisms: vec![prism],
            box_: Box { x0: -r, y0: -r, x1: r, y1: r },
            bar_d: d,
            info: format!("rebar {size} along z"),
            ..Built::default()
        };
        if let Some(pl) = rp.place.as_ref() {
            built.centers = place_rebar(ctx, pl, d)?;
            let face = pl.get("face").map_or("bottom", |f| f.str().unwrap_or("bottom"));
            built.info = format!("rebar ({}) {} along z @ {} face", built.centers.len(), size, face);
        }
        return Some(built);
    }
    let Some(pv) = rp.points.as_ref() else {
        ctx.p.fail("points", "mode path needs 'points': [[x, y], ...] or Refs".to_string());
        return None;
    };
    let pts = parse_point_list(ctx, "points", pv, true)?;
    let clean = drop_duplicate_points(&pts);
    if clean.len() < 2 {
        ctx.p.fail("points", format!("a path bar needs at least 2 distinct points (got {})", clean.len()));
        return None;
    }
    let br = rp.bend_radius.unwrap_or(3.0 * d);
    let vs: Vec<V2> = clean.iter().map(|q| q.v()).collect();
    let center = pathgeom::fillet(&vs, br + r);
    let rib = pathgeom::ribbon(&center, r, r);
    let box_ = geom::loop_box(&rib);
    let anchors = path_anchors(&vs, &center);
    let note =
        if rp.spacing_note.is_empty() { alloc::string::String::new() } else { format!(" ({})", rp.spacing_note) };
    let prism = Prism {
        material: "rebar".to_string(),
        loops: model::one_loop(rib),
        embedded: true,
        centerline: center,
        sweep_r: r,
        ..Prism::default()
    };
    Some(Built {
        prisms: vec![prism],
        anchors,
        box_,
        nat_z: Some(d),
        points_mode: true,
        bar_d: d,
        info: format!("rebar {} path L={}{}", size, fmt_ft_in(path_len(&vs)), note),
        ..Built::default()
    })
}

/// Anchors on a path bar (SPEC 25.1), all on its centreline: `start`, `end`, `bend_k` at the middle of the bend at
/// interior point k + 1 (the point of the bar nearest that corner) and `seg_k_mid` at the middle of the straight part of
/// leg k (point k to k + 1).
fn path_anchors(vs: &[V2], center: &[geom::Pt]) -> Vec<NamedAnchor> {
    let flat = geom::flatten_polyline(center, false, FLAT_TOL);
    let nearest = |q: V2| {
        let mut best = flat[0];
        for w in flat.windows(2) {
            let d = w[1].sub(w[0]);
            let l2 = d.dot(d);
            let t = if l2 > 0.0 { (q.sub(w[0]).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let p = w[0].lerp(w[1], t);
            if p.dist(q) < best.dist(q) - 1e-12 {
                best = p;
            }
        }
        best
    };
    let n = vs.len();
    let mut out = vec![
        NamedAnchor { name: "start".to_string(), p: flat[0] },
        NamedAnchor { name: "end".to_string(), p: flat[flat.len() - 1] },
    ];
    for k in 1..n - 1 {
        out.push(NamedAnchor { name: format!("bend_{k}"), p: nearest(vs[k]) });
    }
    for k in 0..n - 1 {
        let (a, b) = (vs[k], vs[k + 1]);
        let u = b.sub(a).norm();
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for q in center.iter().map(|q| q.v()) {
            let s = q.sub(a).dot(u);
            if q.sub(a).cross(u).abs() < 1e-6 && s > -1e-6 && s < a.dist(b) + 1e-6 {
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
        let p = if lo <= hi { a.add(u.scale((lo + hi) / 2.0)) } else { nearest(a.mid(b)) };
        out.push(NamedAnchor { name: format!("seg_{}_mid", k + 1), p });
    }
    out
}

fn place_rebar(ctx: &mut Ctx, pl: &Value, d: f64) -> Option<Vec<V2>> {
    if !matches!(pl, Value::Object(_)) {
        ctx.p.fail("place", "param 'place' must be {\"in\": \"comp[.part]\", \"face\": \"bottom|top|left|right|center\", \"cover\": 3, \"count\": 2, \"side_cover\": 3}".to_string());
        return None;
    }
    let Some(in_s) = pl.get("in").and_then(Value::str) else {
        ctx.p.fail("place/in", "place needs \"in\": \"<component>[.<part>]\", the host zone".to_string());
        return None;
    };
    let face = pl.get("face").and_then(Value::str).unwrap_or("bottom");
    let cover = ctx.p.field_len("place", pl, "cover", 1.5)?;
    let side_cover = ctx.p.field_len("place", pl, "side_cover", cover)?;
    let count_i = ctx.p.field_int("place", pl, "count", 1, 1, 200)?;
    let count = num::to_int_clamped::<usize>(count_i as f64, 1, 200);
    let axis = pl.get("axis").and_then(Value::str).unwrap_or("x");
    if axis != "x" && axis != "y" {
        ctx.p.fail("place/axis", format!("place.axis must be \"x\" or \"y\" (got \"{axis}\"): the direction a multi-bar row spreads for face \"center\""));
        return None;
    }
    let station: Option<f64> = match pl.get("station") {
        Some(x) => {
            let Some(s) = units::parse_length(x) else {
                ctx.p.fail("place/station", "place.station must be a length measured from the zone's left face (bottom face with axis \"y\"), e.g. 4 or \"4\\\"\"".to_string());
                return None;
            };
            Some(s)
        }
        None => None,
    };
    if station.is_some() && count > 1 {
        ctx.p.fail("place/station", format!("place.station positions ONE bar; use count 1 (or drop station to spread {count} bars between the faces)"));
        return None;
    }
    let (host_id, part) = match in_s.split_once('.') {
        Some((h, pt)) => (h, Some(pt)),
        None => (in_s, None),
    };
    let scene = ctx.scene;
    let Some(host) = scene.find(host_id) else {
        let ids = scene.comp_ids();
        let path = format!("{}/{}/place/in", ctx.p.base, ctx.p.id);
        if let Some(nn) = model::nearest(host_id, &ids) {
            scene.diags.add_fix(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(ctx.p.id),
                Some(&path),
                format!("place.in: no component '{host_id}'; did you mean '{nn}'?"),
                nn.to_string(),
            );
        } else {
            scene.diags.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(ctx.p.id),
                Some(&path),
                format!("place.in: no component '{host_id}'. Known ids: {}", join_ids(&ids)),
            );
        }
        ctx.p.ok = false;
        return None;
    };
    if host.state != State::Ok {
        ctx.p.fail("place/in", format!("place.in host '{host_id}' did not build; fix its errors first"));
        return None;
    }
    let xf0 = host.xfs.first()?;
    let bx = if let Some(pt) = part {
        let Some(lb) = Scene::part_box(host, pt) else {
            let parts = scene.part_names(host);
            let path = format!("{}/{}/place/in", ctx.p.base, ctx.p.id);
            scene.diags.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(ctx.p.id),
                Some(&path),
                format!(
                    "place.in: component '{host_id}' has no part '{pt}'. Parts: {}",
                    if parts.is_empty() { "(none)".to_string() } else { join_ids(&parts) }
                ),
            );
            ctx.p.ok = false;
            return None;
        };
        world_box(xf0, lb)
    } else {
        world_box(xf0, host.built.box_)
    };
    let r = d / 2.0;
    let zone = zone_region(host, xf0, part.is_some().then_some(bx));
    let mut out: Vec<V2> = Vec::new();
    let spread = |i: usize, lo: f64, hi: f64| lo + (hi - lo) * i as f64 / (count - 1) as f64;
    let dir = match face {
        "bottom" => V2::init(0.0, -1.0),
        "top" => V2::init(0.0, 1.0),
        "left" => V2::init(-1.0, 0.0),
        "right" => V2::init(1.0, 0.0),
        "center" => V2::init(0.0, 0.0),
        _ => {
            ctx.p.fail(
                "place/face",
                format!(
                    "place.face must be one of \"bottom\", \"top\", \"left\", \"right\", \"center\" (got \"{face}\")"
                ),
            );
            return None;
        }
    };
    let off = cover + r;
    if face == "center" {
        let along_y = axis == "y";
        let lo = (if along_y { bx.y0 } else { bx.x0 }) + side_cover + r;
        let hi = (if along_y { bx.y1 } else { bx.x1 }) - side_cover - r;
        if hi < lo - 1e-9 && count > 1 {
            ctx.p.fail(
                "place",
                format!(
                    "zone '{}' is {} {}: bars at side_cover {} do not fit; reduce side_cover or count",
                    in_s,
                    fmt_num(if along_y { bx.height() } else { bx.width() }),
                    if along_y { "tall" } else { "wide" },
                    fmt_num(side_cover)
                ),
            );
            return None;
        }
        let mid_x = (bx.x0 + bx.x1) / 2.0;
        let mid_y = (bx.y0 + bx.y1) / 2.0;
        for i in 0..count {
            let t = if count == 1 { 0.5 } else { i as f64 / (count - 1) as f64 };
            if along_y {
                let y = station.map_or(lo + (hi - lo) * t, |st| bx.y0 + st);
                out.push(V2::init(mid_x, y));
            } else {
                let x = station.map_or(lo + (hi - lo) * t, |st| bx.x0 + st);
                out.push(V2::init(x, mid_y));
            }
        }
    } else if let Some(st) = station {
        let horizontal = dir.x == 0.0;
        let line = if horizontal { bx.x0 + st } else { bx.y0 + st };
        let Some((q, cosn)) = face_at(&zone, dir, horizontal, line) else {
            let msg = format!(
                "place.station {} puts the bar at {} {}, where zone '{in_s}' has no {face} face (the zone spans {} {}..{}). Fix: use a station between 0 and {}",
                fmt_ft_in(st),
                if horizontal { "x" } else { "y" },
                fmt_ft_in(line),
                if horizontal { "x" } else { "y" },
                fmt_ft_in(if horizontal { bx.x0 } else { bx.y0 }),
                fmt_ft_in(if horizontal { bx.x1 } else { bx.y1 }),
                fmt_ft_in(if horizontal { bx.width() } else { bx.height() }),
            );
            ctx.p.fail_code("E_PLACE_OUTSIDE", "place/station", msg);
            return None;
        };
        out.push(q.sub(dir.scale(off / cosn)));
    } else {
        let Some((a, b, outn)) = face_run(&zone, dir) else {
            ctx.p.fail_code(
                "E_PLACE_OUTSIDE",
                "place/face",
                format!("zone '{in_s}' has no edge facing {face}; use another face or a part of the host"),
            );
            return None;
        };
        let len = a.dist(b);
        let u = b.sub(a).scale(1.0 / len);
        let inward = outn.scale(-1.0);
        let lo = side_cover + r;
        let hi = len - side_cover - r;
        if hi < lo - 1e-9 && count > 1 {
            let what = if bx.width() - len > 1e-6 && (face == "bottom" || face == "top") {
                format!("its {face} face (x {}..{}) is {} long", fmt_ft_in(a.x), fmt_ft_in(b.x), fmt_num(len))
            } else if bx.height() - len > 1e-6 && (face == "left" || face == "right") {
                format!("its {face} face (y {}..{}) is {} long", fmt_ft_in(a.y), fmt_ft_in(b.y), fmt_num(len))
            } else if face == "bottom" || face == "top" {
                format!("{} wide", fmt_num(len))
            } else {
                format!("{} tall", fmt_num(len))
            };
            ctx.p.fail(
                "place",
                format!(
                    "zone '{in_s}' is {what}: bars at side_cover {} do not fit ({} usable); reduce side_cover or count",
                    fmt_num(side_cover),
                    fmt_num(hi - lo)
                ),
            );
            return None;
        }
        for i in 0..count {
            let s = if count == 1 { len / 2.0 } else { spread(i, lo, hi) };
            out.push(a.add(u.scale(s)).add(inward.scale(off)));
        }
    }
    for (k, c) in out.iter().enumerate() {
        if geom::locate(*c, &zone, 1e-9) == geom::Loc::Inside {
            continue;
        }
        let which = if out.len() > 1 { format!("bar {} of {}", k + 1, out.len()) } else { "the bar".to_string() };
        let msg = format!(
            "{which} would sit at x {}, y {}, outside its host '{in_s}' (zone x {}..{}, y {}..{}): {} clear cover from the {face} face does not fit there. Fix: lower place.cover{}, or place into a part of the host (\"in\": \"{host_id}.<part>\")",
            fmt_ft_in(c.x),
            fmt_ft_in(c.y),
            fmt_ft_in(bx.x0),
            fmt_ft_in(bx.x1),
            fmt_ft_in(bx.y0),
            fmt_ft_in(bx.y1),
            fmt_ft_in(cover),
            if face == "center" { " / side_cover" } else { "" },
        );
        ctx.p.fail_code("E_PLACE_OUTSIDE", "place", msg);
        return None;
    }
    Some(out)
}

fn rect_v(b: Box) -> Vec<V2> {
    vec![V2::init(b.x0, b.y0), V2::init(b.x1, b.y0), V2::init(b.x1, b.y1), V2::init(b.x0, b.y1)]
}

/// The region bars are placed in (world): the host's outline, cut to the part's box for `in: "comp.part"`. A host
/// without an outline (not concrete or cmu) is its box.
fn zone_region(host: &Comp<'_>, xf: &Xf, part_box: Option<Box>) -> Vec<Vec<V2>> {
    let Some(h) = host.built.host.as_ref() else {
        return vec![rect_v(part_box.unwrap_or_else(|| world_box(xf, host.built.box_)))];
    };
    let outline = clip::from_region(&[xf.apply_loop(&h.outline)], FLAT_TOL);
    let Some(pb) = part_box else { return outline };
    let rect = rect_v(pb);
    let cut = clip::boolean(&outline, core::slice::from_ref(&rect), clip::Op::Intersect);
    let full = pb.width() * pb.height();
    if (clip::total_area(&cut) - full).abs() <= 1e-9 * full.max(1.0) { vec![rect] } else { cut }
}

/// The face of `zone` toward `dir`: the run of collinear edges whose outward normal points within 60 degrees of `dir`
/// and lies furthest along it (ties: the longer, then the first), as (start, end, outward normal) with start at the
/// lower x (bottom/top) or lower y (left/right).
fn face_run(zone: &[Vec<V2>], dir: V2) -> Option<(V2, V2, V2)> {
    let mut best: Option<(V2, V2, V2)> = None;
    let mut best_key = f64::NEG_INFINITY;
    for lp in zone {
        let n = lp.len();
        let normal = |i: usize| {
            let d = lp[(i + 1) % n].sub(lp[i]);
            V2::init(d.y, -d.x).norm()
        };
        let Some(first) = (0..n).find(|&i| normal(i).dot(normal((i + n - 1) % n)) < 1.0 - 1e-9) else { continue };
        let mut k = 0;
        while k < n {
            let i0 = (first + k) % n;
            let nrm = normal(i0);
            let mut j = 1;
            while j < n - k && normal((i0 + j) % n).dot(nrm) >= 1.0 - 1e-9 {
                j += 1;
            }
            let (a, b) = (lp[i0], lp[(i0 + j) % n]);
            k += j;
            if nrm.dot(dir) <= 0.5 {
                continue;
            }
            let key = a.mid(b).dot(dir);
            let longer = best.is_some_and(|(ba, bb, _)| a.dist(b) > ba.dist(bb) + 1e-9);
            if key > best_key + 1e-6 || (key > best_key - 1e-6 && longer) {
                best_key = key;
                best = Some((a, b, nrm));
            }
        }
    }
    let (a, b, nrm) = best?;
    let flip = if dir.x == 0.0 { b.x < a.x } else { b.y < a.y };
    Some(if flip { (b, a, nrm) } else { (a, b, nrm) })
}

/// Where the line x = `line` (`horizontal` faces) or y = `line` meets the outermost edge of `zone` facing `dir`, with
/// the cosine between that edge's normal and `dir`.
fn face_at(zone: &[Vec<V2>], dir: V2, horizontal: bool, line: f64) -> Option<(V2, f64)> {
    let mut best: Option<(V2, f64)> = None;
    for lp in zone {
        for (i, &a) in lp.iter().enumerate() {
            let b = lp[(i + 1) % lp.len()];
            let d = b.sub(a);
            let nrm = V2::init(d.y, -d.x).norm();
            let cosn = nrm.dot(dir);
            if cosn <= 0.5 {
                continue;
            }
            let (ua, ub) = if horizontal { (a.x, b.x) } else { (a.y, b.y) };
            if (ub - ua).abs() < 1e-12 || line < ua.min(ub) - 1e-9 || line > ua.max(ub) + 1e-9 {
                continue;
            }
            let q = a.lerp(b, ((line - ua) / (ub - ua)).clamp(0.0, 1.0));
            if best.is_none_or(|(p, _)| q.dot(dir) > p.dot(dir) + 1e-9) {
                best = Some((q, cosn));
            }
        }
    }
    best
}
