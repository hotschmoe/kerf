//! rebar (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, parse_point_list, path_len, world_box};
use crate::geom::{self, Box, V2};
use crate::json::Value;
use crate::model::fmt_num;
use crate::model::{self, Built, Level, Prism};
use crate::num;
use crate::params::{param_enum, params};
use crate::pathgeom;
use crate::scene::{Scene, State, join_ids};
use crate::units;
use crate::units::ftin;
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
        place: OptRaw => { desc: "cover-based placement (preferred): {in: \"comp[.part]\", face: bottom|top|left|right|center, cover: 3, count: 2, side_cover: cover, axis: x|y, station: in}. bottom/top/left/right: bars at clear `cover` from that face, spread evenly between the zone's adjacent faces at `side_cover` (count 1 centers). center: bars centered in the zone on both axes (e.g. a single #4 in the middle of a stem wall); count > 1 spreads along axis x (default) or y at side_cover. station: ONE bar at that offset from the zone's left face (bottom face with axis y; for bottom/top/left/right faces it sets the along-face position)" },
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
        box_,
        nat_z: Some(d),
        points_mode: true,
        bar_d: d,
        info: format!("rebar {} path L={}{}", size, ftin(path_len(&vs)), note),
        ..Built::default()
    })
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
    let mut out: Vec<V2> = Vec::new();
    let spread = |i: usize, lo: f64, hi: f64| lo + (hi - lo) * i as f64 / (count - 1) as f64;
    match face {
        "center" => {
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
        }
        "bottom" | "top" => {
            let y = if face == "bottom" { bx.y0 + cover + r } else { bx.y1 - cover - r };
            let xa = bx.x0 + side_cover + r;
            let xb = bx.x1 - side_cover - r;
            if xb < xa - 1e-9 && count > 1 {
                ctx.p.fail(
                    "place",
                    format!("zone '{}' is {} wide: bars at side_cover {} do not fit ({} usable); reduce side_cover or count", in_s, fmt_num(bx.width()), fmt_num(side_cover), fmt_num(xb - xa)),
                );
                return None;
            }
            for i in 0..count {
                let x = match station {
                    Some(st) => bx.x0 + st,
                    None if count == 1 => (bx.x0 + bx.x1) / 2.0,
                    None => spread(i, xa, xb),
                };
                out.push(V2::init(x, y));
            }
        }
        "left" | "right" => {
            let x = if face == "left" { bx.x0 + cover + r } else { bx.x1 - cover - r };
            let ya = bx.y0 + side_cover + r;
            let yb = bx.y1 - side_cover - r;
            if yb < ya - 1e-9 && count > 1 {
                ctx.p.fail(
                    "place",
                    format!(
                        "zone '{}' is {} tall: bars at side_cover {} do not fit; reduce side_cover or count",
                        in_s,
                        fmt_num(bx.height()),
                        fmt_num(side_cover)
                    ),
                );
                return None;
            }
            for i in 0..count {
                let y = match station {
                    Some(st) => bx.y0 + st,
                    None if count == 1 => (bx.y0 + bx.y1) / 2.0,
                    None => spread(i, ya, yb),
                };
                out.push(V2::init(x, y));
            }
        }
        _ => {
            ctx.p.fail(
                "place/face",
                format!(
                    "place.face must be one of \"bottom\", \"top\", \"left\", \"right\", \"center\" (got \"{face}\")"
                ),
            );
            return None;
        }
    }
    Some(out)
}
