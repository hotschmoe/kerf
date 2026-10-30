//! Helpers shared by the component builders (SPEC 5): the build context, rectangles/zones/prisms, nominal sizes, point-list parsing,
//! `length`/`until`, cover, mirroring. A component type's own file (`builders/<type>.rs`) holds its `Params` and `build`.

use crate::geom::{self, Box, Pt, V2};
use crate::json::Value;
use crate::limits;
use crate::math;
use crate::model::{self, Built, Params, PartCover, Prism};
use crate::scene::{self, Scene};
use crate::style::Style;
use crate::units;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Ctx<'a, 's, 'p> {
    pub scene: &'a Scene<'s>,
    pub style: &'a Style,
    pub p: Params<'p>,
    /// Placement point (world) used as the origin of point-list builders.
    pub origin: V2,
    /// Placement rotation (radians CCW): `until` measures along the member's rotated run direction.
    pub angle: f64,
}

pub fn box_of_prisms(prisms: &[Prism]) -> Box {
    let mut b = Box::default();
    for p in prisms {
        for l in &p.loops {
            b.add_box(geom::loop_box(l));
        }
    }
    b
}

pub fn zone_rect(name: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> model::Zone {
    model::Zone {
        name: name.into(),
        loops: model::one_loop(model::rect_loop(x0, y0, x1, y1)),
        box_: Box { x0, y0, x1, y1 },
    }
}

pub fn one_prism(prism: Prism) -> Vec<Prism> {
    alloc::vec![prism]
}

pub use crate::model::fmt_num;
pub use crate::units::fmt_ft_in;

/// Rect quads for cut marks. Corners CCW from bottom-left.
pub fn quad_of(x0: f64, y0: f64, x1: f64, y1: f64) -> [V2; 4] {
    [V2::init(x0, y0), V2::init(x1, y0), V2::init(x1, y1), V2::init(x0, y1)]
}

#[derive(Clone, Copy)]
pub struct SawnSize {
    pub t: f64,
    pub d: f64,
}

pub fn sawn_depth(nominal_t: u32, nd: u32) -> Option<f64> {
    if nominal_t >= 6 {
        return match nd {
            6 => Some(5.5),
            8 => Some(7.5),
            10 => Some(9.5),
            12 => Some(11.5),
            _ => None,
        };
    }
    match nd {
        3 => Some(2.5),
        4 => Some(3.5),
        6 => Some(5.5),
        8 => Some(7.25),
        10 => Some(9.25),
        12 => Some(11.25),
        14 => Some(13.25),
        _ => None,
    }
}

pub fn sawn_thickness(nt: u32) -> Option<f64> {
    match nt {
        1 => Some(0.75),
        2 => Some(1.5),
        3 => Some(2.5),
        4 => Some(3.5),
        6 => Some(5.5),
        8 => Some(7.25),
        _ => None,
    }
}

/// Parse `NxM` as a sawn nominal size. None if it is not a recognised nominal size.
pub fn parse_sawn(size: &str) -> Option<SawnSize> {
    let xi = size.find(['x', 'X'])?;
    let a: u32 = size.get(..xi)?.trim_matches(' ').parse().ok()?;
    let b: u32 = size.get(xi + 1..)?.trim_matches(' ').parse().ok()?;
    Some(SawnSize { t: sawn_thickness(a)?, d: sawn_depth(a, b)? })
}

/// Parse `AxB` actual sizes (lengths, e.g. "1.75x11.875" or "1 3/4x9 1/4").
pub fn parse_actual(size: &str) -> Option<SawnSize> {
    let bytes = size.as_bytes();
    let xi = bytes.iter().position(|&c| c == b'x' || c == b'X' || c == 0xc3)?;
    let mut rest = size.get(xi + 1..)?;
    if bytes[xi] == 0xc3 && !rest.is_empty() {
        rest = rest.get(1..)?; // UTF-8 multiplication sign
    }
    let a = units::parse_length_str(size.get(..xi)?)?;
    let b = units::parse_length_str(rest)?;
    if a <= 0.0 || b <= 0.0 || !units::length_in_range(a) || !units::length_in_range(b) {
        return None;
    }
    Some(SawnSize { t: a, d: b })
}

/// Accepted magnitude of a polygon bulge: below the minimum the arc is a straight line to within rounding (and its radius overflows
/// the arc flattener), above the maximum it is a near-full circle with an absurd radius.
pub const MIN_BULGE: f64 = 1e-6;
pub const MAX_BULGE: f64 = 1e3;

/// Parse a points list. Literal [x,y(,b)] entries are relative to the placement point; Refs are
/// absolute. The result is in local coordinates (world - origin for Refs).
pub fn parse_point_list(ctx: &mut Ctx, key: &str, v: &Value, local: bool) -> Option<Vec<Pt>> {
    let Some(arr) = v.arr() else {
        ctx.p.fail(key, format!("param '{key}' must be an array of points: [x, y], [x, y, bulge], \"comp@anchor\" or {{\"ref\": \"comp@anchor\", \"offset\": [dx, dy]}}"));
        return None;
    };
    if arr.len() > limits::MAX_POINTS {
        let m = limits::message(&format!("points in '{key}'"), arr.len(), limits::MAX_POINTS, "");
        ctx.p.fail_code("E_LIMIT", key, format!("{m}. Fix: simplify the outline (a drawing detail rarely needs more than a few dozen vertices), or split it into several components"));
        return None;
    }
    let mut out: Vec<Pt> = Vec::new();
    let mut ok = true;
    for (i, e) in arr.iter().enumerate() {
        let path = format!("{key}/{i}");
        let fpath = format!("{}/{}/{}", ctx.p.base, ctx.p.id, path);
        match e {
            Value::Array(xy) => {
                if xy.len() < 2 || xy.len() > 3 {
                    ctx.p.fail(&path, format!("point {i} of '{key}' must be [x, y] or [x, y, bulge]"));
                    ok = false;
                    continue;
                }
                let x = units::parse_length(&xy[0]);
                let y = units::parse_length(&xy[1]);
                let (Some(x), Some(y)) = (x, y) else {
                    let bad = if units::parse_length(&xy[0]).is_none() { &xy[0] } else { &xy[1] };
                    ctx.p.fail(
                        &path,
                        format!(
                            "point {i} of '{key}': a coordinate must be a length within +-{} inches (got {})",
                            fmt_num(limits::MAX_COORD_IN),
                            model::kind_or_text(bad)
                        ),
                    );
                    ok = false;
                    continue;
                };
                let mut b = 0.0;
                if xy.len() == 3 {
                    let Some(bn) = xy[2].num() else {
                        ctx.p.fail(&path, format!("point {i} of '{key}': the third value is the bulge and must be a number (got {}); use [x, y] for a straight edge", model::kind_or_text(&xy[2])));
                        ok = false;
                        continue;
                    };
                    if bn != 0.0 && !(bn.abs() >= MIN_BULGE && bn.abs() <= MAX_BULGE) {
                        ctx.p.fail(&path, format!("point {i} of '{key}': bulge {} is out of range. Use 0 (or omit it) for a straight edge, or a value with 0.000001 <= |bulge| <= 1000 (bulge = tan(sweep/4): 1 is a half circle, 0.4142 a quarter circle; the sign picks the side)", model::num_text(bn)));
                        ok = false;
                        continue;
                    }
                    b = bn;
                }
                out.push(Pt { x, y, b });
            }
            Value::String(s) => match ctx.scene.resolve_ref_str(s, ctx.p.id, &fpath) {
                Some(w) => {
                    let q = if local { w.sub(ctx.origin) } else { w };
                    out.push(Pt::new(q.x, q.y));
                }
                None => ok = false,
            },
            Value::Object(_) => {
                let Some(rs) = e.get("ref") else {
                    ctx.p.fail(&path, format!("point {i} of '{key}': an object point needs a \"ref\""));
                    ok = false;
                    continue;
                };
                let Some(rstr) = rs.str() else {
                    ctx.p.fail(&path, format!("point {i} of '{key}': \"ref\" must be a string"));
                    ok = false;
                    continue;
                };
                match ctx.scene.resolve_ref_str(rstr, ctx.p.id, &fpath) {
                    Some(w) => {
                        let mut q = w;
                        if let Some(off) = e.get("offset").filter(|o| !o.is_null()) {
                            match ctx.p.offset_pair(&format!("{path}/offset"), off) {
                                Some(o) => q = q.add(o),
                                None => {
                                    ok = false;
                                    continue;
                                }
                            }
                        }
                        if local {
                            q = q.sub(ctx.origin);
                        }
                        out.push(Pt::new(q.x, q.y));
                    }
                    None => ok = false,
                }
            }
            _ => {
                ctx.p.fail(&path, format!("point {i} of '{key}' must be [x, y], a Ref string or {{ref, offset}}"));
                ok = false;
            }
        }
    }
    ok.then_some(out)
}

pub fn drop_duplicate_points(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    for p in pts {
        if out.last().is_some_and(|l| l.v().eql(p.v(), 1e-9)) {
            continue;
        }
        out.push(*p);
    }
    out
}

pub fn oriented_ccw(lp: Vec<Pt>) -> Vec<Pt> {
    if geom::signed_area(&lp) < 0.0 { geom::reverse_loop(&lp) } else { lp }
}

pub fn material_ok(ctx: &mut Ctx, key: &str, name: &str) -> bool {
    if ctx.style.material(name).is_some() {
        return true;
    }
    let names: Vec<&str> = ctx.style.materials.iter().map(|m| m.name.as_str()).collect();
    ctx.p.fail(key, format!("material '{name}' is not defined in the style. Available: {}", scene::join_ids(&names)));
    false
}

/// `length` or `until` (SPEC 17) for members that run along x or y. Returns the length in inches.
pub fn length_or_until(ctx: &mut Ctx, run: &str, hint: &str, note: &mut String) -> Option<f64> {
    let has_len = ctx.p.has("length");
    let Some(uv) = ctx.p.raw("until") else { return ctx.p.len_pos("length", None, hint) };
    if has_len {
        ctx.p.fail(
            "until",
            "give either 'length' or 'until', not both (the engine computes length from 'until')".into(),
        );
        return None;
    }
    let anchor = ctx.p.raw("at").and_then(|at| at.get("anchor")).and_then(Value::str).unwrap_or("bottom_left");
    let along_x = run == "x";
    let mut dir = 0.0;
    if along_x {
        if anchor.ends_with("_left") {
            dir = 1.0;
        } else if anchor.ends_with("_right") {
            dir = -1.0;
        }
    } else if anchor.starts_with("top_") {
        dir = -1.0;
    } else if anchor.starts_with("bottom_") {
        dir = 1.0;
    }
    if dir == 0.0 {
        ctx.p.fail("until", format!("'until' needs a placement anchor at one end of the member: for run {run} use {} (got anchor \"{anchor}\"); center anchors cannot define a growth direction", if along_x { "*_left or *_right" } else { "top_* or bottom_*" }));
        return None;
    }
    let path = format!("{}/{}/until", ctx.p.base, ctx.p.id);
    let Some(target) = ctx.scene.point(uv, ctx.p.id, &path) else {
        ctx.p.ok = false;
        return None;
    };
    let run_dir = if along_x {
        V2::init(math::cos(ctx.angle), math::sin(ctx.angle))
    } else {
        V2::init(-math::sin(ctx.angle), math::cos(ctx.angle))
    };
    let start = ctx.origin.x * run_dir.x + ctx.origin.y * run_dir.y;
    let end = target.x * run_dir.x + target.y * run_dir.y;
    let len = (end - start) * dir;
    if len <= 1e-6 {
        let grows = if along_x {
            if dir > 0.0 { "right (+x)" } else { "left (-x)" }
        } else if dir > 0.0 {
            "up (+y)"
        } else {
            "down (-y)"
        };
        ctx.p.fail("until", format!(
            "'until' target is {} the anchor along {} (anchor at {}, target at {}); the member would have length {}. The anchor grows {}",
            if len < -1e-6 { "behind" } else { "at" },
            if along_x { "x" } else { "y" },
            fmt_num(start),
            fmt_num(end),
            fmt_num(len),
            grows,
        ));
        return None;
    }
    let ref_txt = match uv {
        Value::String(t) => t.as_str(),
        Value::Object(_) => uv.get("ref").map_or("ref", |r| r.str().unwrap_or("ref")),
        _ => "ref",
    };
    *note = format!(" (until {ref_txt})");
    Some(len)
}

pub struct ParsedCover {
    pub cover: model::Cover,
    pub parts: Vec<PartCover>,
}

fn read_cover_fields(p: &mut Params, key: &str, obj: &Value, base: model::Cover) -> Option<model::Cover> {
    Some(model::Cover {
        bottom: p.field_len(key, obj, "bottom", base.bottom)?,
        sides: p.field_len(key, obj, "sides", base.sides)?,
        top: p.field_len(key, obj, "top", base.top)?,
    })
}

/// None: a parameter error (already reported).
pub fn parse_cover(ctx: &mut Ctx, key: &str, base: model::Cover) -> Option<ParsedCover> {
    let Some(v) = ctx.p.raw(key) else { return Some(ParsedCover { cover: base, parts: Vec::new() }) };
    if !matches!(v, Value::Object(_)) {
        ctx.p.fail(key, format!("param '{key}' must be an object like {{\"bottom\": 3, \"sides\": 3, \"top\": 1.5}}"));
        return None;
    }
    let c = read_cover_fields(&mut ctx.p, key, v, base)?;
    let mut parts = Vec::new();
    if let Some(Value::Object(members)) = v.get("parts") {
        for m in members {
            if !matches!(m.value, Value::Object(_)) {
                ctx.p.fail(
                    key,
                    format!(
                        "{key}.parts.{} must be an object like {{\"bottom\": 0.75}} (got {})",
                        m.key,
                        model::kind_or_text(&m.value)
                    ),
                );
                return None;
            }
            let part_key = format!("{key}/parts/{}", m.key);
            let pc = read_cover_fields(&mut ctx.p, &part_key, &m.value, c)?;
            parts.push(PartCover { part: m.key.clone(), cover: pc });
        }
    }
    Some(ParsedCover { cover: c, parts })
}

pub fn mirror_built(b: &Built, xf: &geom::Xf, keep_box: bool) -> Built {
    let mut out = b.clone();
    out.prisms = b.prisms.iter().map(|pr| pr.transform(xf)).collect();
    out.anchors = b.anchors.iter().map(|x| model::NamedAnchor { name: x.name.clone(), p: xf.apply(x.p) }).collect();
    out.zones = b
        .zones
        .iter()
        .map(|z| {
            let loops: Vec<Vec<Pt>> = z.loops.iter().map(|l| xf.apply_loop(l)).collect();
            let mut bx = Box::default();
            for l in &loops {
                bx.add_box(geom::loop_box(l));
            }
            model::Zone { name: z.name.clone(), loops, box_: bx }
        })
        .collect();
    if let Some(h) = &b.host {
        let mut nh = h.clone();
        nh.outline = xf.apply_loop(&h.outline);
        out.host = Some(nh);
    }
    if !keep_box {
        out.box_ = box_of_prisms(&out.prisms);
    }
    out
}

pub fn vs_of(pts: &[Pt]) -> Vec<V2> {
    pts.iter().map(|q| q.v()).collect()
}

pub fn path_len(vs: &[V2]) -> f64 {
    let mut t = 0.0;
    for w in vs.windows(2) {
        t += w[0].dist(w[1]);
    }
    t
}

pub fn world_box(xf: &geom::Xf, b: Box) -> Box {
    let mut out = Box::default();
    for c in [V2::init(b.x0, b.y0), V2::init(b.x1, b.y0), V2::init(b.x1, b.y1), V2::init(b.x0, b.y1)] {
        let w = xf.apply(c);
        out.add_point(w.x, w.y);
    }
    out
}

pub fn gauge_thickness(g: u32) -> Option<f64> {
    Some(match g {
        10 => 0.1345,
        11 => 0.1196,
        12 => 0.1046,
        14 => 0.0747,
        16 => 0.0598,
        18 => 0.0478,
        20 => 0.0359,
        22 => 0.0299,
        24 => 0.0239,
        26 => 0.0179,
        28 => 0.0149,
        _ => return None,
    })
}

/// A style material when the style defines it, else `fallback` (custom styles may lack the v0.1.2 additions).
pub fn material_or<'n>(ctx: &Ctx, name: &'n str, fallback: &'n str) -> &'n str {
    if ctx.style.material(name).is_some() { name } else { fallback }
}

pub fn mirror_about_center(b: &Built) -> Built {
    let cx = (b.box_.x0 + b.box_.x1) / 2.0;
    let xf = geom::Xf::translate(2.0 * cx, 0.0).mul(geom::Xf::scaling(-1.0, 1.0));
    mirror_built(b, &xf, true)
}
