//! sleeve (SPEC 26.4): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, fmt_ft_in, material_ok};
use crate::geom::{self, Box, Pt};
use crate::model::{self, Built, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Run { Z = "z", X = "x", Y = "y" } }

params! {
    pub struct Params {
        pipe: OptLenPos => { def: "null", desc: "outside diameter of the pipe, conduit or duct through the sleeve; null = an empty sleeve (then give od)" },
        od: OptLenPos => { def: "pipe + 2 clearance + 2 wall", desc: "sleeve outside diameter" },
        clearance: Len = 0.5 => { desc: "gap between pipe and sleeve all around (used when od is not given)" },
        wall: LenPos = 0.25 => { desc: "sleeve wall thickness" },
        pipe_wall: LenPos = 0.25 => { desc: "pipe wall thickness" },
        fill: Str = "" => { def: "null", desc: "material packed between pipe and sleeve (joint_filler, sealant, grout); null = open" },
        material: Str = "steel" => { desc: "sleeve material" },
        pipe_material: Str = "steel" => { desc: "pipe material" },
        run: Enum(Run) = Run::Z => { desc: "axis of the pipe: z (seen end-on: rings) or x / y (seen lengthwise, cut along its axis)" },
        length: OptLenPos => { def: "run x/y: required", desc: "sleeve length along its axis, e.g. the wall thickness. Run z: optional, centered on the first section view's cut_z (default: spans the doc run)" },
        through: OptRaw => { def: "null", desc: "id or [ids] of the concrete / cmu_wall hosts the sleeve passes through: each loses the sleeve's outside (od) in section, iso and 3D, as if the host listed the sleeve in its `voids`" },
    }
}

fn ring(c: f64, r_out: f64, r_in: f64) -> Vec<Vec<Pt>> {
    let mut loops = vec![model::circle_loop(c, c, r_out)];
    if r_in > 0.0 {
        loops.push(geom::reverse_loop(&model::circle_loop(c, c, r_in)));
    }
    loops
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let sp = ctx.p.parse::<Params>()?;
    if sp.clearance < 0.0 {
        ctx.p.fail("clearance", format!("clearance must be 0 or more (got {})", model::num_text(sp.clearance)));
        return None;
    }
    let od = match (sp.od, sp.pipe) {
        (Some(od), _) => od,
        (None, Some(pipe)) => pipe + 2.0 * (sp.clearance + sp.wall),
        (None, None) => {
            ctx.p.fail("pipe", "a sleeve needs 'pipe' (the pipe outside diameter; the sleeve is then pipe + 2 clearance + 2 wall) or 'od' (the sleeve outside diameter)".into());
            return None;
        }
    };
    let r = od / 2.0;
    if sp.wall >= r {
        ctx.p.fail(
            "wall",
            format!("wall {} must be less than half the sleeve od {}", model::num_text(sp.wall), model::num_text(od)),
        );
        return None;
    }
    let ri = r - sp.wall;
    if let Some(pipe) = sp.pipe {
        if pipe > 2.0 * ri + 1e-9 {
            ctx.p.fail(
                "pipe",
                format!(
                    "pipe {} does not fit in the sleeve: its inside diameter is {} (od {} - 2 x wall {})",
                    model::num_text(pipe),
                    model::num_text(2.0 * ri),
                    model::num_text(od),
                    model::num_text(sp.wall)
                ),
            );
            return None;
        }
        if sp.pipe_wall >= pipe / 2.0 {
            ctx.p.fail(
                "pipe_wall",
                format!(
                    "pipe_wall {} must be less than half the pipe {}",
                    model::num_text(sp.pipe_wall),
                    model::num_text(pipe)
                ),
            );
            return None;
        }
    }
    if !material_ok(ctx, "material", &sp.material) || !material_ok(ctx, "pipe_material", &sp.pipe_material) {
        return None;
    }
    if !sp.fill.is_empty() && !material_ok(ctx, "fill", &sp.fill) {
        return None;
    }
    let rp = sp.pipe.map_or(0.0, |p| p / 2.0);
    let along = sp.run != Run::Z;
    if along && sp.length.is_none() {
        ctx.p.missing("length", "a sleeve along x or y needs its length, e.g. the wall thickness: \"length\": 8");
        return None;
    }
    let sleeve = |part: &str, material: &str, loops: Vec<Vec<Pt>>| Prism {
        part: part.into(),
        material: material.into(),
        loops,
        ..Prism::default()
    };
    let len = sp.length.unwrap_or(0.0);
    let box_ = match sp.run {
        Run::Z => Box { x0: 0.0, y0: 0.0, x1: od, y1: od },
        Run::X => Box { x0: 0.0, y0: 0.0, x1: len, y1: od },
        Run::Y => Box { x0: 0.0, y0: 0.0, x1: od, y1: len },
    };
    let mut prisms = Vec::new();
    if !along {
        prisms.push(sleeve("sleeve", &sp.material, ring(r, r, ri)));
        if !sp.fill.is_empty() && rp < ri - 1e-9 {
            prisms.push(sleeve("fill", &sp.fill, ring(r, ri, rp)));
        }
        if let Some(pipe) = sp.pipe {
            prisms.push(sleeve("pipe", &sp.pipe_material, ring(r, pipe / 2.0, pipe / 2.0 - sp.pipe_wall)));
        }
    } else {
        let rect = |c0: f64, c1: f64| {
            if sp.run == Run::X {
                model::rect_loop(0.0, r + c0, len, r + c1)
            } else {
                model::rect_loop(r + c0, 0.0, r + c1, len)
            }
        };
        let slab = |part: &str, material: &str, c0: f64, c1: f64, zhalf: f64, zoff: f64| Prism {
            zhalf,
            zoff,
            ..sleeve(part, material, model::one_loop(rect(c0, c1)))
        };
        let tube = |out: &mut Vec<Prism>, part: &str, material: &str, ro: f64, t: f64| {
            out.push(slab(part, material, ro - t, ro, ro, 0.0));
            out.push(slab(part, material, -ro, -ro + t, ro, 0.0));
            out.push(slab(part, material, -ro, ro, t / 2.0, ro - t / 2.0));
            out.push(slab(part, material, -ro, ro, t / 2.0, -(ro - t / 2.0)));
        };
        tube(&mut prisms, "sleeve", &sp.material, r, sp.wall);
        if !sp.fill.is_empty() && rp < ri - 1e-9 {
            prisms.push(slab("fill", &sp.fill, rp, ri, ri, 0.0));
            prisms.push(slab("fill", &sp.fill, -ri, -rp, ri, 0.0));
        }
        if let Some(pipe) = sp.pipe {
            tube(&mut prisms, "pipe", &sp.pipe_material, pipe / 2.0, sp.pipe_wall);
        }
    }
    let mut info = format!("sleeve {} OD x {} wall", fmt_ft_in(od), fmt_ft_in(sp.wall));
    if let Some(p) = sp.pipe {
        info.push_str(&format!(", pipe {}", fmt_ft_in(p)));
    }
    if !sp.fill.is_empty() {
        info.push_str(&format!(", {} fill", sp.fill));
    }
    info.push_str(&format!(" run {}", sp.run.tag()));
    if let Some(l) = sp.length {
        info.push_str(&format!(" L={}", fmt_ft_in(l)));
    }
    let through: String = match ctx.p.raw("through") {
        Some(crate::json::Value::String(s)) => format!(" through {s}"),
        Some(crate::json::Value::Array(a)) => {
            let ids: Vec<&str> = a.iter().filter_map(crate::json::Value::str).collect();
            if ids.is_empty() { String::new() } else { format!(" through {}", ids.join(", ")) }
        }
        _ => String::new(),
    };
    info.push_str(&through);
    Some(Built { prisms, box_, nat_z: if along { Some(od) } else { sp.length }, info, ..Built::default() })
}
