//! panel (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, ftin, length_or_until, material_ok, one_prism, quad_of};
use crate::geom::Box;
use crate::model::{self, Built, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::String;

param_enum! { pub enum Material { Osb = "osb", Plywood = "plywood", Gypsum = "gypsum", FiberCement = "fiber_cement", WoodBoard = "wood_board" } }
param_enum! { pub enum Run { X = "x", Y = "y" } }

params! {
    pub struct Params {
        material: Enum(Material) = Material::Osb => { desc: "osb | plywood | gypsum | fiber_cement | wood_board" },
        thickness: LenPos => { hint: "e.g. 0.4375 for 7/16\" OSB", desc: "e.g. 0.4375 (7/16\"), 0.46875 (15/32), 0.5, 0.625, 0.75" },
        length: OptRaw => { def: "required", desc: "in-plane extent; alternative: `until` (same rule as lumber)" },
        until: OptRaw => { desc: "alternative to length: a Ref; the panel grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate. On a sloped panel (`slope`, e.g. \"@truss\") the run axis is the rotated one: the end is cut square where the Ref projects onto the slope. Roof sheathing: \"slope\": \"@truss\", \"until\": \"truss@top_chord_end\" (no literal length or pitch to keep in sync)" },
        run: Enum(Run) = Run::X => { desc: "in-plane direction of length before rotation: x (length x thickness) or y (thickness x length)" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let pp = ctx.p.parse_all::<Params>();
    let run = pp.run.tag();
    let mut until_note = String::new();
    let length = length_or_until(ctx, run, "the in-plane extent of the panel: \"length\" or \"until\": \"other@anchor\"", &mut until_note);
    if !ctx.p.ok {
        return None;
    }
    let length = length?;
    let material = pp.material.tag();
    if !material_ok(ctx, "material", material) {
        return None;
    }
    let along_x = pp.run == Run::X;
    let w = if along_x { length } else { pp.thickness };
    let h = if along_x { pp.thickness } else { length };
    let prism = Prism {
        material: material.into(),
        loops: model::one_loop(model::rect_loop(0.0, 0.0, w, h)),
        quads: alloc::vec![quad_of(0.0, 0.0, w, h)],
        ..Prism::default()
    };
    Some(Built {
        prisms: one_prism(prism),
        box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: h },
        info: format!("panel {material} {} x {} run {run}{until_note}", ftin(pp.thickness), ftin(length)),
        ..Built::default()
    })
}
