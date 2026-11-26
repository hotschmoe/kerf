use super::common::{Ctx, drop_duplicate_points, parse_point_list, path_len, world_box};
use super::rebar_bends as bends;
use crate::clip;
use crate::geom::{self, Box, FLAT_TOL, Pt, V2, Xf};
use crate::json::Value;
use crate::math;
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
param_enum! { pub enum Plane { Xy = "xy", Xz = "xz" } }

params! {
    pub struct Params {
        size: Str = "#4" => { desc: "#3 .375, #4 .5, #5 .625, #6 .75, #7 .875, #8 1.0 (diameter in)" },
        mode: Enum(Mode) = Mode::AlongZ => { desc: "along_z (continuous bar seen as a dot) or path (bar in the XY plane)" },
        plane: Enum(Plane) = Plane::Xy => { desc: "path: xy = the bar lies in the section plane (points [x, y]); xz = the bar lies flat at height `y` (points [x, z]: corner and intersection bars, bars around an opening). A plan view draws an xz bar as its path, a section as a dot where it crosses the cut and a line where it runs behind it" },
        y: OptRaw => { def: "plane xz: required", desc: "height of the bar's centreline, a length or a Ref (its y), e.g. {\"ref\": \"ftg@bottom_left\", \"offset\": [0, 3.25]}" },
        place: OptRaw => { desc: "cover-based placement (preferred): {in: \"comp[.part]\", face: bottom|top|left|right|center, cover: 3, count: 2, side_cover: cover, axis: x|y, station: in}. bottom/top/left/right: bars at clear `cover` from that face of the host's outline (cut to the part for comp.part; in a polygon, the outermost run of edges facing that way, e.g. the lowest bottom run = a thickened footing), spread evenly along it at `side_cover` from its ends (count 1 centers); a bar centre outside the zone is E_PLACE_OUTSIDE. center: bars centered in the zone on both axes (e.g. a single #4 in the middle of a stem wall); count > 1 spreads along axis x (default) or y at side_cover. station: ONE bar at that offset from the zone's left face (bottom face with axis y; for bottom/top/left/right faces it sets the along-face position), or at the x (y) of a Ref, e.g. \"ftg@vertex_7\" or {ref, offset}, which follows that member when the host is resized" },
        points: OptRaw => { def: "path: required", desc: "polyline [x,y] or Refs (plane xz: [x, z] pairs); bends get radius bend_radius, drawn as fillets" },
        bend_radius: OptLenPos => { def: "3*d_b", desc: "inside bend radius for path bars" },
        spacing_note: Str = "" => { def: "null", desc: "e.g. \"#4 @ 16\\\" O.C.\" for summaries and notes" },
        hook: OptRaw => { desc: "path: standard hooks on the bar ends, {\"start\": {...}, \"end\": {\"angle\": 90|135|180, \"toward\": \"up|down|left|right\"}}: the bend diameter and extension come from the bar size (ACI 318 Table 25.3.1: 6 db inside diameter #3-#8, 12 db extension at 90, 4 db >= 2 1/2 in. at 180; \"kind\": \"tie\" or angle 135 uses Table 25.3.2: 4 db diameter #3-#5, 6 db >= 3 in. extension), never hand-computed points. `toward` is the side the hook turns to as drawn (plane xz: as seen in plan); the end point of `points` stays the out-to-out corner of the bar (90) or its far reach (135, 180); `ext` overrides the extension. Example: {\"mode\":\"path\",\"points\":[[0,3],[30,3]],\"hook\":{\"end\":{\"angle\":90,\"toward\":\"up\"}}}" },
        tie: OptRaw => { desc: "a closed tie, stirrup, crosstie or hoop placed in a member by cover (no `points`, `at` or `mode`): {in: \"comp[.part]\", cover: 1.5, shape: closed|u|crosstie|hoop, hook: 135|90, corner: top_left|top_right|bottom_left|bottom_right (closed: where the hooks lap), open: top|bottom|left|right (u), axis: x|y, station, toward (crosstie)}. Plane xy (default): the tie lies in the section (a beam or grade beam stirrup, a column cut across). Plane xz with `y`: a horizontal tie around a column, pedestal or pier (the host's z extent is its depth: give the column a `z`), drawn as the tie in plan and as a line with a dot at each cut leg in a section; `array` {axis: y} repeats it up the column. Bends and hook extensions are the ACI tie hooks for the bar size. Example: {\"type\":\"rebar\",\"size\":\"#3\",\"plane\":\"xz\",\"y\":20,\"tie\":{\"in\":\"pedestal\",\"cover\":1.5},\"array\":{\"axis\":\"y\",\"count\":3,\"spacing\":-8}}" },
        epoxy: OptRaw => { desc: "path, plane xy: an adhesive dowel, {in: \"comp\", embed: 6, end: start|end, hole: d + 1/8}: the end of the bar that runs into `in` (an `existing` footing, stem, slab or wall, or new concrete) sits in a drilled hole drawn around it (part `epoxy`). End that leg ON the face it enters (a Ref with an offset along the face) and give `embed`: the engine sets the bar `embed` past the face; without `embed` the depth is measured from the points. Anchors embed_face and embed_end dimension it; W_EPOXY when the hole leaves under 1 in. of the host behind it. Example: {\"mode\":\"path\",\"points\":[{\"ref\":\"e_stem@top_right\",\"offset\":[0,-4]},{\"ref\":\"e_stem@top_right\",\"offset\":[24,-4]}],\"epoxy\":{\"in\":\"e_stem\",\"embed\":6}}" },
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
    if let Some(tv) = rp.tie.as_ref() {
        return build_tie(ctx, &rp, tv, size, d);
    }
    if rp.plane == Plane::Xz {
        return build_xz(ctx, &rp, size, d);
    }
    if rp.mode == Mode::AlongZ
        && let Some(key) = ["hook", "epoxy"].into_iter().find(|k| ctx.p.has(k))
    {
        ctx.p.fail(key, format!("'{key}' is for a bar drawn along its length: add \"mode\": \"path\" and 'points' (a bar along z is a dot and shows no {key})"));
        return None;
    }
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
    let mut vs: Vec<V2> = clean.iter().map(|q| q.v()).collect();
    let epoxy = match rp.epoxy.as_ref() {
        Some(ev) => Some(bends::epoxy(ctx, ev, &mut vs, d)?),
        None => None,
    };
    let body = pathgeom::fillet(&vs, br + r);
    let mut anchors = path_anchors(&vs, &body);
    let (center, hook_info) = match rp.hook.as_ref() {
        Some(hv) => {
            let hooks = bends::parse_hooks(ctx, hv, false)?;
            let h = bends::apply_hooks(ctx, &body, &hooks, bends::bar_number(size), d)?;
            anchors.extend(h.anchors);
            (h.center, h.info)
        }
        None => (body, alloc::string::String::new()),
    };
    let rib = pathgeom::ribbon(&center, r, r);
    let mut box_ = geom::loop_box(&rib);
    let note =
        if rp.spacing_note.is_empty() { alloc::string::String::new() } else { format!(" ({})", rp.spacing_note) };
    let mut prisms = vec![bar_prism(rib, center, r)];
    let mut epoxy_info = alloc::string::String::new();
    if let Some(e) = epoxy {
        box_.add_box(geom::loop_box(&e.hole));
        prisms.push(Prism {
            part: "epoxy".to_string(),
            material: super::common::material_or(ctx, "grout", "generic").to_string(),
            loops: model::one_loop(e.hole),
            embedded: true,
            zhalf: r + 0.0625,
            ..Prism::default()
        });
        anchors.push(NamedAnchor { name: "embed_face".to_string(), p: e.face });
        anchors.push(NamedAnchor { name: "embed_end".to_string(), p: e.tip });
        epoxy_info = e.info;
    }
    Some(Built {
        prisms,
        anchors,
        box_,
        nat_z: Some(d),
        points_mode: true,
        bar_d: d,
        info: format!("rebar {} path L={}{}{}{}", size, fmt_ft_in(path_len(&vs)), hook_info, epoxy_info, note),
        ..Built::default()
    })
}

fn bar_prism(rib: Vec<Pt>, center: Vec<Pt>, r: f64) -> Prism {
    Prism {
        material: "rebar".to_string(),
        loops: model::one_loop(rib),
        embedded: true,
        centerline: center,
        sweep_r: r,
        ..Prism::default()
    }
}

fn xz_height(ctx: &mut Ctx, rp: &Params) -> Option<f64> {
    let Some(yv) = rp.y.as_ref() else {
        ctx.p.fail("y", "plane \"xz\" needs 'y': the height of the bar's centreline (a length, or a Ref whose y is used, e.g. {\"ref\": \"ftg@bottom_left\", \"offset\": [0, 3.25]})".to_string());
        return None;
    };
    match units::parse_length(yv) {
        Some(y) => Some(y),
        None => {
            let path = format!("{}/{}/y", ctx.p.base, ctx.p.id);
            let id = ctx.p.id.to_string();
            Some(ctx.scene.point(yv, &id, &path)?.y)
        }
    }
}

fn build_tie(ctx: &mut Ctx, rp: &Params, tv: &Value, size: &str, d: f64) -> Option<Built> {
    let r = d / 2.0;
    for key in ["points", "place", "hook", "epoxy", "at", "rotate"] {
        if ctx.p.has(key) {
            ctx.p.fail(
                key,
                format!(
                    "a 'tie' is placed and shaped by its host ('tie.in', 'tie.cover', 'tie.shape'): remove '{key}'"
                ),
            );
            return None;
        }
    }
    let xz = rp.plane == Plane::Xz;
    let y = if xz { xz_height(ctx, rp)? } else { 0.0 };
    let t = bends::tie(ctx, tv, bends::bar_number(size), d, xz)?;
    ctx.fixed.get_or_insert_with(|| "a tie is placed in its host".to_string());
    let info = format!(
        "rebar {size} {}{}",
        t.info,
        if xz { format!(" at y {}", fmt_ft_in(y)) } else { alloc::string::String::new() }
    );
    if !xz {
        let mut box_ = Box::default();
        let mut prisms = Vec::new();
        for c in t.paths {
            let rib = pathgeom::ribbon(&c, r, r);
            box_.add_box(geom::loop_box(&rib));
            prisms.push(bar_prism(rib, c, r));
        }
        return Some(Built {
            prisms,
            anchors: t.anchors,
            box_,
            nat_z: Some(d),
            points_mode: true,
            bar_d: d,
            info,
            ..Built::default()
        });
    }
    let mut prisms = Vec::new();
    let mut box_ = Box::default();
    for c in &t.paths {
        let zc: Vec<Pt> = c.iter().map(|p| Pt { x: p.x, y: -p.y, b: -p.b }).collect();
        xz_legs(&zc, y, d, &mut prisms, &mut box_);
    }
    let anchors = t.anchors.iter().map(|a| NamedAnchor { name: a.name.clone(), p: V2::init(a.p.x, y) }).collect();
    let plan_z = t.anchors.into_iter().map(|a| NamedAnchor { name: a.name, p: V2::init(0.0, -a.p.y) }).collect();
    Some(Built {
        prisms,
        anchors,
        plan_z,
        box_,
        nat_z: Some(d),
        abs_z: true,
        points_mode: true,
        bar_d: d,
        info,
        ..Built::default()
    })
}

/// The leg prisms of one centreline [x, z] at height y (SPEC 26.3); the first carries the whole path for drawing.
fn xz_legs(center: &[Pt], y: f64, d: f64, prisms: &mut Vec<Prism>, box_: &mut Box) {
    let r = d / 2.0;
    let flat = geom::flatten_polyline(center, false, FLAT_TOL);
    let mut tol = d / 4.0;
    let mut coarse = geom::flatten_polyline(center, false, tol);
    while coarse.len() > XZ_POINTS_MAX && tol < 1e6 {
        tol *= 4.0;
        coarse = geom::flatten_polyline(center, false, tol);
    }
    let total_dz: f64 = coarse.windows(2).map(|w| (w[1].y - w[0].y).abs()).sum();
    let step = d.max(total_dz / XZ_SLABS_MAX as f64);
    let first = prisms.len();
    let leg = |loops: Vec<Pt>, zc: f64, zh: f64| Prism {
        material: "rebar".to_string(),
        loops: model::one_loop(loops),
        embedded: true,
        sweep_r: r,
        zoff: zc,
        zhalf: zh.max(1e-4),
        xz_bar: true,
        ..Prism::default()
    };
    for w in coarse.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dz) = ((b.x - a.x).abs(), (b.y - a.y).abs());
        if dz < 1e-6 {
            let lp = model::rect_loop(a.x.min(b.x), y - r, a.x.max(b.x), y + r);
            prisms.push(leg(lp, a.y, r));
        } else if dx < 1e-6 {
            prisms.push(leg(model::circle_loop(a.x, y, r), (a.y + b.y) / 2.0, dz / 2.0));
        } else {
            let n = num::to_int_clamped::<usize>(math::ceil(dz / step), 1, XZ_STEPS_MAX);
            for k in 0..n {
                let (p, q) = (a.lerp(b, k as f64 / n as f64), a.lerp(b, (k + 1) as f64 / n as f64));
                let lp = model::rect_loop(p.x.min(q.x) - r, y - r, p.x.max(q.x) + r, y + r);
                prisms.push(leg(lp, (p.y + q.y) / 2.0, (q.y - p.y).abs() / 2.0));
            }
        }
    }
    if let Some(p) = prisms.get_mut(first) {
        p.path3 = flat.iter().map(|v| [v.x, y, v.y]).collect();
    }
    for v in &flat {
        box_.add_point(v.x - r, y - r);
        box_.add_point(v.x + r, y + r);
    }
}

const XZ_STEPS_MAX: usize = 48;
const XZ_POINTS_MAX: usize = 200;
const XZ_SLABS_MAX: usize = 400;

fn build_xz(ctx: &mut Ctx, rp: &Params, size: &str, d: f64) -> Option<Built> {
    let r = d / 2.0;
    if rp.place.is_some() {
        ctx.p.fail("place", "plane \"xz\" bars are drawn from 'points' ([x, z] pairs) at height 'y'; 'place' is for bars along z (remove place, or remove plane)".to_string());
        return None;
    }
    if rp.epoxy.is_some() {
        ctx.p.fail("epoxy", "'epoxy' is for a bar in the section plane (plane \"xy\", the default): draw the adhesive dowel in a section through the member it is drilled into".to_string());
        return None;
    }
    let y = xz_height(ctx, rp)?;
    let Some(arr) = rp.points.as_ref().and_then(Value::arr) else {
        ctx.p.fail(
            "points",
            "plane \"xz\" needs 'points': [[x, z], ...], the bar's corners seen from above (model x and z)".to_string(),
        );
        return None;
    };
    let mut vs: Vec<V2> = Vec::new();
    for (i, e) in arr.iter().enumerate() {
        let xz = e
            .arr()
            .filter(|a| a.len() == 2)
            .and_then(|a| Some(V2::init(units::parse_length(&a[0])?, units::parse_length(&a[1])?)));
        let Some(v) = xz else {
            ctx.p.fail(&format!("points/{i}"), format!("point {i} of a plane \"xz\" bar must be [x, z], two lengths (model x and z; the height is 'y'), got {}", model::kind_or_text(e)));
            return None;
        };
        if vs.last().is_none_or(|l: &V2| l.dist(v) > 1e-9) {
            vs.push(v);
        }
    }
    if vs.len() < 2 {
        ctx.p.fail("points", format!("a path bar needs at least 2 distinct points (got {})", vs.len()));
        return None;
    }
    let br = rp.bend_radius.unwrap_or(3.0 * d);
    let body = pathgeom::fillet(&vs, br + r);
    let mut xz_anchors = path_anchors(&vs, &body);
    let (center, hook_info) = match rp.hook.as_ref() {
        Some(hv) => {
            let hooks = bends::parse_hooks(ctx, hv, true)?;
            let h = bends::apply_hooks(ctx, &body, &hooks, bends::bar_number(size), d)?;
            xz_anchors.extend(h.anchors);
            (h.center, h.info)
        }
        None => (body, alloc::string::String::new()),
    };
    let mut prisms: Vec<Prism> = Vec::new();
    let mut box_ = Box::default();
    xz_legs(&center, y, d, &mut prisms, &mut box_);
    let anchors = xz_anchors.iter().map(|a| NamedAnchor { name: a.name.clone(), p: V2::init(a.p.x, y) }).collect();
    let plan_z = xz_anchors.into_iter().map(|a| NamedAnchor { name: a.name, p: V2::init(0.0, a.p.y) }).collect();
    let note =
        if rp.spacing_note.is_empty() { alloc::string::String::new() } else { format!(" ({})", rp.spacing_note) };
    Some(Built {
        prisms,
        anchors,
        plan_z,
        box_,
        nat_z: Some(d),
        abs_z: true,
        points_mode: true,
        bar_d: d,
        info: format!("rebar {size} xz path at y {} L={}{}{}", fmt_ft_in(y), fmt_ft_in(path_len(&vs)), hook_info, note),
        ..Built::default()
    })
}

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
    for (k, &v) in vs.iter().enumerate().take(n - 1).skip(1) {
        out.push(NamedAnchor { name: format!("bend_{k}"), p: nearest(v) });
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

#[derive(Clone, Copy)]
enum Station {
    Offset(f64),
    At(V2),
}

impl Station {
    fn coord(self, lo: f64, horizontal: bool) -> f64 {
        match self {
            Station::Offset(o) => lo + o,
            Station::At(p) if horizontal => p.x,
            Station::At(p) => p.y,
        }
    }
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
    let station: Option<Station> = match pl.get("station") {
        Some(x) => match units::parse_length(x) {
            Some(s) => Some(Station::Offset(s)),
            None if matches!(x, Value::String(_) | Value::Object(_)) => {
                let path = format!("{}/{}/place/station", ctx.p.base, ctx.p.id);
                let id = ctx.p.id.to_string();
                let Some(p) = ctx.scene.point(x, &id, &path) else {
                    ctx.p.ok = false;
                    return None;
                };
                Some(Station::At(p))
            }
            None => {
                ctx.p.fail("place/station", "place.station must be a length measured from the zone's left face (bottom face with axis \"y\"), e.g. 4 or \"4\\\"\", or a Ref whose x (y for left/right faces and axis \"y\") the bar takes, e.g. \"ftg@vertex_7\" or {\"ref\": \"ftg@bottom_left\", \"offset\": [12, 0]}".to_string());
                return None;
            }
        },
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
                let y = station.map_or(lo + (hi - lo) * t, |st| st.coord(bx.y0, false));
                out.push(V2::init(mid_x, y));
            } else {
                let x = station.map_or(lo + (hi - lo) * t, |st| st.coord(bx.x0, true));
                out.push(V2::init(x, mid_y));
            }
        }
    } else if let Some(st) = station {
        let horizontal = dir.x == 0.0;
        let line = st.coord(if horizontal { bx.x0 } else { bx.y0 }, horizontal);
        let Some((q, cosn)) = face_at(&zone, dir, horizontal, line) else {
            let what = match st {
                Station::Offset(o) => fmt_ft_in(o),
                Station::At(_) => format!("(a Ref at {} {})", if horizontal { "x" } else { "y" }, fmt_ft_in(line)),
            };
            let msg = format!(
                "place.station {} puts the bar at {} {}, where zone '{in_s}' has no {face} face (the zone spans {} {}..{}). Fix: use a station between 0 and {}",
                what,
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
