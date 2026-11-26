use super::common::{Ctx, fmt_ft_in, length_or_until, material_ok, oriented_ccw};
use crate::geom::{Box, Pt, V2};
use crate::model::{self, Built, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Orient { Elevation = "elevation", Section = "section" } }
param_enum! { pub enum Ends { Left = "left", Right = "right", Both = "both", None = "none" } }
param_enum! { pub enum Web { Rod = "rod", Angle = "angle" } }
param_enum! { pub enum CutAt { Span = "span", Bearing = "bearing" } }

params! {
    pub struct Params {
        designation: Str => { desc: "SJI designation, free text after the depth and series: \"18K4\", \"20KCS3\", \"24LH06\", \"52DLH13\", \"32G8N8K\" (joist girder: depth G spaces N panel load K). The leading number is the depth in inches; the series letters (K, KCS, CJ: 2 1/2 seat; LH, DLH: 5 seat; G, BG, VG: 7 1/2 seat) pick the seat and member proportions. Member sizes are by the joist manufacturer: the drawing is schematic but true to depth, seat and bearing" },
        orient: Enum(Orient) = Orient::Elevation => { desc: "elevation: seen side-on along its length (chords as double-angle bands, web diagonals between panel points, underslung seat at the bearing end); section: cut end-on across the joist (the chord angle pairs back to back, the web seen beyond), for sections cut across the joists" },
        length: OptRaw => { def: "elevation: required", desc: "elevation: length drawn from the joist end (or until), usually a stub that the crop cuts with a break line. Section: optional length along z (default: spans the doc run)" },
        until: OptRaw => { desc: "elevation alternative to length: a Ref; the joist grows from its placement anchor until its far end reaches the Ref's x (as lumber until)" },
        depth: OptLenPos => { def: "from designation", desc: "joist depth, top of top chord to bottom of bottom chord, when it differs from the designation's leading number" },
        seats: Enum(Ends) = Ends::Left => { desc: "elevation: which ends are bearing ends with an underslung seat: left | right | both | none. An end without a seat is a cut end (chords and webs run to it). Both needs the true joist length" },
        seat_depth: OptLenPos => { def: "2.5 K, 5 LH/DLH, 7.5 G", desc: "bearing depth: top of top chord down to the bearing surface (SJI: 2 1/2 K-series, 5 LH/DLH, 7 1/2 DLH with large chords and joist girders)" },
        seat_length: OptLenPos => { def: "4 K, 6 LH/DLH/G", desc: "length of the seat (shoe) along the joist. SJI minimum bearing: K 2 1/2 on steel, 4 on masonry or concrete; LH/DLH and girders 4 on steel, 6 on masonry or concrete" },
        bottom_chord_extension: Enum(Ends) = Ends::None => { desc: "elevation: left | right | both | none: the bottom chord runs on to that bearing end (to a stabilizer plate or column) instead of stopping at the first panel point" },
        panel: OptLenPos => { def: "24 K, 48 LH/DLH, 60 G", desc: "panel length: spacing of the panel points along each chord (the bottom chord's fall midway between the top chord's). A joist girder's panel is the joist spacing" },
        chord_leg: OptLenPos => { def: "by series and depth", desc: "leg of the chord angles (each chord is two equal-leg angles back to back): 1 1/2 to 2 K, 3 to 3 1/2 LH, 4 to 5 G. Must be less than seat_depth" },
        chord_thickness: OptLenPos => { def: "by series and depth", desc: "thickness of the chord angles" },
        web: OptEnum(Web) => { def: "rod K, angle LH/DLH/G", desc: "rod: round bar webs bent between the chord angles; angle: angle webs (crimped or double), drawn as bands web_size wide" },
        web_size: OptLenPos => { def: "by series and depth", desc: "bar diameter (rod) or angle leg (angle) of the web members" },
        section_at: Enum(CutAt) = CutAt::Span => { desc: "section: span: cut between the bearings (both chords and the web beyond); bearing: cut at the seat (top chord angles and the seat angles under them, seat_depth deep, no bottom chord): the joist end bearing on a joist girder, beam or ledger" },
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Light,
    Long,
    Girder,
}

const SERIES: [(&str, Class); 8] = [
    ("DLH", Class::Long),
    ("LH", Class::Long),
    ("KCS", Class::Light),
    ("K", Class::Light),
    ("CJ", Class::Light),
    ("BG", Class::Girder),
    ("VG", Class::Girder),
    ("G", Class::Girder),
];

struct Sizes {
    leg: f64,
    t: f64,
    web: f64,
    gap: f64,
}

fn parse(designation: &str) -> Option<(f64, &'static str, Class)> {
    let s: String = designation.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    let digits = s.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
    let depth: f64 = s.get(..digits)?.parse().ok()?;
    let rest = s.get(digits..)?;
    let &(tag, class) = SERIES.iter().find(|(tag, _)| rest.starts_with(tag))?;
    (depth > 0.0 && depth <= 240.0).then_some((depth, tag, class))
}

fn defaults(class: Class, d: f64) -> (f64, f64, f64, Web, Sizes) {
    match class {
        Class::Light => {
            let s = if d <= 16.0 {
                Sizes { leg: 1.5, t: 0.125, web: 0.5, gap: 0.5 }
            } else if d <= 24.0 {
                Sizes { leg: 2.0, t: 0.1875, web: 0.625, gap: 0.625 }
            } else {
                Sizes { leg: 2.0, t: 0.25, web: 0.75, gap: 0.75 }
            };
            (2.5, 4.0, 24.0, Web::Rod, s)
        }
        Class::Long => {
            let s = if d <= 32.0 {
                Sizes { leg: 3.0, t: 0.3125, web: 2.0, gap: 0.75 }
            } else {
                Sizes { leg: 3.5, t: 0.375, web: 2.5, gap: 0.75 }
            };
            (5.0, 6.0, 48.0, Web::Angle, s)
        }
        Class::Girder => {
            let s = if d <= 40.0 {
                Sizes { leg: 4.0, t: 0.4375, web: 3.0, gap: 1.0 }
            } else {
                Sizes { leg: 5.0, t: 0.5, web: 3.5, gap: 1.0 }
            };
            (7.5, 6.0, 60.0, Web::Angle, s)
        }
    }
}

fn steel(part: &str, lp: Vec<Pt>) -> Prism {
    Prism { part: part.to_string(), material: "steel".to_string(), loops: model::one_loop(lp), ..Prism::default() }
}

fn clip_x(poly: &[V2], c: f64, keep_below: bool) -> Vec<V2> {
    let inside = |p: V2| if keep_below { p.x <= c + 1e-9 } else { p.x >= c - 1e-9 };
    let mut out = Vec::new();
    for (i, &p) in poly.iter().enumerate() {
        let q = poly[(i + 1) % poly.len()];
        if inside(p) {
            out.push(p);
        }
        if inside(p) != inside(q) && (q.x - p.x).abs() > 1e-12 {
            out.push(p.lerp(q, (c - p.x) / (q.x - p.x)));
        }
    }
    out
}

/// Heel at the origin, horizontal leg b along +x, vertical leg a along +y, root fillet r; mapped by (sx, sy) then moved to (dx, dy).
fn angle_loop(a: f64, b: f64, t: f64, sx: f64, sy: f64, dx: f64, dy: f64) -> Vec<Pt> {
    const Q: f64 = 0.414_213_562_373_095_1;
    let r = t.min(0.45 * (a - t)).min(0.45 * (b - t)).max(0.0);
    let mut lp = vec![Pt::new(0.0, 0.0), Pt::new(b, 0.0), Pt::new(b, t)];
    if r > 1e-9 {
        lp.extend([Pt { x: t + r, y: t, b: -Q }, Pt::new(t, t + r)]);
    } else {
        lp.push(Pt::new(t, t));
    }
    lp.extend([Pt::new(t, a), Pt::new(0.0, a)]);
    oriented_ccw(lp.iter().map(|q| Pt { x: dx + sx * q.x, y: dy + sy * q.y, b: q.b * sx * sy }).collect())
}

struct Geo {
    d: f64,
    sd: f64,
    sl: f64,
    panel: f64,
    leg: f64,
    t: f64,
    web: f64,
    gap: f64,
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let jp = ctx.p.parse::<Params>()?;
    let Some((d0, tag, class)) = parse(&jp.designation) else {
        ctx.p.fail("designation", format!("steel_joist designation \"{}\" must start with the depth in inches and an SJI series: \"18K4\", \"20KCS3\", \"24LH06\", \"52DLH13\", \"32G8N8K\" (series K, KCS, CJ, LH, DLH, G, BG, VG). The text after the series is free", jp.designation));
        return None;
    };
    if !material_ok(ctx, "designation", "steel") {
        return None;
    }
    let (sd0, sl0, panel0, web0, sz) = defaults(class, jp.depth.unwrap_or(d0));
    let web_kind = jp.web.unwrap_or(web0);
    let web = jp.web_size.unwrap_or(if web_kind == web0 {
        sz.web
    } else if web_kind == Web::Rod {
        0.75
    } else {
        1.5
    });
    let g = Geo {
        d: jp.depth.unwrap_or(d0),
        sd: jp.seat_depth.unwrap_or(sd0),
        sl: jp.seat_length.unwrap_or(sl0),
        panel: jp.panel.unwrap_or(panel0),
        leg: jp.chord_leg.unwrap_or(sz.leg),
        t: jp.chord_thickness.unwrap_or(sz.t),
        web,
        gap: if web_kind == Web::Rod { web } else { sz.gap },
    };
    if g.t >= g.leg / 2.0 {
        ctx.p.fail(
            "chord_thickness",
            format!("chord_thickness {} must be under half the chord leg {}", fmt_ft_in(g.t), fmt_ft_in(g.leg)),
        );
        return None;
    }
    if g.leg + g.t >= g.sd {
        ctx.p.fail("chord_leg", format!("chord_leg {} does not fit above a {} deep seat (the seat angles sit under the top chord): use a leg under {} or a deeper seat_depth (SJI: 2 1/2 K, 5 LH/DLH, 7 1/2 girders)", fmt_ft_in(g.leg), fmt_ft_in(g.sd), fmt_ft_in(g.sd - g.t)));
        return None;
    }
    if g.d < g.sd + g.leg || g.d < 2.0 * g.leg + 2.0 {
        ctx.p.fail("depth", format!("a {tag} joist {} deep has no room for its {} chords and {} seat: check the designation's leading number (the depth in inches) or set \"depth\"", fmt_ft_in(g.d), fmt_ft_in(g.leg), fmt_ft_in(g.sd)));
        return None;
    }
    if g.panel < 2.0 * g.web + 2.0 {
        ctx.p.fail("panel", format!("panel {} is too short for {} webs: panel lengths are about 24 (K), 48 (LH) or the joist spacing (girders)", fmt_ft_in(g.panel), fmt_ft_in(g.web)));
        return None;
    }
    let what = match class {
        Class::Girder => "joist girder",
        _ => "joist",
    };
    let head = format!(
        "steel {what} {} ({tag}, {} deep, seat {} x {})",
        jp.designation.trim(),
        fmt_ft_in(g.d),
        fmt_ft_in(g.sd),
        fmt_ft_in(g.sl)
    );
    if jp.orient == Orient::Section {
        return section(ctx, &jp, &g, head);
    }
    elevation(ctx, &jp, &g, head)
}

fn zones_of(prisms: &[Prism]) -> Vec<Zone> {
    let mut zones: Vec<Zone> = Vec::new();
    for p in prisms {
        let mut b = Box::default();
        for l in &p.loops {
            b.add_box(crate::geom::loop_box(l));
        }
        match zones.iter_mut().find(|z| z.name == p.part) {
            Some(z) => z.box_.add_box(b),
            None => zones.push(Zone { name: p.part.clone(), loops: Vec::new(), box_: b }),
        }
    }
    for z in &mut zones {
        z.loops = model::one_loop(model::rect_loop(z.box_.x0, z.box_.y0, z.box_.x1, z.box_.y1));
    }
    zones
}

fn section(ctx: &mut Ctx, jp: &Params, g: &Geo, head: String) -> Option<Built> {
    if ctx.p.has("until") {
        ctx.p.fail(
            "until",
            "'until' only applies to orient elevation (a section joist runs along z: give 'length' or set 'z')".into(),
        );
        return None;
    }
    let length = if ctx.p.has("length") {
        Some(ctx.p.len_pos("length", None, "length of the joist along z, e.g. 24")?)
    } else {
        None
    };
    let (d, leg, t, gap) = (g.d, g.leg, g.t, g.gap);
    let w = 2.0 * leg + gap;
    let (xl, xr) = (leg, leg + gap);
    let bearing = jp.section_at == CutAt::Bearing;
    let mut prisms = vec![
        steel("top_chord", angle_loop(leg, leg, t, -1.0, -1.0, xl, d)),
        steel("top_chord", angle_loop(leg, leg, t, 1.0, -1.0, xr, d)),
    ];
    let y0 = if bearing {
        let a = g.sd - leg;
        let b = leg.min(a.max(2.0 * t + 0.25));
        prisms.push(steel("seat", angle_loop(a, b, t, -1.0, 1.0, xl, d - g.sd)));
        prisms.push(steel("seat", angle_loop(a, b, t, 1.0, 1.0, xr, d - g.sd)));
        d - g.sd
    } else {
        prisms.push(steel("bottom_chord", angle_loop(leg, leg, t, -1.0, 1.0, xl, 0.0)));
        prisms.push(steel("bottom_chord", angle_loop(leg, leg, t, 1.0, 1.0, xr, 0.0)));
        prisms.push(Prism { lengthwise: true, ..steel("web", model::rect_loop(xl, leg, xr, d - leg)) });
        0.0
    };
    let mut anchors = vec![
        ("seat", w / 2.0, d - g.sd),
        ("top_chord", w / 2.0, d),
        ("top_chord_bottom", w / 2.0, d - leg),
        ("top_flange_bottom", w / 2.0, d - t),
        ("top_chord_tip_left", 0.0, d - t),
        ("top_chord_tip_right", w, d - t),
    ];
    if !bearing {
        anchors.extend([
            ("bottom_chord", w / 2.0, 0.0),
            ("bottom_chord_top", w / 2.0, leg),
            ("bottom_chord_tip_left", 0.0, t),
            ("bottom_chord_tip_right", w, t),
            ("web_center", w / 2.0, d / 2.0),
        ]);
    }
    let zones = zones_of(&prisms);
    Some(Built {
        prisms,
        anchors: anchors.into_iter().map(|(n, x, y)| NamedAnchor { name: n.to_string(), p: V2::init(x, y) }).collect(),
        zones,
        box_: Box { x0: 0.0, y0, x1: w, y1: d },
        nat_z: length,
        info: format!(
            "{head} section at {}{}",
            jp.section_at.tag(),
            length.map_or(String::new(), |l| format!(" L={}", fmt_ft_in(l)))
        ),
        ..Built::default()
    })
}

fn elevation(ctx: &mut Ctx, jp: &Params, g: &Geo, head: String) -> Option<Built> {
    let mut until_note = String::new();
    let l = length_or_until(
        ctx,
        "x",
        "a steel_joist in elevation needs the length drawn from its end, e.g. \"length\": 48, or \"until\": \"other@anchor\"",
        &mut until_note,
    )?;
    if !ctx.p.ok {
        return None;
    }
    let (d, leg, t, gap, sd, sl, panel) = (g.d, g.leg, g.t, g.gap, g.sd, g.sl, g.panel);
    let (left, right) = (matches!(jp.seats, Ends::Left | Ends::Both), matches!(jp.seats, Ends::Right | Ends::Both));
    let both = left && right;
    let mirror = right && !left;
    let seated = left || right;
    let bce =
        |e: Ends, at_left: bool| matches!((e, at_left), (Ends::Both, _) | (Ends::Left, true) | (Ends::Right, false));
    let bce_a = bce(jp.bottom_chord_extension, !mirror);
    let bce_b = bce(jp.bottom_chord_extension, false);
    let a = if seated { (panel / 2.0).max(0.75 * d) } else { panel / 2.0 };
    if seated && l < sl + 1.0 {
        ctx.p.fail(
            "length",
            format!(
                "length {} is shorter than the {} seat: draw at least the seat and the first panel (about {})",
                fmt_ft_in(l),
                fmt_ft_in(sl),
                fmt_ft_in(a + panel / 2.0)
            ),
        );
        return None;
    }
    if both && l < 2.0 * a + panel / 2.0 {
        ctx.p.fail("seats", format!("seats \"both\" needs the whole joist: length {} is under two end panels ({}). For a stub at one bearing use seats \"left\" or \"right\"", fmt_ft_in(l), fmt_ft_in(2.0 * a + panel / 2.0)));
        return None;
    }
    let lead = if seated { sl + g.web / 2.0 } else { 0.0 };
    let mut chain: Vec<(f64, bool)> = vec![(lead, true)];
    let (mut bots, mut tops) = (Vec::new(), Vec::new());
    if both {
        let n = crate::math::round((l - 2.0 * a) / panel).max(1.0);
        let p = (l - 2.0 * a) / n;
        let n = crate::num::to_int_clamped::<usize>(n, 1, 4096);
        for k in 0..=n {
            let xb = a + k as f64 * p;
            bots.push(xb);
            chain.push((xb, false));
            if k < n {
                tops.push(xb + p / 2.0);
                chain.push((xb + p / 2.0, true));
            }
        }
        chain.push((l - lead, true));
    } else {
        let mut k = 0usize;
        loop {
            let x = a + k as f64 * panel / 2.0;
            let top = k % 2 == 1;
            chain.push((x, top));
            if x >= l - 1e-6 || k > 8192 {
                break;
            }
            if top {
                tops.push(x)
            } else {
                bots.push(x)
            }
            k += 1;
        }
    }
    let stub = leg.max(2.0);
    let b0 = if seated && !bce_a { (a - stub).max(0.0) } else { 0.0 };
    let b1 = if both && !bce_b { (l - a + stub).min(l) } else { l };
    let mx = |x: f64| if mirror { l - x } else { x };
    let span = |x0: f64, x1: f64| (mx(x0).min(mx(x1)), mx(x0).max(mx(x1)));
    let mut prisms: Vec<Prism> = Vec::new();
    let mut slab = |part: &str, x: (f64, f64), y0: f64, y1: f64, z0: f64, z1: f64| {
        prisms.push(Prism {
            zhalf: (z1 - z0) / 2.0,
            zoff: (z0 + z1) / 2.0,
            lengthwise: true,
            ..steel(part, model::rect_loop(x.0, y0.min(y1), x.1, y0.max(y1)))
        });
    };
    let mut pair = |part: &str, x: (f64, f64), heel: f64, dir: f64, vert: f64| {
        for s in [1.0, -1.0] {
            let (za, zb, zc) = (s * gap / 2.0, s * (gap / 2.0 + t), s * (gap / 2.0 + leg));
            slab(part, x, heel + dir * t, heel + dir * vert, za.min(zb), za.max(zb));
            slab(part, x, heel, heel + dir * t, za.min(zc), za.max(zc));
        }
    };
    pair("top_chord", (0.0, l), d, -1.0, leg);
    pair("bottom_chord", span(b0, b1), 0.0, 1.0, leg);
    if seated {
        pair("seat", span(0.0, sl), d - sd, 1.0, sd - leg);
    }
    if both {
        pair("seat", (l - sl, l), d - sd, 1.0, sd - leg);
    }
    let (yt, yb) = (d - t, t);
    for w in chain.windows(2) {
        let (p, q) = if w[0].1 { (w[0].0, w[1].0) } else { (w[1].0, w[0].0) };
        let len = crate::math::sqrt((q - p) * (q - p) + (yt - yb) * (yt - yb));
        let hw = g.web / 2.0 * len / (yt - yb);
        let poly = [V2::init(p - hw, yt), V2::init(q - hw, yb), V2::init(q + hw, yb), V2::init(p + hw, yt)];
        let full = (q - p).abs() + 2.0 * hw;
        let poly = clip_x(&clip_x(&poly, l, true), 0.0, false);
        let kept = poly.iter().map(|v| v.x).fold(f64::NEG_INFINITY, f64::max)
            - poly.iter().map(|v| v.x).fold(f64::INFINITY, f64::min);
        if poly.len() < 3 || kept < 0.4 * full {
            continue;
        }
        let lp = oriented_ccw(poly.iter().map(|v| Pt::new(mx(v.x), v.y)).collect());
        prisms.push(Prism { zhalf: gap / 2.0, lengthwise: true, ..steel("web", lp) });
    }
    let mut anchors: Vec<NamedAnchor> = Vec::new();
    let mut add = |n: String, x: f64, y: f64| anchors.push(NamedAnchor { name: n, p: V2::init(x, y) });
    add("seat".into(), if mirror { l } else { 0.0 }, d - sd);
    add("seat_left".into(), 0.0, d - sd);
    add("seat_left_inner".into(), sl, d - sd);
    add("seat_right".into(), l, d - sd);
    add("seat_right_inner".into(), l - sl, d - sd);
    let (bx0, bx1) = span(b0, b1);
    for (n, y, x0, x1) in [
        ("top_chord", d, 0.0, l),
        ("top_chord_bottom", d - leg, 0.0, l),
        ("top_flange_bottom", d - t, 0.0, l),
        ("bottom_chord_top", leg, bx0, bx1),
        ("bottom_chord", 0.0, bx0, bx1),
    ] {
        add(n.into(), (x0 + x1) / 2.0, y);
        add(format!("{n}_start"), x0, y);
        add(format!("{n}_end"), x1, y);
    }
    let order = |v: &[f64]| {
        let mut xs: Vec<f64> = v.iter().map(|x| mx(*x)).collect();
        if mirror {
            xs.reverse();
        }
        xs
    };
    for (k, x) in order(&tops).into_iter().enumerate() {
        add(format!("top_panel_{}", k + 1), x, d);
    }
    for (k, x) in order(&bots).into_iter().enumerate() {
        add(format!("bottom_panel_{}", k + 1), x, 0.0);
    }
    let zones = zones_of(&prisms);
    let ext = if jp.bottom_chord_extension == Ends::None {
        String::new()
    } else {
        format!(", bottom chord extension {}", jp.bottom_chord_extension.tag())
    };
    Some(Built {
        prisms,
        anchors,
        zones,
        box_: Box { x0: 0.0, y0: 0.0, x1: l, y1: d },
        nat_z: Some(2.0 * leg + gap),
        info: format!("{head} elevation L={}{until_note}, seats {}{ext}", fmt_ft_in(l), jp.seats.tag()),
        ..Built::default()
    })
}
