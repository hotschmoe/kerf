//! panel (SPEC 5, 28.6): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, fmt_ft_in, length_or_until, material_ok, one_prism, quad_of};
use crate::geom::Box;
use crate::model::{self, Built, Prism};
use crate::params::{ParamEnum, param_enum, params};
use crate::scene::State;
use alloc::format;
use alloc::string::String;

param_enum! { pub enum Material { Osb = "osb", Plywood = "plywood", Gypsum = "gypsum", FiberCement = "fiber_cement", WoodBoard = "wood_board" } }
param_enum! { pub enum Run { X = "x", Y = "y" } }
param_enum! { pub enum Lay { Edge = "edge", Face = "face" } }

params! {
    pub struct Params {
        material: Enum(Material) = Material::Osb => { desc: "osb | plywood | gypsum | fiber_cement | wood_board" },
        thickness: LenPos => { hint: "e.g. 0.4375 for 7/16\" OSB", desc: "e.g. 0.4375 (7/16\"), 0.46875 (15/32), 0.5, 0.625, 0.75" },
        length: OptRaw => { def: "required", desc: "in-plane extent; alternative: `until` (same rule as lumber)" },
        until: OptRaw => { desc: "alternative to length: a Ref; the panel grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate. On a sloped panel (`slope`, e.g. \"@truss\") the run axis is the rotated one: the end is cut square where the Ref projects onto the slope. Roof sheathing: \"slope\": \"@truss\", \"until\": \"truss@top_chord_end\" (no literal length or pitch to keep in sync)" },
        run: Enum(Run) = Run::X => { desc: "in-plane direction of length before rotation: x (length x thickness) or y (thickness x length); lay face: length along x (run x) or y (run y), height across it" },
        lay: Enum(Lay) = Lay::Edge => { desc: "edge: seen edge-on in sections (thickness in-plane, spans z); face: the sheet seen face-on in elevations and iso (wall sheathing, shear walls, portal frames): length x height in-plane, thickness along z (a thin strip in plans)" },
        height: OptLenPos => { def: "required for lay face", desc: "lay face: the sheet's extent across `run` (a wall sheet's height, e.g. 96)" },
        cuts: OptRaw => { desc: "lay edge: end cuts as lumber (`kerf schema cuts`): {type: trim, end: start|end, to: id} cuts the end to the face of `to` (sloped roof sheathing that butts a parapet or a girder face: a plumb end, no overlap), square|plumb|level|bevel; anchors start_top, start_bottom, end_top, end_bottom" },
        on: OptStr => { desc: "lay face: the member whose z face the sheet lies on: \"stud.face_front\" (default face) or \"stud.face_back\"; sets z (instead of `z`)" },
    }
}

/// The z centre of a face-on sheet of thickness `t` laid on `on` ("id", "id#k", "id.face_front", "id.face_back").
fn on_face(ctx: &mut Ctx, on: &str, t: f64) -> Option<f64> {
    let (name, face) = match on.split_once('.') {
        Some((n, f)) => (n, f),
        None => (on, "face_front"),
    };
    if face != "face_front" && face != "face_back" {
        ctx.p.fail(
            "on",
            format!("'on': a face-on sheet lies on a z face: \"{name}.face_front\" or \"{name}.face_back\" (got \".{face}\")"),
        );
        return None;
    }
    let (id, inst) = match name.split_once('#') {
        Some((a, b)) => match b.parse::<u32>() {
            Ok(k) => (a, Some(k)),
            Err(_) => {
                ctx.p.fail(
                    "on",
                    format!("'on': \"{name}\" is not a member name; use \"<id>\" or \"<id>#<k>\" for array instance k"),
                );
                return None;
            }
        },
        None => (name, None),
    };
    let scene = ctx.scene;
    let Some(c) = scene.find(id) else {
        let ids = scene.comp_ids();
        let hint = model::nearest(id, &ids).map_or(String::new(), |n| format!(" Did you mean '{n}'?"));
        ctx.p.fail_code("E_REF_UNKNOWN", "on", format!("'on': no component '{id}'.{hint}"));
        return None;
    };
    if c.id == ctx.p.id {
        ctx.p.fail("on", "'on' names the panel itself; name the stud, post or framing the sheet is nailed to".into());
        return None;
    }
    if c.state != State::Ok {
        ctx.p.fail_code("E_REF_UNKNOWN", "on", format!("'on': component '{id}' did not build (see its own errors)"));
        return None;
    }
    let k = inst.unwrap_or(0);
    let (mut z0, mut z1) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in c.world.iter().filter(|p| p.instance == k && p.kind == model::Kind::Body) {
        z0 = z0.min(p.z0);
        z1 = z1.max(p.z1);
    }
    if z0 >= z1 {
        ctx.p.fail("on", format!("'on': '{name}' has no solid body to lay the sheet on"));
        return None;
    }
    Some(if face == "face_front" { z1 + t / 2.0 } else { z0 - t / 2.0 })
}

/// No sheet is thicker: an edge-on panel past this is a sheet faked face-on with `thickness` = its height (SPEC 28.6).
const FACE_HINT: f64 = 1.5;

/// W_PARAM for an edge-on panel thicker than any sheet, with the op that makes it a face-on sheet of the same size.
fn face_hint(ctx: &mut Ctx, pp: &Params, length: f64) {
    let id = ctx.p.id;
    let z = ctx.p.raw("z").and_then(crate::json::Value::arr).and_then(|a| {
        let z0 = crate::units::parse_length(a.first()?)?;
        let z1 = crate::units::parse_length(a.get(1)?)?;
        Some(z1 - z0)
    });
    let sheet = z.filter(|t| *t > 0.0 && *t <= FACE_HINT).unwrap_or(0.4375);
    let run = if pp.run == Run::X { "" } else { ",\"run\":\"y\"" };
    ctx.p.diags.add_fix(
        model::Level::Warning,
        "W_PARAM",
        Some(id),
        Some(&format!("components/{id}")),
        format!(
            "panel '{id}' is {} thick and {} long: no sheet is that thick. A sheet seen face-on (shear wall, portal frame, wall sheathing in elevation or iso) is \"lay\": \"face\" with its height in \"height\"",
            fmt_ft_in(pp.thickness),
            fmt_ft_in(length)
        ),
        format!(
            "{{\"op\":\"update\",\"path\":\"components/{id}\",\"value\":{{\"lay\":\"face\",\"height\":{},\"thickness\":{}{run}}}}} (z stays; or \"on\": \"<stud>.face_front\" instead of z)",
            super::common::fmt_num(pp.thickness),
            super::common::fmt_num(sheet)
        ),
    );
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let pp = ctx.p.parse_all::<Params>();
    let run = pp.run.tag();
    let face = pp.lay == Lay::Face;
    if !face {
        for key in ["height", "on"] {
            if ctx.p.has(key) {
                ctx.p.fail(key, format!("'{key}' is for a face-on sheet: add \"lay\": \"face\" (length x height in the drawing plane, thickness along z), or remove '{key}'"));
            }
        }
    } else {
        if pp.height.is_none() {
            ctx.p.fail("height", "a face-on panel (lay face) needs \"height\": its extent across run (a wall sheet: its height, e.g. 96)".into());
        }
        if pp.on.is_some() && ctx.p.has("z") {
            ctx.p.fail("on", "give either 'on' (the sheet lies on that member's z face) or 'z', not both".into());
        }
    }
    let mut until_note = String::new();
    let length = length_or_until(
        ctx,
        run,
        "the in-plane extent of the panel: \"length\" or \"until\": \"other@anchor\"",
        &mut until_note,
    );
    if !ctx.p.ok {
        return None;
    }
    let length = length?;
    let material = pp.material.tag();
    if !material_ok(ctx, "material", material) {
        return None;
    }
    if !face && pp.thickness > FACE_HINT {
        face_hint(ctx, &pp, length);
    }
    let along_x = pp.run == Run::X;
    let across = if face { pp.height.unwrap_or(0.0) } else { pp.thickness };
    let w = if along_x { length } else { across };
    let h = if along_x { across } else { length };
    let zc = match pp.on.as_deref() {
        Some(on) if face => Some(on_face(ctx, on, pp.thickness)?),
        _ => None,
    };
    let prism = Prism {
        material: material.into(),
        loops: model::one_loop(model::rect_loop(0.0, 0.0, w, h)),
        quads: alloc::vec![quad_of(0.0, 0.0, w, h)],
        ..Prism::default()
    };
    let info = if face {
        let on = pp.on.as_deref().map_or(String::new(), |o| format!(" on {o}"));
        format!(
            "panel {material} {} x {} x {} lay face run {run}{on}{until_note}",
            fmt_ft_in(pp.thickness),
            fmt_ft_in(length),
            fmt_ft_in(across)
        )
    } else {
        format!("panel {material} {} x {} run {run}{until_note}", fmt_ft_in(pp.thickness), fmt_ft_in(length))
    };
    Some(Built {
        prisms: one_prism(prism),
        box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: h },
        nat_z: face.then_some(pp.thickness),
        nat_zc: zc,
        info,
        ..Built::default()
    })
}
