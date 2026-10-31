//! fill (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, one_prism, oriented_ccw, parse_point_list, vertex_anchors};
use crate::geom;
use crate::model::{self, Built, OutlineMode, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;

param_enum! { pub enum Material { Earth = "earth", Gravel = "gravel", Sand = "sand", CompactedFill = "compacted_fill" } }
param_enum! { pub enum Outline { Top = "top", Full = "full", None = "none" } }

params! {
    pub struct Params {
        material: Enum(Material) = Material::Earth => { desc: "earth | gravel | sand | compacted_fill" },
        points: OptRaw => { def: "required", desc: "polygon [x,y] or Refs" },
        outline: Enum(Outline) = Outline::Top => { desc: "top (stroke only edges with outward normal up: the grade line) | full | none" },
        grade_label: Str = "" => { def: "null", desc: "optional text for annotations" },
        except: OptRaw => { def: "null", desc: "[ids] of solids this fill is not clipped by (SPEC 26.4: a fill draws under every solid a view cuts with it, so a plain rectangle around a footing needs no notches)" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let fp = ctx.p.parse_all::<Params>();
    let Some(pv) = &fp.points else {
        ctx.p.fail("points", "fill needs 'points': a polygon [[x, y], ...] or Refs".into());
        return None;
    };
    if !ctx.p.ok {
        return None;
    }
    let pts = parse_point_list(ctx, "points", pv, true)?;
    let clean = drop_duplicate_points(&pts);
    if clean.len() < 3 {
        ctx.p.fail("points", format!("a fill polygon needs at least 3 distinct points (got {})", clean.len()));
        return None;
    }
    let anchors = vertex_anchors(&clean, true);
    let lp = oriented_ccw(clean);
    let outline = match fp.outline {
        Outline::Top => OutlineMode::Top,
        Outline::Full => OutlineMode::Full,
        Outline::None => OutlineMode::None,
    };
    let box_ = geom::loop_box(&lp);
    let prism = Prism { material: fp.material.tag().into(), loops: model::one_loop(lp), outline, ..Prism::default() };
    Some(Built {
        prisms: one_prism(prism),
        anchors,
        box_,
        points_mode: true,
        info: format!("fill {}", fp.material.tag()),
        ..Built::default()
    })
}
