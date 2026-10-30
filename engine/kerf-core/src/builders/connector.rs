//! connector (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, gauge_thickness, parse_point_list};
use crate::catalog::{HARDWARE, Hardware};
use crate::geom;
use crate::model::fmt_num;
use crate::model::{self, Built, Prism};
use crate::num;
use crate::params::{ParamEnum, param_enum, params};
use crate::pathgeom;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;

param_enum! { pub enum Lay { Edge = "edge", Face = "face" } }
param_enum! { pub enum Side { Left = "left", Right = "right" } }

params! {
    pub struct Params {
        model: Str = "" => { def: "null", desc: "e.g. MSTA36, H2.5A, HETA20, CS16, CS14: fills width/gauge from the hardware table" },
        points: OptRaw => { def: "required", desc: "polyline [x,y] or Refs of the bearing face (lay edge) or centerline (lay face)" },
        lay: Enum(Lay) = Lay::Edge => { desc: "edge: seen edge-on, gauge in-plane growing to `side`, width along Z; face: seen face-on, `width` in-plane centered on the polyline, gauge along Z" },
        side: Enum(Side) = Side::Left => { desc: "lay edge: left of the polyline direction (left of a left-to-right line = up) or right" },
        gauge: OptNum => { def: "18", desc: "12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359" },
        width: OptLenPos => { def: "1.25", desc: "extent along Z (lay edge) or in-plane (lay face)" },
        fasteners: Str = "" => { def: "null", desc: "text for notes, e.g. \"(10) 10d EA. END\"" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let cp = ctx.p.parse_all::<Params>();
    let model_name = cp.model.as_str();
    let hw: Option<&Hardware> =
        if model_name.is_empty() { None } else { HARDWARE.iter().find(|h| h.model.eq_ignore_ascii_case(model_name)) };
    let gauge_default = hw.map_or(18.0, |h| f64::from(h.gauge));
    let gauge_n = cp.gauge.unwrap_or(gauge_default);
    let width = cp.width.unwrap_or(hw.map_or(1.25, |h| h.width));
    let Some(pv) = cp.points.as_ref() else {
        ctx.p.fail("points", "connector needs 'points': the strap polyline, e.g. [\"upper_plate@top_right\", \"beam@top_left\"] (Refs or [x, y])".to_string());
        return None;
    };
    if !ctx.p.ok {
        return None;
    }
    let gauge_i = num::to_int::<u32>(crate::math::round(gauge_n)).unwrap_or(0);
    let Some(thickness) = gauge_thickness(gauge_i) else {
        ctx.p.fail(
            "gauge",
            format!("gauge {} is not in the table; use 10, 11, 12, 14, 16, 18, 20, 22, 24, 26 or 28", fmt_num(gauge_n)),
        );
        return None;
    };
    let pts = parse_point_list(ctx, "points", pv, true)?;
    let clean = drop_duplicate_points(&pts);
    if clean.len() < 2 {
        ctx.p.fail("points", format!("a connector needs at least 2 distinct points (got {})", clean.len()));
        return None;
    }
    let edge = cp.lay == Lay::Edge;
    let rib = if edge {
        pathgeom::ribbon(
            &clean,
            if cp.side == Side::Left { thickness } else { 0.0 },
            if cp.side == Side::Right { thickness } else { 0.0 },
        )
    } else {
        pathgeom::ribbon(&clean, width / 2.0, width / 2.0)
    };
    let box_ = geom::loop_box(&rib);
    let prism = Prism {
        material: "steel".to_string(),
        loops: model::one_loop(rib),
        centerline: clean,
        face_tie: !edge,
        ..Prism::default()
    };
    let kind_text = hw.map_or_else(alloc::string::String::new, |h| format!("{} ", h.kind));
    Some(Built {
        prisms: vec![prism],
        box_,
        nat_z: Some(if edge { width } else { thickness }),
        points_mode: true,
        info: format!(
            "connector {}{}{}{} ga x {} lay {}",
            model_name,
            if model_name.is_empty() { "" } else { " " },
            kind_text,
            gauge_i,
            fmt_ft_in(width),
            cp.lay.tag()
        ),
        ..Built::default()
    })
}
