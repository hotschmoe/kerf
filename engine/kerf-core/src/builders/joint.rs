//! joint (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, box_of_prisms, fmt_num, fmt_ft_in, material_or, oriented_ccw};
use crate::geom::{self, Pt, V2};
use crate::math;
use crate::model::{self, Built, NamedAnchor, Prism};
use crate::params::{ParamEnum, param_enum, params};
use crate::scene::{self, Scene, State};
use alloc::format;
use alloc::vec;

param_enum! { pub enum Kind { Expansion = "expansion", Control = "control", TooledEdge = "tooled_edge", Sealant = "sealant" } }
param_enum! { pub enum Corner { TopRight = "top_right", TopLeft = "top_left", BottomRight = "bottom_right", BottomLeft = "bottom_left" } }

params! {
    pub struct Params {
        kind: Enum(Kind) => { desc: "expansion | control | tooled_edge | sealant" },
        width: OptLenPos => { def: "0.5 (control 0.25)", desc: "expansion: filler thickness; control: notch width at the top; sealant: joint gap width" },
        depth: OptLenPos => { def: "expansion 4, control 1, sealant 0.25", desc: "expansion: filler depth below the top (set to the slab thickness, or give `in`); control: notch depth (default 1/4 of the `in` zone height); sealant: bead depth" },
        in_: OptRaw => { key: "in", desc: "optional host zone \"comp[.part]\" whose height sets the default depth (expansion: full height; control: 1/4)" },
        cap: Len = 0.0 => { desc: "expansion: depth of a sealant cap at the top of the filler (part `sealant`)" },
        radius: LenPos = 0.25 => { desc: "tooled_edge: radius of the rounded corner" },
        corner: Enum(Corner) = Corner::TopRight => { desc: "tooled_edge: which corner of the concrete the point is: top_right (concrete lies left and below), top_left, bottom_right, bottom_left" },
        backer_rod: Bool = true => { desc: "sealant: draw the backer rod circle (diameter 1.25*width) below the bead" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let jp = ctx.p.parse::<Params>()?;
    let k = jp.kind;
    let mut host_h = 0.0;
    if let Some(iv) = &jp.in_ {
        let Some(in_s) = iv.str() else {
            ctx.p.fail("in", "param 'in' must be \"<component>[.<part>]\", the concrete zone the joint cuts (its height sets the default depth)".into());
            return None;
        };
        let (hid, part) = match in_s.split_once('.') {
            Some((h, p)) => (h, Some(p)),
            None => (in_s, None),
        };
        let Some(host) = ctx.scene.find(hid) else {
            let ids = ctx.scene.comp_ids();
            ctx.p.fail("in", format!("no component '{hid}'. Known ids: {}", scene::join_ids(&ids)));
            return None;
        };
        if host.state != State::Ok {
            ctx.p.fail("in", format!("host '{hid}' did not build; fix its errors first"));
            return None;
        }
        let bx = match part {
            Some(pt) => match Scene::part_box(host, pt) {
                Some(b) => b,
                None => {
                    ctx.p.fail("in", format!("component '{hid}' has no part '{pt}'"));
                    return None;
                }
            },
            None => host.built.box_,
        };
        host_h = bx.height();
    }
    let width = jp.width.unwrap_or(if k == Kind::Control { 0.25 } else { 0.5 });
    let depth_def = match k {
        Kind::Control => {
            if host_h > 0.0 {
                host_h / 4.0
            } else {
                1.0
            }
        }
        Kind::Sealant => (0.5 * width).max(0.25),
        _ => {
            if host_h > 0.0 {
                host_h
            } else {
                4.0
            }
        }
    };
    let depth = jp.depth.unwrap_or(depth_def);
    let radius = jp.radius;
    let cap = jp.cap;
    let mut prisms = vec![];
    let mut anchors = vec![NamedAnchor { name: "joint_top".into(), p: V2::init(0.0, 0.0) }];
    let void_mat = material_or(ctx, "void", "generic");
    match k {
        Kind::Expansion => {
            if cap < 0.0 || cap >= depth {
                ctx.p.fail(
                    "cap",
                    format!(
                        "cap (sealant depth at the top of the joint) must be from 0 to less than depth {} (got {})",
                        fmt_num(depth),
                        fmt_num(cap)
                    ),
                );
                return None;
            }
            let hw = width / 2.0;
            prisms.push(Prism {
                part: "filler".into(),
                material: material_or(ctx, "joint_filler", "generic").into(),
                loops: model::one_loop(model::rect_loop(-hw, -depth, hw, -cap)),
                ..Prism::default()
            });
            if cap > 0.0 {
                prisms.push(Prism {
                    part: "sealant".into(),
                    material: material_or(ctx, "sealant", "steel").into(),
                    loops: model::one_loop(model::rect_loop(-hw, -cap, hw, 0.0)),
                    ..Prism::default()
                });
            }
        }
        Kind::Control => {
            let tri = vec![Pt::new(-width / 2.0, 0.0), Pt::new(0.0, -depth), Pt::new(width / 2.0, 0.0)];
            prisms.push(Prism {
                part: "notch".into(),
                material: void_mat.into(),
                loops: model::one_loop(oriented_ccw(tri)),
                embedded: true,
                ..Prism::default()
            });
        }
        Kind::TooledEdge => {
            let r = radius;
            let loop0 =
                [Pt::new(0.0, 0.0), Pt { x: -r, y: 0.0, b: geom::bulge_from_sweep(-math::PI / 2.0) }, Pt::new(0.0, -r)];
            let c = jp.corner;
            let sx = if matches!(c, Corner::TopLeft | Corner::BottomLeft) { -1.0 } else { 1.0 };
            let sy = if matches!(c, Corner::BottomLeft | Corner::BottomRight) { -1.0 } else { 1.0 };
            let lp = oriented_ccw(geom::Xf::scaling(sx, sy).apply_loop(&loop0));
            prisms.push(Prism {
                part: "radius".into(),
                material: void_mat.into(),
                loops: model::one_loop(lp),
                embedded: true,
                ..Prism::default()
            });
            anchors.push(NamedAnchor { name: "corner".into(), p: V2::init(0.0, 0.0) });
        }
        Kind::Sealant => {
            let hw = width / 2.0;
            prisms.push(Prism {
                part: "bead".into(),
                material: material_or(ctx, "sealant", "steel").into(),
                loops: model::one_loop(model::rect_loop(-hw, -depth, hw, 0.0)),
                ..Prism::default()
            });
            if jp.backer_rod {
                let rd = 1.25 * width;
                prisms.push(Prism {
                    part: "rod".into(),
                    material: material_or(ctx, "backer_rod", "generic").into(),
                    loops: model::one_loop(model::circle_loop(0.0, -depth - rd / 2.0, rd / 2.0)),
                    ..Prism::default()
                });
            }
        }
    }
    Some(Built {
        box_: box_of_prisms(&prisms),
        prisms,
        anchors,
        info: format!("joint {} {} wide x {} deep", k.tag(), fmt_ft_in(width), fmt_ft_in(depth)),
        ..Built::default()
    })
}
