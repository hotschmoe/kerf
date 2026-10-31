//! ijoist (SPEC 27.1): prefabricated wood I-joist from a generic series table (depth x flange width), its parameters and
//! its builder. The cross-section is built in (u, v), u across the flanges, v up. Run z lays it out end-on: flanges cut
//! with the wood X, the OSB web between them. Run x lays it along x as slabs at their true z (flanges, web, stiffeners,
//! squash blocks), drawn as an elevation even where the section plane passes through it, so the side view shows the
//! flange lines and the web outline with no lamination, and iso, plan and 3D see the real I shape.

use super::common::{Ctx, fmt_ft_in, length_or_until, material_ok, quad_of};
use crate::geom::{Box, V2};
use crate::model::{self, Built, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Run { Z = "z", X = "x" } }
param_enum! { pub enum Ends { None = "none", Left = "left", Right = "right", Both = "both" } }

params! {
    pub struct Params {
        size: Str => { desc: "depth x flange width from the series table, e.g. \"11-7/8 x 2-5/16\" (either order). Depths 9-1/2, 11-7/8, 14, 16; 18, 20, 22, 24 with 2-5/16 or 3-1/2 flanges. Flange widths 1-3/4, 2-1/16, 2-5/16, 2-1/2, 3-1/2 (3-1/2 from 11-7/8 deep). Brand series names are not sizes: give that series' depth and flange width" },
        run: Enum(Run) = Run::X => { desc: "x: seen from the side along its length (joists perpendicular to the wall in a wall section); z: cut end-on (joists parallel to the section)" },
        length: OptRaw => { def: "run x: required", desc: "member length (or `until`). Run z: optional, the joist is then that long along z, centered on `z` or the first section view's cut_z (default: spans the doc run)" },
        until: OptRaw => { desc: "run x alternative to length: a Ref; the joist grows from its placement anchor until its far end reaches the Ref's x (as lumber `until`)" },
        flange_depth: OptLenPos => { def: "table", desc: "flange thickness (vertical): 1-3/8, 1-1/2 for 2-1/2 flanges" },
        web: OptLenPos => { def: "table", desc: "web thickness: 3/8 (7/16 for 3-1/2 flanges and depths over 16)" },
        stiffeners: Enum(Ends) = Ends::None => { desc: "none | left | right | both: web stiffener pair (part `stiffener`, one each side of the web, 3-1/2 along the joist, tight to the bottom flange, 1/8 gap under the top flange) at that end of a run x joist; run z: any value draws the pair at the section (the joist's z centre, or the cut when it spans the run)" },
        squash_blocks: Enum(Ends) = Ends::None => { desc: "none | left | right | both: 2x4 squash block pair (part `squash_block`, 1-1/2 each side of the joist, full depth, 3-1/2 along it) at that end of a run x joist (point loads through the floor); run z: any value draws the pair at the section" },
        cuts: OptRaw => { desc: "run x: end cuts as lumber (`kerf schema cuts`: square, plumb, level, bevel, trim to a member's face); a sloped joist with a plumb or trim end bears in a hanger" },
        blocking: Bool = false => { desc: "run z: a blocking panel or rim cut end-on: the flanges get one diagonal instead of the X" },
    }
}

struct Series {
    flange: f64,
    flange_depth: f64,
    depths: &'static [f64],
}

const LIGHT: &[f64] = &[9.5, 11.875, 14.0, 16.0];
const DEEP: &[f64] = &[9.5, 11.875, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0];
const WIDE: &[f64] = &[11.875, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0];

const SERIES: [Series; 5] = [
    Series { flange: 1.75, flange_depth: 1.375, depths: LIGHT },
    Series { flange: 2.0625, flange_depth: 1.375, depths: LIGHT },
    Series { flange: 2.3125, flange_depth: 1.375, depths: DEEP },
    Series { flange: 2.5, flange_depth: 1.5, depths: LIGHT },
    Series { flange: 3.5, flange_depth: 1.375, depths: WIDE },
];

const STIFFENER_W: f64 = 3.5;
const STIFFENER_GAP: f64 = 0.125;
const SQUASH_T: f64 = 1.5;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.02
}

fn size_text(d: f64, b: f64) -> String {
    format!("{} x {}", dash_frac(d), dash_frac(b))
}

/// `11-7/8` style (the catalog form) of a length on the 1/16 grid.
fn dash_frac(x: f64) -> String {
    let t = fmt_ft_in(x);
    let t = t.trim_end_matches('"');
    t.replacen(' ', "-", 1)
}

/// The table entry nearest to (depth, flange) for the error message.
fn nearest(d: f64, b: f64) -> (f64, f64) {
    let mut best = (11.875, 2.3125);
    let mut score = f64::INFINITY;
    for s in &SERIES {
        for &dd in s.depths {
            let e = (dd - d).abs() + 4.0 * (s.flange - b).abs();
            if e < score {
                score = e;
                best = (dd, s.flange);
            }
        }
    }
    best
}

struct Slab {
    part: &'static str,
    material: &'static str,
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    /// Extent along the joist (run x: x0..x1; run z: centred on the section, `x1 - x0` long).
    x0: f64,
    x1: f64,
    cut_mark: bool,
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let jp = ctx.p.parse::<Params>()?;
    let size = jp.size.as_str();
    let parsed = super::common::parse_actual(size).map(|s| (s.t.max(s.d), s.t.min(s.d)));
    let series =
        parsed.and_then(|(d, b)| SERIES.iter().find(|s| near(s.flange, b) && s.depths.iter().any(|&x| near(x, d))));
    let (Some((d, _)), Some(series)) = (parsed, series) else {
        let (nd, nb) = parsed.map_or((11.875, 2.3125), |(d, b)| nearest(d, b));
        let brand = size.bytes().any(|c| c.is_ascii_alphabetic() && c != b'x' && c != b'X');
        let why = if brand {
            "brand series names (TJI, LPI, BCI ...) are not sizes here: give the series' depth and flange width"
        } else {
            "it is not in the series table"
        };
        ctx.p.fail(
            "size",
            format!(
                "ijoist size \"{size}\": {why}. Sizes are \"depth x flange width\": depths 9-1/2, 11-7/8, 14, 16 (18, 20, 22, 24 with 2-5/16 or 3-1/2 flanges); flange widths 1-3/4, 2-1/16, 2-5/16, 2-1/2, 3-1/2. Nearest: {{\"size\": \"{}\"}}",
                size_text(nd, nb)
            ),
        );
        return None;
    };
    let bf = series.flange;
    let tf = jp.flange_depth.unwrap_or(series.flange_depth);
    let tw = jp.web.unwrap_or(if bf > 3.0 || d > 16.5 { 0.4375 } else { 0.375 });
    if 2.0 * tf >= d - 1.0 {
        ctx.p.fail(
            "flange_depth",
            format!(
                "flange_depth {} leaves no web in a {} deep joist (two flanges must be under {})",
                fmt_ft_in(tf),
                fmt_ft_in(d),
                fmt_ft_in((d - 1.0) / 2.0)
            ),
        );
        return None;
    }
    if tw >= bf {
        ctx.p.fail("web", format!("web {} must be thinner than the flange width {}", fmt_ft_in(tw), fmt_ft_in(bf)));
        return None;
    }
    let mut until_note = String::new();
    if jp.run == Run::Z && ctx.p.has("until") {
        ctx.p.fail(
            "until",
            "'until' only applies to run x (a run z joist spans the document run: give 'length' or set 'z')".into(),
        );
        return None;
    }
    let mut z_length = None;
    let mut length = 0.0;
    if jp.run == Run::Z {
        if ctx.p.has("length") {
            z_length = Some(ctx.p.len_pos("length", None, "length of a run z joist along z, e.g. 24")?);
        }
    } else {
        length = length_or_until(
            ctx,
            "x",
            "an ijoist with run x needs its length, e.g. \"length\": 96, or \"until\": \"other@anchor\"",
            &mut until_note,
        )?;
    }
    if !ctx.p.ok {
        return None;
    }
    for m in ["wood", "osb"] {
        if !material_ok(ctx, "size", m) {
            return None;
        }
    }

    let xl = (bf - tw) / 2.0;
    let xr = (bf + tw) / 2.0;
    let run_x = jp.run == Run::X;
    let full = if run_x { length } else { STIFFENER_W };
    let mut slabs = vec![
        Slab {
            part: "bottom_flange",
            material: "wood",
            u0: 0.0,
            u1: bf,
            v0: 0.0,
            v1: tf,
            x0: 0.0,
            x1: full,
            cut_mark: true,
        },
        Slab { part: "web", material: "osb", u0: xl, u1: xr, v0: tf, v1: d - tf, x0: 0.0, x1: full, cut_mark: false },
        Slab {
            part: "top_flange",
            material: "wood",
            u0: 0.0,
            u1: bf,
            v0: d - tf,
            v1: d,
            x0: 0.0,
            x1: full,
            cut_mark: true,
        },
    ];
    let ends = |e: Ends| -> Vec<(f64, f64)> {
        if !run_x {
            return if e == Ends::None { Vec::new() } else { vec![(0.0, STIFFENER_W)] };
        }
        let w = STIFFENER_W.min(length / 2.0);
        let mut out = Vec::new();
        if matches!(e, Ends::Left | Ends::Both) {
            out.push((0.0, w));
        }
        if matches!(e, Ends::Right | Ends::Both) {
            out.push((length - w, length));
        }
        out
    };
    for (x0, x1) in ends(jp.stiffeners) {
        for (u0, u1) in [(0.0, xl), (xr, bf)] {
            slabs.push(Slab {
                part: "stiffener",
                material: "wood",
                u0,
                u1,
                v0: tf,
                v1: d - tf - STIFFENER_GAP,
                x0,
                x1,
                cut_mark: true,
            });
        }
    }
    for (x0, x1) in ends(jp.squash_blocks) {
        for (u0, u1) in [(-SQUASH_T, 0.0), (bf, bf + SQUASH_T)] {
            slabs.push(Slab { part: "squash_block", material: "wood", u0, u1, v0: 0.0, v1: d, x0, x1, cut_mark: true });
        }
    }

    let mut prisms = Vec::new();
    let mut zones: Vec<Zone> = Vec::new();
    let mut add_zone = |name: &str, b: Box| match zones.iter_mut().find(|z| z.name == name) {
        Some(z) => {
            z.box_.add_box(b);
            z.loops = model::one_loop(model::rect_loop(z.box_.x0, z.box_.y0, z.box_.x1, z.box_.y1));
        }
        None => zones.push(Zone {
            name: name.to_string(),
            loops: model::one_loop(model::rect_loop(b.x0, b.y0, b.x1, b.y1)),
            box_: b,
        }),
    };
    let section_zoff = if run_x || z_length.is_some() || ctx.p.has("z") {
        0.0
    } else {
        let mid = (ctx.scene.run[0] + ctx.scene.run[1]) / 2.0;
        ctx.scene.default_z.map_or(0.0, |z| z - mid)
    };
    for s in &slabs {
        let whole = s.part != "stiffener" && s.part != "squash_block";
        let pr = if run_x {
            Prism {
                part: s.part.to_string(),
                material: s.material.to_string(),
                loops: model::one_loop(model::rect_loop(s.x0, s.v0, s.x1, s.v1)),
                zhalf: (s.u1 - s.u0) / 2.0,
                zoff: (s.u0 + s.u1) / 2.0 - bf / 2.0,
                lengthwise: true,
                ..Prism::default()
            }
        } else {
            Prism {
                part: s.part.to_string(),
                material: s.material.to_string(),
                loops: model::one_loop(model::rect_loop(s.u0, s.v0, s.u1, s.v1)),
                quads: if s.cut_mark { vec![quad_of(s.u0, s.v0, s.u1, s.v1)] } else { Vec::new() },
                blocking: jp.blocking && whole,
                zhalf: if whole { 0.0 } else { (s.x1 - s.x0) / 2.0 },
                zoff: if whole { 0.0 } else { section_zoff },
                ..Prism::default()
            }
        };
        let b = if run_x {
            Box { x0: s.x0, y0: s.v0, x1: s.x1, y1: s.v1 }
        } else {
            Box { x0: s.u0, y0: s.v0, x1: s.u1, y1: s.v1 }
        };
        add_zone(s.part, b);
        prisms.push(pr);
    }

    let anchor = |name: &str, p: V2| NamedAnchor { name: name.into(), p };
    let mut anchors = Vec::new();
    let mut info =
        format!("ijoist {} (flange {} x {}, web {})", size_text(d, bf), fmt_ft_in(bf), fmt_ft_in(tf), fmt_ft_in(tw));
    let box_;
    let nat;
    if run_x {
        for (n, v) in [
            ("top_flange", d),
            ("top_flange_bottom", d - tf),
            ("web_center", d / 2.0),
            ("bottom_flange_top", tf),
            ("bottom_flange", 0.0),
        ] {
            anchors.push(anchor(n, V2::init(length / 2.0, v)));
            anchors.push(anchor(&format!("{n}_start"), V2::init(0.0, v)));
            anchors.push(anchor(&format!("{n}_end"), V2::init(length, v)));
        }
        anchors.push(anchor("bearing_left", V2::init(0.0, 0.0)));
        anchors.push(anchor("bearing_right", V2::init(length, 0.0)));
        box_ = Box { x0: 0.0, y0: 0.0, x1: length, y1: d };
        nat = Some(bf);
        info.push_str(&format!(" run x L={}{until_note}", fmt_ft_in(length)));
    } else {
        for (n, u, v) in [
            ("top_flange", bf / 2.0, d),
            ("bottom_flange", bf / 2.0, 0.0),
            ("web_center", bf / 2.0, d / 2.0),
            ("web_left", xl, d / 2.0),
            ("web_right", xr, d / 2.0),
            ("top_flange_tip_left", 0.0, d - tf),
            ("top_flange_tip_right", bf, d - tf),
            ("bottom_flange_tip_left", 0.0, tf),
            ("bottom_flange_tip_right", bf, tf),
        ] {
            anchors.push(anchor(n, V2::init(u, v)));
        }
        box_ = Box { x0: 0.0, y0: 0.0, x1: bf, y1: d };
        nat = z_length;
        info.push_str(" run z");
        if let Some(l) = z_length {
            info.push_str(&format!(" L={}", fmt_ft_in(l)));
        }
    }
    if jp.stiffeners != Ends::None {
        info.push_str(&format!(" + stiffeners {}", jp.stiffeners.tag()));
    }
    if jp.squash_blocks != Ends::None {
        info.push_str(&format!(" + squash blocks {}", jp.squash_blocks.tag()));
    }
    Some(Built { prisms, anchors, zones, box_, nat_z: nat, info, ..Built::default() })
}
