//! truss (SPEC 5.8, 27.1): its parameters (`Params`, the single source for the parser and the catalog) and its builder.
//! Side view (the truss in the section plane): a heel/tail end for the pitched, scissor and mono profiles, or a
//! parallel-chord (floor / flat) truss with its open webs. End-on (the truss cut across, running along z): the chords cut
//! and a web strip beyond them.

use super::common::{Ctx, fmt_ft_in, fmt_num, mirror_built, parse_sawn, quad_of, zone_rect};
use crate::clip;
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
param_enum! { pub enum Webs { Auto = "auto", Bare = "none", KingPost = "king_post", Fink = "fink", Howe = "howe", Warren = "warren" } }

params! {
    pub struct Params {
        profile: Enum(Profile) = Profile::Pitched => { desc: "pitched (heel rising inward) | scissor (bottom chord rises at bottom_pitch) | mono (high end: end vertical heel_height tall, top chord falling inward) | parallel_chord (floor or flat truss: depth, open webs)" },
        orient: Enum(Orient) = Orient::Side => { desc: "side: the truss lies in the section plane | end_on: cut across (trusses parallel to the section): chords cut, a web strip beyond; needs depth" },
        exterior: Enum(Exterior) = Exterior::Left => { desc: "side of the heel/overhang (right mirrors)" },
        pitch: Raw = Value::String("4:12".into()) => { def: "4:12", desc: "rise:run of the top chord (not for parallel_chord)" },
        bottom_pitch: OptRaw => { def: "scissor: half the pitch", desc: "scissor: rise:run of the bottom chord, less than pitch" },
        top_chord: Str = "2x4" => { def: "2x4 (parallel_chord: 4x2)", desc: "sawn nominal size, depth in-plane; \"4x2\" is a 2x4 laid flat (floor truss chord: 1 1/2 in-plane, 3 1/2 thick)" },
        bottom_chord: Str = "2x4" => { def: "2x4 (parallel_chord: 4x2)", desc: "as top_chord" },
        heel: Enum(Heel) = Heel::Standard => { desc: "standard (top chord's lower edge through the bottom of the bottom chord at the bearing's outer edge) | raised (energy heel: a heel vertical heel_height tall)" },
        heel_height: OptLenPos => { def: "raised: 12; bearing face: top chord + bottom chord depth; mono: from span", desc: "raised heel, mono high end, bearing face: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
        span: OptLenPos => { def: "schematic: about 2 1/4 x span_shown (mono with heel_height: where its low end closes)", desc: "pitched, scissor, mono: overall span, bearing outer edge to the far bearing's outer edge; places the web panel points; span_shown >= span draws the whole truss" },
        webs: Enum(Webs) = Webs::Auto => { desc: "pitched/scissor: auto (by span: king_post to 16', fink to 32', howe beyond; fink when span is schematic) | fink | howe | king_post | none; mono: auto (warren) | warren | howe | none" },
        parapet: OptLenPos => { desc: "integral parapet: the end vertical at the bearing runs this far above the top of the top chord, which butts into it (no overhang; pitched, scissor, mono, parallel_chord)" },
        depth: OptLenPos => { desc: "parallel_chord: overall depth, bottom of bottom chord to top of top chord (e.g. 18); end_on: the truss height at the cut" },
        bearing: Enum(Bearing) = Bearing::BottomChord => { desc: "bottom_chord: bears under its bottom chord | top_chord (parallel_chord): the top chord runs over the support, the bottom chord and end vertical drop inside it | face: hung at a beam or ledger face by its end vertical (no overhang; heel_height = the end height)" },
        bearing_width: LenPos = 3.5 => { desc: "width of the support under the bearing" },
        overhang: Len = 12.0 => { def: "12 (parallel_chord and mono: 0)", desc: "horizontal distance from outer face of bearing to the tail end" },
        tail: Enum(Tail) = Tail::Plumb => { desc: "plumb | square cut" },
        span_shown: LenPos = 48.0 => { desc: "how far into the building to draw (crop/break at the end)" },
        verticals: OptRaw => { desc: "parallel_chord side: vertical webs at these panel points, each the distance from bearing_outer to the web's centre (e.g. [24]: the vertical the truss maker adds over a shear panel or a point load); the diagonals restart beside each one. Anchors vertical_1.. (centre), vertical_1_top / _bottom (on the chords); nail to it as member \"truss.vertical\"" },
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
        ctx.p.fail(
            "top_chord",
            format!(
                "top_chord \"{top}\" is not a sawn nominal size like \"2x4\", \"2x6\" (or \"4x2\" for a 2x4 laid flat)"
            ),
        );
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
    let side_only: &[&str] = if tp.orient == Orient::EndOn {
        &["span", "webs", "parapet", "verticals"]
    } else if parallel {
        &["span", "webs"]
    } else {
        &["verticals"]
    };
    if let Some(key) = side_only.iter().find(|k| ctx.p.has(k)) {
        let what = if tp.orient == Orient::EndOn {
            "a side-view truss (orient \"side\"): an end-on truss is cut across"
        } else if *key == "verticals" {
            "profile \"parallel_chord\" (a pitched, scissor or mono truss takes a web pattern: webs \"howe\" has a vertical at every panel point)"
        } else {
            "profiles pitched, scissor and mono (a parallel_chord truss spaces its webs by web_spacing)"
        };
        ctx.p.fail(key, format!("'{key}' only applies to {what}"));
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

fn parse_pitch(ctx: &mut Ctx, key: &str, v: &Value) -> Option<f64> {
    let Some(theta) = units::parse_slope(v) else {
        ctx.p.fail(
            key,
            format!("param '{key}' must be rise:run like \"4:12\" or degrees (got {})", model::kind_or_text(v)),
        );
        return None;
    };
    if theta <= 0.0 || theta >= math::PI / 2.0 {
        ctx.p.fail(key, format!("{key} must slope up (between 0 and 90 degrees)"));
        return None;
    }
    Some(theta)
}

/// In-plane width of a heel-profile web or heel vertical (a 2x4 on edge).
const WEB_W: f64 = 3.5;

/// The outline of a pitched, scissor or mono truss over its whole span, in local coordinates (the drawn end's bearing
/// outer edge at the origin, the truss rising toward +x): the top chord's lower edge and the bottom chord's top edge.
struct Outline {
    l: f64,
    y0: f64,
    sl: f64,
    mono: bool,
    scissor: bool,
    sb: f64,
    bw: f64,
    db: f64,
    db_v: f64,
    v_thick: f64,
}

impl Outline {
    fn half(&self, x: f64) -> f64 {
        if self.mono { x } else { x.min(self.l - x) }
    }
    /// Lower edge of the top chord (extended past the bearings for the tails).
    fn yt(&self, x: f64) -> f64 {
        self.y0 + self.sl * self.half(x)
    }
    /// Underside of the bottom chord.
    fn yb0(&self, x: f64) -> f64 {
        if self.scissor { self.sb * (self.half(x) - self.bw).max(0.0) } else { 0.0 }
    }
    /// Top edge of the bottom chord.
    fn yb(&self, x: f64) -> f64 {
        if self.scissor { self.yb0(x) + self.db_v } else { self.db }
    }
    /// Direction of the top chord's lower edge at x (left to right).
    fn top_dir(&self, x: f64) -> V2 {
        V2::init(1.0, if self.mono || x < self.l / 2.0 { self.sl } else { -self.sl })
    }
    /// Direction of the bottom chord's top edge at x.
    fn bot_dir(&self, x: f64) -> V2 {
        let x0 = self.half(x);
        let rising = self.scissor && x0 > self.bw;
        let slope = if !rising {
            0.0
        } else if self.mono || x < self.l / 2.0 {
            self.sb
        } else {
            -self.sb
        };
        V2::init(1.0, slope)
    }
    fn top(&self, x: f64) -> V2 {
        V2::init(x, self.yt(x))
    }
    fn bot(&self, x: f64) -> V2 {
        V2::init(x, self.yb(x))
    }
    /// A web panel point on the top chord's centreline.
    fn top_panel(&self, x: f64) -> V2 {
        V2::init(x, self.yt(x) + self.v_thick / 2.0)
    }
    /// A web panel point on the bottom chord's centreline.
    fn bot_panel(&self, x: f64) -> V2 {
        V2::init(x, self.yb(x) - self.db_v / 2.0)
    }
    /// The x in [lo, hi] where the top chord's lower edge comes down to the bottom chord's top (the heel joint), by
    /// bisection on a stretch where the clear height rises from <= 0 at `lo`.
    fn meet(&self, lo: f64, hi: f64) -> f64 {
        let (mut a, mut b) = (lo, hi);
        for _ in 0..60 {
            let m = (a + b) / 2.0;
            if self.yt(m) - self.yb(m) > 0.0 {
                b = m;
            } else {
                a = m;
            }
        }
        (a + b) / 2.0
    }
    /// Everything under the top chord's lower edge, from `x0` to `x1`.
    fn under(&self, x0: f64, x1: f64) -> Vec<V2> {
        let deep = -1.0e4;
        let mut pts = vec![V2::init(x0, deep), V2::init(x1, deep), self.top(x1)];
        if !self.mono && x0 < self.l / 2.0 && self.l / 2.0 < x1 {
            pts.push(self.top(self.l / 2.0));
        }
        pts.push(self.top(x0));
        pts
    }
    /// Everything above the bottom chord's top edge, from `x0` to `x1`.
    fn above(&self, x0: f64, x1: f64) -> Vec<V2> {
        let high = 1.0e4;
        let mut pts = vec![self.bot(x0)];
        if self.scissor {
            for x in [self.bw, self.l / 2.0, self.l - self.bw] {
                if x0 < x && x < x1 {
                    pts.push(self.bot(x));
                }
            }
        }
        pts.push(self.bot(x1));
        pts.push(V2::init(x1, high));
        pts.push(V2::init(x0, high));
        pts
    }
    /// The bottom chord band from `x0` to `x1` (both edges).
    fn band(&self, x0: f64, x1: f64) -> Vec<V2> {
        let mut xs = vec![x0];
        if self.scissor {
            for x in [self.bw, self.l / 2.0, self.l - self.bw] {
                if x0 < x && x < x1 {
                    xs.push(x);
                }
            }
        }
        xs.push(x1);
        let mut pts: Vec<V2> = xs.iter().map(|&x| V2::init(x, self.yb0(x))).collect();
        pts.extend(xs.iter().rev().map(|&x| V2::init(x, self.yb(x))));
        pts
    }
}

/// Wood prisms (one per outer loop, with the holes inside it) from a boolean result.
fn region_prisms(part: &str, region: &[Vec<V2>]) -> Vec<Prism> {
    let mut out = Vec::new();
    for outer in region.iter().filter(|l| geom::signed_area_v(l) > 1e-6) {
        let mut loops = vec![outer.iter().map(|&p| Pt::at(p, 0.0)).collect::<Vec<Pt>>()];
        for h in region.iter().filter(|h| geom::signed_area_v(h) < -1e-6) {
            if h.first().is_some_and(|&p| geom::point_in_loop_eo(p, outer)) {
                loops.push(h.iter().map(|&p| Pt::at(p, 0.0)).collect());
            }
        }
        out.push(Prism { part: part.to_string(), material: "wood".into(), loops, ..Prism::default() });
    }
    out
}

/// A web strip `w` wide along the centreline `a`-`b` (`a` on the top chord running along `ca`, `b` on the bottom chord
/// along `cb`), run on past both ends far enough that clipping it to the clear space between the chords cuts both of its
/// edges on the chord edges.
fn web_strip(a: V2, b: V2, ca: V2, cb: V2, w: f64) -> Vec<V2> {
    let d = b.sub(a).norm();
    let n = d.perp().scale(w / 2.0);
    let ext = |c: V2| (w / 2.0 / d.cross(c.norm()).abs().max(0.05)).min(48.0) + 1.0;
    let (a, b) = (a.sub(d.scale(ext(ca))), b.add(d.scale(ext(cb))));
    let mut lp = vec![a.add(n), b.add(n), b.sub(n), a.sub(n)];
    clip::orient_ccw(&mut lp);
    lp
}

/// The web centrelines of `pattern` over the whole truss, as (top chord point, bottom chord point) pairs. `xl`/`xr`: the
/// inner faces of the end verticals (heel ends: the bearing outer edges).
fn web_lines(o: &Outline, pattern: Webs, xl: f64, xr: f64, tall: [bool; 2]) -> Vec<(V2, V2)> {
    let l = o.l;
    let clear = |x: f64| o.yt(x) - o.yb(x);
    let mut half: Vec<(f64, f64)> = Vec::new();
    let mut center: Vec<(f64, f64)> = Vec::new();
    if o.mono {
        let mut xs = vec![xl];
        let mut x = xl;
        while clear(x) >= 6.0 && x < xr - 1.0 && xs.len() < 64 {
            x = (x + clear(x)).min(xr);
            if xr - x < 6.0 {
                x = xr;
            }
            xs.push(x);
        }
        let mut out = Vec::new();
        match pattern {
            Webs::Howe => {
                for w in xs.windows(2) {
                    out.push((o.top_panel(w[1]), o.bot_panel(w[1])));
                    out.push((o.top_panel(w[0]), o.bot_panel(w[1])));
                }
            }
            Webs::Warren | Webs::Auto => {
                for (k, w) in xs.windows(2).enumerate() {
                    out.push(if k % 2 == 0 {
                        (o.top_panel(w[0]), o.bot_panel(w[1]))
                    } else {
                        (o.top_panel(w[1]), o.bot_panel(w[0]))
                    });
                }
            }
            _ => {}
        }
        return out.into_iter().filter(|(t, b)| clear(t.x) >= 2.0 && clear(b.x) >= 2.0).collect();
    }
    let mut first_bottom = None;
    match pattern {
        Webs::KingPost => center.push((l / 2.0, l / 2.0)),
        Webs::Fink | Webs::Auto => {
            half.push((l / 4.0, l / 3.0));
            half.push((l / 2.0, l / 3.0));
            first_bottom = Some(l / 3.0);
        }
        Webs::Howe => {
            let n = howe_panels(l);
            let p = l / n as f64;
            center.push((l / 2.0, l / 2.0));
            for k in 1..n / 2 {
                half.push((k as f64 * p, k as f64 * p));
                half.push((k as f64 * p, (k + 1) as f64 * p));
            }
            first_bottom = Some(p);
        }
        _ => {}
    }
    let mut out: Vec<(V2, V2)> = center.iter().map(|&(t, b)| (o.top_panel(t), o.bot_panel(b))).collect();
    for &(t, b) in &half {
        out.push((o.top_panel(t), o.bot_panel(b)));
        out.push((o.top_panel(l - t), o.bot_panel(l - b)));
    }
    if let Some(fb) = first_bottom {
        if tall[0] {
            out.push((o.top_panel(xl), o.bot_panel(fb)));
        }
        if tall[1] {
            out.push((o.top_panel(xr), o.bot_panel(l - fb)));
        }
    }
    out.into_iter().filter(|(t, b)| clear(t.x) >= 2.0 && clear(b.x) >= 2.0).collect()
}

/// Howe panel count for a span: 4 up to 28', 6 up to 40', else 8.
fn howe_panels(l: f64) -> usize {
    if l <= 28.0 * 12.0 {
        4
    } else if l <= 40.0 * 12.0 {
        6
    } else {
        8
    }
}

/// Pitched, scissor and mono trusses in side view: the drawn end (heel, raised heel, hung end, mono high end or integral
/// parapet), the tail, the chords and the web pattern of the whole truss, cut at `span_shown` (or the whole truss).
fn heel_side(ctx: &mut Ctx, tp: &Params, ch: &Chords) -> Option<Built> {
    let theta = parse_pitch(ctx, "pitch", &tp.pitch)?;
    let (dt, db) = (ch.dt, ch.db);
    let mono = tp.profile == Profile::Mono;
    let scissor = tp.profile == Profile::Scissor;
    let face = tp.bearing == Bearing::Face;
    let parapet = tp.parapet;
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
            ctx.p.fail(
                "bottom_pitch",
                format!(
                    "a scissor truss's bottom_pitch must be flatter than its pitch {}:12 (got {}:12)",
                    fmt_num(s * 12.0),
                    fmt_num(math::tan(theta_b) * 12.0)
                ),
            );
            return None;
        }
    } else if ctx.p.has("bottom_pitch") {
        ctx.p.fail("bottom_pitch", "'bottom_pitch' only applies to profile \"scissor\"".into());
        return None;
    }
    if mono && matches!(tp.webs, Webs::KingPost | Webs::Fink) {
        ctx.p.fail("webs", format!("webs \"{}\" is a pitched-truss pattern; a mono truss takes \"warren\" (the default), \"howe\" or \"none\"", tp.webs.tag()));
        return None;
    }
    if !mono && tp.webs == Webs::Warren {
        ctx.p.fail("webs", "webs \"warren\" is the mono pattern; a pitched or scissor truss takes \"fink\", \"howe\", \"king_post\" or \"none\" (default \"auto\": by span)".into());
        return None;
    }
    let sb = math::tan(theta_b);
    let db_v = db / math::cos(theta_b);
    if let Some(h) = parapet {
        if h < 2.0 {
            ctx.p.fail("parapet", format!("parapet {} is too short: it is the height of the end vertical above the top of the top chord (e.g. 24)", fmt_ft_in(h)));
            return None;
        }
        if ctx.p.has("overhang") && tp.overhang != 0.0 {
            ctx.p.fail("overhang", format!("a truss with an integral parapet has no overhang (its end vertical runs up past the top chord): remove 'overhang' (got {})", fmt_ft_in(tp.overhang)));
            return None;
        }
    }
    let tall_end = tp.heel == Heel::Raised || mono || face || parapet.is_some();
    let explicit_span = tp.span;
    let mut heel_height = tp.heel_height;
    if heel_height.is_none() && tall_end && !mono {
        heel_height = Some(if face {
            v_thick + db
        } else if tp.heel == Heel::Raised {
            12.0
        } else {
            v_thick
        });
    }
    let y0_of = |hh: f64| db_v + hh - v_thick;
    let peak_span =
        |clear: f64, y0: f64| 2.0 * (clear + db_v - y0 - sb * tp.bearing_width).max(0.0) / (s - sb).max(1e-3);
    let schematic = |y0: f64| -> f64 {
        let fink = if tall_end { 0.0 } else { 4.0 * (8.0 + db_v) / (s - sb).max(1e-3) };
        math::ceil(2.25 * tp.span_shown).max(math::ceil(fink)).max(math::ceil(peak_span(8.0, y0)))
    };
    let (l, y0) = if mono {
        match (heel_height, explicit_span) {
            (Some(hh), Some(l)) => {
                if hh < v_thick {
                    ctx.p.fail("heel_height", format!("heel_height {} is smaller than the top chord's vertical thickness {}; the high end must be at least that tall", fmt_num(hh), fmt_num(v_thick)));
                    return None;
                }
                let y0 = y0_of(hh);
                if y0 - s * l < -1e-6 {
                    ctx.p.fail("span", format!("the mono top chord falls below the bottom chord before its far end: at {}:12 a {} high end reaches the bottom of the bottom chord at x = {}: set span to at most {}, or heel_height to at least {} (or remove one: the other follows)", fmt_num(s * 12.0), fmt_ft_in(hh), fmt_num(y0 / s), fmt_num(math::floor(y0 / s)), fmt_num(math::ceil((s * l - db_v + v_thick) * 8.0) / 8.0)));
                    return None;
                }
                (l, y0)
            }
            (Some(hh), None) => {
                if hh < v_thick {
                    ctx.p.fail("heel_height", format!("heel_height {} is smaller than the top chord's vertical thickness {}; the high end must be at least that tall", fmt_num(hh), fmt_num(v_thick)));
                    return None;
                }
                let y0 = y0_of(hh);
                (y0 / s, y0)
            }
            (None, l) => {
                let l = l.unwrap_or_else(|| math::ceil(2.25 * tp.span_shown));
                heel_height = Some(s * l + v_thick - db_v);
                (l, s * l)
            }
        }
    } else {
        let y0 = match heel_height.filter(|_| tall_end) {
            Some(hh) => {
                if hh < v_thick {
                    ctx.p.fail("heel_height", format!("heel_height {} is smaller than the top chord's vertical thickness {}; the raised heel must be at least that tall", fmt_num(hh), fmt_num(v_thick)));
                    return None;
                }
                y0_of(hh)
            }
            None => 0.0,
        };
        (explicit_span.unwrap_or_else(|| schematic(y0)), y0)
    };
    let o = Outline { l, y0, sl: sigma * s, mono, scissor, sb, bw: tp.bearing_width.min(l / 2.0), db, db_v, v_thick };
    if !mono && o.yt(l / 2.0) - o.yb(l / 2.0) < 3.0 {
        let heel = if tall_end { "a taller heel_height" } else { "a raised heel" };
        let what = if scissor {
            "a flatter bottom_pitch, a steeper pitch or a longer span".to_string()
        } else {
            format!("a steeper pitch, {heel} or a longer span")
        };
        ctx.p.fail(
            "span",
            format!(
                "span {} leaves {} between the chords at the peak (3\" minimum); give {what} (span at least {} at this pitch and heel)",
                fmt_ft_in(l),
                fmt_ft_in((o.yt(l / 2.0) - o.yb(l / 2.0)).max(0.0)),
                fmt_num(math::ceil(peak_span(3.0, y0)))
            ),
        );
        return None;
    }
    let overhang = if (mono || face || parapet.is_some()) && !ctx.p.has("overhang") { 0.0 } else { tp.overhang };
    if face && overhang != 0.0 {
        ctx.p.fail(
            "overhang",
            format!(
                "a truss hung at a face (bearing \"face\") has no overhang: remove 'overhang' (got {})",
                fmt_ft_in(overhang)
            ),
        );
        return None;
    }
    let full = tp.span_shown >= l - 1e-9;
    let xe = tp.span_shown.min(l);
    let x_tail = -overhang;
    let far_tail = if full && !mono { l + overhang } else { xe };
    let ev_w = if parapet.is_some() || mono || face { dt.max(WEB_W) } else { WEB_W }.min(xe / 2.0);
    let tall_far_end = if mono { o.yt(l) >= db_v - 1e-9 } else { tall_end };
    let tall_far = full && tall_far_end;
    let y_par = parapet.map(|h| o.yt(0.0) + v_thick + h);
    let mut prisms: Vec<Prism> = Vec::new();
    let mut zones: Vec<Zone> = Vec::new();

    let tail_bottom = V2::init(x_tail, o.yt(x_tail));
    let mut tail_top = V2::init(x_tail, tail_bottom.y + v_thick);
    if tp.tail == Tail::Square {
        tail_top = tail_bottom.add(V2::init(-sigma * math::sin(theta), math::cos(theta)).scale(dt));
    }
    let x_tc0 = if parapet.is_some() { ev_w } else { x_tail };
    let mut top_loops: Vec<Vec<Pt>> = Vec::new();
    let xa = if mono { xe } else { xe.min(l / 2.0) };
    top_loops.push(if parapet.is_some() {
        vec![
            Pt::at(o.top(x_tc0), 0.0),
            Pt::at(o.top(xa), 0.0),
            Pt::new(xa, o.yt(xa) + v_thick),
            Pt::new(x_tc0, o.yt(x_tc0) + v_thick),
        ]
    } else {
        vec![Pt::at(tail_bottom, 0.0), Pt::at(o.top(xa), 0.0), Pt::new(xa, o.yt(xa) + v_thick), Pt::at(tail_top, 0.0)]
    });
    if !mono && far_tail > l / 2.0 + 1e-9 {
        let xm = l / 2.0;
        let mut far_top = V2::init(far_tail, o.yt(far_tail) + v_thick);
        if full && tp.tail == Tail::Square {
            far_top = V2::init(l - tail_top.x, tail_top.y);
        }
        let mut lp = vec![
            Pt::at(o.top(xm), 0.0),
            Pt::at(o.top(far_tail), 0.0),
            Pt::at(far_top, 0.0),
            Pt::new(xm, o.yt(xm) + v_thick),
        ];
        if full && parapet.is_some() {
            lp = vec![
                Pt::at(o.top(xm), 0.0),
                Pt::at(o.top(l - ev_w), 0.0),
                Pt::new(l - ev_w, o.yt(l - ev_w) + v_thick),
                Pt::new(xm, o.yt(xm) + v_thick),
            ];
        }
        top_loops.push(lp);
    }
    let xbe = xe;
    let bot_region =
        clip::boolean(&[o.band(0.0, xbe)], &[o.under(x_tail - 10.0, far_tail + 10.0)], clip::Op::Intersect);
    let bots = region_prisms("bottom_chord", &bot_region);
    for pr in &bots {
        if let Some(lp) = pr.loops.first() {
            zones.push(zone_of("bottom_chord", lp));
        }
    }
    prisms.extend(bots);
    for lp in &top_loops {
        zones.push(zone_of("top_chord", lp));
        prisms.push(wood("top_chord", lp.clone()));
    }
    let mut extra: Vec<NamedAnchor> = Vec::new();
    let ev_part = if mono || face || parapet.is_some() { "end_vertical" } else { "heel_web" };
    let vertical = |x0: f64, x1: f64, top: Option<f64>| -> Vec<Pt> {
        match top {
            Some(yp) => vec![Pt::at(o.bot(x0), 0.0), Pt::at(o.bot(x1), 0.0), Pt::new(x1, yp), Pt::new(x0, yp)],
            None => {
                vec![Pt::at(o.bot(x0), 0.0), Pt::at(o.bot(x1), 0.0), Pt::at(o.top(x1), 0.0), Pt::at(o.top(x0), 0.0)]
            }
        }
    };
    if tall_end {
        let lp = vertical(0.0, ev_w, y_par);
        zones.push(zone_of(ev_part, &lp));
        prisms.push(wood(ev_part, lp));
        if ev_part == "end_vertical" {
            extra.push(anchor("end_vertical", V2::init(0.0, (db_v + o.y0) / 2.0)));
            extra.push(anchor("end_top", V2::init(0.0, y_par.unwrap_or(o.y0 + v_thick))));
            extra.push(anchor("end_bottom", V2::init(0.0, 0.0)));
        }
        if let Some(yp) = y_par {
            extra.push(anchor("parapet_top", V2::init(0.0, yp)));
            extra.push(anchor("parapet_top_inner", V2::init(ev_w, yp)));
            extra.push(anchor("parapet_roof_inner", V2::init(ev_w, o.yt(ev_w) + v_thick)));
        }
    }
    if tall_far {
        let lp = vertical(l - ev_w, l, y_par);
        prisms.push(wood(ev_part, lp));
    }
    let pattern = match tp.webs {
        Webs::Auto if mono => Webs::Warren,
        Webs::Auto if explicit_span.is_some() => {
            if l <= 16.0 * 12.0 {
                Webs::KingPost
            } else if l <= 32.0 * 12.0 {
                Webs::Fink
            } else {
                Webs::Howe
            }
        }
        Webs::Auto => Webs::Fink,
        w => w,
    };
    let xl = if tall_end { ev_w } else { 0.0 };
    let xr = if tall_far_end { l - ev_w } else { l };
    let lines = web_lines(&o, pattern, xl, xr, [tall_end, tall_far_end]);
    let mut first_panel: Option<f64> = None;
    if !lines.is_empty() && xr.min(xe) > xl + 0.5 {
        let mut strips: Vec<Vec<V2>> = Vec::new();
        for &(a, b) in &lines {
            strips = clip::boolean(&strips, &[web_strip(a, b, o.top_dir(a.x), o.bot_dir(b.x), WEB_W)], clip::Op::Unite);
            let x = a.x.min(b.x);
            first_panel = Some(first_panel.map_or(x, |f: f64| f.min(x)));
        }
        let clear = clip::boolean(&[o.under(xl, xr.min(xe))], &[o.above(xl, xr.min(xe))], clip::Op::Intersect);
        let webs = clip::boolean(&strips, &clear, clip::Op::Intersect);
        let wps = region_prisms("web", &webs);
        if !wps.is_empty() {
            let b = box_of(&wps);
            zones.push(zone_rect("web", b.x0, b.y0, b.x1, b.y1));
        }
        prisms.extend(wps);
    }
    let x_h = if tall_end { 0.0 } else { o.meet(0.0, l / 2.0) };
    if tp.plate {
        let (px0, px1, py0, py1) = if tall_end {
            let py1 = (db_v + 0.5).max(o.y0.min(o.yt(5.25)) + 0.5);
            (0.25, 5.25, 0.25, py1)
        } else {
            (0.25, x_h.clamp(4.0, 8.0).min(xe), 0.25, db.min(v_thick) - 0.25)
        };
        let mut plates = vec![(px0, px1)];
        if full && !mono {
            plates.push((l - px1, l - px0));
        }
        plates.retain(|&(a, b)| b > a + 0.25 && py1 > py0 + 0.25);
        for &(a, b) in &plates {
            prisms.push(Prism {
                part: "plate".into(),
                material: "steel".into(),
                loops: model::one_loop(model::rect_loop(a, py0, b, py1)),
                kind: Kind::Ghost,
                pen: Some(Pen::Hidden),
                embedded: true,
                ..Prism::default()
            });
        }
        if !plates.is_empty() {
            zones.push(zone_rect("plate", px0, py0, px1, py1));
        }
    }
    if parapet.is_none() {
        let mut tb = Box::default();
        tb.add_point(tail_bottom.x, tail_bottom.y);
        tb.add_point(tail_top.x, tail_top.y);
        tb.add_point(0.0, o.yt(0.0));
        tb.add_point(0.0, o.yt(0.0) + v_thick);
        zones.push(Zone {
            name: "tail".into(),
            loops: model::one_loop(model::rect_loop(tb.x0, tb.y0, tb.x1, tb.y1)),
            box_: tb,
        });
    }
    let top_at_bearing = o.yt(0.0) + v_thick;
    let x_end = if full { far_tail } else { xe };
    let mut anchors =
        vec![anchor("bearing_outer", V2::init(0.0, 0.0)), anchor("bearing_inner", V2::init(tp.bearing_width, 0.0))];
    if parapet.is_none() {
        anchors.push(anchor("tail_bottom", tail_bottom));
        anchors.push(anchor("tail_top", tail_top));
    }
    anchors.extend([
        anchor("top_chord_at_bearing", V2::init(0.0, top_at_bearing)),
        anchor(
            "heel_outer",
            V2::init(0.0, if tall_end { (db_v + top_at_bearing) / 2.0 } else { top_at_bearing / 2.0 }),
        ),
        anchor("top_chord_bottom_at_bearing", V2::init(0.0, o.yt(0.0))),
        anchor("heel_block_top", V2::init(0.0, top_at_bearing)),
        anchor("top_chord_end", V2::init(x_end, o.yt(x_end) + v_thick)),
        anchor("bottom_chord_top_inner", V2::init(xbe, o.yb(xbe))),
    ]);
    if !tall_end {
        anchors.push(anchor("heel_joint", o.bot(x_h)));
    }
    if !mono && xe >= l / 2.0 - 1e-9 {
        anchors.push(anchor("peak", V2::init(l / 2.0, o.yt(l / 2.0) + v_thick)));
    }
    if full {
        anchors.push(anchor("bearing_far", V2::init(l, 0.0)));
    }
    anchors.extend(extra);
    let profile = match tp.profile {
        Profile::Scissor => format!(" scissor (bottom {}:12)", fmt_num(sb * 12.0)),
        Profile::Mono => " mono".to_string(),
        _ => String::new(),
    };
    let end = if let Some(h) = parapet {
        format!(" integral parapet {}", fmt_ft_in(h))
    } else if face {
        " hung at face".to_string()
    } else if mono {
        format!(" high end {}", fmt_ft_in(heel_height.unwrap_or(0.0)))
    } else if tp.heel == Heel::Raised {
        format!(" raised heel {}", fmt_ft_in(heel_height.unwrap_or(0.0)))
    } else {
        " standard heel".to_string()
    };
    let span_txt = if explicit_span.is_some() || mono && tp.heel_height.is_some() {
        fmt_ft_in(l)
    } else {
        format!("{} schematic", fmt_ft_in(l))
    };
    let webs_txt = match (pattern, first_panel) {
        (Webs::Bare, _) => "no webs".to_string(),
        (_, Some(x)) if x >= xe => {
            format!("{} webs from x={} (past span_shown)", pattern.tag(), fmt_num(math::floor(x)))
        }
        (_, _) => format!("{} webs", pattern.tag()),
    };
    Some(Built {
        box_: box_of(&prisms),
        prisms,
        anchors,
        zones,
        nat_z: Some(ch.tt.max(ch.tb)),
        pitch: sigma * theta,
        info: format!(
            "truss {}:12{profile}{end}, {}+{} chords, ovh {}, span {span_txt}, {webs_txt}",
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
        ctx.p.fail(
            "depth",
            "profile \"parallel_chord\" needs 'depth' (overall, bottom of bottom chord to top of top chord, e.g. 18)"
                .into(),
        );
        return None;
    };
    for key in ["heel", "heel_height", "bottom_pitch", "pitch"] {
        if ctx.p.has(key) {
            ctx.p.fail(
                key,
                format!("'{key}' does not apply to profile \"parallel_chord\" (its chords are level: give depth)"),
            );
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
    if let Some(h) = tp.parapet {
        if tcb {
            ctx.p.fail("parapet", "an integral parapet stands on the end vertical at the bearing: use bearing \"bottom_chord\" (a top-chord-bearing truss runs its top chord over the wall)".into());
            return None;
        }
        if h < 2.0 {
            ctx.p.fail("parapet", format!("parapet {} is too short: it is the height of the end vertical above the top of the top chord (e.g. 24)", fmt_ft_in(h)));
            return None;
        }
        if overhang != 0.0 {
            ctx.p.fail("overhang", format!("a truss with an integral parapet has no overhang (its end vertical runs up past the top chord): remove 'overhang' (got {})", fmt_ft_in(overhang)));
            return None;
        }
    }
    if face && overhang != 0.0 {
        ctx.p.fail(
            "overhang",
            format!(
                "a truss hung at a face (bearing \"face\") has no overhang: remove 'overhang' (got {})",
                fmt_ft_in(overhang)
            ),
        );
        return None;
    }
    let flat = ch.dt < ch.tt;
    let w = if flat { dt.min(db) } else { 3.5 };
    let xe = tp.span_shown;
    let x_body = if tcb { tp.bearing_width } else { 0.0 };
    if xe < x_body + w + 6.0 {
        ctx.p.fail(
            "span_shown",
            format!(
                "span_shown {} is too short to draw the truss body: give at least {}",
                fmt_num(xe),
                fmt_num(math::ceil(x_body + w + 6.0))
            ),
        );
        return None;
    }
    let y_ev = tp.parapet.map_or(d - dt, |h| d + h);
    let x_top0 = if tp.parapet.is_some() { x_body + w } else { -overhang };
    let top_loop = model::rect_loop(x_top0, d - dt, xe, d);
    let bot_loop = model::rect_loop(x_body, 0.0, xe, db);
    let ev = model::rect_loop(x_body, db, x_body + w, y_ev);
    let mut prisms = vec![
        wood("bottom_chord", bot_loop.clone()),
        wood("top_chord", top_loop.clone()),
        wood("end_vertical", ev.clone()),
    ];
    let mut zones =
        vec![zone_of("bottom_chord", &bot_loop), zone_of("top_chord", &top_loop), zone_of("end_vertical", &ev)];
    let p = tp.web_spacing.unwrap_or(h);
    let hw = web_half(w, h, p);
    if p <= 2.0 * hw + 0.25 {
        ctx.p.fail(
            "web_spacing",
            format!(
                "web_spacing {} is too tight for {} webs over {} between the chords: give at least {}",
                fmt_num(p),
                fmt_ft_in(w),
                fmt_ft_in(h),
                fmt_num(math::ceil(2.0 * hw + 1.0))
            ),
        );
        return None;
    }
    let (yb, yt) = (db, d - dt);
    let verts = parse_verticals(ctx, x_body + w, xe, w)?;
    let mut webs = Box::default();
    let mut x0 = x_body + w;
    let mut vert_anchors: Vec<NamedAnchor> = Vec::new();
    for (k, &xv) in verts.iter().enumerate() {
        diagonals(&mut prisms, &mut webs, [x0, xv - w / 2.0], [yb, yt], w, p, false);
        let lp = model::rect_loop(xv - w / 2.0, yb, xv + w / 2.0, yt);
        webs.add_box(geom::loop_box(&lp));
        prisms.push(wood("vertical", lp));
        vert_anchors.push(anchor(&format!("vertical_{}", k + 1), V2::init(xv, (yb + yt) / 2.0)));
        vert_anchors.push(anchor(&format!("vertical_{}_top", k + 1), V2::init(xv, yt)));
        vert_anchors.push(anchor(&format!("vertical_{}_bottom", k + 1), V2::init(xv, yb)));
        x0 = xv + w / 2.0;
    }
    diagonals(&mut prisms, &mut webs, [x0, xe], [yb, yt], w, p, true);
    if !webs.is_empty() {
        zones.push(zone_rect("web", webs.x0, webs.y0, webs.x1, webs.y1));
    }
    if let (Some(&a), Some(&b)) = (verts.first(), verts.last()) {
        zones.push(zone_rect("vertical", a - w / 2.0, yb, b + w / 2.0, yt));
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
        anchor("end_top", V2::init(x_body, y_ev.max(d))),
        anchor("end_bottom", V2::init(x_body, 0.0)),
        anchor("heel_outer", V2::init(x_body, (db + d - dt) / 2.0)),
    ];
    if overhang > 0.0 {
        anchors.push(anchor("tail_bottom", V2::init(-overhang, d - dt)));
        anchors.push(anchor("tail_top", V2::init(-overhang, d)));
    }
    anchors.extend(vert_anchors);
    if let Some(h) = tp.parapet {
        anchors.push(anchor("parapet_top", V2::init(x_body, d + h)));
        anchors.push(anchor("parapet_top_inner", V2::init(x_body + w, d + h)));
        anchors.push(anchor("parapet_roof_inner", V2::init(x_body + w, d)));
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
        info: format!(
            "truss parallel chord {} deep, {}+{} chords, {bearing}, webs @ {}{}",
            fmt_ft_in(d),
            ch.top,
            ch.bot,
            fmt_ft_in(p),
            if verts.is_empty() {
                String::new()
            } else {
                format!(
                    ", vertical{} at {}",
                    if verts.len() == 1 { "" } else { "s" },
                    verts.iter().map(|&x| fmt_ft_in(x)).collect::<Vec<_>>().join(", ")
                )
            }
        ),
        ..Built::default()
    })
}

/// Alternating diagonal webs of width `w` between the chord faces `y` over `x`, starting down from the top chord at
/// `x[0]`. `open`: panels of `p` until no full one fits before `x[1]` (the truss runs on past the drawing); closed: the
/// whole number of panels nearest `p` that fills `x` exactly (between two verticals).
fn diagonals(prisms: &mut Vec<Prism>, webs: &mut Box, x: [f64; 2], y: [f64; 2], w: f64, p: f64, open: bool) {
    let (len, h) = (x[1] - x[0], y[1] - y[0]);
    let fits = |s: f64| s > 2.0 * web_half(w, h, s) + 0.25;
    let (step, n) = if open {
        (p, math::floor((len - 0.5) / p).max(0.0))
    } else {
        let mut n = math::round(len / p).max(1.0);
        while n > 1.0 && !fits(len / n) {
            n -= 1.0;
        }
        (len / n, if fits(len / n) { n } else { 0.0 })
    };
    let hw = web_half(w, h, step);
    let mut k = 0.0;
    while k < n {
        let x0 = x[0] + k * step;
        let lp = if k % 2.0 == 0.0 {
            vec![Pt::new(x0 + step - 2.0 * hw, y[0]), Pt::new(x0 + step, y[0]), Pt::new(x0 + 2.0 * hw, y[1]), Pt::new(x0, y[1])]
        } else {
            vec![Pt::new(x0, y[0]), Pt::new(x0 + 2.0 * hw, y[0]), Pt::new(x0 + step, y[1]), Pt::new(x0 + step - 2.0 * hw, y[1])]
        };
        webs.add_box(geom::loop_box(&lp));
        prisms.push(wood("web", lp));
        k += 1.0;
    }
}

/// `verticals` of a parallel-chord truss: web centres measured from the bearing's outer edge, increasing, each web of
/// width `w` clear of the end vertical (inner face at `x_min`) and of the others, inside the drawn length `xe`.
fn parse_verticals(ctx: &mut Ctx, x_min: f64, xe: f64, w: f64) -> Option<Vec<f64>> {
    let Some(v) = ctx.p.raw("verticals").filter(|v| !v.is_null()) else { return Some(Vec::new()) };
    let fix = "e.g. \"verticals\": [24] (a vertical web centred 24\" from bearing_outer)";
    let Some(list) = v.arr() else {
        ctx.p.fail(
            "verticals",
            format!("verticals must be a list of distances from bearing_outer (got {}); {fix}", model::kind_or_text(v)),
        );
        return None;
    };
    let mut out: Vec<f64> = Vec::new();
    for (k, e) in list.iter().enumerate() {
        let Some(x) = units::parse_length(e) else {
            ctx.p.fail(
                &format!("verticals/{k}"),
                format!("verticals[{k}] must be a length (got {}); {fix}", model::kind_or_text(e)),
            );
            return None;
        };
        let lo = out.last().map_or(x_min, |&q| q + w / 2.0);
        if x - w / 2.0 < lo - 1e-9 || x + w / 2.0 > xe + 1e-9 {
            let why = match out.last() {
                Some(&q) if x - w / 2.0 < lo - 1e-9 => format!(
                    "it overlaps verticals[{}] at {} (list them in increasing order, at least {} apart)",
                    k - 1,
                    fmt_ft_in(q),
                    fmt_ft_in(w)
                ),
                _ => format!(
                    "the drawn truss runs from the end vertical's inner face at {} to span_shown {}",
                    fmt_ft_in(x_min),
                    fmt_ft_in(xe)
                ),
            };
            ctx.p.fail(
                &format!("verticals/{k}"),
                format!(
                    "verticals[{k}] at {}: a {} wide vertical web does not fit: {why}; give a centre from {} to {}",
                    fmt_ft_in(x),
                    fmt_ft_in(w),
                    fmt_ft_in(lo + w / 2.0),
                    fmt_ft_in(xe - w / 2.0)
                ),
            );
            return None;
        }
        out.push(x);
    }
    Some(out)
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
        ctx.p.fail(
            "depth",
            format!(
                "depth {} is not more than the two chords ({} + {}): give the overall height at the cut",
                fmt_ft_in(d),
                fmt_ft_in(ch.db),
                fmt_ft_in(dt_v)
            ),
        );
        return None;
    }
    let plies = usize::from(tp.plies);
    let tw = ch.tt.max(ch.tb);
    let w = tw * plies as f64;
    let mut prisms = Vec::new();
    let mut zones = Vec::new();
    for (part, t, y0, y1) in [("bottom_chord", ch.tb, 0.0, ch.db), ("top_chord", ch.tt, d - dt_v, d)] {
        let x0 = (tw - t) / 2.0;
        let quads: Vec<[V2; 4]> =
            (0..plies).map(|k| quad_of(x0 + k as f64 * tw, y0, x0 + k as f64 * tw + t, y1)).collect();
        let ply_lines: Vec<[V2; 2]> =
            (1..plies).map(|k| [V2::init(k as f64 * tw, y0), V2::init(k as f64 * tw, y1)]).collect();
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
        info: format!(
            "truss{girder} end-on {} deep at the cut, {}+{} chords, webs @ {}",
            fmt_ft_in(d),
            ch.top,
            ch.bot,
            fmt_ft_in(spacing)
        ),
        ..Built::default()
    })
}
