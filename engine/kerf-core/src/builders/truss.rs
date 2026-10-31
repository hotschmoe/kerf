//! truss (SPEC 5.8, 27.1): its parameters (`Params`, the single source for the parser and the catalog) and its builder.
//! Side view (the truss in the section plane): a heel/tail end for the pitched, scissor and mono profiles, or a
//! parallel-chord (floor / flat) truss with its open webs. End-on (the truss cut across, running along z): the chords cut
//! and a web strip beyond them.

use super::common::{Ctx, fmt_ft_in, fmt_num, mirror_built, parse_sawn, quad_of, zone_rect};
use crate::geom::{self, Box, Pt, V2};
use crate::json::Value;
use crate::math;
use crate::model::{self, Built, Kind, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use crate::pen::Pen;
use crate::units;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Exterior { Left = "left", Right = "right" } }
param_enum! { pub enum Heel { Standard = "standard", Raised = "raised" } }
param_enum! { pub enum Tail { Plumb = "plumb", Square = "square" } }
param_enum! { pub enum Profile { Pitched = "pitched", Scissor = "scissor", Mono = "mono", ParallelChord = "parallel_chord" } }
param_enum! { pub enum Orient { Side = "side", EndOn = "end_on" } }
param_enum! { pub enum Bearing { BottomChord = "bottom_chord", TopChord = "top_chord", Face = "face" } }

params! {
    pub struct Params {
        profile: Enum(Profile) = Profile::Pitched => { desc: "pitched (heel rising inward) | scissor (bottom chord rises at bottom_pitch) | mono (high end: end vertical heel_height tall, top chord falling inward) | parallel_chord (floor or flat truss: depth, open webs)" },
        orient: Enum(Orient) = Orient::Side => { desc: "side: the truss lies in the section plane | end_on: cut across (trusses parallel to the section): chords cut, a web strip beyond; needs depth" },
        exterior: Enum(Exterior) = Exterior::Left => { desc: "side of the heel/overhang (right mirrors)" },
        pitch: Raw = Value::String("4:12".into()) => { def: "4:12", desc: "rise:run of the top chord (not for parallel_chord)" },
        bottom_pitch: OptRaw => { def: "scissor: half the pitch", desc: "scissor: rise:run of the bottom chord, less than pitch" },
        top_chord: Str = "2x4" => { def: "2x4 (parallel_chord: 4x2)", desc: "sawn nominal size, depth in-plane; \"4x2\" is a 2x4 laid flat (floor truss chord: 1 1/2 in-plane, 3 1/2 thick)" },
        bottom_chord: Str = "2x4" => { def: "2x4 (parallel_chord: 4x2)", desc: "as top_chord" },
        heel: Enum(Heel) = Heel::Standard => { desc: "standard | raised" },
        heel_height: OptLenPos => { desc: "raised heel, mono, bearing face: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
        depth: OptLenPos => { desc: "parallel_chord: overall depth, bottom of bottom chord to top of top chord (e.g. 18); end_on: the truss height at the cut" },
        bearing: Enum(Bearing) = Bearing::BottomChord => { desc: "bottom_chord: bears under its bottom chord | top_chord (parallel_chord): the top chord runs over the support, the bottom chord and end vertical drop inside it | face: hung at a beam or ledger face by its end vertical (no overhang; pitched/scissor/mono need heel_height)" },
        bearing_width: LenPos = 3.5 => { desc: "width of the support under the bearing" },
        overhang: Len = 12.0 => { def: "12 (parallel_chord and mono: 0)", desc: "horizontal distance from outer face of bearing to the tail end" },
        tail: Enum(Tail) = Tail::Plumb => { desc: "plumb | square cut" },
        span_shown: LenPos = 48.0 => { desc: "how far into the building to draw (crop/break at the end)" },
        web_spacing: OptLenPos => { def: "side: the clear height between chords; end_on: 24", desc: "parallel_chord side: horizontal distance between web panel points (diagonals alternate up and down); end_on: spacing of the webs along z" },
        plies: Int(u8, 1, 4) = 1 => { desc: "end_on: girder plies side by side" },
        length: OptLenPos => { desc: "end_on: length along z, centered on `z` or the first section view's cut_z (default: spans the doc run)" },
        plate: Bool = true => { desc: "draw the heel truss plate outline (dashed hidden pen; heel profiles)" },
    }
}

/// A chord size: (depth in the truss plane, thickness across it). "4x2" is a 2x4 laid flat.
fn chord_size(s: &str) -> Option<(f64, f64)> {
    if let Some(sz) = parse_sawn(s) {
        return Some((sz.d, sz.t));
    }
    let xi = s.find(['x', 'X'])?;
    let flipped = format!("{}x{}", s.get(xi + 1..)?.trim(), s.get(..xi)?.trim());
    parse_sawn(&flipped).map(|sz| (sz.t, sz.d))
}

fn anchor(name: &str, p: V2) -> NamedAnchor {
    NamedAnchor { name: name.into(), p }
}

fn wood(part: &str, lp: Vec<Pt>) -> Prism {
    Prism { part: part.to_string(), material: "wood".into(), loops: model::one_loop(lp), ..Prism::default() }
}

fn zone_of(name: &str, lp: &[Pt]) -> Zone {
    Zone { name: name.into(), loops: model::one_loop(lp.to_vec()), box_: geom::loop_box(lp) }
}

fn box_of(prisms: &[Prism]) -> Box {
    let mut b = Box::default();
    for pr in prisms.iter().filter(|pr| pr.kind != Kind::Ghost) {
        for l in &pr.loops {
            b.add_box(geom::loop_box(l));
        }
    }
    b
}

struct Chords {
    top: String,
    bot: String,
    dt: f64,
    tt: f64,
    db: f64,
    tb: f64,
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let tp = ctx.p.parse::<Params>()?;
    let parallel = tp.profile == Profile::ParallelChord;
    let default_chord = if parallel { "4x2" } else { "2x4" };
    let top = if ctx.p.has("top_chord") { tp.top_chord.clone() } else { default_chord.to_string() };
    let bot = if ctx.p.has("bottom_chord") { tp.bottom_chord.clone() } else { default_chord.to_string() };
    let Some((dt, tt)) = chord_size(&top) else {
        ctx.p.fail("top_chord", format!("top_chord \"{top}\" is not a sawn nominal size like \"2x4\", \"2x6\" (or \"4x2\" for a 2x4 laid flat)"));
        return None;
    };
    let Some((db, tb)) = chord_size(&bot) else {
        ctx.p.fail("bottom_chord", format!("bottom_chord \"{bot}\" is not a sawn nominal size like \"2x4\", \"2x6\" (or \"4x2\" for a 2x4 laid flat)"));
        return None;
    };
    let ch = Chords { top, bot, dt, tt, db, tb };
    if tp.bearing == Bearing::TopChord && !parallel {
        ctx.p.fail("bearing", "bearing \"top_chord\" is drawn for profile \"parallel_chord\" (floor and flat trusses); a pitched truss bears on its bottom chord or hangs at a face (bearing \"face\")".into());
        return None;
    }
    if tp.orient == Orient::EndOn {
        return end_on(ctx, &tp, &ch);
    }
    if ctx.p.has("plies") || ctx.p.has("length") {
        let key = if ctx.p.has("plies") { "plies" } else { "length" };
        ctx.p.fail(key, format!("'{key}' only applies to orient \"end_on\" (a side-view truss is one member in the section plane: set its spacing with `z` and `array`)"));
        return None;
    }
    let built = if parallel { parallel_side(ctx, &tp, &ch)? } else { heel_side(ctx, &tp, &ch)? };
    Some(if tp.exterior == Exterior::Right {
        let mut m = mirror_built(&built, &geom::Xf::scaling(-1.0, 1.0), false);
        m.pitch = -built.pitch;
        m
    } else {
        built
    })
}

fn lower(x: f64, y0: f64, sl: f64) -> f64 {
    y0 + sl * x
}

fn parse_pitch(ctx: &mut Ctx, key: &str, v: &Value) -> Option<f64> {
    let Some(theta) = units::parse_slope(v) else {
        ctx.p.fail(key, format!("param '{key}' must be rise:run like \"4:12\" or degrees (got {})", model::kind_or_text(v)));
        return None;
    };
    if theta <= 0.0 || theta >= math::PI / 2.0 {
        ctx.p.fail(key, format!("{key} must slope up (between 0 and 90 degrees)"));
        return None;
    }
    Some(theta)
}

/// Pitched, scissor and mono trusses in side view: the heel (or the mono's high end) at the bearing and the tail.
fn heel_side(ctx: &mut Ctx, tp: &Params, ch: &Chords) -> Option<Built> {
    let theta = parse_pitch(ctx, "pitch", &tp.pitch)?;
    let (dt, db) = (ch.dt, ch.db);
    let mono = tp.profile == Profile::Mono;
    let scissor = tp.profile == Profile::Scissor;
    let face = tp.bearing == Bearing::Face;
    let sigma = if mono { -1.0 } else { 1.0 };
    let s = math::tan(theta);
    let c = math::cos(theta);
    let v_thick = dt / c;
    let mut theta_b = 0.0;
    if scissor {
        theta_b = match ctx.p.raw("bottom_pitch") {
            Some(v) => parse_pitch(ctx, "bottom_pitch", v)?,
            None => math::atan(s / 2.0),
        };
        if theta_b >= theta - 1e-9 {
            ctx.p.fail("bottom_pitch", format!("a scissor truss's bottom_pitch must be flatter than its pitch {}:12 (got {}:12)", fmt_num(s * 12.0), fmt_num(math::tan(theta_b) * 12.0)));
            return None;
        }
    } else if ctx.p.has("bottom_pitch") {
        ctx.p.fail("bottom_pitch", "'bottom_pitch' only applies to profile \"scissor\"".into());
        return None;
    }
    let sb = math::tan(theta_b);
    let db_v = db / math::cos(theta_b);
    let tall_end = tp.heel == Heel::Raised || mono || face;
    let mut y_low0 = db_v;
    if tall_end {
        let Some(hh) = tp.heel_height else {
            let why = if mono {
                "profile \"mono\" needs 'heel_height' (vertical height of its high end at the bearing outer edge, top of bottom chord to top of top chord)"
            } else if face {
                "bearing \"face\" needs 'heel_height' (vertical height of the end vertical at the face, top of bottom chord to top of top chord)"
            } else {
                "heel 'raised' needs 'heel_height' (vertical height at the bearing outer edge from top of bottom chord to top of top chord)"
            };
            ctx.p.fail("heel_height", why.into());
            return None;
        };
        if hh < v_thick {
            ctx.p.fail("heel_height", format!("heel_height {} is smaller than the top chord's vertical thickness {}; the raised heel must be at least that tall", fmt_num(hh), fmt_num(v_thick)));
            return None;
        }
        y_low0 = db_v + hh - v_thick;
    }
    let overhang = if (mono || face) && !ctx.p.has("overhang") { 0.0 } else { tp.overhang };
    if face && overhang != 0.0 {
        ctx.p.fail("overhang", format!("a truss hung at a face (bearing \"face\") has no overhang: remove 'overhang' (got {})", fmt_ft_in(overhang)));
        return None;
    }
    let sl = sigma * s;
    let x_tail = -overhang;
    let xe = tp.span_shown;
    let bot_top = |x: f64| if scissor { db_v + sb * (x - tp.bearing_width).max(0.0) } else { db };
    if lower(xe, y_low0, sl) < bot_top(xe) - 1e-9 {
        let meet = (bot_top(0.0) - y_low0) / sl;
        ctx.p.fail(
            "span_shown",
            format!(
                "the top chord meets the bottom chord at x = {} before span_shown {}: set span_shown to at most {} or raise heel_height",
                fmt_num(meet),
                fmt_num(xe),
                fmt_num(math::floor(meet))
            ),
        );
        return None;
    }
    let tail_bottom = V2::init(x_tail, lower(x_tail, y_low0, sl));
    let mut tail_top = V2::init(x_tail, tail_bottom.y + v_thick);
    if tp.tail == Tail::Square {
        tail_top = tail_bottom.add(V2::init(-sigma * math::sin(theta), math::cos(theta)).scale(dt));
    }
    let top_loop = vec![
        Pt::at(tail_bottom, 0.0),
        Pt::new(xe, lower(xe, y_low0, sl)),
        Pt::new(xe, lower(xe, y_low0, sl) + v_thick),
        Pt::at(tail_top, 0.0),
    ];
    let bot_loop = if scissor {
        let bw = tp.bearing_width.min(xe);
        vec![
            Pt::new(0.0, 0.0),
            Pt::new(bw, 0.0),
            Pt::new(xe, sb * (xe - bw)),
            Pt::new(xe, sb * (xe - bw) + db_v),
            Pt::new(bw, db_v),
            Pt::new(0.0, db_v),
        ]
    } else {
        model::rect_loop(0.0, 0.0, xe, db)
    };
    let mut prisms = vec![wood("bottom_chord", bot_loop.clone()), wood("top_chord", top_loop.clone())];
    let mut zones: Vec<Zone> = vec![zone_of("bottom_chord", &bot_loop), zone_of("top_chord", &top_loop)];
    let mut extra: Vec<NamedAnchor> = Vec::new();
    if tall_end {
        let (part, web_w) = if mono || face { ("end_vertical", dt.max(3.5).min(xe)) } else { ("heel_web", 1.5) };
        let web = vec![
            Pt::new(0.0, db_v),
            Pt::new(web_w, bot_top(web_w)),
            Pt::new(web_w, lower(web_w, y_low0, sl)),
            Pt::new(0.0, y_low0),
        ];
        zones.push(zone_of(part, &web));
        prisms.push(wood(part, web));
        if part == "end_vertical" {
            extra.push(anchor("end_vertical", V2::init(0.0, (db_v + y_low0) / 2.0)));
            extra.push(anchor("end_top", V2::init(0.0, y_low0 + v_thick)));
            extra.push(anchor("end_bottom", V2::init(0.0, 0.0)));
        }
    }
    if tp.plate {
        let py1 = (db_v + 0.5).max(y_low0.min(lower(5.25, y_low0, sl)) + 0.5);
        let pl = model::rect_loop(0.25, 0.25, 5.25, py1);
        prisms.push(Prism {
            part: "plate".into(),
            material: "steel".into(),
            loops: model::one_loop(pl),
            kind: Kind::Ghost,
            pen: Some(Pen::Hidden),
            embedded: true,
            ..Prism::default()
        });
        zones.push(zone_rect("plate", 0.25, 0.25, 5.25, py1));
    }
    {
        let mut tb = Box::default();
        tb.add_point(tail_bottom.x, tail_bottom.y);
        tb.add_point(tail_top.x, tail_top.y);
        tb.add_point(0.0, lower(0.0, y_low0, sl));
        tb.add_point(0.0, lower(0.0, y_low0, sl) + v_thick);
        zones.push(Zone {
            name: "tail".into(),
            loops: model::one_loop(model::rect_loop(tb.x0, tb.y0, tb.x1, tb.y1)),
            box_: tb,
        });
    }
    let top_at_bearing = lower(0.0, y_low0, sl) + v_thick;
    let mut anchors = vec![
        anchor("bearing_outer", V2::init(0.0, 0.0)),
        anchor("bearing_inner", V2::init(tp.bearing_width, 0.0)),
        anchor("tail_bottom", tail_bottom),
        anchor("tail_top", tail_top),
        anchor("top_chord_at_bearing", V2::init(0.0, top_at_bearing)),
        anchor("heel_outer", V2::init(0.0, (db_v + top_at_bearing) / 2.0)),
        anchor("top_chord_bottom_at_bearing", V2::init(0.0, lower(0.0, y_low0, sl))),
        anchor("heel_block_top", V2::init(0.0, top_at_bearing)),
        anchor("top_chord_end", V2::init(xe, lower(xe, y_low0, sl) + v_thick)),
        anchor("bottom_chord_top_inner", V2::init(xe, bot_top(xe))),
    ];
    anchors.extend(extra);
    let profile = match tp.profile {
        Profile::Scissor => format!(" scissor (bottom {}:12)", fmt_num(sb * 12.0)),
        Profile::Mono => " mono".to_string(),
        _ => String::new(),
    };
    let end = if face {
        " hung at face".to_string()
    } else if mono {
        format!(" high end {}", fmt_ft_in(tp.heel_height.unwrap_or(0.0)))
    } else {
        format!(" {} heel", tp.heel.tag())
    };
    Some(Built {
        box_: box_of(&prisms),
        prisms,
        anchors,
        zones,
        nat_z: Some(ch.tt.max(ch.tb)),
        pitch: sigma * theta,
        info: format!(
            "truss {}:12{profile}{end}, {}+{} chords, ovh {}",
            fmt_num(s * 12.0),
            ch.top,
            ch.bot,
            fmt_ft_in(overhang)
        ),
        ..Built::default()
    })
}

/// The half-width along a chord of a web of width `w` that climbs `h` over the horizontal run `p` between its outer
/// edges (the edges meet the chords on horizontal cuts).
fn web_half(w: f64, h: f64, p: f64) -> f64 {
    let mut hw = w / 2.0;
    for _ in 0..8 {
        let dx = (p - 2.0 * hw).max(1e-3);
        let sin = h / math::sqrt(dx * dx + h * h);
        hw = w / (2.0 * sin);
    }
    hw
}

/// A parallel-chord (floor or flat) truss in side view: chords, the end vertical and alternating diagonal webs.
fn parallel_side(ctx: &mut Ctx, tp: &Params, ch: &Chords) -> Option<Built> {
    let Some(d) = tp.depth else {
        ctx.p.fail("depth", "profile \"parallel_chord\" needs 'depth' (overall, bottom of bottom chord to top of top chord, e.g. 18)".into());
        return None;
    };
    for key in ["heel", "heel_height", "bottom_pitch", "pitch"] {
        if ctx.p.has(key) {
            ctx.p.fail(key, format!("'{key}' does not apply to profile \"parallel_chord\" (its chords are level: give depth)"));
            return None;
        }
    }
    let (dt, db) = (ch.dt, ch.db);
    let h = d - dt - db;
    if h < 3.0 {
        ctx.p.fail("depth", format!("depth {} leaves {} between the chords ({} + {}); a parallel-chord truss needs at least 3\" for its webs", fmt_ft_in(d), fmt_ft_in(h.max(0.0)), fmt_ft_in(db), fmt_ft_in(dt)));
        return None;
    }
    let tcb = tp.bearing == Bearing::TopChord;
    let face = tp.bearing == Bearing::Face;
    let overhang = if ctx.p.has("overhang") { tp.overhang } else { 0.0 };
    if face && overhang != 0.0 {
        ctx.p.fail("overhang", format!("a truss hung at a face (bearing \"face\") has no overhang: remove 'overhang' (got {})", fmt_ft_in(overhang)));
        return None;
    }
    let flat = ch.dt < ch.tt;
    let w = if flat { dt.min(db) } else { 3.5 };
    let xe = tp.span_shown;
    let x_body = if tcb { tp.bearing_width } else { 0.0 };
    if xe < x_body + w + 6.0 {
        ctx.p.fail("span_shown", format!("span_shown {} is too short to draw the truss body: give at least {}", fmt_num(xe), fmt_num(math::ceil(x_body + w + 6.0))));
        return None;
    }
    let top_loop = model::rect_loop(-overhang, d - dt, xe, d);
    let bot_loop = model::rect_loop(x_body, 0.0, xe, db);
    let ev = model::rect_loop(x_body, db, x_body + w, d - dt);
    let mut prisms = vec![wood("bottom_chord", bot_loop.clone()), wood("top_chord", top_loop.clone()), wood("end_vertical", ev.clone())];
    let mut zones = vec![zone_of("bottom_chord", &bot_loop), zone_of("top_chord", &top_loop), zone_of("end_vertical", &ev)];
    let p = tp.web_spacing.unwrap_or(h);
    let hw = web_half(w, h, p);
    if p <= 2.0 * hw + 0.25 {
        ctx.p.fail("web_spacing", format!("web_spacing {} is too tight for {} webs over {} between the chords: give at least {}", fmt_num(p), fmt_ft_in(w), fmt_ft_in(h), fmt_num(math::ceil(2.0 * hw + 1.0))));
        return None;
    }
    let (yb, yt) = (db, d - dt);
    let mut x = x_body + w;
    let mut down = true;
    let mut webs = Box::default();
    while x + p <= xe - 0.5 {
        let lp = if down {
            vec![Pt::new(x + p - 2.0 * hw, yb), Pt::new(x + p, yb), Pt::new(x + 2.0 * hw, yt), Pt::new(x, yt)]
        } else {
            vec![Pt::new(x, yb), Pt::new(x + 2.0 * hw, yb), Pt::new(x + p, yt), Pt::new(x + p - 2.0 * hw, yt)]
        };
        webs.add_box(geom::loop_box(&lp));
        prisms.push(wood("web", lp));
        x += p;
        down = !down;
    }
    if !webs.is_empty() {
        zones.push(zone_rect("web", webs.x0, webs.y0, webs.x1, webs.y1));
    }
    let y_bear = if tcb { d - dt } else { 0.0 };
    let mut anchors = vec![
        anchor("bearing_outer", V2::init(0.0, y_bear)),
        anchor("bearing_inner", V2::init(tp.bearing_width, y_bear)),
        anchor("top_chord_at_bearing", V2::init(0.0, d)),
        anchor("top_chord_bottom_at_bearing", V2::init(0.0, d - dt)),
        anchor("heel_block_top", V2::init(0.0, d)),
        anchor("top_chord_end", V2::init(xe, d)),
        anchor("bottom_chord_top_inner", V2::init(xe, db)),
        anchor("bottom_chord_end", V2::init(xe, 0.0)),
        anchor("end_vertical", V2::init(x_body, (db + d - dt) / 2.0)),
        anchor("end_top", V2::init(x_body, d)),
        anchor("end_bottom", V2::init(x_body, 0.0)),
        anchor("heel_outer", V2::init(x_body, (db + d - dt) / 2.0)),
    ];
    if overhang > 0.0 {
        anchors.push(anchor("tail_bottom", V2::init(-overhang, d - dt)));
        anchors.push(anchor("tail_top", V2::init(-overhang, d)));
    }
    let bearing = match tp.bearing {
        Bearing::BottomChord => "bottom chord bearing",
        Bearing::TopChord => "top chord bearing",
        Bearing::Face => "hung at face",
    };
    Some(Built {
        box_: box_of(&prisms),
        prisms,
        anchors,
        zones,
        nat_z: Some(ch.tt.max(ch.tb)),
        info: format!("truss parallel chord {} deep, {}+{} chords, {bearing}, webs @ {}", fmt_ft_in(d), ch.top, ch.bot, fmt_ft_in(p)),
        ..Built::default()
    })
}

/// A truss cut across (running along z): the plies of both chords cut, and a web strip between them behind the cut
/// at every `web_spacing` along the truss.
fn end_on(ctx: &mut Ctx, tp: &Params, ch: &Chords) -> Option<Built> {
    let Some(d) = tp.depth else {
        ctx.p.fail("depth", "orient \"end_on\" needs 'depth' (the truss height at the cut, bottom of bottom chord to top of top chord, e.g. 18 for a floor truss or the height of a roof truss where the section crosses it)".into());
        return None;
    };
    let parallel = tp.profile == Profile::ParallelChord;
    let theta = if parallel { 0.0 } else { parse_pitch(ctx, "pitch", &tp.pitch)? };
    let dt_v = ch.dt / math::cos(theta);
    let h = d - dt_v - ch.db;
    if h <= 0.0 {
        ctx.p.fail("depth", format!("depth {} is not more than the two chords ({} + {}): give the overall height at the cut", fmt_ft_in(d), fmt_ft_in(ch.db), fmt_ft_in(dt_v)));
        return None;
    }
    let plies = usize::from(tp.plies);
    let tw = ch.tt.max(ch.tb);
    let w = tw * plies as f64;
    let mut prisms = Vec::new();
    let mut zones = Vec::new();
    for (part, t, y0, y1) in [("bottom_chord", ch.tb, 0.0, ch.db), ("top_chord", ch.tt, d - dt_v, d)] {
        let x0 = (tw - t) / 2.0;
        let quads: Vec<[V2; 4]> = (0..plies).map(|k| quad_of(x0 + k as f64 * tw, y0, x0 + k as f64 * tw + t, y1)).collect();
        let ply_lines: Vec<[V2; 2]> = (1..plies).map(|k| [V2::init(k as f64 * tw, y0), V2::init(k as f64 * tw, y1)]).collect();
        let lp = model::rect_loop(x0, y0, x0 + (plies - 1) as f64 * tw + t, y1);
        zones.push(zone_of(part, &lp));
        prisms.push(Prism { quads, ply_lines, ..wood(part, lp) });
    }
    let web_t = if ch.dt < ch.tt { ch.dt.min(ch.db) } else { 1.5 };
    let spacing = tp.web_spacing.unwrap_or(24.0);
    let span = tp.length.unwrap_or(ctx.scene.run[1] - ctx.scene.run[0]);
    let cut = if tp.length.is_some() || ctx.p.has("z") {
        0.0
    } else {
        let mid = (ctx.scene.run[0] + ctx.scene.run[1]) / 2.0;
        ctx.scene.default_z.map_or(0.0, |z| z - mid)
    };
    let web = model::rect_loop(0.0, ch.db, w, d - dt_v);
    zones.push(zone_of("web", &web));
    let mut offs: Vec<f64> = Vec::new();
    let mut o = cut - spacing / 2.0;
    while o - web_t / 2.0 >= -span / 2.0 && offs.len() < 64 {
        offs.push(o);
        o -= spacing;
    }
    let mut o = cut + spacing / 2.0;
    while o + web_t / 2.0 <= span / 2.0 && offs.len() < 128 {
        offs.push(o);
        o += spacing;
    }
    if offs.is_empty() {
        offs.push(cut - (span / 2.0).min(spacing / 2.0) + web_t / 2.0);
    }
    for o in offs {
        prisms.push(Prism { zhalf: web_t / 2.0, zoff: o, ..wood("web", web.clone()) });
    }
    let anchors = vec![
        anchor("top_chord", V2::init(w / 2.0, d)),
        anchor("top_chord_bottom", V2::init(w / 2.0, d - dt_v)),
        anchor("bottom_chord", V2::init(w / 2.0, 0.0)),
        anchor("bottom_chord_top", V2::init(w / 2.0, ch.db)),
        anchor("web_center", V2::init(w / 2.0, (ch.db + d - dt_v) / 2.0)),
    ];
    let girder = if plies > 1 { format!(" ({plies}-ply)") } else { String::new() };
    Some(Built {
        prisms,
        anchors,
        zones,
        box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: d },
        nat_z: tp.length,
        info: format!("truss{girder} end-on {} deep at the cut, {}+{} chords, webs @ {}", fmt_ft_in(d), ch.top, ch.bot, fmt_ft_in(spacing)),
        ..Built::default()
    })
}
