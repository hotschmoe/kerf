//! insulation (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, oriented_ccw, parse_point_list, vertex_anchors};
use crate::geom::{self, Pt};
use crate::math;
use crate::model::{self, Built, Kind, Prism};
use crate::num;
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::f64::consts::PI;

param_enum! { pub enum Form { Rigid = "rigid", Batt = "batt" } }

params! {
    pub struct Params {
        form: Enum(Form) = Form::Rigid => { desc: "rigid | batt" },
        width: OptLenPos => { hint: "give width and height, or points", also: &["height"], def: "rect: required unless points", desc: "box size" },
        height: OptLenPos => { hint: "give width and height, or points", row: false, desc: "box size" },
        points: OptRaw => { desc: "polygon alternative to width/height" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let ip = ctx.p.parse::<Params>()?;
    let lp: Vec<Pt>;
    let mut points_mode = false;
    let mut anchors = Vec::new();
    if let Some(pv) = ip.points.as_ref() {
        let pts = parse_point_list(ctx, "points", pv, true)?;
        let clean = drop_duplicate_points(&pts);
        if clean.len() < 3 {
            ctx.p.fail("points", "insulation polygon needs at least 3 points".to_string());
            return None;
        }
        anchors = vertex_anchors(&clean, true);
        lp = oriented_ccw(clean);
        points_mode = true;
    } else {
        let hint = "give width and height, or points";
        let Some(w) = ip.width else {
            ctx.p.missing("width", hint);
            return None;
        };
        let Some(h) = ip.height else {
            ctx.p.missing("height", hint);
            return None;
        };
        lp = model::rect_loop(0.0, 0.0, w, h);
    }
    let batt = ip.form == Form::Batt;
    let bx = geom::loop_box(&lp);
    let mut prism = Prism {
        material: if batt { "insulation_batt" } else { "insulation_rigid" }.to_string(),
        loops: model::one_loop(lp),
        ..Prism::default()
    };
    if batt {
        prism.kind = Kind::Batt;
        prism.line_pts = batt_symbol(bx);
    }
    Some(Built {
        prisms: vec![prism],
        anchors,
        box_: bx,
        points_mode,
        info: format!("insulation {}", ip.form.tag()),
        ..Built::default()
    })
}

fn batt_symbol(bx: geom::Box) -> Vec<Pt> {
    let horizontal = bx.width() >= bx.height();
    let long = if horizontal { bx.width() } else { bx.height() };
    let short = if horizontal { bx.height() } else { bx.width() };
    let loops = math::round(long / (short * 0.9)).clamp(2.0, 2000.0);
    let pad = short * 0.2;
    let pitch = (long - 2.0 * pad) / loops;
    let loop_w = 1.7 * pitch / (2.0 * PI);
    let amp = 0.42 * short;
    const STEPS_PER: usize = 20;
    let total = num::to_int_clamped::<usize>(loops, 2, 2000) * STEPS_PER;
    let mut out = Vec::with_capacity(total + 1);
    for i in 0..=total {
        let t = 2.0 * PI * loops * i as f64 / total as f64;
        let u = pad + (long - 2.0 * pad) * i as f64 / total as f64 - loop_w * math::sin(t);
        let v = amp * math::cos(t);
        if horizontal {
            out.push(Pt::new(bx.x0 + u, (bx.y0 + bx.y1) / 2.0 + v));
        } else {
            out.push(Pt::new((bx.x0 + bx.x1) / 2.0 + v, bx.y0 + u));
        }
    }
    out
}
