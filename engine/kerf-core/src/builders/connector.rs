//! connector (SPEC 5, 27.2): its parameters (`Params`, the single source for the parser and the catalog) and its builder.
//! Straps, ties and angles are a thickened polyline (`points`, or flush on a member face with `on`); hangers, post caps,
//! post bases and holdowns are built in `hardware.rs` from the members they connect.

use super::common::{Ctx, drop_duplicate_points, gauge_thickness, parse_point_list, vertex_anchors};
use super::hardware;
use crate::catalog::{self, Hardware, ModelMatch, Series};
use crate::geom;
use crate::model::fmt_num;
use crate::model::{self, Built, Diag, Level, Prism};
use crate::num;
use crate::params::{ParamEnum, param_enum, params};
use crate::pathgeom;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Lay { Edge = "edge", Face = "face" } }
param_enum! { pub enum Side { Left = "left", Right = "right" } }
param_enum! { pub enum Kind { Strap = "strap", Tie = "tie", Angle = "angle", Hanger = "hanger", PostCap = "post_cap", PostBase = "post_base", Holdown = "holdown" } }
param_enum! { pub enum Mount { Face = "face", TopFlange = "top_flange" } }
param_enum! { pub enum Face { Left = "left", Right = "right", Front = "front", Back = "back", Top = "top", Bottom = "bottom" } }

params! {
    pub struct Params {
        model: Str = "" => { def: "null", desc: "manufacturer part for the note and the summary, e.g. MSTA36, H2.5A, LUS210, ITS2.06/11.88, CCQ44, ABU44, HDU5: a table model fills width/gauge/length, a known family (`kerf schema connector` lists them) fills kind, mount and gauge; anything else is W_UNKNOWN_MODEL unless you give gauge (and width for a strap)" },
        kind: OptEnum(Kind) => { def: "strap with points or on, else the model's family (tie or angle with members)", desc: "strap | tie | angle: the polyline below (`points`, or `on` a member face), or tie | angle placed from two `members`; hanger: carried + carrying; post_cap: post + beam; post_base: post (+ standoff); holdown: post + rod. Hanger, cap, base, holdown and member ties are sized from their members (no points, no at)" },
        members: OptRaw => { def: "null", desc: "tie | angle: the two members it joins, e.g. [\"rafter\", \"plate\"] (id, id#k or a list drawn as one): tie wraps the joint (H2.5A, H3, RSP4, LTP4), angle sits in the inside corner (A34, A35)" },
        legs: OptRaw => { def: "tie: half the model length each (2 3/4); angle 1 7/16", desc: "tie | angle with members: [on the first member, on the second] leg lengths" },
        front: OptBool => { def: "null: in front where a section cuts one of its members", desc: "true: hardware draws solid over what it overlaps in sections and elevations (as engineers draw it), false: at its true place, dashed where hidden" },
        points: OptRaw => { def: "required for a strap without on", desc: "polyline [x,y] or Refs of the bearing face (lay edge) or centerline (lay face)" },
        on: OptStr => { def: "null", desc: "\"<member>.face_left|face_right|face_top|face_bottom|face_front|face_back\": the strap lies flush on that face in 3D (outside it, gauge thick, no z arithmetic, no `side`): points snap onto the face (default: the whole face along the member), width centered on the member (left/right/top/bottom) or in-plane (front/back)" },
        lay: Enum(Lay) = Lay::Edge => { desc: "edge: seen edge-on, gauge in-plane growing to `side`, width along Z; face: seen face-on, `width` in-plane centered on the polyline, gauge along Z (with `on` the face sets it)" },
        side: Enum(Side) = Side::Left => { desc: "lay edge: left of the polyline direction (left of a left-to-right line = up) or right (with `on` the face sets it)" },
        gauge: OptNum => { def: "18 (or the model's)", desc: "10 .1345, 11 .1196, 12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359" },
        width: OptLenPos => { def: "1.25 (straps); hanger: the carried member; cap/base: the post; holdown 2.5", desc: "strap: extent along Z (lay edge) or in-plane (lay face); hanger: inside seat width; holdown: body width along the face" },
        fasteners: Str = "" => { def: "null", desc: "text for notes, e.g. \"(10) 10d EA. END\"" },
        mount: OptEnum(Mount) => { def: "face (TF models: top_flange)", desc: "hanger: face (back flanges nailed to the carrying member's face) or top_flange (flange over its top)" },
        carried: OptRaw => { def: "required for hanger", desc: "hanger: the joist/beam it carries (`id`, `id#k`, or a list of ids drawn as one member, e.g. an I-joist's flanges and web); seat under its end, sides on its faces" },
        carrying: OptRaw => { def: "required for hanger", desc: "hanger: the member (id or list) whose face the carried member ends at" },
        post: OptRaw => { def: "required for post_cap, post_base, holdown", desc: "the post (id, or a list for a built-up stud pack) the cap, base or holdown fits" },
        beam: OptRaw => { def: "required for post_cap", desc: "post_cap: the beam (id or list of plies) sitting on the post" },
        rod: OptStr => { def: "null (W_HOLDOWN_ROD)", desc: "holdown: the anchor_bolt or bolt through its seat; the seat reaches the rod and the body centers on it" },
        face: OptEnum(Face) => { def: "holdown: toward the rod; post_base: left; tie/angle: front", desc: "holdown: the post face it is screwed to; post_base: left/right puts the side plates on the x faces, front/back on the z faces; tie/angle with members: front/back on the z faces (seen face-on), left/right (top/bottom for side-by-side members) on that side of the joint (seen edge-on; `z` then moves it along the members, E_PARAM on front/back)" },
        height: OptLenPos => { def: "hanger 0.85 x carried depth; cap 1/2 beam depth (max 7); base 3 1/2; holdown 10", desc: "hanger: side height; post_cap: side plates up the beam; post_base: side plates up the post; holdown: body height" },
        seat: OptLenPos => { def: "hanger 2, holdown 3/8", desc: "hanger: seat length under the carried member; holdown: seat thickness" },
        standoff: OptLen => { def: "1", desc: "post_base: post bottom above the base plate bottom (0: post on the plate)" },
        seat_height: OptLen => { def: "0", desc: "holdown: seat bottom above the post bottom" },
    }
}

/// What `model` says about the connector.
pub struct ModelInfo {
    pub name: String,
    pub table: Option<&'static Hardware>,
    pub series: Option<&'static Series>,
}

impl ModelInfo {
    fn kind(&self) -> Option<Kind> {
        if let Some(s) = self.series {
            return Kind::from_tag(s.kind);
        }
        self.table.map(|h| if h.kind.contains("hanger") { Kind::Hanger } else { Kind::Strap })
    }

    pub fn gauge(&self) -> Option<f64> {
        self.table.map(|h| f64::from(h.gauge)).or(self.series.map(|s| f64::from(s.gauge)))
    }

    pub fn strap_width(&self) -> Option<f64> {
        self.table.map(|h| h.width).or(self.series.map(|s| s.width).filter(|w| *w > 0.0))
    }

    pub fn mount(&self) -> Mount {
        let tf = self.name.to_ascii_uppercase().ends_with("TF");
        if tf || self.series.is_some_and(|s| s.mount == "top_flange") { Mount::TopFlange } else { Mount::Face }
    }

    pub fn desc(&self) -> &'static str {
        self.table.map(|h| h.kind).or(self.series.map(|s| s.desc)).unwrap_or("")
    }
}

/// The resolved gauge thickness, or None (reported).
pub fn thickness(ctx: &mut Ctx, gauge_n: f64) -> Option<(u32, f64)> {
    let gauge_i = num::to_int::<u32>(crate::math::round(gauge_n)).unwrap_or(0);
    let Some(t) = gauge_thickness(gauge_i) else {
        ctx.p.fail(
            "gauge",
            format!("gauge {} is not in the table; use 10, 11, 12, 14, 16, 18, 20, 22, 24, 26 or 28", fmt_num(gauge_n)),
        );
        return None;
    };
    Some((gauge_i, t))
}

fn unknown_model(ctx: &Ctx, mi: &ModelInfo, strap: bool) -> Diag {
    let id = ctx.p.id;
    let names = catalog::model_names();
    let near = model::nearest(&mi.name, &names).map_or(String::new(), |n| format!(" Nearest known: {n}."));
    let need = if strap { "\"gauge\" and \"width\"" } else { "\"gauge\"" };
    Diag {
        level: Level::Warning,
        code: "W_UNKNOWN_MODEL",
        id: Some(id.to_string()),
        path: Some(format!("components/{id}/model")),
        message: format!(
            "connector '{id}': model \"{}\" is not in the hardware table nor a known family, so its gauge{} defaulted (18 ga{}).{near}",
            mi.name,
            if strap { " and width" } else { "" },
            if strap { ", 1 1/4\" wide" } else { "" }
        ),
        fix: Some(format!(
            "if it is a typo, correct \"model\"; else give {need} from the manufacturer's catalog (the note text carries the model), which silences this warning"
        )),
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let cp = ctx.p.parse_all::<Params>();
    let name = cp.model.clone();
    let mi = match catalog::resolve_model(&name) {
        _ if name.is_empty() => ModelInfo { name, table: None, series: None },
        ModelMatch::Exact(h) => ModelInfo { name, table: Some(h), series: None },
        ModelMatch::Series(s) => ModelInfo { name, table: None, series: Some(s) },
        ModelMatch::Unknown => ModelInfo { name, table: None, series: None },
    };
    let polyline = cp.points.is_some() || cp.on.is_some();
    let kind = cp.kind.unwrap_or_else(|| {
        if polyline {
            Kind::Strap
        } else if cp.members.is_some() {
            if mi.desc().contains("angle") { Kind::Angle } else { Kind::Tie }
        } else {
            mi.kind().unwrap_or(Kind::Strap)
        }
    });
    if !ctx.p.ok {
        return None;
    }
    let strap = matches!(kind, Kind::Strap | Kind::Tie | Kind::Angle);
    let mut warns = Vec::new();
    if !mi.name.is_empty()
        && mi.table.is_none()
        && mi.series.is_none()
        && (cp.gauge.is_none() || (strap && cp.width.is_none()))
    {
        warns.push(unknown_model(ctx, &mi, strap));
    }
    let mut built = if strap && cp.members.is_some() {
        hardware::build(ctx, &cp, &mi, kind)?
    } else if strap {
        build_strap(ctx, &cp, &mi, kind)?
    } else {
        hardware::build(ctx, &cp, &mi, kind)?
    };
    built.hw.front = cp.front;
    warns.append(&mut built.hw.warns);
    built.hw.warns = warns;
    Some(built)
}

fn build_strap(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, kind: Kind) -> Option<Built> {
    for key in [
        "mount",
        "carried",
        "carrying",
        "post",
        "beam",
        "rod",
        "face",
        "height",
        "seat",
        "standoff",
        "seat_height",
        "legs",
    ] {
        if ctx.p.has(key) {
            let owner = if key == "legs" {
                "a tie or angle placed from \"members\""
            } else {
                "kind hanger, post_cap, post_base or holdown"
            };
            ctx.p.fail(
                key,
                format!(
                    "'{key}' belongs to {owner}; this connector is kind {}: set \"kind\" or remove '{key}'",
                    kind.tag()
                ),
            );
        }
    }
    let gauge_n = cp.gauge.or(mi.gauge()).unwrap_or(18.0);
    let width = cp.width.or(mi.strap_width()).unwrap_or(1.25);
    if let Some(on) = cp.on.as_deref() {
        if !ctx.p.ok {
            return None;
        }
        let (gauge_i, t) = thickness(ctx, gauge_n)?;
        return hardware::build_on(ctx, cp, mi, on, gauge_i, t, width);
    }
    let Some(pv) = cp.points.as_ref() else {
        ctx.p.fail("points", "connector needs 'points': the strap polyline, e.g. [\"upper_plate@top_right\", \"beam@top_left\"] (Refs or [x, y]), or \"on\": \"<member>.face_left\" for a strap flush on a member face; a hanger, post cap, post base or holdown takes \"kind\" and its members instead".to_string());
        return None;
    };
    if !ctx.p.ok {
        return None;
    }
    let (gauge_i, thickness) = thickness(ctx, gauge_n)?;
    let pts = parse_point_list(ctx, "points", pv, true)?;
    let clean = drop_duplicate_points(&pts);
    if clean.len() < 2 {
        ctx.p.fail("points", format!("a connector needs at least 2 distinct points (got {})", clean.len()));
        return None;
    }
    let edge = cp.lay == Lay::Edge;
    let rib = if edge {
        pathgeom::ribbon(
            &clean,
            if cp.side == Side::Left { thickness } else { 0.0 },
            if cp.side == Side::Right { thickness } else { 0.0 },
        )
    } else {
        pathgeom::ribbon(&clean, width / 2.0, width / 2.0)
    };
    let box_ = geom::loop_box(&rib);
    let anchors = vertex_anchors(&clean, false);
    let prism = Prism {
        material: "steel".to_string(),
        loops: model::one_loop(rib),
        centerline: clean,
        face_tie: !edge,
        ..Prism::default()
    };
    Some(Built {
        prisms: vec![prism],
        anchors,
        box_,
        nat_z: Some(if edge { width } else { thickness }),
        points_mode: true,
        info: strap_info(mi, gauge_i, width, cp.lay.tag(), None),
        ..Built::default()
    })
}

pub fn strap_info(mi: &ModelInfo, gauge_i: u32, width: f64, lay: &str, on: Option<&str>) -> String {
    let desc = mi.desc();
    format!(
        "connector {}{}{}{} ga x {} lay {}{}",
        mi.name,
        if mi.name.is_empty() { "" } else { " " },
        if desc.is_empty() { String::new() } else { format!("{desc} ") },
        gauge_i,
        fmt_ft_in(width),
        lay,
        on.map_or(String::new(), |o| format!(" on {o}"))
    )
}
