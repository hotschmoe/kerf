//! solid (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, material_ok, one_prism, oriented_ccw, parse_point_list};
use crate::geom;
use crate::model::{self, Built, Prism};
use crate::params::params;
use crate::units;
use alloc::format;

params! {
    pub struct Params {
        profile: OptRaw => { def: "required", desc: "{rect:[w,h]} | {circle:d} | {points:[...]}" },
        material: Str => { desc: "any style material (aluminum, steel, ...)" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let sp = ctx.p.parse_all::<Params>();
    let Some(prof) = &sp.profile else {
        ctx.p.fail(
            "profile",
            "solid needs 'profile': {\"rect\": [w, h]}, {\"circle\": d} or {\"points\": [[x, y], ...]}".into(),
        );
        return None;
    };
    if !ctx.p.ok {
        return None;
    }
    if !material_ok(ctx, "material", &sp.material) {
        return None;
    }
    let lp;
    let mut points_mode = false;
    if let Some(rv) = prof.get("rect") {
        let Some([wv, hv]) = rv.arr().and_then(|a| <&[_; 2]>::try_from(a).ok()).map(|a| [&a[0], &a[1]]) else {
            ctx.p.fail("profile/rect", "profile.rect must be [width, height]".into());
            return None;
        };
        let (w, h) = (units::parse_length(wv), units::parse_length(hv));
        let (Some(w), Some(h)) = (w, h) else {
            ctx.p.fail("profile/rect", "profile.rect must be two positive lengths [width, height]".into());
            return None;
        };
        if w <= 0.0 || h <= 0.0 {
            ctx.p.fail("profile/rect", "profile.rect must be two positive lengths [width, height]".into());
            return None;
        }
        lp = model::rect_loop(0.0, 0.0, w, h);
    } else if let Some(cv) = prof.get("circle") {
        let dd = units::parse_length(cv).filter(|d| *d > 0.0);
        let Some(dd) = dd else {
            ctx.p.fail("profile/circle", "profile.circle must be a positive diameter".into());
            return None;
        };
        lp = model::circle_loop(dd / 2.0, dd / 2.0, dd / 2.0);
    } else if let Some(pv) = prof.get("points") {
        let pts = parse_point_list(ctx, "profile/points", pv, true)?;
        let clean = drop_duplicate_points(&pts);
        if clean.len() < 3 {
            ctx.p.fail("profile/points", "profile.points needs at least 3 points".into());
            return None;
        }
        lp = oriented_ccw(clean);
        points_mode = true;
    } else {
        ctx.p.fail("profile", "profile must have one of 'rect', 'circle' or 'points'".into());
        return None;
    }
    let box_ = geom::loop_box(&lp);
    let prism = Prism { material: sp.material.clone(), loops: model::one_loop(lp), ..Prism::default() };
    Some(Built {
        prisms: one_prism(prism),
        box_,
        points_mode,
        info: format!("solid {} (escape hatch)", sp.material),
        ..Built::default()
    })
}
