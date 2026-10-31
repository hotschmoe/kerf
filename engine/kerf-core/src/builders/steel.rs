//! steel (structural shapes): its parameters (`Params`, the single source for the parser and the catalog) and its builder.
//! Dimensions come from `steel_shapes` (AISC subset). The cross-section is built in its own (u, v) plane, u across, v up,
//! then laid out for the orientation: section (u, v) = (x, y) extruded along z; elevation (length, v) in-plane with u along z;
//! plan (length, u) in-plane with v along z. Members seen lengthwise are stacks of rectangular slabs (flanges, web, walls,
//! legs) at their own z, so iso and 3D stay true, and draw as elevations even when the section plane passes through them.

use super::common::{Ctx, fmt_ft_in, length_or_until, material_ok, oriented_ccw};
use super::steel_shapes::{self, Dims, Shape};
use crate::geom::{Box, Pt, V2};
use crate::model::{self, Built, Kind, NamedAnchor, Prism};
use crate::params::{ParamEnum, param_enum, params};
use crate::pen::Pen;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Orient { Section = "section", Elevation = "elevation", Plan = "plan" } }
param_enum! { pub enum Run { X = "x", Y = "y" } }
param_enum! { pub enum LongLeg { Vertical = "vertical", Horizontal = "horizontal" } }
param_enum! { pub enum Outstand { Back = "back", Front = "front" } }

params! {
    pub struct Params {
        shape: Str => { desc: "AISC designation from the embedded table: W8x18, HSS4x4x1/4 (rect, nominal wall), HSS4.000x0.250 (round), PIPE3STD / PIPE3XS, L4x3x3/8, C8x11.5, MC8x18.7, PL1/2x8 (plate thickness x width, any size). HSS and pipe walls draw at the design thickness 0.93 x nominal" },
        orient: Enum(Orient) = Orient::Section => { desc: "section: cut end-on, the member runs along z (true flange/web/wall thicknesses, solid steel fill when cut); elevation: seen from the side along its length (depth in-plane: a W shows its flanges as lines, an HSS its walls as hidden lines); plan: seen from above along its length (flange width in-plane, web as hidden lines). Elevation and plan never fill, even where the section plane passes through them" },
        run: Enum(Run) = Run::X => { desc: "elevation/plan: in-plane direction of the length, x (beam) or y (column, post). Run y puts the cross-section's top (elevation) or left (plan) toward +x" },
        length: OptRaw => { def: "elevation/plan: required", desc: "member length (or `until`). Section: optional; the member is then that long along z, centered on the first section view's cut_z (default: spans the doc run)" },
        until: OptRaw => { desc: "elevation/plan alternative to length: a Ref; the member grows from its placement anchor along its run until its far end reaches the Ref (as lumber `until`)" },
        long_leg: Enum(LongLeg) = LongLeg::Vertical => { desc: "angles: which leg is vertical in the cross-section: vertical (LLV) or horizontal (LLH). The vertical leg is on the left (heel bottom-left); mirror: true puts it on the right" },
        outstand: Enum(Outstand) = Outstand::Back => { desc: "elevation of angles and channels: the horizontal leg / the flanges point back (away from the viewer, -z: you see the vertical leg or web face-on, the rest as hidden lines) or front (+z)" },
        fillets: Bool = true => { desc: "draw the root fillets (W k-region, angle and channel roots) and HSS corner radii (outside 2t, inside t) in section" },
        grade: Str = "" => { def: "null", desc: "free text for notes and the summary, e.g. \"A992\", \"A500 GR. C\", \"A36\", \"A53 GR. B\"" },
    }
}

const Q: f64 = 0.414_213_562_373_095_1;

/// A rectangular piece of the cross-section (u0..u1 across, v0..v1 up) for lengthwise layouts.
struct Slab {
    part: &'static str,
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
}

fn slab(part: &'static str, u0: f64, u1: f64, v0: f64, v1: f64) -> Slab {
    Slab { part, u0, u1, v0, v1 }
}

/// The cross-section of a shape in (u, v): width `w` along u, height `h` along v.
struct Geo {
    w: f64,
    h: f64,
    /// Section pieces: (part, loop). Closed shapes are split in two halves (simple loops for the mesh).
    pieces: Vec<(&'static str, Vec<Pt>)>,
    elev: Vec<Slab>,
    plan: Vec<Slab>,
    elev_hidden: Vec<f64>,
    plan_hidden: Vec<f64>,
    anchors: Vec<(&'static str, f64, f64)>,
    elev_levels: Vec<(&'static str, f64)>,
    plan_levels: Vec<(&'static str, f64)>,
}

fn p(x: f64, y: f64) -> Pt {
    Pt::new(x, y)
}

fn pb(x: f64, y: f64, b: f64) -> Pt {
    Pt { x, y, b }
}

/// The loop reflected about u = w/2, counter-clockwise again.
fn mirror_u(lp: &[Pt], w: f64) -> Vec<Pt> {
    oriented_ccw(lp.iter().map(|q| Pt { x: w - q.x, y: q.y, b: -q.b }).collect())
}

fn w_geo(d: f64, bf: f64, tw: f64, tf: f64, k: f64, fillets: bool) -> Geo {
    let xl = (bf - tw) / 2.0;
    let xr = (bf + tw) / 2.0;
    let r = if fillets { (k - tf).clamp(0.0, 0.9 * xl).min(0.45 * (d - 2.0 * tf)) } else { 0.0 };
    let lp = if r > 1e-9 {
        vec![
            p(0.0, 0.0),
            p(bf, 0.0),
            p(bf, tf),
            pb(xr + r, tf, -Q),
            p(xr, tf + r),
            pb(xr, d - tf - r, -Q),
            p(xr + r, d - tf),
            p(bf, d - tf),
            p(bf, d),
            p(0.0, d),
            p(0.0, d - tf),
            pb(xl - r, d - tf, -Q),
            p(xl, d - tf - r),
            pb(xl, tf + r, -Q),
            p(xl - r, tf),
            p(0.0, tf),
        ]
    } else {
        vec![
            p(0.0, 0.0),
            p(bf, 0.0),
            p(bf, tf),
            p(xr, tf),
            p(xr, d - tf),
            p(bf, d - tf),
            p(bf, d),
            p(0.0, d),
            p(0.0, d - tf),
            p(xl, d - tf),
            p(xl, tf),
            p(0.0, tf),
        ]
    };
    let flanges_and_web = || {
        vec![
            slab("top_flange", 0.0, bf, d - tf, d),
            slab("web", xl, xr, tf, d - tf),
            slab("bottom_flange", 0.0, bf, 0.0, tf),
        ]
    };
    Geo {
        w: bf,
        h: d,
        pieces: vec![("", lp)],
        elev: flanges_and_web(),
        plan: flanges_and_web(),
        elev_hidden: Vec::new(),
        plan_hidden: vec![xl, xr],
        anchors: vec![
            ("top_flange", bf / 2.0, d),
            ("bottom_flange", bf / 2.0, 0.0),
            ("web_center", bf / 2.0, d / 2.0),
            ("web_left", xl, d / 2.0),
            ("web_right", xr, d / 2.0),
            ("top_flange_tip_left", 0.0, d - tf),
            ("top_flange_tip_right", bf, d - tf),
            ("bottom_flange_tip_left", 0.0, tf),
            ("bottom_flange_tip_right", bf, tf),
        ],
        elev_levels: vec![
            ("top_flange", d),
            ("top_flange_bottom", d - tf),
            ("web_center", d / 2.0),
            ("bottom_flange_top", tf),
            ("bottom_flange", 0.0),
        ],
        plan_levels: vec![
            ("flange_tip_left", 0.0),
            ("web_left", xl),
            ("web_center", bf / 2.0),
            ("web_right", xr),
            ("flange_tip_right", bf),
        ],
    }
}

fn rect_geo(h: f64, b: f64, t: f64, fillets: bool) -> Geo {
    let (ro, ri) =
        if fillets { ((2.0 * t).min(0.45 * b.min(h)), t.min(0.45 * (b.min(h) - 2.0 * t))) } else { (0.0, 0.0) };
    let m = b / 2.0;
    let mut left = vec![p(m, h)];
    if ro > 1e-9 {
        left.extend([pb(ro, h, Q), pb(0.0, h - ro, 0.0), pb(0.0, ro, Q), p(ro, 0.0)]);
    } else {
        left.extend([p(0.0, h), p(0.0, 0.0)]);
    }
    left.extend([p(m, 0.0), p(m, t)]);
    if ri > 1e-9 {
        left.extend([pb(t + ri, t, -Q), p(t, t + ri), pb(t, h - t - ri, -Q), p(t + ri, h - t)]);
    } else {
        left.extend([p(t, t), p(t, h - t)]);
    }
    left.push(p(m, h - t));
    let right = mirror_u(&left, b);
    Geo {
        w: b,
        h,
        pieces: vec![("wall", left), ("wall", right)],
        elev: vec![
            slab("wall", 0.0, t, 0.0, h),
            slab("wall", b - t, b, 0.0, h),
            slab("wall", t, b - t, h - t, h),
            slab("wall", t, b - t, 0.0, t),
        ],
        plan: vec![
            slab("wall", 0.0, b, h - t, h),
            slab("wall", 0.0, b, 0.0, t),
            slab("wall", 0.0, t, t, h - t),
            slab("wall", b - t, b, t, h - t),
        ],
        elev_hidden: vec![t, h - t],
        plan_hidden: vec![t, b - t],
        anchors: vec![
            ("inner_top", m, h - t),
            ("inner_bottom", m, t),
            ("inner_left", t, h / 2.0),
            ("inner_right", b - t, h / 2.0),
        ],
        elev_levels: vec![("inner_top", h - t), ("center_line", h / 2.0), ("inner_bottom", t)],
        plan_levels: vec![("inner_left", t), ("center_line", m), ("inner_right", b - t)],
    }
}

fn round_geo(od: f64, t: f64) -> Geo {
    let r = od / 2.0;
    let left = vec![pb(r, od, 1.0), p(r, 0.0), pb(r, t, -1.0), p(r, od - t)];
    let right = mirror_u(&left, od);
    Geo {
        w: od,
        h: od,
        pieces: vec![("wall", left), ("wall", right)],
        elev: vec![slab("tube", 0.0, od, 0.0, od)],
        plan: vec![slab("tube", 0.0, od, 0.0, od)],
        elev_hidden: vec![t, od - t],
        plan_hidden: vec![t, od - t],
        anchors: vec![
            ("inner_top", r, od - t),
            ("inner_bottom", r, t),
            ("inner_left", t, r),
            ("inner_right", od - t, r),
        ],
        elev_levels: vec![("inner_top", od - t), ("center_line", r), ("inner_bottom", t)],
        plan_levels: vec![("inner_left", t), ("center_line", r), ("inner_right", od - t)],
    }
}

fn angle_geo(a: f64, b: f64, t: f64, r: f64, fillets: bool, outstand: Outstand) -> Geo {
    let r = if fillets { r.min(0.45 * (a - t)).min(0.45 * (b - t)) } else { 0.0 };
    let mut lp = vec![p(0.0, 0.0), p(b, 0.0), p(b, t)];
    if r > 1e-9 {
        lp.extend([pb(t + r, t, -Q), p(t, t + r)]);
    } else {
        lp.push(p(t, t));
    }
    lp.extend([p(t, a), p(0.0, a)]);
    let legs = || vec![slab("vertical_leg", 0.0, t, 0.0, a), slab("horizontal_leg", t, b, 0.0, t)];
    Geo {
        w: b,
        h: a,
        pieces: vec![("", lp)],
        elev: legs(),
        plan: legs(),
        elev_hidden: if outstand == Outstand::Back { vec![t] } else { Vec::new() },
        plan_hidden: Vec::new(),
        anchors: vec![
            ("heel", 0.0, 0.0),
            ("heel_inside", t, t),
            ("toe_vertical", t / 2.0, a),
            ("toe_horizontal", b, t / 2.0),
            ("vertical_leg_face", t, (a + t) / 2.0),
            ("horizontal_leg_top", (b + t) / 2.0, t),
        ],
        elev_levels: vec![("toe_vertical", a), ("horizontal_leg_top", t), ("heel", 0.0)],
        plan_levels: vec![("heel", 0.0), ("vertical_leg_face", t), ("toe_horizontal", b)],
    }
}

fn channel_geo(d: f64, bf: f64, tw: f64, tf: f64, r: f64, fillets: bool, outstand: Outstand) -> Geo {
    let r = if fillets { r } else { 0.0 };
    let mut lp = vec![p(0.0, 0.0), p(bf, 0.0), p(bf, tf)];
    if r > 1e-9 {
        lp.extend([pb(tw + r, tf, -Q), p(tw, tf + r), pb(tw, d - tf - r, -Q), p(tw + r, d - tf)]);
    } else {
        lp.extend([p(tw, tf), p(tw, d - tf)]);
    }
    lp.extend([p(bf, d - tf), p(bf, d), p(0.0, d)]);
    Geo {
        w: bf,
        h: d,
        pieces: vec![("", lp)],
        elev: vec![
            slab("web", 0.0, tw, 0.0, d),
            slab("top_flange", tw, bf, d - tf, d),
            slab("bottom_flange", tw, bf, 0.0, tf),
        ],
        plan: vec![
            slab("top_flange", 0.0, bf, d - tf, d),
            slab("web", 0.0, tw, tf, d - tf),
            slab("bottom_flange", 0.0, bf, 0.0, tf),
        ],
        elev_hidden: if outstand == Outstand::Back { vec![tf, d - tf] } else { Vec::new() },
        plan_hidden: vec![tw],
        anchors: vec![
            ("web_back", 0.0, d / 2.0),
            ("web_face", tw, d / 2.0),
            ("top_flange", bf / 2.0, d),
            ("bottom_flange", bf / 2.0, 0.0),
            ("top_flange_tip", bf, d - tf),
            ("bottom_flange_tip", bf, tf),
        ],
        elev_levels: vec![
            ("top_flange", d),
            ("top_flange_bottom", d - tf),
            ("web_center", d / 2.0),
            ("bottom_flange_top", tf),
            ("bottom_flange", 0.0),
        ],
        plan_levels: vec![("web_back", 0.0), ("web_face", tw), ("flange_tip", bf)],
    }
}

fn plate_geo(t: f64, w: f64) -> Geo {
    Geo {
        w,
        h: t,
        pieces: vec![("", model::rect_loop(0.0, 0.0, w, t))],
        elev: vec![slab("plate", 0.0, w, 0.0, t)],
        plan: vec![slab("plate", 0.0, w, 0.0, t)],
        elev_hidden: Vec::new(),
        plan_hidden: Vec::new(),
        anchors: vec![
            ("edge_left", 0.0, t / 2.0),
            ("edge_right", w, t / 2.0),
            ("face_top", w / 2.0, t),
            ("face_bottom", w / 2.0, 0.0),
        ],
        elev_levels: vec![("face_top", t), ("face_bottom", 0.0)],
        plan_levels: vec![("edge_left", 0.0), ("edge_right", w)],
    }
}

fn geometry(shape: &Shape, sp: &Params) -> Geo {
    match shape.dims {
        Dims::W { d, bf, tw, tf, k } => w_geo(d, bf, tw, tf, k, sp.fillets),
        Dims::Rect { h, b, t } => rect_geo(h, b, t, sp.fillets),
        Dims::Round { od, t } => round_geo(od, t),
        Dims::Angle { vert, horiz, t, r } => {
            let (a, b) = if sp.long_leg == LongLeg::Vertical { (vert, horiz) } else { (horiz, vert) };
            angle_geo(a, b, t, r, sp.fillets, sp.outstand)
        }
        Dims::Channel { d, bf, tw, tf, r } => channel_geo(d, bf, tw, tf, r, sp.fillets, sp.outstand),
        Dims::Plate { t, w } => plate_geo(t, w),
    }
}

fn steel_prism(part: &str, lp: Vec<Pt>) -> Prism {
    Prism { part: part.to_string(), material: "steel".to_string(), loops: model::one_loop(lp), ..Prism::default() }
}

fn hidden_line(a: V2, b: V2, zhalf: f64) -> Prism {
    Prism {
        part: "hidden".to_string(),
        material: "steel".to_string(),
        kind: Kind::Ghost,
        pen: Some(Pen::Hidden),
        line_pts: vec![Pt::at(a, 0.0), Pt::at(b, 0.0)],
        embedded: true,
        zhalf,
        ..Prism::default()
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let sp = ctx.p.parse::<Params>()?;
    let shape = match steel_shapes::lookup(&sp.shape) {
        Ok(s) => s,
        Err(m) => {
            ctx.p.fail("shape", m);
            return None;
        }
    };
    if !material_ok(ctx, "shape", "steel") {
        return None;
    }
    let geo = geometry(&shape, &sp);
    let grade = if sp.grade.is_empty() { String::new() } else { format!(" {}", sp.grade) };
    if sp.orient == Orient::Section {
        if ctx.p.has("until") {
            ctx.p.fail("until", "'until' only applies to orient elevation or plan (a section member runs along z: give 'length' or set 'z')".into());
            return None;
        }
        let length = if ctx.p.has("length") {
            Some(ctx.p.len_pos("length", None, "length of the member along z, e.g. 24")?)
        } else {
            None
        };
        let prisms = geo.pieces.iter().map(|(part, lp)| steel_prism(part, lp.clone())).collect();
        let anchors =
            geo.anchors.iter().map(|&(n, u, v)| NamedAnchor { name: n.to_string(), p: V2::init(u, v) }).collect();
        let len_txt = length.map_or(String::new(), |l| format!(" L={}", fmt_ft_in(l)));
        return Some(Built {
            prisms,
            anchors,
            box_: Box { x0: 0.0, y0: 0.0, x1: geo.w, y1: geo.h },
            nat_z: length,
            info: format!("steel {}{grade} section{len_txt}", shape.name),
            ..Built::default()
        });
    }
    let mut until_note = String::new();
    let len = length_or_until(
        ctx,
        sp.run.tag(),
        "a steel member in elevation or plan needs its length, e.g. \"length\": 120, or \"until\": \"other@anchor\"",
        &mut until_note,
    )?;
    if !ctx.p.ok {
        return None;
    }
    let elevation = sp.orient == Orient::Elevation;
    let along_x = sp.run == Run::X;
    let (slabs, hidden, levels) = if elevation {
        (&geo.elev, &geo.elev_hidden, &geo.elev_levels)
    } else {
        (&geo.plan, &geo.plan_hidden, &geo.plan_levels)
    };
    let across = if elevation { geo.h } else { geo.w };
    let depth = if elevation { geo.w } else { geo.h };
    let at = |s: f64, c: f64| if along_x { V2::init(s, c) } else { V2::init(c, s) };
    let sz = if sp.outstand == Outstand::Back { -1.0 } else { 1.0 };
    let mut prisms: Vec<Prism> = Vec::new();
    for sl in slabs {
        let (c0, c1, z0, z1) = if elevation { (sl.v0, sl.v1, sl.u0, sl.u1) } else { (sl.u0, sl.u1, sl.v0, sl.v1) };
        let a = at(0.0, c0);
        let b = at(len, c1);
        let zmid = (z0 + z1) / 2.0 - depth / 2.0;
        prisms.push(Prism {
            zhalf: (z1 - z0) / 2.0,
            zoff: if elevation { sz * zmid } else { zmid },
            lengthwise: true,
            ..steel_prism(sl.part, model::rect_loop(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)))
        });
    }
    for &c in hidden {
        prisms.push(hidden_line(at(0.0, c), at(len, c), depth / 2.0));
    }
    let mut anchors: Vec<NamedAnchor> = Vec::new();
    for &(n, c) in levels {
        anchors.push(NamedAnchor { name: n.to_string(), p: at(len / 2.0, c) });
        anchors.push(NamedAnchor { name: format!("{n}_start"), p: at(0.0, c) });
        anchors.push(NamedAnchor { name: format!("{n}_end"), p: at(len, c) });
    }
    let far = at(len, across);
    Some(Built {
        prisms,
        anchors,
        box_: Box { x0: 0.0, y0: 0.0, x1: far.x, y1: far.y },
        nat_z: Some(depth),
        info: format!(
            "steel {}{grade} {} run {} L={}{until_note}",
            shape.name,
            sp.orient.tag(),
            sp.run.tag(),
            fmt_ft_in(len)
        ),
        ..Built::default()
    })
}
