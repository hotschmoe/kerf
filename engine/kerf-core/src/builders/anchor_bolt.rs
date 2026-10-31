//! anchor bolt (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, box_of_prisms, oriented_ccw};
use crate::geom::{self, Pt, V2};
use crate::model::fmt_num;
use crate::model::{self, Built, NamedAnchor, Prism};
use crate::num;
use crate::params::{ParamEnum, param_enum, params};
use crate::pathgeom;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::f64::consts::PI;

param_enum! { pub enum Hook { J = "J", L = "L", Headed = "headed", None = "none", Wedge = "wedge", Screw = "screw", Adhesive = "adhesive" } }

params! {
    pub struct Params {
        diameter: LenPos = 0.5 => { desc: "0.5 or 0.625 typical" },
        embed: OptLenPos => { def: "7 (4 for wedge/screw)", desc: "length below the placement point (top of concrete); effective embedment for wedge/screw" },
        projection: OptLen => { def: "2.5 (with plate: through the plate, washer and nut)", desc: "length above the placement point; 0 (a rod cut flush) needs nut_washer false" },
        hook: Enum(Hook) = Hook::J => { desc: "J: 180 degree bend toward +x, inside radius 1.5*d, returning up hook_len from the lowest point; L: 90 degree bend toward +x, horizontal leg ends hook_len from the shaft centerline; headed: square head 2*d wide, 0.5*d thick; none; wedge: post-installed expansion anchor (straight shaft, expansion clip 1.15*d wide x 0.6*embed long at the embedded end, nut+washer); screw: Titen HD style concrete screw (thread ticks along the embedment, hex washer head at the top, no nut); adhesive: threaded rod in a drilled hole filled with adhesive (the hole drawn grout-hatched, `hole` wide, full embed)" },
        hole: OptLenPos => { def: "d + 1/8", desc: "hook adhesive: drilled hole diameter" },
        hook_len: OptLenPos => { def: "J 2, L 3", desc: "hook leg length in inches (see hook)" },
        nut_washer: Bool = true => { desc: "draw nut (1.5*d wide, 0.875*d tall, top at projection - 0.25*d) and washer (2.25*d wide, 0.125 thick) under it" },
        plate: OptStr => { def: "null", desc: "the base plate (or any member) the rod passes through: the nut and washer sit on its top at the rod, and projection defaults to reach through them" },
        leveling_nut: Bool = false => { desc: "a leveling nut and washer under `plate` (needs plate)" },
        plate_washer: OptStr => { def: "null", desc: "\"WxWxT\" plate washer under the top nut instead of the round washer, e.g. \"3x3x0.229\" or \"3x3x1/4\"" },
    }
}

fn steel(part: &str, loops: Vec<Vec<Pt>>, zhalf: f64) -> Prism {
    Prism { part: part.to_string(), material: "steel".to_string(), loops, embedded: true, zhalf, ..Prism::default() }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let ap = ctx.p.parse::<Params>()?;
    let d = ap.diameter;
    let h = ap.hook;
    let post = matches!(h, Hook::Wedge | Hook::Screw | Hook::Adhesive);
    let embed = ap.embed.unwrap_or(if post { 4.0 } else { 7.0 });
    let pw = match ap.plate_washer.as_deref() {
        Some(t) => Some(parse_plate_washer(ctx, t)?),
        None => None,
    };
    let washer_t = pw.map_or(0.125, |w| w.1);
    let nut_h = 0.875 * d;
    let plate_ys = match ap.plate.as_deref() {
        Some(pl) => Some(plate_faces(ctx, pl)?),
        None => None,
    };
    if ap.leveling_nut && plate_ys.is_none() {
        ctx.p.fail(
            "leveling_nut",
            "leveling_nut needs \"plate\": the base plate the rod passes through (the nut goes under it)".to_string(),
        );
        return None;
    }
    let proj = match (ap.projection, plate_ys) {
        (Some(p), _) => p,
        (None, Some((_, top))) => crate::math::ceil((top + washer_t + nut_h + 0.25 * d) * 16.0 - 1e-9) / 16.0,
        (None, None) => 2.5,
    };
    if proj < 0.0 || (proj == 0.0 && ap.nut_washer && h != Hook::Screw) {
        ctx.p.fail("projection", format!("projection must be greater than 0 for a rod with a nut (got {}); a rod cut flush with the concrete is projection 0 with nut_washer false", fmt_num(proj)));
        return None;
    }
    if let Some((_, top)) = plate_ys
        && ap.nut_washer
        && proj < top + washer_t + nut_h
    {
        ctx.p.fail("projection", format!("projection {} ends inside the plate, washer and nut (they need {} above the concrete); remove projection to fit it, or raise it", fmt_ft_in(proj), fmt_ft_in(top + washer_t + nut_h + 0.25 * d)));
        return None;
    }
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
        prisms.push(steel(
            "washer",
            model::one_loop(model::rect_loop(-0.95 * d, proj - head_h - fl_t, 0.95 * d, proj - head_h)),
            0.95 * d,
        ));
        prisms.push(steel(
            "head",
            model::one_loop(model::rect_loop(-0.75 * d, proj - head_h, 0.75 * d, proj)),
            0.75 * d,
        ));
    } else if ap.nut_washer {
        let (ytop, nut_bot) = match plate_ys {
            Some((_, top)) => (top + washer_t + nut_h, top + washer_t),
            None => (proj - 0.25 * d, proj - 0.25 * d - nut_h),
        };
        let ww = pw.map_or(1.125 * d, |w| w.0 / 2.0);
        prisms.push(steel("washer", model::one_loop(model::rect_loop(-ww, nut_bot - washer_t, ww, nut_bot)), ww));
        prisms.push(steel("nut", model::one_loop(model::rect_loop(-0.75 * d, nut_bot, 0.75 * d, ytop)), 0.75 * d));
    }
    if ap.leveling_nut
        && let Some((bot, _)) = plate_ys
    {
        prisms.push(steel(
            "leveling_washer",
            model::one_loop(model::rect_loop(-1.125 * d, bot - 0.125, 1.125 * d, bot)),
            1.125 * d,
        ));
        prisms.push(steel(
            "leveling_nut",
            model::one_loop(model::rect_loop(-0.75 * d, bot - 0.125 - nut_h, 0.75 * d, bot - 0.125)),
            0.75 * d,
        ));
    }
    if h == Hook::Adhesive {
        let hr = ap.hole.unwrap_or(d + 0.125) / 2.0;
        if hr <= r {
            ctx.p.fail("hole", format!("hole {} must be wider than the rod ({})", fmt_ft_in(2.0 * hr), fmt_ft_in(d)));
            return None;
        }
        let mat = super::common::material_or(ctx, "grout", "concrete");
        prisms.insert(
            0,
            Prism {
                material: mat.to_string(),
                ..steel("adhesive", model::one_loop(model::rect_loop(-hr, -embed, hr, 0.0)), hr)
            },
        );
    }
    let bx = box_of_prisms(&prisms);
    Some(Built {
        anchors: vec![NamedAnchor { name: "top_of_concrete".to_string(), p: V2::init(0.0, 0.0) }],
        box_: bx,
        nat_z: Some(d),
        info: format!(
            "anchor_bolt {}\" dia, embed {}, proj {}, {}{}",
            fmt_num(d),
            fmt_ft_in(embed),
            fmt_ft_in(proj),
            h.tag(),
            if post { "" } else { " hook" }
        ) + &extras(&ap),
        prisms,
        ..Built::default()
    })
}

fn extras(ap: &Params) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    if let Some(p) = &ap.plate {
        out.push_str(&format!(", through {p}"));
    }
    if ap.leveling_nut {
        out.push_str(", leveling nut");
    }
    if let Some(w) = &ap.plate_washer {
        out.push_str(&format!(", plate washer {w}"));
    }
    out
}

/// "WxWxT" (lengths): the plate washer's width and thickness.
fn parse_plate_washer(ctx: &mut Ctx, t: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = t.split(['x', 'X']).collect();
    let vals: Vec<f64> = parts.iter().filter_map(|p| crate::units::parse_length_str(p.trim())).collect();
    if parts.len() != 3 || vals.len() != 3 || vals.iter().any(|v| *v <= 0.0) || vals[2] >= vals[0] {
        ctx.p.fail(
            "plate_washer",
            format!("plate_washer must be \"WxWxT\" in inches, e.g. \"3x3x0.229\" or \"3x3x1/4\" (got \"{t}\")"),
        );
        return None;
    }
    Some((vals[0].max(vals[1]), vals[2]))
}

/// The bottom and top (local y, from the placement point) of the member the rod passes through, on the rod's axis.
fn plate_faces(ctx: &mut Ctx, name: &str) -> Option<(f64, f64)> {
    let m = super::hardware::member(ctx, "plate", name)?;
    let o = ctx.origin;
    let ys = crate::pathclip::scan(&m.flat, o.x, true);
    let (Some(&bot), Some(&top)) = (ys.first(), ys.last()) else {
        ctx.p.fail(
            "plate",
            format!(
                "plate '{}' is not over the rod (x {}): it spans x {}..{}",
                m.id,
                fmt_ft_in(o.x),
                fmt_ft_in(m.bx.x0),
                fmt_ft_in(m.bx.x1)
            ),
        );
        return None;
    };
    if bot < o.y {
        ctx.p.fail(
            "plate",
            format!(
                "plate '{}' bottom (y {}) is below the rod's top of concrete (y {}); the rod passes up through the plate",
                m.id,
                fmt_ft_in(bot),
                fmt_ft_in(o.y)
            ),
        );
        return None;
    }
    Some((bot - o.y, top - o.y))
}
