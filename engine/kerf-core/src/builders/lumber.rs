//! lumber (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{
    Ctx, fmt_ft_in, length_or_until, material_ok, material_or, one_prism, parse_actual, parse_sawn, quad_of,
};
use crate::geom::{Box, V2, Xf};
use crate::model::{self, Built, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

param_enum! { pub enum Product { Sawn = "sawn", Lvl = "lvl", Psl = "psl", Lsl = "lsl", Glulam = "glulam" } }
param_enum! { pub enum Run { Z = "z", X = "x", Y = "y" } }
param_enum! { pub enum Orient { Upright = "upright", Flat = "flat" } }
param_enum! { pub enum Face { Wide = "wide", Narrow = "narrow" } }
param_enum! { pub enum Barrier { SillSeal = "sill_seal", Membrane = "membrane" } }

params! {
    pub struct Params {
        size: Str => { desc: "sawn nominal \"TxD\": T 1, 2, 3, 4 with D 3..14 (\"1x4\", \"2x10\", \"3x8\", \"4x12\"), T 6, 8 with D 6..12 (\"6x6\", \"8x10\"); or actual \"1.75x11.875\" (thickness x depth) for lvl/psl/lsl/glulam" },
        product: Enum(Product) = Product::Sawn => { desc: "sawn | lvl | psl | lsl | glulam" },
        run: Enum(Run) = Run::Z => { desc: "axis the length runs along: z (seen end-on in section), x or y" },
        orient: Enum(Orient) = Orient::Upright => { desc: "run z only: upright (depth vertical) or flat (depth horizontal)" },
        face: Enum(Face) = Face::Wide => { desc: "run x/y only: face seen by the viewer: wide (depth in-plane) or narrow (thickness in-plane)" },
        length: OptRaw => { def: "required for run x/y", desc: "member length (inches or ft-in string); alternative: `until`. Run z: optional, the member is then that long along z, centered on `z` when it is a number, else on the first section view's cut_z (default: spans the doc run; `\"z\": [z0, z1]` sets both ends)" },
        until: OptRaw => { desc: "run x/y alternative to length: a Ref (or {ref, offset}); the member grows from its placement anchor along its run axis until its far end reaches the Ref's coordinate on that axis (anchor *_left grows right, *_right left, top_* down, bottom_* up; center anchors are an error; length and until together are E_PARAM). Example jack stud: \"at\": {\"anchor\": \"top_left\", \"to\": \"beam@bottom_left\"}, \"until\": \"bottom_plate@top_left\"" },
        plies: Int(u8, 1, 8) = 1 => { desc: "built-up members; plies stack along X for run z, along Z otherwise; draws ply lines" },
        treated: Bool = false => { desc: "preservative treated (material wood_treated; W_UNTREATED_CONTACT checks)" },
        blocking: Bool = false => { desc: "discontinuous member: section mark is one diagonal instead of an X" },
        grade: Str = "" => { def: "null", desc: "free text e.g. \"#2 DF-L\" for notes" },
        barrier: OptEnum(Barrier) => { desc: "sill_seal | membrane: draws a 1/8\" sealer strip under the member (part `barrier`, material sill_seal) and the member sits on top of it, so bottom_* anchors are the strip underside and the member top is 1/8\" higher than without. Clears W_UNTREATED_CONTACT for untreated wood on concrete/CMU (put it on the wood member that bears on the masonry; add a note, e.g. `SILL SEALER`)" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let lp = ctx.p.parse::<Params>()?;
    let size = lp.size.as_str();
    let run = lp.run.tag();
    let treated = lp.treated;
    let is_sawn = lp.product == Product::Sawn;
    let Some(sz) = parse_sawn(size).or_else(|| parse_actual(size)) else {
        ctx.p.fail("size", format!("size \"{size}\" is not recognised. Use sawn nominal \"TxD\" (1x, 2x, 3x, 4x: depth 3..14, e.g. \"2x10\", \"3x8\"; 6x, 8x: depth 6..12, e.g. \"6x8\"), or an actual \"thickness x depth\" such as \"1.75x9.25\""));
        return None;
    };
    let plies = usize::from(lp.plies);
    let n_plies = f64::from(lp.plies);
    let mut length = 0.0;
    let mut until_note = String::new();
    if lp.run == Run::Z && ctx.p.has("until") {
        ctx.p.fail(
            "until",
            "'until' only applies to lumber with run x or y (run z spans the document run; set 'z' instead)".into(),
        );
        return None;
    }
    let mut z_length = None;
    if lp.run == Run::Z && ctx.p.has("length") {
        z_length = Some(ctx.p.len_pos("length", None, "length of a run z member along z, e.g. 24")?);
    }
    if lp.run != Run::Z {
        length = length_or_until(
            ctx,
            run,
            "lumber with run x or y needs its length, e.g. \"length\": 92.625, or \"until\": \"other@anchor\"",
            &mut until_note,
        )?;
    }
    if !ctx.p.ok {
        return None;
    }

    let t = sz.t;
    let d = sz.d;
    let w;
    let h;
    let mut nat = z_length;
    let mut quads: Vec<[V2; 4]> = Vec::new();
    let mut ply_lines: Vec<[V2; 2]> = Vec::new();
    let wide = lp.face == Face::Wide;
    if lp.run == Run::Z {
        if lp.orient != Orient::Flat {
            w = t * n_plies;
            h = d;
            for k in 0..plies {
                let x0 = k as f64 * t;
                quads.push(quad_of(x0, 0.0, x0 + t, h));
                if k > 0 {
                    ply_lines.push([V2::init(x0, 0.0), V2::init(x0, h)]);
                }
            }
        } else {
            w = d;
            h = t * n_plies;
            for k in 0..plies {
                let y0 = k as f64 * t;
                quads.push(quad_of(0.0, y0, w, y0 + t));
                if k > 0 {
                    ply_lines.push([V2::init(0.0, y0), V2::init(w, y0)]);
                }
            }
        }
    } else {
        let along_x = lp.run == Run::X;
        let in_plane = if wide { d } else { t * n_plies };
        let out_of_plane = if wide { t * n_plies } else { d };
        nat = Some(out_of_plane);
        if along_x {
            w = length;
            h = in_plane;
        } else {
            w = in_plane;
            h = length;
        }
        if !wide && plies > 1 {
            for k in 1..plies {
                let o = k as f64 * t;
                if along_x {
                    ply_lines.push([V2::init(0.0, o), V2::init(w, o)]);
                } else {
                    ply_lines.push([V2::init(o, 0.0), V2::init(o, h)]);
                }
            }
        }
    }
    let material = if !is_sawn {
        "wood_engineered"
    } else if treated {
        "wood_treated"
    } else {
        "wood"
    };
    if !material_ok(ctx, "material", material) {
        return None;
    }
    let prism = Prism {
        material: material.into(),
        loops: model::one_loop(model::rect_loop(0.0, 0.0, w, h)),
        quads: if lp.run == Run::Z { quads } else { Vec::new() },
        blocking: lp.blocking,
        ply_lines,
        ..Prism::default()
    };
    let mut info = String::from("lumber ");
    if plies > 1 {
        info.push_str(&format!("({plies}) "));
    }
    let pt = if treated { " PT" } else { "" };
    if lp.run == Run::Z {
        info.push_str(&format!("{size}{pt} {} run z", lp.orient.tag()));
        if let Some(l) = z_length {
            info.push_str(&format!(" L={}", fmt_ft_in(l)));
        }
    } else {
        info.push_str(&format!("{size} {run}{pt} {} run {run} L={}{until_note}", lp.face.tag(), fmt_ft_in(length)));
    }
    if let Some(bk) = lp.barrier {
        let seal_t = 0.125;
        let raised = prism.transform(&Xf::translate(0.0, seal_t));
        let strip = Prism {
            part: "barrier".into(),
            material: material_or(ctx, "sill_seal", "generic").into(),
            loops: model::one_loop(model::rect_loop(0.0, 0.0, w, seal_t)),
            ..Prism::default()
        };
        info.push_str(&format!(" + {}", bk.tag()));
        return Some(Built {
            prisms: alloc::vec![strip, raised],
            box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: h + seal_t },
            nat_z: nat,
            info,
            ..Built::default()
        });
    }
    Some(Built {
        prisms: one_prism(prism),
        box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: h },
        nat_z: nat,
        info,
        ..Built::default()
    })
}
