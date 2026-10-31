//! bolt (structural bolt group): its parameters (`Params`, the single source for the parser and the catalog) and its builder.
//! Heavy hex head and nut (across flats 1.5d + 1/8, head 0.625d tall, nut d tall), F436 washer (2d OD, 5/32 thick) under
//! the nut, thread stick-out 0.5d. The head bears at the local origin; the grip runs down (side) or back along -z (end).

use super::common::{Ctx, box_of_prisms, fmt_ft_in, fmt_num};
use crate::geom::{Pt, V2};
use crate::model::{self, Built, NamedAnchor, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Orient { Side = "side", End = "end" } }
param_enum! { pub enum Line { X = "x", Y = "y", Z = "z" } }
param_enum! { pub enum BoltKind { Bolt = "bolt", WeldStud = "weld_stud", HeadedStud = "headed_stud" } }

params! {
    pub struct Params {
        kind: Enum(BoltKind) = BoltKind::Bolt => { desc: "bolt: head, grip, washer and nut; weld_stud: threaded stud welded at the placement point (no head), grip UP through the plies, washer and nut on top (wood nailers on steel); headed_stud: shear stud welded at the placement point, `grip` = its length after welding, head 1.6d x 0.5d (embed plates, composite decks). Studs are side only: rotate the component to point them" },
        diameter: LenPos = 0.75 => { desc: "bolt diameter: 0.5, 0.625, 0.75 (typical), 0.875, 1" },
        grade: Str = "A325" => { desc: "free text for notes and the summary: A307, A325, A490, F1554 ..." },
        grip: LenPos = 1.0 => { desc: "thickness of the plies clamped between head and nut (sum of plate and flange thicknesses); the head bears at the placement point" },
        orient: Enum(Orient) = Orient::Side => { desc: "side: the axis lies in the drawing plane, head up (+y) at the placement point, grip downward (bolts through a base plate or a flange seen in section; rotate 180 for head down); end: the axis runs along z, seen end-on as the hex head, washer and shank circle (bolts through a shear tab or web seen face-on), head toward the viewer" },
        count: Int(u16, 1, 50) = 1 => { desc: "bolts in the row (a gauge line)" },
        spacing: LenPos = 3.0 => { desc: "center-to-center spacing along `line` (AISC minimum 2 2/3 d, 3d preferred; W_BOLT_SPACING)" },
        line: Enum(Line) = Line::X => { desc: "direction of the row from bolt 1: x or z for side bolts, x or y for end bolts" },
        washer: Bool = true => { desc: "draw the hardened washer under the nut" },
        counterbore: Bool = false => { desc: "weld_stud: washer and nut sit in a bored pocket in the top ply, the stud end flush with the top of the grip, so nothing projects above the plies (sheathing or joists bear flat); draws the pocket (washer OD + 1/8\", part `counterbore`). Needs grip >= washer + nut + d/2" },
    }
}

pub const WASHER_T: f64 = 0.15625;

/// Across-flats width of the heavy hex head and nut.
pub fn hex_flats(d: f64) -> f64 {
    1.5 * d + 0.125
}

fn steel(part: &str, loops: Vec<Vec<Pt>>) -> Prism {
    Prism { part: part.to_string(), material: "steel".to_string(), loops, embedded: true, ..Prism::default() }
}

fn hex_loop(c: V2, flats: f64) -> Vec<Pt> {
    let r = flats / crate::math::sqrt(3.0);
    let h = flats / 2.0;
    vec![
        Pt::new(c.x + r, c.y),
        Pt::new(c.x + r / 2.0, c.y + h),
        Pt::new(c.x - r / 2.0, c.y + h),
        Pt::new(c.x - r, c.y),
        Pt::new(c.x - r / 2.0, c.y - h),
        Pt::new(c.x + r / 2.0, c.y - h),
    ]
}

/// The bolt axis positions in local XY, in bolt order (the same for every bolt of a z row).
pub fn offsets(count: usize, spacing: f64, line: Line) -> Vec<(V2, f64)> {
    (0..count)
        .map(|k| {
            let s = k as f64 * spacing;
            match line {
                Line::X => (V2::init(s, 0.0), 0.0),
                Line::Y => (V2::init(0.0, s), 0.0),
                Line::Z => (V2::init(0.0, 0.0), s),
            }
        })
        .collect()
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let bp = ctx.p.parse::<Params>()?;
    let d = bp.diameter;
    let side = bp.orient == Orient::Side;
    if side && bp.line == Line::Y {
        ctx.p.fail("line", "line y runs along the axis of a side bolt: use line x (row across the drawing) or z (row into the drawing)".into());
        return None;
    }
    if !side && bp.line == Line::Z {
        ctx.p.fail("line", "line z runs along the axis of an end bolt: use line x or y".into());
        return None;
    }
    if bp.kind != BoltKind::Bolt {
        if !side {
            ctx.p.fail("orient", format!("a {} is welded to a face and drawn side-on: use orient side and rotate the component to point it (e.g. rotate 90 for a stud on a column flange seen in plan)", bp.kind.tag()));
            return None;
        }
        if bp.counterbore {
            let need = if bp.washer { WASHER_T } else { 0.0 } + 1.5 * d;
            if bp.kind != BoltKind::WeldStud {
                ctx.p.fail("counterbore", "counterbore is for a weld_stud (its nut and washer recessed into the top ply); remove it or set kind \"weld_stud\"".into());
                return None;
            }
            if bp.grip < need - 1e-9 {
                ctx.p.fail("counterbore", format!("a counterbore holds the washer, nut and d/2 of stud ({}), deeper than the grip {}: set grip to the plies' thickness (at least {}) or remove counterbore", fmt_ft_in(need), fmt_ft_in(bp.grip), fmt_ft_in(need)));
                return None;
            }
        }
        return Some(stud(&bp));
    }
    if bp.counterbore {
        ctx.p.fail("counterbore", "counterbore is for a weld_stud (its nut and washer recessed into the top ply); remove it or set kind \"weld_stud\"".into());
        return None;
    }
    let f = hex_flats(d);
    let head_h = 0.625 * d;
    let nut_h = d;
    let wt = if bp.washer { WASHER_T } else { 0.0 };
    let stick = 0.5 * d;
    let grip = bp.grip;
    let tail = grip + wt + nut_h + stick;
    let mut prisms: Vec<Prism> = Vec::new();
    let mut anchors = vec![NamedAnchor { name: "head_bearing".to_string(), p: V2::init(0.0, 0.0) }];
    let offs = offsets(usize::from(bp.count), bp.spacing, bp.line);
    for (k, &(o, zo)) in offs.iter().enumerate() {
        if side {
            let cl = vec![Pt::new(o.x, o.y), Pt::new(o.x, o.y - tail)];
            prisms.push(Prism {
                centerline: cl,
                sweep_r: d / 2.0,
                zoff: zo,
                ..steel("shank", model::one_loop(model::rect_loop(o.x - d / 2.0, -tail, o.x + d / 2.0, 0.0)))
            });
            prisms.push(Prism {
                zhalf: f / 2.0,
                zoff: zo,
                ..steel("head", model::one_loop(model::rect_loop(o.x - f / 2.0, 0.0, o.x + f / 2.0, head_h)))
            });
            if bp.washer {
                prisms.push(Prism {
                    zhalf: d,
                    zoff: zo,
                    ..steel("washer", model::one_loop(model::rect_loop(o.x - d, -grip - wt, o.x + d, -grip)))
                });
            }
            let nb = -grip - wt;
            prisms.push(Prism {
                zhalf: f / 2.0,
                zoff: zo,
                ..steel("nut", model::one_loop(model::rect_loop(o.x - f / 2.0, nb - nut_h, o.x + f / 2.0, nb)))
            });
        } else {
            let place = |pr: Prism, z0: f64, z1: f64| Prism {
                zoff: (z0 + z1) / 2.0 + grip / 2.0,
                zhalf: (z1 - z0) / 2.0,
                ..pr
            };
            prisms.push(place(steel("head", model::one_loop(hex_loop(o, f))), 0.0, head_h));
            prisms.push(place(steel("shank", model::one_loop(model::circle_loop(o.x, o.y, d / 2.0))), -tail, 0.0));
            if bp.washer {
                prisms.push(place(
                    steel("washer", model::one_loop(model::circle_loop(o.x, o.y, d))),
                    -grip - wt,
                    -grip,
                ));
            }
            prisms.push(place(steel("nut", model::one_loop(hex_loop(o, f))), -grip - wt - nut_h, -grip - wt));
        }
        anchors.push(NamedAnchor { name: format!("bolt_{}", k + 1), p: o });
    }
    anchors.push(NamedAnchor {
        name: "nut_bearing".to_string(),
        p: if side { V2::init(0.0, -grip) } else { V2::init(0.0, 0.0) },
    });
    let bx = box_of_prisms(&prisms);
    let n = bp.count;
    let row =
        if n > 1 { format!(", ({n}) @ {} along {}", fmt_ft_in(bp.spacing), bp.line.tag()) } else { String::new() };
    Some(Built {
        prisms,
        anchors,
        box_: bx,
        nat_z: Some(if side { d } else { grip }),
        info: format!("bolt {}\" dia. {}, grip {}, {}{row}", fmt_num(d), bp.grade, fmt_ft_in(grip), bp.orient.tag()),
        ..Built::default()
    })
}

/// A weld stud (threaded, nut and washer on the plies above) or a headed shear stud, welded at the local origin and
/// pointing up (+y).
fn stud(bp: &Params) -> Built {
    let d = bp.diameter;
    let f = hex_flats(d);
    let grip = bp.grip;
    let weld = |o: V2| {
        vec![
            Pt::new(o.x - 0.8 * d, 0.0),
            Pt::new(o.x + 0.8 * d, 0.0),
            Pt::new(o.x + 0.5 * d, 0.2 * d),
            Pt::new(o.x - 0.5 * d, 0.2 * d),
        ]
    };
    let mut prisms: Vec<Prism> = Vec::new();
    let mut anchors = vec![
        NamedAnchor { name: "head_bearing".to_string(), p: V2::init(0.0, 0.0) },
        NamedAnchor { name: "weld_face".to_string(), p: V2::init(0.0, 0.0) },
    ];
    let offs = offsets(usize::from(bp.count), bp.spacing, bp.line);
    for (k, &(o, zo)) in offs.iter().enumerate() {
        prisms.push(Prism { zhalf: 0.8 * d, zoff: zo, ..steel("weld", model::one_loop(weld(o))) });
        if bp.kind == BoltKind::WeldStud {
            let wt = if bp.washer { WASHER_T } else { 0.0 };
            let seat = if bp.counterbore { grip - wt - 1.5 * d } else { grip };
            let top = seat + wt + d + 0.5 * d;
            if bp.counterbore {
                let hr = d + 0.0625;
                prisms.push(Prism {
                    zhalf: hr,
                    zoff: zo,
                    material: "void".to_string(),
                    ..steel("counterbore", model::one_loop(model::rect_loop(o.x - hr, seat, o.x + hr, grip)))
                });
            }
            prisms.push(Prism {
                centerline: vec![Pt::new(o.x, 0.0), Pt::new(o.x, top)],
                sweep_r: d / 2.0,
                zoff: zo,
                ..steel("shank", model::one_loop(model::rect_loop(o.x - d / 2.0, 0.0, o.x + d / 2.0, top)))
            });
            if bp.washer {
                prisms.push(Prism {
                    zhalf: d,
                    zoff: zo,
                    ..steel("washer", model::one_loop(model::rect_loop(o.x - d, seat, o.x + d, seat + wt)))
                });
            }
            prisms.push(Prism {
                zhalf: f / 2.0,
                zoff: zo,
                ..steel(
                    "nut",
                    model::one_loop(model::rect_loop(o.x - f / 2.0, seat + wt, o.x + f / 2.0, seat + wt + d)),
                )
            });
        } else {
            let (hd, ht) = (1.6 * d, 0.5 * d);
            prisms.push(Prism {
                centerline: vec![Pt::new(o.x, 0.0), Pt::new(o.x, grip - ht)],
                sweep_r: d / 2.0,
                zoff: zo,
                ..steel("shank", model::one_loop(model::rect_loop(o.x - d / 2.0, 0.0, o.x + d / 2.0, grip - ht)))
            });
            prisms.push(Prism {
                zhalf: hd / 2.0,
                zoff: zo,
                ..steel("head", model::one_loop(model::rect_loop(o.x - hd / 2.0, grip - ht, o.x + hd / 2.0, grip)))
            });
        }
        anchors.push(NamedAnchor { name: format!("bolt_{}", k + 1), p: o });
    }
    let seat = if bp.counterbore { grip - if bp.washer { WASHER_T } else { 0.0 } - 1.5 * d } else { grip };
    anchors.push(NamedAnchor { name: "nut_bearing".to_string(), p: V2::init(0.0, seat) });
    let bx = box_of_prisms(&prisms);
    let n = bp.count;
    let row =
        if n > 1 { format!(", ({n}) @ {} along {}", fmt_ft_in(bp.spacing), bp.line.tag()) } else { String::new() };
    let what = if bp.kind == BoltKind::WeldStud { "grip" } else { "length" };
    let cb = if bp.counterbore { ", nut in counterbore" } else { "" };
    Built {
        prisms,
        anchors,
        box_: bx,
        nat_z: Some(d),
        info: format!("bolt {} {}\" dia., {what} {}{cb}{row}", bp.kind.tag(), fmt_num(d), fmt_ft_in(grip)),
        ..Built::default()
    }
}
