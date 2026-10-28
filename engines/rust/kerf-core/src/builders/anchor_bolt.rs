//! anchor bolt (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, box_of_prisms, oriented_ccw};
use crate::model::fmt_num;
use crate::units::ftin;
use crate::geom::{self, Pt, V2};
use crate::model::{self, Built, NamedAnchor, Prism};
use crate::num;
use crate::params::{ParamEnum, param_enum, params};
use crate::pathgeom;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::f64::consts::PI;

param_enum! { pub enum Hook { J = "J", L = "L", Headed = "headed", None = "none", Wedge = "wedge", Screw = "screw" } }

params! {
    pub struct Params {
        diameter: LenPos = 0.5 => { desc: "0.5 or 0.625 typical" },
        embed: OptLenPos => { def: "7 (4 for wedge/screw)", desc: "length below the placement point (top of concrete); effective embedment for wedge/screw" },
        projection: LenPos = 2.5 => { desc: "length above the placement point" },
        hook: Enum(Hook) = Hook::J => { desc: "J: 180 degree bend toward +x, inside radius 1.5*d, returning up hook_len from the lowest point; L: 90 degree bend toward +x, horizontal leg ends hook_len from the shaft centerline; headed: square head 2*d wide, 0.5*d thick; none; wedge: post-installed expansion anchor (straight shaft, expansion clip 1.15*d wide x 0.6*embed long at the embedded end, nut+washer); screw: Titen HD style concrete screw (thread ticks along the embedment, hex washer head at the top, no nut)" },
        hook_len: OptLenPos => { def: "J 2, L 3", desc: "hook leg length in inches (see hook)" },
        nut_washer: Bool = true => { desc: "draw nut (1.5*d wide, 0.875*d tall, top at projection - 0.25*d) and washer (2.25*d wide, 0.125 thick) under it" },
    }
}

fn steel(part: &str, loops: Vec<Vec<Pt>>, zhalf: f64) -> Prism {
    Prism { part: part.to_string(), material: "steel".to_string(), loops, embedded: true, zhalf, ..Prism::default() }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let ap = ctx.p.parse::<Params>()?;
    let d = ap.diameter;
    let h = ap.hook;
    let post = matches!(h, Hook::Wedge | Hook::Screw);
    let embed = ap.embed.unwrap_or(if post { 4.0 } else { 7.0 });
    let proj = ap.projection;
    let hook_len = ap.hook_len.unwrap_or(if h == Hook::L { 3.0 } else { 2.0 });
    let r = d / 2.0;
    let rc = 1.5 * d + r;
    let mut cl: Vec<Pt> = vec![Pt { x: 0.0, y: proj, b: 0.0 }];
    let y_bottom_cl = -embed + r;
    match h {
        Hook::J => {
            let yb = y_bottom_cl + rc;
            cl.push(Pt { x: 0.0, y: yb, b: geom::bulge_from_sweep(PI) });
            cl.push(Pt { x: 2.0 * rc, y: yb, b: 0.0 });
            cl.push(Pt { x: 2.0 * rc, y: -embed + hook_len, b: 0.0 });
        }
        Hook::L => {
            let yb = y_bottom_cl + rc;
            cl.push(Pt { x: 0.0, y: yb, b: geom::bulge_from_sweep(PI / 2.0) });
            cl.push(Pt { x: rc, y: y_bottom_cl, b: 0.0 });
            cl.push(Pt { x: hook_len.max(rc + 0.01), y: y_bottom_cl, b: 0.0 });
        }
        Hook::Headed => cl.push(Pt { x: 0.0, y: -embed + 0.5 * d, b: 0.0 }),
        _ => cl.push(Pt { x: 0.0, y: -embed, b: 0.0 }),
    }
    let rib = pathgeom::ribbon(&cl, r, r);
    let mut prisms: Vec<Prism> = Vec::new();
    prisms.push(Prism { centerline: cl, sweep_r: r, ..steel("shank", model::one_loop(rib), 0.0) });
    if h == Hook::Headed {
        prisms.push(steel("shank", model::one_loop(model::rect_loop(-d, -embed, d, -embed + 0.5 * d)), d));
    }
    if h == Hook::Wedge {
        let hw = 0.575 * d;
        let ch = (0.35 * d).min(0.2 * embed);
        let clip = vec![
            Pt::new(-hw + ch, -embed),
            Pt::new(hw - ch, -embed),
            Pt::new(hw, -embed + ch),
            Pt::new(hw, -embed + 0.6 * embed),
            Pt::new(-hw, -embed + 0.6 * embed),
            Pt::new(-hw, -embed + ch),
        ];
        prisms.push(steel("clip", model::one_loop(clip), hw));
    }
    if h == Hook::Screw {
        let td = 0.22 * d;
        let pitch = 0.5 * d;
        let n = num::to_int_clamped::<usize>(crate::math::floor(embed / pitch), 0, 80);
        if n >= 1 {
            for sgn in [-1.0, 1.0] {
                let mut strip = vec![Pt::new(sgn * r, 0.0)];
                for i in 0..n {
                    let fi = i as f64;
                    strip.push(Pt::new(sgn * (r + td), -(fi + 0.5) * pitch));
                    strip.push(Pt::new(sgn * r, -(fi + 1.0) * pitch));
                }
                let lp = oriented_ccw(strip);
                prisms.push(steel("threads", model::one_loop(lp), r + td));
            }
        }
        let head_h = 0.6 * d;
        let fl_t = 0.1 * d;
        prisms.push(steel("washer", model::one_loop(model::rect_loop(-0.95 * d, proj - head_h - fl_t, 0.95 * d, proj - head_h)), 0.95 * d));
        prisms.push(steel("head", model::one_loop(model::rect_loop(-0.75 * d, proj - head_h, 0.75 * d, proj)), 0.75 * d));
    } else if ap.nut_washer {
        let nut_h = 0.875 * d;
        let ytop = proj - 0.25 * d;
        let nut_bot = ytop - nut_h;
        prisms.push(steel("washer", model::one_loop(model::rect_loop(-1.125 * d, nut_bot - 0.125, 1.125 * d, nut_bot)), 1.125 * d));
        prisms.push(steel("nut", model::one_loop(model::rect_loop(-0.75 * d, nut_bot, 0.75 * d, ytop)), 0.75 * d));
    }
    let bx = box_of_prisms(&prisms);
    Some(Built {
        anchors: vec![NamedAnchor { name: "top_of_concrete".to_string(), p: V2::init(0.0, 0.0) }],
        box_: bx,
        nat_z: Some(d),
        info: format!("anchor_bolt {}\" dia, embed {}, proj {}, {}{}", fmt_num(d), ftin(embed), ftin(proj), h.tag(), if post { "" } else { " hook" }),
        prisms,
        ..Built::default()
    })
}
