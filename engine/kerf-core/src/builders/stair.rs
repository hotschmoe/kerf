//! Stairs (SPEC 32.3): a concrete flight on grade or a cut wood stringer with treads and risers, from rise and tread.

use super::common::{Ctx, fmt_ft_in, material_ok, oriented_ccw, parse_cover, parse_sawn};
use super::rebar::rebar_diameter;
use crate::geom::{self, Box, Pt, V2};
use crate::math;
use crate::model::{self, Built, HostInfo, Level, NamedAnchor, Prism, Zone, fmt_num};
use crate::params::{param_enum, params};
use crate::pathgeom;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Kind { Concrete = "concrete", Wood = "wood" } }

params! {
    pub struct Params {
        kind: Enum(Kind) = Kind::Concrete => { desc: "concrete (a flight cast on grade: steps, waist, base toe, top landing slab) or wood (a cut stringer with treads and risers)" },
        risers: Int(u8, 1, 40) = 4 => { desc: "number of risers; the flight climbs risers x rise and runs (risers - 1) x tread to the top riser" },
        rise: LenPos = 7.0 => { desc: "riser height; W_STAIR_GEOMETRY over 7 3/4 (IRC R311.7.5.1; IBC 1011.5.2 allows 7)" },
        tread: LenPos = 11.0 => { desc: "tread depth, riser face to riser face; W_STAIR_GEOMETRY under 10 (IRC R311.7.5.2; IBC 11)" },
        nosing: Len = 0.0 => { desc: "concrete: how far each riser face rakes back at its foot (0 = plumb risers); wood: tread overhang past the riser face" },
        waist: LenPos = 6.0 => { desc: "concrete: throat thickness, square to the soffit from the inside corner of the steps" },
        toe_width: Len = 10.0 => { desc: "concrete: width of the base toe under the first riser (0 = no toe, the soffit runs out to the front)" },
        toe_depth: Len = 12.0 => { desc: "concrete: depth of the base toe below the lower floor" },
        landing: LenPos = 24.0 => { desc: "concrete: length of the top slab cast with the flight, from the top riser (butt the floor slab to stair@landing_end_top)" },
        landing_thickness: LenPos = 4.0 => { desc: "concrete: thickness of that top slab" },
        bars: Str = "" => { def: "null", desc: "concrete: main bar size, e.g. \"#4\": draws the main bar (part main_bar: down into the toe, along the soffit at cover.bottom, bent level into the landing) and the cross bars on it as dots @ bar_spacing (part cross_bars)" },
        nosing_bars: Str = "" => { def: "null", desc: "concrete: nosing bar size, e.g. \"#4\": one bar along each nosing (part nosing_bars)" },
        bar_spacing: LenPos = 12.0 => { desc: "concrete: spacing of the cross bars along the main bar" },
        material: Str = "concrete" => { desc: "concrete: any style material" },
        size: Str = "2x12" => { desc: "wood: stringer size, sawn nominal" },
        stringers: Int(u8, 1, 8) = 3 => { desc: "wood: number of stringers across width (one is cut by the section)" },
        tread_thickness: LenPos = 1.5 => { desc: "wood: tread board thickness (1 1/2 = 2x, 1 = 5/4)" },
        riser_thickness: Len = 0.75 => { desc: "wood: riser board thickness (0 = open risers)" },
        treated: Bool = false => { desc: "wood: preservative treated stringers (exterior stairs, stringers bearing on concrete)" },
        width: OptLenPos => { def: "wood 36; concrete: the doc run", desc: "width of the flight along z" },
    }
}

fn anchor(name: &str, p: V2) -> NamedAnchor {
    NamedAnchor { name: name.to_string(), p }
}

fn zone(name: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Zone {
    Zone {
        name: name.to_string(),
        loops: model::one_loop(model::rect_loop(x0, y0, x1, y1)),
        box_: Box { x0, y0, x1, y1 },
    }
}

fn geometry_check(ctx: &Ctx, sp: &Params) {
    let id = ctx.p.id;
    let mut bad: Vec<String> = Vec::new();
    if sp.rise > 7.75 + 1e-9 {
        bad.push(format!("rise {} is over 7 3/4\" (IRC R311.7.5.1)", fmt_ft_in(sp.rise)));
    }
    if sp.tread < 10.0 - 1e-9 {
        bad.push(format!("tread {} is under 10\" (IRC R311.7.5.2)", fmt_ft_in(sp.tread)));
    }
    if bad.is_empty() {
        return;
    }
    ctx.scene.diags.add_fix(
        Level::Warning,
        "W_STAIR_GEOMETRY",
        Some(id),
        Some(&format!("components/{id}")),
        format!("stair '{id}': {}", bad.join("; ")),
        format!("use a rise of 7 3/4\" or less and a tread of 10\" or more (commercial, IBC 1011.5.2: 7\" and 11\"), or acknowledge W_STAIR_GEOMETRY on '{id}' where the detail is schematic and the architect sets the geometry"),
    );
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let sp = ctx.p.parse::<Params>()?;
    geometry_check(ctx, &sp);
    match sp.kind {
        Kind::Concrete => concrete(ctx, &sp),
        Kind::Wood => wood(ctx, &sp),
    }
}

fn concrete(ctx: &mut Ctx, sp: &Params) -> Option<Built> {
    for key in ["size", "stringers", "tread_thickness", "riser_thickness", "treated"] {
        if ctx.p.has(key) {
            ctx.p.fail(key, format!("'{key}' belongs to a wood stair: add \"kind\": \"wood\" or remove it"));
            return None;
        }
    }
    let cov = parse_cover(ctx, "cover", model::Cover::default())?;
    if !material_ok(ctx, "material", &sp.material) {
        return None;
    }
    let n = f64::from(sp.risers);
    let (rise, tread, nz, waist) = (sp.rise, sp.tread, sp.nosing, sp.waist);
    if nz < 0.0 || nz > tread / 2.0 {
        ctx.p.fail("nosing", format!("nosing must be 0 to half the tread (got {})", fmt_num(nz)));
        return None;
    }
    let (tw, td) = (sp.toe_width.max(0.0), sp.toe_depth.max(0.0));
    let (ll, lt) = (sp.landing, sp.landing_thickness);
    let theta = math::atan2(rise, tread);
    let (sin, cos) = (math::sin(theta), math::cos(theta));
    let slope = rise / tread;
    let soffit = |x: f64| slope * (x - nz) - waist / cos;
    let top_y = n * rise;
    let x_top = (n - 1.0) * tread;
    let x_end = x_top + ll;
    let under = top_y - lt;
    let x_meet = nz + (under + waist / cos) / slope;
    if x_meet > x_end - 1e-6 {
        ctx.p.fail(
            "landing",
            format!("the soffit meets the underside of the top slab {} past the top riser, beyond the {} landing: lengthen 'landing' to at least {} (or thicken 'landing_thickness')", fmt_ft_in(x_meet - x_top), fmt_ft_in(ll), fmt_ft_in(math::ceil(x_meet - x_top + 1.0))),
        );
        return None;
    }
    let mut pts: Vec<Pt> = Vec::new();
    let has_toe = tw > 1e-9 && td > 1e-9 && soffit(tw) > -td + 1e-6;
    if has_toe {
        pts.push(Pt::new(0.0, -td));
    } else {
        pts.push(Pt::new(0.0, soffit(0.0).min(-1e-3)));
    }
    pts.push(Pt::new(0.0, 0.0));
    let mut anchors = vec![anchor("foot", V2::init(0.0, 0.0))];
    for k in 1..=u32::from(sp.risers) {
        let kf = f64::from(k);
        let foot = V2::init((kf - 1.0) * tread + nz, (kf - 1.0) * rise);
        let nose = V2::init((kf - 1.0) * tread, kf * rise);
        if !(k == 1 && nz < 1e-9) {
            pts.push(Pt::at(foot, 0.0));
        }
        pts.push(Pt::at(nose, 0.0));
        anchors.push(anchor(&format!("nosing_{k}"), nose));
        anchors.push(anchor(&format!("riser_{k}_foot"), foot));
        if kf < n {
            anchors.push(anchor(&format!("tread_{k}_mid"), V2::init((kf - 0.5) * tread, kf * rise)));
        }
    }
    pts.push(Pt::new(x_end, top_y));
    pts.push(Pt::new(x_end, under));
    pts.push(Pt::new(x_meet, under));
    let soffit_low = if has_toe {
        pts.push(Pt::new(tw, soffit(tw)));
        pts.push(Pt::new(tw, -td));
        V2::init(tw, soffit(tw))
    } else {
        V2::init(0.0, soffit(0.0).min(-1e-3))
    };
    let soffit_high = V2::init(x_meet, under);
    let outline = oriented_ccw(super::common::drop_duplicate_points(&pts));
    let box_ = geom::loop_box(&outline);
    let mid_k = f64::from((sp.risers / 2).max(1));
    let throat_top = V2::init(mid_k * tread + nz, mid_k * rise);
    let throat_bottom = throat_top.add(V2::init(sin, -cos).scale(waist));
    anchors.extend([
        anchor("top", V2::init(x_top, top_y)),
        anchor("landing_end_top", V2::init(x_end, top_y)),
        anchor("landing_end_bottom", V2::init(x_end, under)),
        anchor("landing_top_mid", V2::init(x_top + ll / 2.0, top_y)),
        anchor("soffit_top", soffit_high),
        anchor("soffit_bottom", soffit_low),
        anchor("soffit_mid", soffit_low.mid(soffit_high)),
        anchor("throat_top", throat_top),
        anchor("throat_bottom", throat_bottom),
    ]);
    let mut zones = vec![zone("landing", x_meet.min(x_top), under, x_end, top_y)];
    if has_toe {
        anchors.push(anchor("toe_bottom_left", V2::init(0.0, -td)));
        anchors.push(anchor("toe_bottom_right", V2::init(tw, -td)));
        anchors.push(anchor("toe_bottom_center", V2::init(tw / 2.0, -td)));
        zones.push(zone("toe", 0.0, -td, tw, 0.0));
    }
    let mut prisms =
        vec![Prism { material: sp.material.clone(), loops: model::one_loop(outline.clone()), ..Prism::default() }];
    let mut info = format!(
        "stair concrete {} risers {} x {} treads, {} waist{}",
        sp.risers,
        fmt_ft_in(rise),
        fmt_ft_in(tread),
        fmt_ft_in(waist),
        if has_toe { format!(", toe {} x {}", fmt_ft_in(tw), fmt_ft_in(td)) } else { String::new() }
    );
    let bar_d = |ctx: &mut Ctx, key: &str, size: &str| -> Option<Option<f64>> {
        if size.is_empty() {
            return Some(None);
        }
        match rebar_diameter(size) {
            Some(d) => Some(Some(d)),
            None => {
                ctx.p.fail(key, format!("'{key}' must be a bar size #3 to #11, e.g. \"#4\" (got \"{size}\")"));
                None
            }
        }
    };
    let rebar = |part: &str, loops: Vec<Pt>, center: Vec<Pt>, r: f64| Prism {
        part: part.to_string(),
        material: "rebar".to_string(),
        loops: model::one_loop(loops),
        embedded: true,
        centerline: center,
        sweep_r: r,
        zhalf: r,
        ..Prism::default()
    };
    if let Some(d) = bar_d(ctx, "bars", &sp.bars)? {
        let r = d / 2.0;
        let off = cov.cover.bottom + r;
        let perp = V2::init(-sin, cos);
        let line = |x: f64| V2::init(x, soffit(x)).add(perp.scale(off));
        let slab_y = under + (lt / 2.0).min(lt - cov.cover.top - r).max(r);
        let x_bend = {
            let p0 = line(0.0);
            p0.x + (slab_y - p0.y) / slope
        };
        let mut path: Vec<V2> = Vec::new();
        if has_toe {
            let xv = tw - cov.cover.sides - r;
            let p0 = line(0.0);
            let y_at = p0.y + (xv - p0.x) * slope;
            path.push(V2::init(xv, -td + cov.cover.bottom));
            path.push(V2::init(xv, y_at));
        } else {
            path.push(line(cov.cover.sides + r));
        }
        path.push(V2::init(x_bend, slab_y));
        path.push(V2::init(x_end - cov.cover.sides.min(2.0), slab_y));
        let center = pathgeom::fillet(&path, 3.0 * d + r);
        prisms.push(rebar("main_bar", pathgeom::ribbon(&center, r, r), center, r));
        let a = path[path.len() - 3];
        let b = path[path.len() - 2];
        let run_len = a.dist(b);
        let u = b.sub(a).norm();
        let count = crate::num::to_int_clamped::<usize>(math::floor(run_len / sp.bar_spacing), 1, 200);
        let first = (run_len - (count as f64 - 1.0) * sp.bar_spacing) / 2.0;
        for k in 0..count {
            let c = a.add(u.scale(first + k as f64 * sp.bar_spacing)).add(perp.scale(d));
            prisms.push(Prism {
                part: "cross_bars".to_string(),
                material: "rebar".to_string(),
                loops: model::one_loop(model::circle_loop(c.x, c.y, r)),
                embedded: true,
                sweep_r: r,
                ..Prism::default()
            });
        }
        anchors.push(anchor("main_bar_mid", a.mid(b)));
        anchors.push(anchor("main_bar_top", V2::init(x_end - cov.cover.sides.min(2.0), slab_y)));
        anchors.push(anchor("main_bar_bend", V2::init(x_bend, slab_y)));
        info.push_str(&format!(", {} main bar, cross bars @ {}", sp.bars, fmt_ft_in(sp.bar_spacing)));
    }
    if let Some(d) = bar_d(ctx, "nosing_bars", &sp.nosing_bars)? {
        let r = d / 2.0;
        let inset = cov.cover.top + r;
        for k in 1..=u32::from(sp.risers) {
            let kf = f64::from(k);
            let c = V2::init((kf - 1.0) * tread + inset + nz * 0.25, kf * rise - inset);
            prisms.push(Prism {
                part: "nosing_bars".to_string(),
                material: "rebar".to_string(),
                loops: model::one_loop(model::circle_loop(c.x, c.y, r)),
                embedded: true,
                sweep_r: r,
                ..Prism::default()
            });
            if k == 1 {
                anchors.push(anchor("nosing_bar_1", c));
            }
        }
        info.push_str(&format!(", {} nosing bars", sp.nosing_bars));
    }
    Some(Built {
        prisms,
        anchors,
        zones,
        box_,
        nat_z: sp.width,
        host: Some(HostInfo { outline, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set }),
        info,
        ..Built::default()
    })
}

fn wood(ctx: &mut Ctx, sp: &Params) -> Option<Built> {
    for key in [
        "waist",
        "toe_width",
        "toe_depth",
        "landing",
        "landing_thickness",
        "bars",
        "nosing_bars",
        "bar_spacing",
        "cover",
        "material",
    ] {
        if ctx.p.has(key) {
            ctx.p.fail(key, format!("'{key}' belongs to a concrete stair: remove it (a wood stair takes size, stringers, tread_thickness, riser_thickness, nosing, treated)"));
            return None;
        }
    }
    let Some(sz) = parse_sawn(&sp.size) else {
        ctx.p.fail("size", format!("stringer size \"{}\" is not a sawn nominal size; use e.g. \"2x12\"", sp.size));
        return None;
    };
    if sp.risers < 2 {
        ctx.p.fail("risers", "a wood stringer needs at least 2 risers".to_string());
        return None;
    }
    let n = f64::from(sp.risers);
    let (rise, tread, nose, tt, rt) =
        (sp.rise, sp.tread, sp.nosing.max(0.0), sp.tread_thickness, sp.riser_thickness.max(0.0));
    if tt >= rise {
        ctx.p.fail(
            "tread_thickness",
            format!("tread_thickness {} must be less than the rise {}", fmt_num(tt), fmt_num(rise)),
        );
        return None;
    }
    let theta = math::atan2(rise, tread);
    let cos = math::cos(theta);
    let slope = rise / tread;
    let depth = sz.d;
    let throat = depth - rise * cos;
    let id = ctx.p.id;
    if throat < 3.5 - 1e-9 {
        ctx.scene.diags.add_fix(
            Level::Warning,
            "W_STAIR_THROAT",
            Some(id),
            Some(&format!("components/{id}/size")),
            format!("stringer '{id}': a {} cut for a {} rise and {} tread leaves a {} throat under the notches (3 1/2\" is the usual minimum)", sp.size, fmt_ft_in(rise), fmt_ft_in(tread), fmt_ft_in(throat.max(0.0))),
            "use a deeper stringer (2x12 or 2x14, or LVL), a lower rise or a shorter tread".to_string(),
        );
    }
    if throat <= 0.25 {
        ctx.p.fail(
            "size",
            format!(
                "a {} stringer is cut through by a {} x {} step: use a deeper size",
                sp.size,
                fmt_ft_in(rise),
                fmt_ft_in(tread)
            ),
        );
        return None;
    }
    let edge = |x: f64| slope * (x - rt) + rise - tt;
    let bottom = |x: f64| edge(x) - depth / cos;
    let x_top = (n - 1.0) * tread + rt;
    let x_seat = rt + (depth / cos - rise + tt) / slope;
    let mut pts: Vec<Pt> = vec![Pt::new(rt, 0.0)];
    if x_seat < x_top {
        pts.push(Pt::new(x_seat, 0.0));
        pts.push(Pt::new(x_top, bottom(x_top)));
    } else {
        pts.push(Pt::new(x_top, 0.0));
    }
    pts.push(Pt::new(x_top, (n - 1.0) * rise - tt));
    let steps = u32::from(sp.risers) - 1;
    for k in (1..=steps).rev() {
        let kf = f64::from(k);
        let xk = (kf - 1.0) * tread + rt;
        pts.push(Pt::new(xk, kf * rise - tt));
        if k > 1 {
            pts.push(Pt::new(xk, (kf - 1.0) * rise - tt));
        }
    }
    let outline = oriented_ccw(super::common::drop_duplicate_points(&pts));
    let material = if sp.treated { "wood_treated" } else { "wood" };
    if !material_ok(ctx, "material", material) {
        return None;
    }
    let width = sp.width.unwrap_or(36.0);
    let st = sz.t;
    let count = usize::from(sp.stringers);
    let mut prisms: Vec<Prism> = Vec::new();
    for k in 0..count {
        let zoff = if count == 1 { 0.0 } else { (k as f64 / (count as f64 - 1.0) - 0.5) * (width - st) };
        let zoff = if count % 2 == 0 && k == count / 2 { 0.0 } else { zoff };
        prisms.push(Prism {
            part: "stringer".to_string(),
            material: material.to_string(),
            loops: model::one_loop(outline.clone()),
            zoff,
            zhalf: st / 2.0,
            along_grain: true,
            ..Prism::default()
        });
    }
    let board = |part: &str, x0: f64, y0: f64, x1: f64, y1: f64| Prism {
        part: part.to_string(),
        material: material.to_string(),
        loops: model::one_loop(model::rect_loop(x0, y0, x1, y1)),
        quads: vec![super::common::quad_of(x0, y0, x1, y1)],
        ..Prism::default()
    };
    let mut anchors = vec![
        anchor("foot", V2::init(0.0, 0.0)),
        anchor("seat_front", V2::init(rt, 0.0)),
        anchor("seat_back", V2::init(x_seat.min(x_top), 0.0)),
        anchor("top", V2::init((n - 1.0) * tread, n * rise)),
        anchor("top_cut_top", V2::init(x_top, (n - 1.0) * rise - tt)),
        anchor("top_cut_bottom", V2::init(x_top, bottom(x_top).max(0.0))),
        anchor("top_bearing", V2::init(x_top, ((n - 1.0) * rise - tt + bottom(x_top).max(0.0)) / 2.0)),
    ];
    for k in 1..=steps {
        let kf = f64::from(k);
        let x0 = (kf - 1.0) * tread;
        prisms.push(board("tread", x0 - nose, kf * rise - tt, x0 + tread + rt, kf * rise));
        anchors.push(anchor(&format!("nosing_{k}"), V2::init(x0 - nose, kf * rise)));
        anchors.push(anchor(&format!("tread_{k}_mid"), V2::init(x0 + tread / 2.0, kf * rise)));
    }
    if rt > 1e-9 {
        for k in 1..=u32::from(sp.risers) {
            let kf = f64::from(k);
            let x0 = (kf - 1.0) * tread;
            let y0 = (kf - 1.0) * rise;
            let y1 = if kf < n { kf * rise - tt } else { kf * rise };
            prisms.push(board("riser", x0, y0, x0 + rt, y1));
        }
    }
    let mid_x = (rt + x_top) / 2.0;
    let mid_k = f64::from((sp.risers / 2).max(1));
    let throat_top = V2::init(mid_k * tread + rt, mid_k * rise - tt);
    anchors.extend([
        anchor("soffit_mid", V2::init(mid_x.max(x_seat.min(x_top)), bottom(mid_x.max(x_seat.min(x_top))))),
        anchor("throat_top", throat_top),
        anchor("throat_bottom", throat_top.add(V2::init(math::sin(theta), -cos).scale(throat))),
    ]);
    let mut box_ = geom::loop_box(&outline);
    for p in &prisms {
        for l in &p.loops {
            box_.add_box(geom::loop_box(l));
        }
    }
    Some(Built {
        prisms,
        anchors,
        box_,
        nat_z: Some(width),
        info: format!(
            "stair wood ({}) {}{} stringers, {} risers {} x {} treads, throat {}",
            sp.stringers,
            sp.size,
            if sp.treated { " PT" } else { "" },
            sp.risers,
            fmt_ft_in(rise),
            fmt_ft_in(tread),
            fmt_ft_in(throat)
        ),
        ..Built::default()
    })
}
