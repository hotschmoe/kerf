//! membrane (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, parse_point_list, path_len, vs_of};
use crate::geom::{self, V2};
use crate::json::Value;
use crate::math;
use crate::model::fmt_num;
use crate::model::{self, Built, Kind, Prism};
use crate::params::{ParamEnum, param_enum, params};
use crate::pathgeom;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;

fn membrane_thickness(material: &str) -> f64 {
    match material {
        "vapor_retarder" => 0.04,
        "shingles" => 0.25,
        "underlayment" => 0.06,
        "wrb" => 0.04,
        "flashing_membrane" => 0.06,
        _ => 0.05,
    }
}

param_enum! { pub enum Material { Membrane = "membrane", Underlayment = "underlayment", VaporRetarder = "vapor_retarder", Wrb = "wrb", Shingles = "shingles", FlashingMembrane = "flashing_membrane" } }
param_enum! { pub enum Side { Left = "left", Right = "right" } }

params! {
    pub struct Params {
        material: Enum(Material) = Material::Membrane => { desc: "underlayment | vapor_retarder | wrb | shingles | flashing_membrane | membrane" },
        points: OptRaw => { def: "required", desc: "polyline [x,y] or Refs" },
        thickness: OptLenPos => { def: "per material", desc: "draw thickness (vapor retarder 0.04, shingles 0.25 typical)" },
        side: Enum(Side) = Side::Left => { desc: "which side of the polyline direction the thickness grows: left of dx,dy is (-dy,dx), right the opposite" },
        until: OptRaw => { desc: "a Ref (or {ref, offset}): the LAST segment grows or shrinks along its own direction until its end reaches the Ref's coordinate along that direction. With `slope`: \"@truss\" and points [[0,0],[12,0]] a roofing layer follows the roof and stops at e.g. \"truss@top_chord_end\"" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let mp = ctx.p.parse_all::<Params>();
    let material = mp.material.tag();
    let Some(pv) = mp.points.as_ref() else {
        ctx.p.fail("points", "membrane needs 'points': the layer polyline, e.g. [\"roof_sheathing@top_left\", \"roof_sheathing@top_right\"]".to_string());
        return None;
    };
    if !ctx.p.ok {
        return None;
    }
    let pts = parse_point_list(ctx, "points", pv, true)?;
    let mut clean = drop_duplicate_points(&pts);
    if clean.len() < 2 {
        ctx.p.fail("points", format!("a membrane needs at least 2 distinct points (got {})", clean.len()));
        return None;
    }
    let mut until_note = String::new();
    if let Some(uv) = mp.until.as_ref() {
        let path = format!("{}/{}/until", ctx.p.base, ctx.p.id);
        let Some(target) = ctx.scene.point(uv, ctx.p.id, &path) else {
            ctx.p.ok = false;
            return None;
        };
        let last = clean.len() - 1;
        let seg = V2::init(clean[last].x - clean[last - 1].x, clean[last].y - clean[last - 1].y);
        if seg.len() < 1e-9 {
            ctx.p.fail(
                "until",
                "'until' needs a last segment with a direction (the last two points coincide)".to_string(),
            );
            return None;
        }
        let u = seg.norm();
        let rel = target.sub(ctx.origin);
        let ca = math::cos(-ctx.angle);
        let sa = math::sin(-ctx.angle);
        let tl = V2::init(rel.x * ca - rel.y * sa, rel.x * sa + rel.y * ca);
        let new_len = (tl.x - clean[last - 1].x) * u.x + (tl.y - clean[last - 1].y) * u.y;
        if new_len <= 1e-6 {
            ctx.p.fail("until", format!("'until' target lies at or behind the start of the last segment along its direction (length would be {})", fmt_num(new_len)));
            return None;
        }
        clean[last].x = clean[last - 1].x + u.x * new_len;
        clean[last].y = clean[last - 1].y + u.y * new_len;
        let ref_txt = match uv {
            Value::String(t) => t.as_str(),
            Value::Object(_) => uv.get("ref").map_or("ref", |r| r.str().unwrap_or("ref")),
            _ => "ref",
        };
        until_note = format!(" (until {ref_txt})");
    }
    let left = mp.side == Side::Left;
    let t = mp.thickness.unwrap_or_else(|| membrane_thickness(material));
    let rib = pathgeom::ribbon(&clean, if left { t } else { 0.0 }, if left { 0.0 } else { t });
    let line = pathgeom::offset_open(&clean, if left { t / 2.0 } else { -t / 2.0 });
    let box_ = geom::loop_box(&rib);
    let info = format!("membrane {} {} thick L={}{}", material, fmt_ft_in(t), fmt_ft_in(path_len(&vs_of(&clean))), until_note);
    let prism = Prism {
        material: material.to_string(),
        loops: model::one_loop(rib),
        kind: Kind::Line,
        line_pts: line,
        ticks: material == "shingles",
        centerline: clean,
        ..Prism::default()
    };
    Some(Built { prisms: vec![prism], box_, points_mode: true, info, ..Built::default() })
}
