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
param_enum! { pub enum BoltKind { Bolt = "bolt", WeldStud = "weld_stud", HeadedStud = "headed_stud", LagScrew = "lag_screw", WoodScrew = "wood_screw", SelfDrillingScrew = "self_drilling_screw" } }
param_enum! { pub enum Hole { Standard = "standard", Oversize = "oversize", ShortSlot = "short_slot", LongSlot = "long_slot" } }
param_enum! { pub enum SlotDir { X = "x", Y = "y" } }

params! {
    pub struct Params {
        kind: Enum(BoltKind) = BoltKind::Bolt => { desc: "bolt: head, grip, washer and nut; lag_screw: hex head and washer on the placement point, plain shank then threads and a point, `length` long, no nut (steel plate or wood side member to wood); self_drilling_screw: metal sheet screw (threads and point, no nut, no AISC bolt edge/spacing or wood penetration check; product design by manufacturer/AISI); wood_screw: structural wood screw with a hex washer head (SDS, SDWS, #14: put the model in grade), threads and point, no nut (built-up LVL plies, ledgers); weld_stud: threaded stud welded at the placement point (no head), grip UP through the plies, washer and nut on top (wood nailers on steel); headed_stud: shear stud welded at the placement point, `grip` = its length after welding, head 1.6d x 0.5d (embed plates, composite decks). Studs are side only: rotate the component to point them" },
        diameter: LenPos = 0.75 => { desc: "bolt diameter: 0.5, 0.625, 0.75 (typical), 0.875, 1" },
        grade: Str = "A325" => { desc: "free text for notes and the summary: A307, A325, A490, F1554 ..." },
        grip: LenPos = 1.0 => { desc: "thickness of the plies clamped between head and nut (sum of plate and flange thicknesses); the head bears at the placement point" },
        orient: Enum(Orient) = Orient::Side => { desc: "side: the axis lies in the drawing plane, head up (+y) at the placement point, grip downward (bolts through a base plate or a flange seen in section; rotate 180 for head down); end: the axis runs along z, seen end-on as the hex head, washer and shank circle (bolts through a shear tab or web seen face-on), head toward the viewer" },
        count: Int(u16, 1, 50) = 1 => { desc: "bolts in the row (a gauge line)" },
        spacing: LenPos = 3.0 => { desc: "center-to-center spacing along `line` (AISC minimum 2 2/3 d, 3d preferred; W_BOLT_SPACING)" },
        line: Enum(Line) = Line::X => { desc: "direction of the row from bolt 1: x or z for side bolts, x or y for end bolts" },
        washer: Bool = true => { desc: "draw the hardened washer under the nut" },
        length: OptLenPos => { def: "screws: required", desc: "lag_screw / wood_screw / self_drilling_screw: length under the head. The point lands at anchor tip; penetration into the main member = length - grip" },
        thread: OptLenPos => { def: "lag: length/2 + 1/2 (6 max); wood_screw: 2/3 length", desc: "lag_screw / wood_screw / self_drilling_screw: threaded length up from the point; the rest is plain shank (keep the threads out of the side member)" },
        hole: Enum(Hole) = Hole::Standard => { desc: "hole in the ply under the head (AISC 360 Table J3.3): standard (d + 1/16), oversize, short_slot or long_slot (slip connections at joist seats, ledger angles, deflection clips). End-on bolts draw the oversize hole or slot outline around the bolt (a standard hole is hidden by the head); side-on bolts carry it in the summary and notes only" },
        slot_length: OptLenPos => { def: "table: short d + 5/16, long 2.5 d", desc: "overall length of a slot when the detail gives it (e.g. 1.5 or 2.5), with hole short_slot or long_slot" },
        slot_dir: Enum(SlotDir) = SlotDir::X => { desc: "end-on bolts: direction of the slot's long axis in the drawing, x (horizontal slot) or y (vertical slot)" },
        counterbore: Bool = false => { desc: "weld_stud: washer and nut sit in a bored pocket in the top ply, the stud end flush with the top of the grip, so nothing projects above the plies (sheathing or joists bear flat); draws the pocket (washer OD + 1/8\", part `counterbore`). Needs grip >= washer + nut + d/2" },
    }
}

pub const WASHER_T: f64 = 0.15625;

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
    let screw_kind = matches!(bp.kind, BoltKind::LagScrew | BoltKind::WoodScrew | BoltKind::SelfDrillingScrew);
    let slotted = matches!(bp.hole, Hole::ShortSlot | Hole::LongSlot);
    if bp.slot_length.is_some() && !slotted {
        ctx.p.fail(
            "slot_length",
            "slot_length needs a slotted hole: add \"hole\": \"long_slot\" (or \"short_slot\")".into(),
        );
        return None;
    }
    if let Some(sl) = bp.slot_length
        && sl <= hole_size(d, Hole::Standard).0
    {
        ctx.p.fail("slot_length", format!("slot_length {} is not longer than the slot's width {} (bolt diameter + 1/16): give the overall slot length, e.g. {}", fmt_ft_in(sl), fmt_ft_in(hole_size(d, Hole::Standard).0), fmt_ft_in(2.5 * d)));
        return None;
    }
    if !screw_kind && (ctx.p.has("length") || ctx.p.has("thread")) {
        let k = if ctx.p.has("length") { "length" } else { "thread" };
        ctx.p.fail(
            k,
            format!(
                "'{k}' is for kind lag_screw, wood_screw or self_drilling_screw; a {} is sized by \"grip\" (the plies it clamps{})",
                bp.kind.tag(),
                if bp.kind == BoltKind::HeadedStud { ": the stud's length" } else { "" }
            ),
        );
        return None;
    }
    if screw_kind {
        if bp.counterbore {
            ctx.p.fail("counterbore", "counterbore is for a weld_stud (its nut and washer recessed into the top ply); remove it or set kind \"weld_stud\"".into());
            return None;
        }
        let Some(len) = bp.length else {
            ctx.p.fail(
                "length",
                format!(
                    "a {} needs \"length\": its length under the head, e.g. \"length\": {}",
                    bp.kind.tag(),
                    if bp.kind == BoltKind::LagScrew { "6" } else { "3.5" }
                ),
            );
            return None;
        };
        let thread = bp.thread.unwrap_or(if bp.kind == BoltKind::LagScrew {
            (len / 2.0 + 0.5).min(6.0).min(len)
        } else {
            len * 2.0 / 3.0
        });
        if thread > len + 1e-9 {
            ctx.p.fail(
                "thread",
                format!(
                    "thread {} is longer than the screw ({}): give the threaded length up from the point",
                    fmt_ft_in(thread),
                    fmt_ft_in(len)
                ),
            );
            return None;
        }
        if len < 2.0 * d {
            ctx.p.fail(
                "length",
                format!(
                    "length {} is under 2 diameters of a {} screw: give the length under the head",
                    fmt_ft_in(len),
                    fmt_ft_in(d)
                ),
            );
            return None;
        }
        return Some(screw(&bp, len, thread));
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
            if let Some(h) = hole_outline(&bp, o) {
                prisms.push(h);
            }
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
        info: format!(
            "bolt {}\" dia. {}, grip {}, {}{row}{}",
            fmt_num(d),
            bp.grade,
            fmt_ft_in(grip),
            bp.orient.tag(),
            hole_text(&bp)
        ),
        ..Built::default()
    })
}

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
    let cb = if bp.counterbore { ", nut in counterbore".to_string() } else { hole_text(bp) };
    Built {
        prisms,
        anchors,
        box_: bx,
        nat_z: Some(d),
        info: format!("bolt {} {}\" dia., {what} {}{cb}{row}", bp.kind.tag(), fmt_num(d), fmt_ft_in(grip)),
        ..Built::default()
    }
}

/// (width, length) of the hole for a bolt of diameter d (AISC 360-16 Table J3.3).
pub fn hole_size(d: f64, hole: Hole) -> (f64, f64) {
    let std = if d < 1.0 - 1e-9 { d + 0.0625 } else { d + 0.125 };
    match hole {
        Hole::Standard => (std, std),
        Hole::Oversize => {
            let o = if d <= 0.5 + 1e-9 {
                d + 0.125
            } else if d <= 0.875 + 1e-9 {
                d + 0.1875
            } else if d <= 1.0 + 1e-9 {
                d + 0.25
            } else {
                d + 0.3125
            };
            (o, o)
        }
        Hole::ShortSlot => (
            std,
            std + if d <= 0.5 + 1e-9 {
                0.125
            } else if d <= 1.0 + 1e-9 {
                0.1875
            } else {
                0.25
            },
        ),
        Hole::LongSlot => (std, 2.5 * d),
    }
}

fn hole_dims(bp: &Params) -> (f64, f64) {
    let (w, l) = hole_size(bp.diameter, bp.hole);
    (w, bp.slot_length.unwrap_or(l))
}

fn hole_text(bp: &Params) -> String {
    let (w, l) = hole_dims(bp);
    match bp.hole {
        Hole::Standard => String::new(),
        Hole::Oversize => format!(", oversize hole {}", fmt_ft_in(w)),
        Hole::ShortSlot | Hole::LongSlot => format!(
            ", {} {} x {} along {}",
            bp.hole.tag().replace('_', " "),
            fmt_ft_in(w),
            fmt_ft_in(l),
            bp.slot_dir.tag()
        ),
    }
}

fn hole_outline(bp: &Params, o: V2) -> Option<Prism> {
    if bp.hole == Hole::Standard {
        return None;
    }
    let (w, l) = hole_dims(bp);
    let (r, a) = (w / 2.0, (l - w).max(0.0) / 2.0);
    let lp = if a < 1e-9 {
        model::circle_loop(o.x, o.y, r)
    } else if bp.slot_dir == SlotDir::X {
        vec![
            Pt::new(o.x - a, o.y - r),
            Pt { x: o.x + a, y: o.y - r, b: 1.0 },
            Pt::new(o.x + a, o.y + r),
            Pt { x: o.x - a, y: o.y + r, b: 1.0 },
        ]
    } else {
        vec![
            Pt::new(o.x + r, o.y - a),
            Pt { x: o.x + r, y: o.y + a, b: 1.0 },
            Pt::new(o.x - r, o.y + a),
            Pt { x: o.x - r, y: o.y - a, b: 1.0 },
        ]
    };
    Some(Prism {
        part: "hole".to_string(),
        kind: model::Kind::Ghost,
        pen: Some(crate::pen::Pen::Steel),
        ..steel("hole", model::one_loop(lp))
    })
}

fn thread_loop(x: f64, y0: f64, y1: f64, d: f64) -> Vec<Pt> {
    let (rc, rr) = (d / 2.0, 0.37 * d);
    let tip = d.min(0.5 * (y0 - y1));
    let body = y0 - y1 - tip;
    let n = crate::num::to_int_clamped::<usize>(crate::math::floor(body / (d / 3.0)), 1, 400);
    let p = body / n as f64;
    let mut right = vec![V2::init(rr, y0)];
    for i in 0..n {
        right.push(V2::init(rc, y0 - (i as f64 + 0.5) * p));
        right.push(V2::init(rr, y0 - (i as f64 + 1.0) * p));
    }
    let mut lp: Vec<Pt> = right.iter().map(|v| Pt::new(x + v.x, v.y)).collect();
    lp.push(Pt::new(x, y1));
    lp.extend(right.iter().rev().map(|v| Pt::new(x - v.x, v.y)));
    super::common::oriented_ccw(lp)
}

fn screw(bp: &Params, len: f64, thread: f64) -> Built {
    let d = bp.diameter;
    let lag = bp.kind == BoltKind::LagScrew;
    let side = bp.orient == Orient::Side;
    let (wt, wr, hh, f) = if lag {
        (if bp.washer { WASHER_T } else { 0.0 }, d, 0.667 * d, hex_flats(d))
    } else {
        (0.15 * d, 1.1 * d, 0.5 * d, 1.5 * d)
    };
    let plain = len - thread;
    let mut prisms: Vec<Prism> = Vec::new();
    let mut anchors = vec![NamedAnchor { name: "head_bearing".to_string(), p: V2::init(0.0, 0.0) }];
    let offs = offsets(usize::from(bp.count), bp.spacing, bp.line);
    for (k, &(o, zo)) in offs.iter().enumerate() {
        if side {
            if plain > 1e-9 {
                prisms.push(Prism {
                    centerline: vec![Pt::new(o.x, 0.0), Pt::new(o.x, -plain)],
                    sweep_r: d / 2.0,
                    zoff: zo,
                    ..steel("shank", model::one_loop(model::rect_loop(o.x - d / 2.0, -plain, o.x + d / 2.0, 0.0)))
                });
            }
            prisms.push(Prism {
                zhalf: d / 2.0,
                zoff: zo,
                ..steel("thread", model::one_loop(thread_loop(o.x, -plain, -len, d)))
            });
            if wt > 0.0 {
                prisms.push(Prism {
                    zhalf: wr,
                    zoff: zo,
                    ..steel("washer", model::one_loop(model::rect_loop(o.x - wr, 0.0, o.x + wr, wt)))
                });
            }
            prisms.push(Prism {
                zhalf: f / 2.0,
                zoff: zo,
                ..steel("head", model::one_loop(model::rect_loop(o.x - f / 2.0, wt, o.x + f / 2.0, wt + hh)))
            });
        } else {
            let place = |pr: Prism, z0: f64, z1: f64| Prism {
                zoff: (z0 + z1) / 2.0 + bp.grip / 2.0,
                zhalf: (z1 - z0) / 2.0,
                ..pr
            };
            if wt > 0.0 {
                prisms.push(place(steel("washer", model::one_loop(model::circle_loop(o.x, o.y, wr))), 0.0, wt));
            }
            prisms.push(place(steel("head", model::one_loop(hex_loop(o, f))), wt, wt + hh));
            prisms.push(place(steel("shank", model::one_loop(model::circle_loop(o.x, o.y, d / 2.0))), -len, 0.0));
            if let Some(h) = hole_outline(bp, o) {
                prisms.push(h);
            }
        }
        anchors.push(NamedAnchor { name: format!("bolt_{}", k + 1), p: o });
    }
    if side {
        anchors.push(NamedAnchor { name: "nut_bearing".to_string(), p: V2::init(0.0, -bp.grip) });
        anchors.push(NamedAnchor { name: "thread_start".to_string(), p: V2::init(0.0, -plain) });
        anchors.push(NamedAnchor { name: "tip".to_string(), p: V2::init(0.0, -len) });
    }
    let bx = box_of_prisms(&prisms);
    let n = bp.count;
    let row =
        if n > 1 { format!(", ({n}) @ {} along {}", fmt_ft_in(bp.spacing), bp.line.tag()) } else { String::new() };
    let grade = if default_grade(bp) { String::new() } else { format!(" {}", bp.grade) };
    Built {
        prisms,
        anchors,
        box_: bx,
        nat_z: Some(if side { d } else { bp.grip }),
        info: format!(
            "bolt {}{grade} {}\" dia. x {} (thread {}), {}{row}{}",
            bp.kind.tag(),
            fmt_num(d),
            fmt_ft_in(len),
            fmt_ft_in(thread),
            bp.orient.tag(),
            hole_text(bp)
        ),
        ..Built::default()
    }
}

fn default_grade(bp: &Params) -> bool {
    bp.grade == "A325"
}
