use super::common::{
    Ctx, drop_duplicate_points, oriented_ccw, parse_point_list, sawn_depth, sawn_thickness, vertex_anchors,
};
use super::connector::{Face, Kind, ModelInfo, Mount, Params, strap_info, thickness};
use crate::clip;
use crate::geom::{self, Box, FLAT_TOL, Pt, V2};
use crate::json::Value;
use crate::math;
use crate::model::{self, Built, Diag, Level, NamedAnchor, Prism};
use crate::params::ParamEnum;
use crate::pathclip;
use crate::pathgeom;
use crate::scene::{Scene, State};
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub struct Member {
    pub id: String,
    pub ty: &'static str,
    pub idx: usize,
    pub inst: usize,
    pub bx: Box,
    pub z0: f64,
    pub z1: f64,
    pub flat: Vec<Vec<V2>>,
    pub chord: Vec<Vec<V2>>,
    pub parts: Vec<(u32, u32)>,
}

impl Member {
    fn zc(&self) -> f64 {
        (self.z0 + self.z1) / 2.0
    }
}

pub fn member(ctx: &mut Ctx, key: &str, name: &str) -> Option<Member> {
    let (head, part) = name.split_once('.').map_or((name, None), |(h, p)| (h, Some(p)));
    let (id, inst) = match head.split_once('#') {
        Some((a, b)) => {
            match b.parse::<usize>() {
                Ok(k) => (a, k),
                Err(_) => {
                    ctx.p.fail(key, format!("'{key}': \"{name}\" is not a member name; use \"<id>\" or \"<id>#<k>\" for array instance k"));
                    return None;
                }
            }
        }
        None => (head, 0),
    };
    let scene: &Scene = ctx.scene;
    let Some(c) = scene.find(id) else {
        let ids = scene.comp_ids();
        let hint = model::nearest(id, &ids).map_or(String::new(), |n| format!(" Did you mean '{n}'?"));
        ctx.p.fail_code("E_REF_UNKNOWN", key, format!("'{key}': no component '{id}'.{hint}"));
        return None;
    };
    if c.state != State::Ok {
        ctx.p.fail_code("E_REF_UNKNOWN", key, format!("'{key}': component '{id}' did not build (see its own errors)"));
        return None;
    }
    if c.id == ctx.p.id {
        ctx.p.fail(key, format!("'{key}' names the connector itself"));
        return None;
    }
    if inst >= c.xfs.len() {
        ctx.p.fail_code(
            "E_REF_UNKNOWN",
            key,
            format!("'{key}': component '{id}' has {} instance(s); #{inst} does not exist", c.xfs.len()),
        );
        return None;
    }
    if let Some(pn) = part
        && !c.world.iter().any(|p| p.part == pn)
    {
        ctx.p.fail_code(
            "E_REF_UNKNOWN",
            key,
            format!("'{key}': '{id}' has no part '{pn}'; parts: {}", scene.part_names(c).join(", ")),
        );
        return None;
    }
    let stringer = c.ty.ty == crate::catalog::Type::Stair
        && c.node.get("kind").and_then(Value::str) == Some("wood")
        && part.is_none_or(|p| p == "stringer");
    let selected = if stringer {
        let cut = scene.default_z.unwrap_or((scene.run[0] + scene.run[1]) / 2.0);
        c.world.iter().filter(|p| p.instance as usize == inst && p.part == "stringer").min_by(|a, b| {
            let dist = |p: &Prism| ((p.z0 + p.z1) / 2.0 - cut).abs();
            dist(a).total_cmp(&dist(b))
        })
    } else {
        None
    };
    let mut bx = Box::default();
    let (mut z0, mut z1) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut flat = Vec::new();
    let mut chord = Vec::new();
    for p in c.world.iter().filter(|p| {
        p.instance as usize == inst
            && p.kind == model::Kind::Body
            && part.is_none_or(|pn| p.part == pn)
            && (!stringer || selected.is_some_and(|q| core::ptr::eq(*p, q)))
    }) {
        let f = clip::from_region(&p.loops, FLAT_TOL);
        bx.add_box(clip::loops_box(&f));
        if c.ty.name() == "truss" && p.part == "bottom_chord" {
            chord.extend(f.iter().cloned());
        }
        flat.extend(f);
        z0 = z0.min(p.z0);
        z1 = z1.max(p.z1);
    }
    if bx.is_empty() || z0 >= z1 {
        let mut parts: Vec<&str> = Vec::new();
        for p in c.world.iter().filter(|p| p.kind == model::Kind::Body && !p.part.is_empty()) {
            if !parts.contains(&p.part.as_str()) {
                parts.push(&p.part);
            }
        }
        ctx.p.fail(key, format!("'{key}': '{name}' has no solid body to mount on; available parts: {}. Use '{head}' for the whole member", parts.join(", ")));
        return None;
    }
    let parts = vec![(c.index, u32::try_from(inst).unwrap_or(0))];
    Some(Member {
        id: if part.is_some() { name.to_string() } else { c.id.clone() },
        ty: c.ty.name(),
        idx: c.index as usize,
        inst,
        bx,
        z0,
        z1,
        flat,
        chord,
        parts,
    })
}

pub(super) struct Kit {
    pub(super) prisms: Vec<Prism>,
    zlo: f64,
    zhi: f64,
}

impl Kit {
    pub(super) fn new() -> Kit {
        Kit { prisms: Vec::new(), zlo: f64::INFINITY, zhi: f64::NEG_INFINITY }
    }

    pub(super) fn poly(&mut self, part: &str, pts: Vec<Pt>, z: [f64; 2], nails: Vec<V2>) {
        let (z0, z1) = (z[0].min(z[1]), z[0].max(z[1]));
        if z1 - z0 < 1e-6 || geom::signed_area(&pts).abs() < 1e-9 {
            return;
        }
        self.zlo = self.zlo.min(z0);
        self.zhi = self.zhi.max(z1);
        self.prisms.push(Prism {
            part: part.to_string(),
            material: "steel".to_string(),
            loops: model::one_loop(oriented_ccw(pts)),
            z0,
            z1,
            hw_dash: true,
            nails,
            ..Prism::default()
        });
    }

    pub(super) fn rect(&mut self, part: &str, x: [f64; 2], y: [f64; 2], z: [f64; 2], nails: Vec<V2>) {
        let (x0, x1) = (x[0].min(x[1]), x[0].max(x[1]));
        let (y0, y1) = (y[0].min(y[1]), y[0].max(y[1]));
        if x1 - x0 < 1e-6 || y1 - y0 < 1e-6 {
            return;
        }
        self.poly(part, model::rect_loop(x0, y0, x1, y1), z, nails);
    }

    fn frame_box(&mut self, part: &str, fr: &Frame, u: [f64; 2], y: [f64; 2], v: [f64; 2], nails: &[(f64, f64)]) {
        let uw = [fr.f + fr.s * u[0], fr.f + fr.s * u[1]];
        if fr.along_x {
            let n = nails.iter().map(|&(_, yy)| V2::init((uw[0] + uw[1]) / 2.0, yy)).collect();
            self.rect(part, uw, y, v, n);
        } else {
            let n = nails.iter().map(|&(vv, yy)| V2::init(vv, yy)).collect();
            self.rect(part, v, y, uw, n);
        }
    }

    fn frame_poly(&mut self, part: &str, fr: &Frame, uy: &[(f64, f64)], v: [f64; 2]) {
        if fr.along_x {
            let pts = uy.iter().map(|&(u, y)| Pt::new(fr.f + fr.s * u, y)).collect();
            self.poly(part, pts, v, Vec::new());
            return;
        }
        let ys: Vec<f64> = uy.iter().map(|p| p.1).collect();
        let (y0, y1) =
            (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        let flat: Vec<V2> = uy.iter().map(|&(u, y)| V2::init(u, y)).collect();
        let n = 4;
        for k in 0..n {
            let ya = y0 + (y1 - y0) * k as f64 / n as f64;
            let yb = y0 + (y1 - y0) * (k + 1) as f64 / n as f64;
            let us = pathclip::scan(core::slice::from_ref(&flat), (ya + yb) / 2.0, false);
            let (Some(&ua), Some(&ub)) = (us.first(), us.last()) else { continue };
            self.frame_box(part, fr, [ua, ub], [ya, yb], v, &[]);
        }
    }

    pub(super) fn finish(self, origin: V2, anchors: Vec<NamedAnchor>, info: String, hw: model::HwInfo) -> Built {
        let zc = (self.zlo + self.zhi) / 2.0;
        let prisms: Vec<Prism> = self
            .prisms
            .into_iter()
            .map(|mut p| {
                p.zoff = (p.z0 + p.z1) / 2.0 - zc;
                p.zhalf = (p.z1 - p.z0) / 2.0;
                p.loops = p
                    .loops
                    .iter()
                    .map(|l| l.iter().map(|q| Pt { x: q.x - origin.x, y: q.y - origin.y, b: q.b }).collect())
                    .collect();
                p.nails = p.nails.iter().map(|n| n.sub(origin)).collect();
                for q in &mut p.centerline {
                    (q.x, q.y) = (q.x - origin.x, q.y - origin.y);
                }
                p
            })
            .collect();
        let box_ = super::common::box_of_prisms(&prisms);
        Built {
            prisms,
            anchors: anchors.into_iter().map(|a| NamedAnchor { name: a.name, p: a.p.sub(origin) }).collect(),
            box_,
            nat_z: Some(self.zhi - self.zlo),
            nat_zc: Some(zc),
            points_mode: true,
            info,
            hw,
            ..Built::default()
        }
    }
}

struct Frame {
    along_x: bool,
    f: f64,
    s: f64,
}

fn r16(x: f64) -> f64 {
    math::floor(x * 16.0 + 1e-9) / 16.0
}

fn r16_up(x: f64) -> f64 {
    math::ceil(x * 16.0 - 1e-9) / 16.0
}

fn reject_placement(ctx: &mut Ctx, kind: Kind) -> bool {
    for key in ["at", "rotate", "slope", "mirror", "z", "points", "on", "lay", "side"] {
        if ctx.p.has(key) {
            let why = match key {
                "z" => "its z comes from the members",
                "points" | "on" | "lay" | "side" => "it is shaped from its members, not a polyline",
                _ => "it is placed from its members",
            };
            ctx.p.fail(key, format!("'{key}' is not used by kind {}: {why}; remove '{key}'", kind.tag()));
        }
    }
    ctx.p.ok
}

pub(super) fn need(ctx: &mut Ctx, v: Option<&Value>, key: &str, kind: Kind, hint: &str) -> Option<Member> {
    let Some(v) = v else {
        ctx.p.fail(key, format!("kind {} needs '{key}': {hint}", kind.tag()));
        return None;
    };
    let names: Vec<&str> = match v {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) if !a.is_empty() && a.iter().all(|e| e.str().is_some()) => {
            a.iter().filter_map(Value::str).collect()
        }
        _ => {
            ctx.p
                .fail(key, format!("'{key}' must be a component id or a list of ids (got {})", model::kind_or_text(v)));
            return None;
        }
    };
    let mut out: Option<Member> = None;
    for n in names {
        let m = member(ctx, key, n)?;
        out = Some(match out {
            None => m,
            Some(mut u) => {
                u.id = format!("{}+{}", u.id, m.id);
                u.bx.add_box(m.bx);
                u.z0 = u.z0.min(m.z0);
                u.z1 = u.z1.max(m.z1);
                u.flat.extend(m.flat);
                u.chord.extend(m.chord);
                u.parts.extend(m.parts);
                u
            }
        });
    }
    out
}

pub fn build(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, kind: Kind) -> Option<Built> {
    let tie = matches!(kind, Kind::Strap | Kind::Tie | Kind::Angle);
    if tie {
        if kind == Kind::Strap {
            ctx.p.fail("kind", "a connector placed from \"members\" is kind tie (it wraps the joint: H2.5A, H3, RSP4, LTP4) or angle (in the inside corner: A34, A35); set \"kind\" to one of them, or remove \"members\" and give \"points\" or \"on\" for a strap".to_string());
        }
        for key in ["at", "rotate", "slope", "mirror", "points", "on", "lay"] {
            if ctx.p.has(key) {
                ctx.p.fail(
                    key,
                    format!(
                        "'{key}' is not used by a {} placed from \"members\": it is shaped from them; remove '{key}'",
                        kind.tag()
                    ),
                );
            }
        }
    } else if !reject_placement(ctx, kind) {
        return None;
    }
    let only: &[&str] = match kind {
        Kind::Hanger => &["mount", "carried", "carrying", "height", "seat", "width"],
        Kind::PostCap => &["post", "beam", "height"],
        Kind::PostBase => &["post", "standoff", "height", "face"],
        Kind::Holdown => &["post", "rod", "face", "height", "seat", "seat_height", "width"],
        _ => &["members", "legs", "along", "face", "width"],
    };
    if !tie && ctx.p.has("members") {
        ctx.p
            .fail("members", format!("'members' places a tie or angle; kind {} takes {}", kind.tag(), only.join(", ")));
    }
    for key in ["legs", "along", "gap"] {
        if !tie && ctx.p.has(key) {
            ctx.p.fail(
                key,
                format!(
                    "'{key}' belongs to a tie or angle placed from \"members\"; kind {} takes {}",
                    kind.tag(),
                    only.join(", ")
                ),
            );
        }
    }
    if matches!(kind, Kind::Holdown | Kind::PostBase) && matches!(cp.face, Some(Face::Top | Face::Bottom)) {
        ctx.p
            .fail("face", format!("kind {} is on a side face of the post: use left, right, front or back", kind.tag()));
    }
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
        "width",
    ] {
        if ctx.p.has(key) && !only.contains(&key) {
            ctx.p.fail(key, format!("'{key}' is not used by kind {}; it takes {}", kind.tag(), only.join(", ")));
        }
    }
    if !ctx.p.ok {
        return None;
    }
    let gauge_n = cp.gauge.or(mi.gauge()).unwrap_or(match kind {
        Kind::Holdown | Kind::PostCap => 12.0,
        Kind::PostBase => 12.0,
        _ => 18.0,
    });
    let (gauge_i, t) = thickness(ctx, gauge_n)?;
    match kind {
        Kind::Hanger => hanger(ctx, cp, mi, gauge_i, t),
        Kind::PostCap => post_cap(ctx, cp, mi, gauge_i, t),
        Kind::PostBase => post_base(ctx, cp, mi, gauge_i, t),
        Kind::Holdown => holdown(ctx, cp, mi, gauge_i, t),
        _ => super::tie::build(ctx, cp, mi, kind, gauge_i, t),
    }
}

pub(super) fn model_text(mi: &ModelInfo) -> String {
    if mi.name.is_empty() { String::new() } else { format!("{} ", mi.name) }
}

pub(super) fn warn(ctx: &Ctx, code: &'static str, key: &str, message: String, fix: String) -> Diag {
    let id = ctx.p.id;
    Diag {
        level: Level::Warning,
        code,
        id: Some(id.to_string()),
        path: Some(format!("components/{id}/{key}")),
        message,
        fix: Some(fix),
    }
}

fn update_op(id: &str, key: &str, value: &str) -> String {
    format!("{{\"op\":\"update\",\"path\":\"components/{id}\",\"value\":{{\"{key}\":{value}}}}}")
}

fn num_json(x: f64) -> String {
    model::fmt_num(math::round(x * 10000.0) / 10000.0)
}

pub(super) fn y_at(flat: &[Vec<V2>], x: f64) -> Option<(f64, f64)> {
    let ys = pathclip::scan(flat, x, true);
    Some((*ys.first()?, *ys.last()?))
}

fn model_size(name: &str, prefix: &str) -> Option<(f64, f64)> {
    let up = name.to_ascii_uppercase();
    let rest = up.strip_prefix(prefix)?;
    let rest = rest.trim_end_matches("TF").trim_end_matches('-');
    if let Some((w, d)) = rest.split_once('/') {
        let num = |t: &str| -> Option<f64> {
            let t = t.split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?;
            t.parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 100.0)
        };
        return Some((num(w)?, num(d)?));
    }
    let (body, plies) = match rest.split_once('-') {
        Some((b, n)) => (b, n.parse::<u32>().ok()?),
        None => (rest, 1),
    };
    if body.len() < 2 || !body.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let nt: u32 = body[..1].parse().ok()?;
    let nd: u32 = body[1..].parse().ok()?;
    let t = sawn_thickness(nt)?;
    let d = sawn_depth(nt, nd)?;
    Some((t * f64::from(plies), d))
}

fn hanger(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, gauge_i: u32, t: f64) -> Option<Built> {
    let kind = Kind::Hanger;
    let c = need(ctx, cp.carried.as_ref(), "carried", kind, "the joist or beam it carries, e.g. \"joist\"");
    let b = need(ctx, cp.carrying.as_ref(), "carrying", kind, "the member it hangs from, e.g. \"header\"");
    let (c, b) = (c?, b?);
    let mount = cp.mount.unwrap_or_else(|| mi.mount());
    let overlap = |a0: f64, a1: f64, b0: f64, b1: f64| a1.min(b1) - a0.max(b0) > 1e-6;
    let yo = overlap(c.bx.y0, c.bx.y1, b.bx.y0, b.bx.y1);
    let xo = overlap(c.bx.x0, c.bx.x1, b.bx.x0, b.bx.x1);
    let zo = overlap(c.z0, c.z1, b.z0, b.z1);
    let mut cands: Vec<(f64, bool, f64, f64)> = Vec::new();
    if yo && zo {
        cands.push((c.bx.x0 - b.bx.x1, true, b.bx.x1, 1.0));
        cands.push((b.bx.x0 - c.bx.x1, true, b.bx.x0, -1.0));
    }
    if yo && xo {
        cands.push((c.z0 - b.z1, false, b.z1, 1.0));
        cands.push((b.z0 - c.z1, false, b.z0, -1.0));
    }
    let pick =
        cands.iter().filter(|k| k.0 >= -0.25 && k.0 <= 0.5).fold(None::<(f64, bool, f64, f64)>, |best, k| match best {
            Some(bb) if bb.0.abs() <= k.0.abs() => Some(bb),
            _ => Some(*k),
        });
    let Some((gap, along_x, f, s)) = pick else {
        if yo && zo && xo && matches!(c.ty, "lumber" | "ijoist" | "steel") && !c.id.contains('+') {
            let end = if c.bx.x0 + c.bx.x1 < b.bx.x0 + b.bx.x1 { "end" } else { "start" };
            ctx.p.fail(
                "carried",
                format!(
                    "hanger: '{}' runs into '{}' (x {}..{} against {}..{}): a sloped or overlong member must be cut at the face it hangs from. Fix: {} (a plumb cut at that face; keep any other cuts in the list)",
                    c.id,
                    b.id,
                    fmt_ft_in(c.bx.x0),
                    fmt_ft_in(c.bx.x1),
                    fmt_ft_in(b.bx.x0),
                    fmt_ft_in(b.bx.x1),
                    hanger_cut_fix(ctx, &c, &b, end)
                ),
            );
            return None;
        }
        ctx.p.fail(
            "carried",
            format!(
                "hanger: '{}' does not end at a face of '{}' (it must butt a side face within 1/2\"): '{}' spans x {}..{}, y {}..{}, z {}..{}; '{}' spans x {}..{}, y {}..{}, z {}..{}. Fix: run the carried member up to the carrying member's face (e.g. \"until\": \"{}@middle_left\") or name the right members",
                c.id, b.id, c.id, fmt_ft_in(c.bx.x0), fmt_ft_in(c.bx.x1), fmt_ft_in(c.bx.y0), fmt_ft_in(c.bx.y1), fmt_ft_in(c.z0), fmt_ft_in(c.z1),
                b.id, fmt_ft_in(b.bx.x0), fmt_ft_in(b.bx.x1), fmt_ft_in(b.bx.y0), fmt_ft_in(b.bx.y1), fmt_ft_in(b.z0), fmt_ft_in(b.z1), b.id
            ),
        );
        return None;
    };
    let seat_len = cp.seat.unwrap_or(2.0);
    let fr = Frame { along_x, f, s };
    let end = f + s * gap.max(0.0);
    let (ybot_f, ybot_e, depth, wc, cv) = if along_x {
        let probe = |u: f64| y_at(&c.flat, end + s * u);
        let (Some(a), Some(e)) = (probe(0.01), probe((seat_len - 0.01).max(0.02))) else {
            ctx.p.fail("carried", format!("hanger: '{}' has no profile at the face of '{}'", c.id, b.id));
            return None;
        };
        let slope = (e.0 - a.0) / (seat_len - 0.02).max(0.01);
        let at_face = a.0 - slope * (gap.max(0.0) + 0.01);
        (at_face, at_face + slope * (gap.max(0.0) + seat_len), a.1 - a.0, c.z1 - c.z0, c.zc())
    } else {
        (c.bx.y0, c.bx.y0, c.bx.height(), c.bx.width(), (c.bx.x0 + c.bx.x1) / 2.0)
    };
    let w = cp.width.unwrap_or(wc);
    let chord = if along_x {
        y_at(&c.chord, end + s * 0.01).map(|(lo, hi)| hi - lo)
    } else if !c.chord.is_empty() {
        Some(clip::loops_box(&c.chord).height())
    } else {
        None
    };
    let h = cp.height.unwrap_or_else(|| {
        mi.table.filter(|h| h.kind.contains("hanger")).map_or_else(|| r16(0.85 * chord.unwrap_or(depth)), |h| h.length)
    });
    let fl = 1.0;
    let (v0, v1) = (cv - w / 2.0, cv + w / 2.0);
    let ulen = gap.max(0.0) + seat_len;
    let yb = |u: f64| ybot_f + (ybot_e - ybot_f) * u / ulen;
    let mut kit = Kit::new();
    let nail_rows = [0.3, 0.7];
    if along_x {
        let quad = |u0: f64, u1: f64, lo: f64, hi: f64| -> Vec<Pt> {
            vec![
                Pt::new(f + s * u0, yb(u0) + lo),
                Pt::new(f + s * u1, yb(u1) + lo),
                Pt::new(f + s * u1, yb(u1) + hi),
                Pt::new(f + s * u0, yb(u0) + hi),
            ]
        };
        kit.poly("seat", quad(0.0, ulen, -t, 0.0), [v0 - t, v1 + t], Vec::new());
        let nails: Vec<V2> = nail_rows.iter().map(|k| V2::init(f + s * ulen * 0.6, yb(ulen * 0.6) + h * k)).collect();
        kit.poly("side", quad(0.0, ulen, 0.0, h), [v0 - t, v0], nails.clone());
        kit.poly("side", quad(0.0, ulen, 0.0, h), [v1, v1 + t], nails);
    } else {
        let u = [0.0, ulen];
        kit.frame_box("seat", &fr, u, [ybot_f - t, ybot_f], [v0 - t, v1 + t], &[]);
        kit.frame_box("side", &fr, u, [ybot_f, ybot_f + h], [v0 - t, v0], &[]);
        kit.frame_box("side", &fr, u, [ybot_f, ybot_f + h], [v1, v1 + t], &[]);
    }
    let y0 = yb(0.0);
    match mount {
        Mount::Face => {
            for (a, bnd) in [(v0 - t - fl, v0 - t), (v1 + t, v1 + t + fl)] {
                let nails: Vec<(f64, f64)> = [0.25, 0.5, 0.75].iter().map(|k| ((a + bnd) / 2.0, y0 + h * k)).collect();
                kit.frame_box("flange", &fr, [0.0, t], [y0, y0 + h], [a, bnd], &nails);
            }
        }
        Mount::TopFlange => {
            let top = b.bx.y1;
            if top < y0 + h * 0.5 {
                ctx.p.fail("mount", format!("hanger: a top-flange hanger needs '{}' to reach above the seat: its top is at y {}, the seat at {}; use mount face", b.id, fmt_ft_in(top), fmt_ft_in(y0)));
                return None;
            }
            let reach = if along_x { b.bx.width() } else { b.z1 - b.z0 };
            let ft = reach.min(2.5);
            kit.frame_box("back", &fr, [0.0, t], [y0, top], [v0 - t, v1 + t], &[]);
            kit.frame_box("top_flange", &fr, [-ft, t], [top, top + t], [v0 - t, v1 + t], &[]);
        }
    }
    let mut warns = Vec::new();
    let id = ctx.p.id.to_string();
    let (brace, braced) = match chord {
        Some(d) => (d, format!("the bottom chord of '{}'", c.id)),
        None => (depth, format!("'{}'", c.id)),
    };
    if h < 0.6 * brace - 1e-6 {
        warns.push(warn(ctx, "W_HANGER_SIZE", "height", format!("hanger '{id}' is {} tall for {braced}, which is {} deep: under 60% of the depth, so it does not brace the member", fmt_ft_in(h), fmt_ft_in(brace)), format!("use a deeper hanger: {}", update_op(&id, "height", &num_json(r16_up(0.6 * brace).max(r16(0.85 * brace)))))));
    }
    if h > depth + 1e-6 {
        warns.push(warn(
            ctx,
            "W_HANGER_SIZE",
            "height",
            format!("hanger '{id}' is {} tall but '{}' is only {} deep", fmt_ft_in(h), c.id, fmt_ft_in(depth)),
            update_op(&id, "height", &num_json(r16(0.85 * depth))),
        ));
    }
    if cp.height.is_none() && mi.table.is_none() && chord.is_some() && !mi.name.is_empty() {
        warns.push(warn(ctx, "W_HANGER_SIZE", "height", format!("hanger '{id}': model {} has no catalog height; the schematic uses 85% of the bottom chord depth ({}), not the full truss. Set the actual manufacturer height", mi.name, fmt_ft_in(h)), update_op(&id, "height", &num_json(h))));
    }
    if along_x
        && (ybot_e - ybot_f).abs() > 0.01
        && !mi.name.is_empty()
        && !mi.name.to_ascii_uppercase().starts_with("LSS")
    {
        warns.push(warn(ctx, "W_HANGER_SLOPE", "model", format!("hanger '{id}': '{}' slopes at its seat; {} has no modeled slope/skew adjustment. The drawn sloping seat is schematic", c.id, mi.name), format!("select a slope-adjustable hanger, e.g. {} (confirm the model and dimensions with its manufacturer)", update_op(&id, "model", "\"LSSU\""))));
    }
    if (w - wc).abs() > 0.125 {
        warns.push(warn(
            ctx,
            "W_HANGER_SIZE",
            "width",
            format!("hanger '{id}' seat is {} wide but '{}' is {} wide", fmt_ft_in(w), c.id, fmt_ft_in(wc)),
            format!("remove \"width\" (the seat then fits '{}') or pick the hanger for that width", c.id),
        ));
    }
    if let Some(sr) = mi.series
        && let Some((mw, md)) = model_size(&mi.name, sr.prefix)
    {
        if brace > md + 0.5 || brace < md - 1.0 {
            warns.push(warn(
                ctx,
                "W_HANGER_SIZE",
                "model",
                format!(
                    "hanger '{id}': model {} is made for a member {} deep; {braced} is {} deep{}",
                    mi.name,
                    fmt_ft_in(md),
                    fmt_ft_in(brace),
                    if brace > md { " (hanger too small)" } else { "" }
                ),
                format!("pick the {} model sized for {} x {}", sr.prefix, fmt_ft_in(wc), fmt_ft_in(brace)),
            ));
        }
        if (mw - wc).abs() > 0.25 {
            warns.push(warn(
                ctx,
                "W_HANGER_SIZE",
                "model",
                format!(
                    "hanger '{id}': model {} is made for a member {} wide; '{}' is {} wide",
                    mi.name,
                    fmt_ft_in(mw),
                    c.id,
                    fmt_ft_in(wc)
                ),
                format!(
                    "pick the {} model for a {} wide member{}",
                    sr.prefix,
                    fmt_ft_in(wc),
                    if mi.name.contains('/') { "" } else { " (a -2 or -3 suffix for plies)" }
                ),
            ));
        }
    }
    let seat_pt = if along_x { V2::init(f + s * ulen / 2.0, yb(ulen / 2.0)) } else { V2::init(cv, ybot_f) };
    let info = format!(
        "connector {}hanger ({}) {gauge_i} ga, {} tall x {} seat, carries {} from {}",
        model_text(mi),
        mount.tag().replace('_', " "),
        fmt_ft_in(h),
        fmt_ft_in(w),
        c.id,
        b.id
    );
    let origin = ctx.origin;
    let joins = [
        c.id.split('+').map(crate::compile::host_id).map(String::from).collect(),
        b.id.split('+').map(crate::compile::host_id).map(String::from).collect(),
    ];
    let members = c.parts.iter().chain(&b.parts).copied().collect();
    let hw = model::HwInfo { warns, joins, members, ..Default::default() };
    Some(kit.finish(origin, vec![NamedAnchor { name: "seat".into(), p: seat_pt }], info, hw))
}

fn hanger_cut_fix(ctx: &Ctx, c: &Member, b: &Member, end: &str) -> String {
    let Some(node) = ctx.scene.comps.get(c.idx).map(|c| &c.node) else { return String::new() };
    let mut cuts = node.get("cuts").and_then(Value::arr).unwrap_or(&[]).to_vec();
    let trim = cuts.iter().any(|cut| {
        cut.get("type").and_then(Value::str) == Some("trim")
            && cut.get("end").and_then(Value::str) == Some(end)
            && cut
                .get("to")
                .and_then(Value::str)
                .is_some_and(|id| crate::compile::host_id(id) == crate::compile::host_id(&b.id))
    });
    let face = if end == "end" { b.bx.x0 } else { b.bx.x1 };
    let cut = if trim {
        format!(
            "{{\"type\":\"bevel\",\"end\":\"{end}\",\"line\":[[{},{}],[{},{}]]}}",
            num_json(face),
            num_json(c.bx.y0 - 1.0),
            num_json(face),
            num_json(c.bx.y1 + 1.0)
        )
    } else {
        format!("{{\"type\":\"trim\",\"end\":\"{end}\",\"to\":\"{}\"}}", b.id)
    };
    if let Ok(cut) = crate::json::parse(cut.as_bytes()) {
        cuts.retain(|cut| cut.get("end").and_then(Value::str) != Some(end));
        cuts.push(cut);
    }
    let mut bytes = Vec::new();
    crate::json::write_compact(&mut bytes, &Value::Array(cuts));
    let json = String::from_utf8(bytes).unwrap_or_default();
    format!(
        "{}{}",
        if trim {
            "the existing trim missed the host centerline; replace that end cut by an explicit face line: "
        } else {
            ""
        },
        update_op(&c.id, "cuts", &json)
    )
}

fn post_cap(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, gauge_i: u32, t: f64) -> Option<Built> {
    let kind = Kind::PostCap;
    let p = need(ctx, cp.post.as_ref(), "post", kind, "the post under the beam");
    let b = need(ctx, cp.beam.as_ref(), "beam", kind, "the beam on the post");
    let (p, b) = (p?, b?);
    let top = p.bx.y1;
    if (b.bx.y0 - top).abs() > 0.25 || b.bx.x1 < p.bx.x0 || b.bx.x0 > p.bx.x1 || b.z1 < p.z0 || b.z0 > p.z1 {
        ctx.p.fail(
            "beam",
            format!(
                "post_cap: '{}' does not sit on '{}': the beam bottom is at y {} (x {}..{}, z {}..{}), the post top at y {} (x {}..{}, z {}..{}). Fix: place the beam on the post, e.g. \"at\": {{\"anchor\": \"bottom_center\", \"to\": \"{}@top_center\"}}",
                b.id, p.id, fmt_ft_in(b.bx.y0), fmt_ft_in(b.bx.x0), fmt_ft_in(b.bx.x1), fmt_ft_in(b.z0), fmt_ft_in(b.z1),
                fmt_ft_in(top), fmt_ft_in(p.bx.x0), fmt_ft_in(p.bx.x1), fmt_ft_in(p.z0), fmt_ft_in(p.z1), p.id
            ),
        );
        return None;
    }
    let several: Vec<Member> = match cp.beam.as_ref() {
        Some(Value::Array(a)) if a.len() > 1 => {
            a.iter().filter_map(Value::str).filter_map(|n| member(ctx, "beam", n)).collect()
        }
        _ => Vec::new(),
    };
    if !ctx.p.ok {
        return None;
    }
    let beams: &[Member] = if several.is_empty() { core::slice::from_ref(&b) } else { &several };
    let depth = beams.iter().map(|m| m.bx.height()).fold(0.0, f64::max);
    let hu = cp.height.unwrap_or_else(|| r16((0.5 * depth).min(7.0)));
    let hd = r16(hu.min(0.5 * p.bx.height()));
    let (px0, px1, pz0, pz1) = (p.bx.x0, p.bx.x1, p.z0, p.z1);
    let mut kit = Kit::new();
    let along_z = |m: &Member| m.z1 - m.z0 > m.bx.width();
    let (mut sx, mut sz) = ([px0, px1], [pz0, pz1]);
    for m in beams {
        if along_z(m) {
            sx = [sx[0].min(m.bx.x0), sx[1].max(m.bx.x1)];
        } else {
            sz = [sz[0].min(m.z0), sz[1].max(m.z1)];
        }
    }
    kit.rect("seat", [sx[0] - t, sx[1] + t], [top - t, top], [sz[0] - t, sz[1] + t], Vec::new());
    let grid = |x0: f64, x1: f64, y0: f64, y1: f64| -> Vec<V2> {
        let mut v = Vec::new();
        for fy in [0.33, 0.67] {
            for fx in [0.3, 0.7] {
                v.push(V2::init(x0 + (x1 - x0) * fx, y0 + (y1 - y0) * fy));
            }
        }
        v
    };
    let (mut x_faces, mut z_faces) = (false, false);
    for (i, m) in beams.iter().enumerate() {
        let mz = along_z(m);
        let span = match (several.is_empty(), mz) {
            (true, true) => [pz0, pz1],
            (true, false) => [px0, px1],
            (false, true) => cap_span([pz0, pz1], [m.z0, m.z1]),
            (false, false) => cap_span([px0, px1], [m.bx.x0, m.bx.x1]),
        };
        for (face, out) in [(if mz { m.bx.x0 } else { m.z0 }, -1.0), (if mz { m.bx.x1 } else { m.z1 }, 1.0)] {
            let butts: Vec<[f64; 2]> = beams
                .iter()
                .enumerate()
                .filter(|(k, o)| {
                    let near = if mz {
                        if out < 0.0 { o.bx.x1 } else { o.bx.x0 }
                    } else if out < 0.0 {
                        o.z1
                    } else {
                        o.z0
                    };
                    *k != i && (near - face).abs() <= 0.0625
                })
                .map(|(_, o)| if mz { [o.z0, o.z1] } else { [o.bx.x0, o.bx.x1] })
                .collect();
            let th = if out < 0.0 { [face - t, face] } else { [face, face + t] };
            for piece in cut_out(span, &butts).into_iter().filter(|s| s[1] - s[0] >= 0.5) {
                if mz {
                    kit.rect("beam_side", th, [top, top + hu], piece, Vec::new());
                } else {
                    kit.rect("beam_side", piece, [top, top + hu], th, grid(piece[0], piece[1], top, top + hu));
                }
            }
        }
        if mz {
            z_faces = true;
        } else {
            x_faces = true;
        }
    }
    if z_faces {
        kit.rect("post_side", [px0, px1], [top - hd, top - t], [pz1, pz1 + t], grid(px0, px1, top - hd, top));
        kit.rect("post_side", [px0, px1], [top - hd, top - t], [pz0 - t, pz0], grid(px0, px1, top - hd, top));
    }
    if x_faces {
        kit.rect("post_side", [px0 - t, px0], [top - hd, top - t], [pz0, pz1], Vec::new());
        kit.rect("post_side", [px1, px1 + t], [top - hd, top - t], [pz0, pz1], Vec::new());
    }
    let info = format!(
        "connector {}post cap {gauge_i} ga, {} up the beam, {} on {}",
        model_text(mi),
        fmt_ft_in(hu),
        b.id,
        p.id
    );
    let origin = ctx.origin;
    let hw = model::HwInfo { members: p.parts.iter().chain(&b.parts).copied().collect(), ..Default::default() };
    Some(kit.finish(origin, vec![NamedAnchor { name: "seat".into(), p: V2::init((px0 + px1) / 2.0, top) }], info, hw))
}

fn cap_span(post: [f64; 2], beam: [f64; 2]) -> [f64; 2] {
    let w = post[1] - post[0];
    let over = [post[0].max(beam[0]), post[1].min(beam[1])];
    if over[1] - over[0] >= 0.5 * w - 1e-6 {
        over
    } else if beam[0] > post[0] + 1e-6 {
        [beam[0], beam[1].min(beam[0] + w)]
    } else {
        [beam[0].max(beam[1] - w), beam[1]]
    }
}

fn cut_out(span: [f64; 2], holes: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = vec![span];
    for h in holes {
        out = out
            .into_iter()
            .flat_map(|s| {
                if h[1] <= s[0] || h[0] >= s[1] {
                    vec![s]
                } else {
                    [[s[0], h[0]], [h[1], s[1]]].into_iter().filter(|p| p[1] > p[0]).collect()
                }
            })
            .collect();
    }
    out
}

fn post_base(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, gauge_i: u32, t: f64) -> Option<Built> {
    let kind = Kind::PostBase;
    let p = need(ctx, cp.post.as_ref(), "post", kind, "the post it holds")?;
    let so = cp.standoff.unwrap_or(1.0);
    if so < 0.0 {
        ctx.p.fail("standoff", format!("standoff must be 0 or more (got {})", fmt_ft_in(so)));
        return None;
    }
    let h = cp.height.unwrap_or(3.5);
    let (px0, px1, pz0, pz1, py0) = (p.bx.x0, p.bx.x1, p.z0, p.z1, p.bx.y0);
    let yb = py0 - so;
    let mut kit = Kit::new();
    kit.rect("base", [px0 - t, px1 + t], [yb, yb + t], [pz0 - t, pz1 + t], Vec::new());
    if so > 2.0 * t {
        kit.rect("seat", [px0, px1], [py0 - t, py0], [pz0, pz1], Vec::new());
    }
    let x_faces = !matches!(cp.face, Some(Face::Front | Face::Back));
    let nails =
        |x0: f64, x1: f64| vec![V2::init((x0 + x1) / 2.0, py0 + h * 0.4), V2::init((x0 + x1) / 2.0, py0 + h * 0.75)];
    if x_faces {
        kit.rect("side", [px0 - t, px0], [yb, py0 + h], [pz0, pz1], Vec::new());
        kit.rect("side", [px1, px1 + t], [yb, py0 + h], [pz0, pz1], Vec::new());
    } else {
        kit.rect("side", [px0, px1], [yb, py0 + h], [pz1, pz1 + t], nails(px0, px1));
        kit.rect("side", [px0, px1], [yb, py0 + h], [pz0 - t, pz0], nails(px0, px1));
    }
    let info =
        format!("connector {}post base {gauge_i} ga, standoff {}, under {}", model_text(mi), fmt_ft_in(so), p.id);
    let bears = model::Bears {
        y: yb,
        plan: [px0 - t, px1 + t, pz0 - t, pz1 + t],
        post: p.id.split('+').map(crate::compile::host_id).map(String::from).collect(),
        post_y0: py0,
        standoff: so,
    };
    let hw = model::HwInfo { bears: Some(bears), members: p.parts.clone(), ..Default::default() };
    let origin = ctx.origin;
    Some(kit.finish(
        origin,
        vec![NamedAnchor { name: "base_bottom".into(), p: V2::init((px0 + px1) / 2.0, yb) }],
        info,
        hw,
    ))
}

fn holdown(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, gauge_i: u32, t: f64) -> Option<Built> {
    let kind = Kind::Holdown;
    let p = need(ctx, cp.post.as_ref(), "post", kind, "the post or stud pack it is screwed to")?;
    let rod = match cp.rod.as_deref() {
        Some(r) => {
            let m = member(ctx, "rod", r)?;
            if m.ty != "anchor_bolt" && m.ty != "bolt" {
                ctx.p.fail("rod", format!("'rod' must name an anchor_bolt or bolt; '{}' is a {}", m.id, m.ty));
                return None;
            }
            Some(m)
        }
        None => None,
    };
    if mi.series.is_some_and(|s| s.prefix == "DTT") {
        return tension_tie(ctx, cp, mi, gauge_i, t, &p, rod.as_ref());
    }
    let scene: &Scene = ctx.scene;
    let rod_axis = rod.as_ref().map(|m| {
        let c = &scene.comps[m.idx];
        let mut bx = Box::default();
        let (mut z0, mut z1) = (f64::INFINITY, f64::NEG_INFINITY);
        for q in c.world.iter().filter(|q| q.instance as usize == m.inst && q.part == "shank") {
            bx.add_box(clip::loops_box(&clip::from_region(&q.loops, FLAT_TOL)));
            z0 = z0.min(q.z0);
            z1 = z1.max(q.z1);
        }
        if bx.is_empty() {
            bx = m.bx;
            (z0, z1) = (m.z0, m.z1);
        }
        (V2::init((bx.x0 + bx.x1) / 2.0, (z0 + z1) / 2.0), bx.width().min(z1 - z0), bx.y1)
    });
    let inside = rod_axis.is_some_and(|(a, _, _)| a.x > p.bx.x0 && a.x < p.bx.x1 && a.y > p.z0 && a.y < p.z1);
    let face = match (cp.face, rod_axis) {
        (Some(f), _) => f,
        (None, Some((a, _, _))) if a.x < p.bx.x0 => Face::Left,
        (None, Some((a, _, _))) if a.x > p.bx.x1 => Face::Right,
        (None, Some((a, _, _))) if a.y > p.z1 => Face::Front,
        (None, Some((a, _, _))) if a.y < p.z0 => Face::Back,
        _ => {
            if inside {
                Face::Front
            } else {
                Face::Left
            }
        }
    };
    let fr = match face {
        Face::Left | Face::Top | Face::Bottom => Frame { along_x: true, f: p.bx.x0, s: -1.0 },
        Face::Right => Frame { along_x: true, f: p.bx.x1, s: 1.0 },
        Face::Front => Frame { along_x: false, f: p.z1, s: 1.0 },
        Face::Back => Frame { along_x: false, f: p.z0, s: -1.0 },
    };
    let along_face = !fr.along_x && inside;
    let (d, vc, rod_r) = match rod_axis {
        Some((a, dia, _)) if along_face => ((dia / 2.0 + t + 0.5).max(1.375), a.x, dia / 2.0),
        Some((a, dia, _)) => {
            let (ru, rv) = if fr.along_x { (a.x, a.y) } else { (a.y, a.x) };
            ((ru - fr.f) * fr.s, rv, dia / 2.0)
        }
        None => (1.5, if fr.along_x { p.zc() } else { (p.bx.x0 + p.bx.x1) / 2.0 }, 0.3125),
    };
    let id = ctx.p.id.to_string();
    if d <= rod_r + t {
        let fix = if inside {
            format!(
                "; the rod is inside the post outline, which reads as a rod under the post in a section through it: put the holdown on the post's front or back face, {}",
                update_op(&id, "face", "\"front\"")
            )
        } else {
            String::new()
        };
        ctx.p.fail("face", format!("holdown: the rod is {} from the {} face of '{}', inside or against the post; the seat must reach past the rod{fix}", fmt_ft_in(d), face.tag(), p.id));
        return None;
    }
    let h = cp.height.unwrap_or(10.0);
    let w = cp.width.unwrap_or(2.5);
    let ts = cp.seat.unwrap_or(0.375);
    let ys = p.bx.y0 + cp.seat_height.unwrap_or(0.0);
    let e = (rod_r + 0.375).max(0.5);
    let ue = d + e;
    let h1 = (0.3 * h).min(3.0);
    let v = [vc - w / 2.0, vc + w / 2.0];
    let mut kit = Kit::new();
    kit.frame_box("back", &fr, [0.0, t], [ys, ys + h], v, &[]);
    kit.frame_box("seat", &fr, [0.0, ue], [ys, ys + ts], v, &[]);
    let side = [(0.0, ys), (ue, ys), (ue, ys + h1), (t, ys + h), (0.0, ys + h)];
    kit.frame_poly("side", &fr, &side, [v[0], v[0] + t]);
    kit.frame_poly("side", &fr, &side, [v[1] - t, v[1]]);
    let mut yk = ys + 1.5;
    while yk < (ys + h - 0.5).min(ys + 60.0) {
        let n0 = kit.prisms.len();
        kit.frame_box("screw", &fr, [-1.75, 0.0], [yk - 0.0625, yk + 0.0625], [vc - 0.0625, vc + 0.0625], &[]);
        for q in &mut kit.prisms[n0..] {
            q.embedded = true;
            q.hw_dash = false;
        }
        yk += 1.5;
    }
    let seat_top = ys + ts;
    let mut warns = Vec::new();
    match (&rod, rod_axis) {
        (None, _) => {
            let near = scene.find(&id).and_then(|me| nearest_rod(scene, me, p.bx, p.zc()));
            let fix = match near {
                Some(r) => format!("name the rod through the seat: {}", update_op(&id, "rod", &format!("\"{r}\""))),
                None => "add an anchor_bolt (or a threaded rod as a bolt) under the seat beside the post and set \"rod\" to its id".to_string(),
            };
            warns.push(warn(ctx, "W_HOLDOWN_ROD", "rod", format!("holdown '{id}' has no rod: the anchor bolt or threaded rod through its seat is what carries the uplift"), fix));
        }
        (Some(m), Some((_, dia, top))) => {
            let need_top = seat_top + 0.125 + 0.875 * dia;
            if top < need_top - 0.01 {
                let c = &scene.comps[m.idx];
                let fix = match Scene::anchor_point(c, m.inst, None, "top_of_concrete") {
                    Some(toc) if c.ty.name() == "anchor_bolt" => {
                        let proj = seat_top - toc.y + 0.125 + 1.125 * dia;
                        format!(
                            "make the rod reach through the seat: {}",
                            update_op(&m.id, "projection", &num_json(r16_up(proj)))
                        )
                    }
                    _ => format!(
                        "lengthen '{}' so it ends at least {} above the seat",
                        m.id,
                        fmt_ft_in(need_top - seat_top)
                    ),
                };
                warns.push(warn(ctx, "W_HOLDOWN_ROD", "rod", format!("holdown '{id}': rod '{}' ends at y {}, below the seat top (y {}) plus washer and nut; its nut cannot bear on the seat", m.id, fmt_ft_in(top), fmt_ft_in(seat_top)), fix));
            }
        }
        _ => {}
    }
    let seat_pt = if fr.along_x { V2::init(fr.f + fr.s * d, seat_top) } else { V2::init(vc, seat_top) };
    let info = format!(
        "connector {}holdown {gauge_i} ga, {} tall, {} face of {}{}{}",
        model_text(mi),
        fmt_ft_in(h),
        face.tag(),
        p.id,
        rod.as_ref().map_or(String::new(), |m| format!(", rod {}", m.id)),
        if along_face {
            format!(" (rod under the post: seat {} out from the face)", fmt_ft_in(d))
        } else {
            String::new()
        }
    );
    let hw = model::HwInfo { warns, rod: rod.map(|m| m.id), members: p.parts.clone(), ..Default::default() };
    let origin = ctx.origin;
    Some(kit.finish(origin, vec![NamedAnchor { name: "seat_top".into(), p: seat_pt }], info, hw))
}

fn tension_tie(
    ctx: &mut Ctx,
    cp: &Params,
    mi: &ModelInfo,
    gauge_i: u32,
    t: f64,
    post: &Member,
    rod: Option<&Member>,
) -> Option<Built> {
    if cp.seat_height.is_some() {
        ctx.p.fail(
            "seat_height",
            format!("a DTT seat follows the bolt nut bearing; Fix: {}", update_op(ctx.p.id, "seat_height", "null")),
        );
        return None;
    }
    let Some(rod) = rod else {
        ctx.p.fail(
            "rod",
            "a tension tie needs rod: the horizontal bolt through its seat and the connected wood members".into(),
        );
        return None;
    };
    let rc = &ctx.scene.comps[rod.idx];
    let xf = rc.xfs[rod.inst];
    let axis = xf.apply_dir(V2::init(0.0, -1.0));
    if axis.y.abs() > 1e-6 || rc.node.get("orient").and_then(Value::str) == Some("end") {
        let id = ctx.p.id;
        let warns = vec![warn(
            ctx,
            "W_CONNECTOR_ORIENT",
            "rod",
            format!(
                "tension tie '{id}': only a horizontal x-axis bolt is supported; no upright holdown surrogate is drawn for rod '{}'",
                rod.id
            ),
            format!(
                "use a horizontal bolt: {} (then place its head through the wood connection)",
                update_op(&rod.id, "rotate", "90")
            ),
        )];
        return Some(Built {
            box_: post.bx,
            points_mode: true,
            hw: model::HwInfo { warns, ..Default::default() },
            info: format!("connector {}tension tie (draws nothing: unsupported rod orientation)", model_text(mi)),
            ..Built::default()
        });
    }
    let nut = Scene::anchor_point(rc, rod.inst, None, "nut_bearing")?;
    let sign = if axis.x > 0.0 { 1.0 } else { -1.0 };
    let face = cp.face.unwrap_or(if rod.zc() >= post.zc() { Face::Front } else { Face::Back });
    if !matches!(face, Face::Front | Face::Back) {
        ctx.p.fail(
            "face",
            format!(
                "horizontal tension tie screws to a front/back joist face; Fix: {}",
                update_op(ctx.p.id, "face", "\"front\"")
            ),
        );
        return None;
    }
    let zf = if face == Face::Front { post.z1 } else { post.z0 };
    let out = if face == Face::Front { 1.0 } else { -1.0 };
    let length = cp.height.unwrap_or(6.9375);
    let width = cp.width.unwrap_or(3.25);
    let seat_t = cp.seat.unwrap_or(t);
    let xs = [nut.x, nut.x + sign * length];
    let ys = [nut.y - width / 2.0, nut.y + width / 2.0];
    let mut kit = Kit::new();
    kit.rect("back", xs, ys, [zf, zf + out * t], Vec::new());
    kit.rect("seat", [nut.x, nut.x + sign * seat_t], ys, [zf, rod.zc() + out * 0.5], Vec::new());
    for k in [0.25, 0.5, 0.75] {
        let x = nut.x + sign * length * k;
        kit.rect("screw", [x - 0.0625, x + 0.0625], [nut.y - 0.0625, nut.y + 0.0625], [zf - out * 1.5, zf], Vec::new());
        if let Some(p) = kit.prisms.last_mut() {
            p.embedded = true;
            p.hw_dash = false;
        }
    }
    let mut warns = Vec::new();
    if xs[0].min(xs[1]) < post.bx.x0 - 0.125
        || xs[0].max(xs[1]) > post.bx.x1 + 0.125
        || ys[0] < post.bx.y0
        || ys[1] > post.bx.y1
    {
        warns.push(warn(ctx,"W_CONNECTOR_ORIENT","post",format!("tension tie '{}': its horizontal body at x {}..{}, y {}..{} extends off '{}'; the bolt's nut bearing sets the seat",ctx.p.id,fmt_ft_in(xs[0].min(xs[1])),fmt_ft_in(xs[0].max(xs[1])),fmt_ft_in(ys[0]),fmt_ft_in(ys[1]),post.id),format!("place the bolt nut at the joist end and center it within the joist depth; resize the body if required: {}",update_op(ctx.p.id,"height",&num_json(length.min(post.bx.width()))))));
    }
    let members = post.parts.iter().chain(&rod.parts).copied().collect();
    let hw = model::HwInfo { warns, members, ..Default::default() };
    Some(kit.finish(
        ctx.origin,
        vec![NamedAnchor { name: "seat_top".into(), p: nut }, NamedAnchor { name: "seat".into(), p: nut }],
        format!(
            "connector {}horizontal tension tie {gauge_i} ga, {} long x {} wide on {} face of {}, bolt {}",
            model_text(mi),
            fmt_ft_in(length),
            fmt_ft_in(width),
            face.tag(),
            post.id,
            rod.id
        ),
        hw,
    ))
}

fn nearest_rod(scene: &Scene, holdown: &crate::scene::Comp, post: Box, zc: f64) -> Option<String> {
    let mut best: Option<(f64, String)> = None;
    for c in &scene.comps {
        if c.state != State::Ok || !matches!(c.ty.name(), "anchor_bolt" | "bolt") || !scene.together(holdown, c) {
            continue;
        }
        let Some(q) = c.world.first() else { continue };
        let b = geom::loop_box(&q.loops[0]);
        let dx = (post.x0 - b.x1).max(b.x0 - post.x1).max(0.0);
        let dz = ((q.z0 + q.z1) / 2.0 - zc).abs();
        let dist = dx + dz;
        if dist <= 8.0 && best.as_ref().is_none_or(|bb| dist < bb.0) {
            best = Some((dist, c.id.clone()));
        }
    }
    best.map(|b| b.1)
}

const FACES: [&str; 6] = ["face_left", "face_right", "face_top", "face_bottom", "face_front", "face_back"];

pub fn build_wrap(
    ctx: &mut Ctx,
    cp: &Params,
    mi: &ModelInfo,
    on: &Value,
    gauge_i: u32,
    t: f64,
    width: f64,
) -> Option<Built> {
    let faces = on.arr().unwrap_or(&[]);
    let names: Vec<(&str, &str)> = faces.iter().filter_map(Value::str).filter_map(|s| s.rsplit_once('.')).collect();
    if names.len() != 3
        || names[1].1 != "face_top"
        || !matches!((names[0].1, names[2].1), ("face_front", "face_back") | ("face_back", "face_front"))
    {
        ctx.p.fail("on","a wrap uses [\"post.face_front\",\"beam.face_top\",\"post.face_back\"] (or back/top/front); other multi-face paths are not supported".into());
        return None;
    }
    for key in ["at", "z", "rotate", "slope", "mirror", "lay", "side"] {
        if ctx.p.has(key) {
            ctx.p.fail(key, format!("a wrap is placed from its faces; remove '{key}'"));
        }
    }
    if !ctx.p.ok {
        return None;
    }
    let a = member(ctx, "on", names[0].0)?;
    let b = member(ctx, "on", names[1].0)?;
    let c = member(ctx, "on", names[2].0)?;
    let (front, back) = if names[0].1 == "face_front" { (&a, &c) } else { (&c, &a) };
    let pts = match cp.points.as_ref() {
        Some(v) => Some(parse_point_list(ctx, "points", v, true)?),
        None => None,
    };
    let xc = pts.as_ref().and_then(|p| p.first()).map_or((a.bx.x0 + a.bx.x1) / 2.0, |p| p.x + ctx.origin.x);
    if pts.as_ref().is_some_and(|p| p.iter().any(|p| (p.x + ctx.origin.x - xc).abs() > 1e-6 || p.b != 0.0)) {
        ctx.p.fail(
            "points",
            "wrap points must lie on one vertical station (same x, no bulges); their lower y sets the strap ends"
                .into(),
        );
        return None;
    }
    if [&a, &b, &c].iter().any(|m| xc - width / 2.0 < m.bx.x0 - 1e-6 || xc + width / 2.0 > m.bx.x1 + 1e-6)
        || (front.z1 - b.z1).abs() > 0.125
        || (back.z0 - b.z0).abs() > 0.125
        || a.bx.y1 > b.bx.y1
        || c.bx.y1 > b.bx.y1
    {
        ctx.p.fail("on","wrap faces must align with the beam's front/back, and the full strap width must fit on all three faces; set points to a shared x station".into());
        return None;
    }
    let bottom = pts.as_ref().map(|p| p.iter().map(|p| p.y + ctx.origin.y).fold(f64::INFINITY, f64::min));
    let ya = bottom.unwrap_or(a.bx.y0);
    let yc = bottom.unwrap_or(c.bx.y0);
    let top = b.bx.y1;
    if ya >= top || yc >= top {
        ctx.p.fail("points", "wrap lower ends must be below the beam top".into());
        return None;
    }
    let x = [xc - width / 2.0, xc + width / 2.0];
    let mut kit = Kit::new();
    for (face, lo, hi, z) in [
        (
            names[0].1,
            ya,
            top,
            if names[0].1 == "face_front" { [front.z1, front.z1 + t] } else { [back.z0 - t, back.z0] },
        ),
        (
            names[2].1,
            yc,
            top,
            if names[2].1 == "face_front" { [front.z1, front.z1 + t] } else { [back.z0 - t, back.z0] },
        ),
    ] {
        kit.rect(face, x, [lo, hi], z, Vec::new());
        if let Some(p) = kit.prisms.last_mut() {
            p.face_tie = true;
        }
    }
    kit.rect("wrap_top", x, [top, top + t], [back.z0 - t, front.z1 + t], Vec::new());
    let members = a.parts.iter().chain(&b.parts).chain(&c.parts).copied().collect();
    Some(kit.finish(
        ctx.origin,
        vec![
            NamedAnchor { name: "start".into(), p: V2::init(xc, ya) },
            NamedAnchor { name: "end".into(), p: V2::init(xc, yc) },
            NamedAnchor { name: "top".into(), p: V2::init(xc, top) },
        ],
        format!(
            "connector {}wrap strap {gauge_i} ga x {}, front/top/back over {}",
            model_text(mi),
            fmt_ft_in(width),
            b.id
        ),
        model::HwInfo { members, ..Default::default() },
    ))
}

pub fn build_on(
    ctx: &mut Ctx,
    cp: &Params,
    mi: &ModelInfo,
    on: &str,
    gauge_i: u32,
    t: f64,
    width: f64,
) -> Option<Built> {
    let Some((mname, face)) = on.rsplit_once('.').filter(|(_, f)| FACES.contains(f)) else {
        ctx.p.fail(
            "on",
            format!("'on' must be \"<member>.<face>\" with face one of {} (got \"{on}\")", FACES.join(", ")),
        );
        return None;
    };
    for key in [
        "side",
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
    ] {
        if ctx.p.has(key) {
            ctx.p.fail(
                key,
                format!("'{key}' is not used with 'on': the face sets the strap's side and z; remove '{key}'"),
            );
        }
    }
    let m = member(ctx, "on", mname)?;
    let z_face = matches!(face, "face_front" | "face_back");
    if ctx.p.has("lay") && (cp.lay == super::connector::Lay::Face) != z_face {
        ctx.p.fail(
            "lay",
            format!(
                "on {face}: the strap is seen {} there, so lay is {}; remove 'lay'",
                if z_face { "face-on" } else { "edge-on" },
                if z_face { "face" } else { "edge" }
            ),
        );
    }
    if !ctx.p.ok {
        return None;
    }
    ctx.fixed = Some(format!("a strap 'on' a face takes its place from the face of '{mname}'"));
    let origin = ctx.origin;
    let pts: Option<Vec<Pt>> = match cp.points.as_ref() {
        Some(pv) => {
            let local = parse_point_list(ctx, "points", pv, true)?;
            Some(local.iter().map(|q| Pt { x: q.x + origin.x, y: q.y + origin.y, b: q.b }).collect())
        }
        None => None,
    };
    let bx = m.bx;
    let (xc, yc) = ((bx.x0 + bx.x1) / 2.0, (bx.y0 + bx.y1) / 2.0);
    let mut nat_z = width;
    let mut zc = m.zc();
    let mut face_tie = false;
    let mut centerline: Vec<Pt>;
    let rib: Vec<Pt>;
    let mut lay = "edge";
    match face {
        "face_left" | "face_right" | "face_top" | "face_bottom" => {
            let (vertical, fc, out) = match face {
                "face_left" => (true, bx.x0, V2::init(-1.0, 0.0)),
                "face_right" => (true, bx.x1, V2::init(1.0, 0.0)),
                "face_top" => (false, bx.y1, V2::init(0.0, 1.0)),
                _ => (false, bx.y0, V2::init(0.0, -1.0)),
            };
            let along_z = !vertical && pts.is_none() && m.z1 - m.z0 > bx.width();
            if along_z {
                let y = [fc, fc + out.y * t];
                let lp = model::rect_loop(xc - width / 2.0, y[0].min(y[1]), xc + width / 2.0, y[0].max(y[1]));
                centerline = vec![Pt::new(xc - width / 2.0, fc), Pt::new(xc + width / 2.0, fc)];
                rib = lp;
                nat_z = m.z1 - m.z0;
            } else {
                let raw = pts.unwrap_or_else(|| {
                    if vertical {
                        vec![Pt::new(fc, bx.y0), Pt::new(fc, bx.y1)]
                    } else {
                        vec![Pt::new(bx.x0, fc), Pt::new(bx.x1, fc)]
                    }
                });
                centerline = drop_duplicate_points(
                    &raw.iter().map(|q| if vertical { Pt::new(fc, q.y) } else { Pt::new(q.x, fc) }).collect::<Vec<_>>(),
                );
                if centerline.len() < 2 {
                    ctx.p.fail("points", format!("on {face}: the points snap onto the face line and leave fewer than 2 distinct points; give points along the face"));
                    return None;
                }
                let d = centerline[1].v().sub(centerline[0].v());
                let left = V2::init(-d.y, d.x);
                rib = if left.dot(out) > 0.0 {
                    pathgeom::ribbon(&centerline, t, 0.0)
                } else {
                    pathgeom::ribbon(&centerline, 0.0, t)
                };
            }
        }
        _ => {
            lay = "face";
            let raw = pts.unwrap_or_else(|| {
                if bx.height() >= bx.width() {
                    vec![Pt::new(xc, bx.y0), Pt::new(xc, bx.y1)]
                } else {
                    vec![Pt::new(bx.x0, yc), Pt::new(bx.x1, yc)]
                }
            });
            centerline = drop_duplicate_points(&raw);
            if centerline.len() < 2 {
                ctx.p.fail("points", "a connector needs at least 2 distinct points".to_string());
                return None;
            }
            rib = pathgeom::ribbon(&centerline, width / 2.0, width / 2.0);
            nat_z = t;
            zc = if face == "face_front" { m.z1 + t / 2.0 } else { m.z0 - t / 2.0 };
            face_tie = face == "face_front";
        }
    }
    let local =
        |l: &[Pt]| -> Vec<Pt> { l.iter().map(|q| Pt { x: q.x - origin.x, y: q.y - origin.y, b: q.b }).collect() };
    let rib = local(&rib);
    centerline = local(&centerline);
    let anchors = vertex_anchors(&centerline, false);
    let prism = Prism {
        material: "steel".to_string(),
        loops: model::one_loop(rib),
        centerline,
        face_tie,
        hw_dash: true,
        ..Prism::default()
    };
    let box_ = super::common::box_of_prisms(core::slice::from_ref(&prism));
    Some(Built {
        prisms: vec![prism],
        anchors,
        box_,
        nat_z: Some(nat_z),
        nat_zc: Some(zc),
        points_mode: true,
        info: strap_info(mi, gauge_i, width, lay, Some(on)),
        hw: model::HwInfo { members: m.parts.clone(), ..Default::default() },
        ..Built::default()
    })
}
