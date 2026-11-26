//! Rebar hooks, closed ties, stirrups and adhesive dowels (SPEC 32.2): centrelines for 'rebar' built from ACI bend tables.

use super::common::{Ctx, world_box};
use crate::geom::{self, Box, Pt, RAD_PER_DEG, V2};
use crate::json::Value;
use crate::math;
use crate::model::{self, Level, NamedAnchor, fmt_num};
use crate::pathgeom;
use crate::scene::{Scene, State, join_ids};
use crate::units::{self, fmt_ft_in};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub fn bar_number(size: &str) -> u32 {
    size.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()).unwrap_or(4)
}

/// Inside bend diameter and straight extension: ACI 318-19 Table 25.3.1 (standard hooks) and 25.3.2 (stirrup and tie hooks).
pub fn hook_dims(no: u32, d: f64, angle: u32, tie: bool) -> (f64, f64) {
    if tie {
        let dia = if no <= 5 { 4.0 * d } else { 6.0 * d };
        let ext = match angle {
            90 if no <= 5 => (6.0 * d).max(3.0),
            90 => 12.0 * d,
            135 => (6.0 * d).max(3.0),
            _ => (4.0 * d).max(2.5),
        };
        return (dia, ext);
    }
    let dia = if no <= 8 { 6.0 * d } else { 8.0 * d };
    let ext = if angle == 90 { 12.0 * d } else { (4.0 * d).max(2.5) };
    (dia, ext)
}

fn rot(v: V2, a: f64) -> V2 {
    let (s, c) = (math::sin(a), math::cos(a));
    V2::init(v.x * c - v.y * s, v.x * s + v.y * c)
}

fn only_keys(ctx: &mut Ctx, key: &str, v: &Value, allowed: &[&str], example: &str) -> bool {
    let Value::Object(members) = v else {
        ctx.p.fail(key, format!("'{key}' must be an object, e.g. {example} (got {})", model::kind_or_text(v)));
        return false;
    };
    for m in members {
        if !allowed.contains(&m.key.as_str()) {
            let hint = model::nearest(&m.key, allowed).map_or(String::new(), |n| format!(" Did you mean '{n}'?"));
            ctx.p.fail(
                &format!("{key}/{}", m.key),
                format!("unknown key '{}' in '{key}'.{hint} Allowed: {}; e.g. {example}", m.key, allowed.join(", ")),
            );
            return false;
        }
    }
    true
}

#[derive(Clone, Copy)]
pub struct HookEnd {
    pub angle: u32,
    pub toward: V2,
    pub tie: bool,
    pub ext: Option<f64>,
}

const HOOK_EXAMPLE: &str = "{\"end\": {\"angle\": 90, \"toward\": \"down\"}}";

fn toward_vec(name: &str, xz: bool) -> Option<V2> {
    let up = if xz { -1.0 } else { 1.0 };
    Some(match name {
        "up" => V2::init(0.0, up),
        "down" => V2::init(0.0, -up),
        "left" => V2::init(-1.0, 0.0),
        "right" => V2::init(1.0, 0.0),
        _ => return None,
    })
}

fn parse_hook_end(ctx: &mut Ctx, key: &str, v: &Value, xz: bool) -> Option<HookEnd> {
    if !only_keys(ctx, key, v, &["angle", "toward", "kind", "ext"], HOOK_EXAMPLE) {
        return None;
    }
    let angle = match v.get("angle").and_then(Value::num) {
        Some(90.0) => 90,
        Some(135.0) => 135,
        Some(180.0) => 180,
        _ => {
            ctx.p.fail(
                &format!("{key}/angle"),
                format!("{key}.angle must be 90, 135 or 180 (degrees of the standard hook), e.g. {HOOK_EXAMPLE}"),
            );
            return None;
        }
    };
    let Some(toward) = v.get("toward").and_then(Value::str).and_then(|t| toward_vec(t, xz)) else {
        ctx.p.fail(
            &format!("{key}/toward"),
            format!("{key}.toward is required: \"up\", \"down\", \"left\" or \"right\", the side of the bar the hook turns to as drawn{} (a horizontal bar hooks up or down, a vertical bar left or right)", if xz { " in plan" } else { "" }),
        );
        return None;
    };
    let tie = match v.get("kind").filter(|k| !k.is_null()) {
        None => angle == 135,
        Some(Value::String(s)) if s == "standard" => false,
        Some(Value::String(s)) if s == "tie" => true,
        Some(o) => {
            ctx.p.fail(&format!("{key}/kind"), format!("{key}.kind must be \"standard\" (ACI 318 Table 25.3.1, the default for 90 and 180) or \"tie\" (stirrup and tie hooks, Table 25.3.2, the default for 135), got {}", model::kind_or_text(o)));
            return None;
        }
    };
    let ext = match v.get("ext").filter(|e| !e.is_null()) {
        None => None,
        Some(e) => match units::parse_length(e).filter(|x| *x > 0.0) {
            Some(x) => Some(x),
            None => {
                ctx.p.fail(&format!("{key}/ext"), format!("{key}.ext must be a positive length (the straight extension past the bend; default: the ACI minimum), got {}", model::kind_or_text(e)));
                return None;
            }
        },
    };
    Some(HookEnd { angle, toward, tie, ext })
}

pub fn parse_hooks(ctx: &mut Ctx, v: &Value, xz: bool) -> Option<[Option<HookEnd>; 2]> {
    if !only_keys(ctx, "hook", v, &["start", "end"], HOOK_EXAMPLE) {
        return None;
    }
    let mut out = [None, None];
    for (k, name) in ["start", "end"].iter().enumerate() {
        if let Some(e) = v.get(name).filter(|e| !e.is_null()) {
            out[k] = Some(parse_hook_end(ctx, &format!("hook/{name}"), e, xz)?);
        }
    }
    if out[0].is_none() && out[1].is_none() {
        ctx.p.fail("hook", format!("'hook' needs \"start\" and/or \"end\", e.g. {HOOK_EXAMPLE}"));
        return None;
    }
    Some(out)
}

struct Arc3 {
    pts: [Pt; 3],
    centre: V2,
    mid: V2,
}

/// A bend that leaves the tangent point 't0' travelling along 'u', turns 'angle' degrees toward 'side' on centreline radius 'rc' and runs 'ext' straight.
fn hook_arc(t0: V2, u: V2, side: V2, rc: f64, angle: u32, ext: f64) -> Arc3 {
    let centre = t0.add(side.scale(rc));
    let sgn = if u.cross(side) > 0.0 { 1.0 } else { -1.0 };
    let phi = sgn * f64::from(angle) * RAD_PER_DEG;
    let spoke = t0.sub(centre);
    let t1 = centre.add(rot(spoke, phi));
    let tip = t1.add(rot(u, phi).scale(ext));
    Arc3 {
        pts: [Pt::at(t0, geom::bulge_from_sweep(phi)), Pt::at(t1, 0.0), Pt::at(tip, 0.0)],
        centre,
        mid: centre.add(rot(spoke, phi / 2.0)),
    }
}

fn hook_one(
    ctx: &mut Ctx,
    body: &mut Vec<Pt>,
    h: HookEnd,
    which: &str,
    no: u32,
    d: f64,
    anchors: &mut Vec<NamedAnchor>,
) -> Option<String> {
    let n = body.len();
    let (p, prev) = (body[n - 1].v(), body[n - 2].v());
    let u = p.sub(prev).norm();
    let across = u.perp().dot(h.toward);
    if across.abs() < 0.2 {
        ctx.p.fail(
            &format!("hook/{which}/toward"),
            format!(
                "hook.{which}.toward points along the bar there; the hook turns to a side of the bar: use {}",
                if u.x.abs() > u.y.abs() { "\"up\" or \"down\"" } else { "\"left\" or \"right\"" }
            ),
        );
        return None;
    }
    let side = u.perp().scale(if across > 0.0 { 1.0 } else { -1.0 });
    let (dia, min_ext) = hook_dims(no, d, h.angle, h.tie);
    let ext = h.ext.unwrap_or(min_ext);
    let rc = dia / 2.0 + d / 2.0;
    if body[n - 2].b != 0.0 || p.dist(prev) < rc - 1e-6 {
        ctx.p.fail(
            &format!("hook/{which}"),
            format!("the {which} leg of the bar is {} long, shorter than the hook's bend radius {} (inside bend diameter {} for this bar): lengthen that leg or drop the hook", fmt_ft_in(p.dist(prev)), fmt_ft_in(rc), fmt_ft_in(dia)),
        );
        return None;
    }
    let arc = hook_arc(p.sub(u.scale(rc)), u, side, rc, h.angle, ext);
    body.pop();
    if body.last().is_some_and(|l| l.v().eql(arc.pts[0].v(), 1e-9)) {
        body.pop();
    }
    body.extend(arc.pts);
    anchors.push(NamedAnchor { name: format!("{which}_hook_end"), p: arc.pts[2].v() });
    anchors.push(NamedAnchor { name: format!("{which}_hook_tangent"), p: arc.pts[0].v() });
    anchors.push(NamedAnchor { name: format!("{which}_hook_bend"), p: arc.mid });
    anchors.push(NamedAnchor { name: format!("{which}_hook_center"), p: arc.centre });
    Some(format!(
        ", {} {} hook at {which} (bend dia {}, ext {})",
        if h.tie { "tie" } else { "std" },
        h.angle,
        fmt_ft_in(dia),
        fmt_ft_in(ext)
    ))
}

pub struct Hooked {
    pub center: Vec<Pt>,
    pub anchors: Vec<NamedAnchor>,
    pub info: String,
}

pub fn apply_hooks(ctx: &mut Ctx, body: &[Pt], hooks: &[Option<HookEnd>; 2], no: u32, d: f64) -> Option<Hooked> {
    let mut center = body.to_vec();
    let mut anchors = Vec::new();
    let mut info = String::new();
    if let Some(h) = hooks[0] {
        center = pathgeom::reverse_open(&center);
        info.push_str(&hook_one(ctx, &mut center, h, "start", no, d, &mut anchors)?);
        center = pathgeom::reverse_open(&center);
    }
    if let Some(h) = hooks[1] {
        info.push_str(&hook_one(ctx, &mut center, h, "end", no, d, &mut anchors)?);
    }
    Some(Hooked { center, anchors, info })
}

pub struct Tie {
    pub paths: Vec<Vec<Pt>>,
    pub anchors: Vec<NamedAnchor>,
    pub info: String,
}

const TIE_EXAMPLE: &str = "{\"in\": \"pedestal\", \"cover\": 1.5}";

struct TieZone {
    bx: Box,
    name: String,
}

fn tie_zone(ctx: &mut Ctx, tv: &Value, xz: bool) -> Option<TieZone> {
    let Some(in_s) = tv.get("in").and_then(Value::str) else {
        ctx.p.fail(
            "tie/in",
            format!("tie needs \"in\": \"<component>[.<part>]\", the member it is placed in, e.g. {TIE_EXAMPLE}"),
        );
        return None;
    };
    let (host_id, part) = match in_s.split_once('.') {
        Some((h, pt)) => (h, Some(pt)),
        None => (in_s, None),
    };
    let scene = ctx.scene;
    let path = format!("{}/{}/tie/in", ctx.p.base, ctx.p.id);
    let Some(host) = scene.find(host_id) else {
        let ids = scene.comp_ids();
        let hint = model::nearest(host_id, &ids)
            .map_or(format!(" Known ids: {}", join_ids(&ids)), |n| format!(" Did you mean '{n}'?"));
        scene.diags.add(
            Level::Error,
            "E_REF_UNKNOWN",
            Some(ctx.p.id),
            Some(&path),
            format!("tie.in: no component '{host_id}'.{hint}"),
        );
        ctx.p.ok = false;
        return None;
    };
    if host.state != State::Ok {
        ctx.p.fail("tie/in", format!("tie.in host '{host_id}' did not build; fix its errors first"));
        return None;
    }
    let xf0 = host.xfs.first()?;
    let local = match part {
        Some(pt) => match Scene::part_box(host, pt) {
            Some(b) => b,
            None => {
                let parts = scene.part_names(host);
                scene.diags.add(
                    Level::Error,
                    "E_REF_UNKNOWN",
                    Some(ctx.p.id),
                    Some(&path),
                    format!(
                        "tie.in: component '{host_id}' has no part '{pt}'. Parts: {}",
                        if parts.is_empty() { "(none)".to_string() } else { join_ids(&parts) }
                    ),
                );
                ctx.p.ok = false;
                return None;
            }
        },
        None => host.built.box_,
    };
    let mut bx = world_box(xf0, local);
    if xz {
        let z = host.zs.first()?;
        bx = Box { x0: bx.x0, y0: -z[1], x1: bx.x1, y1: -z[0] };
    }
    Some(TieZone { bx, name: in_s.to_string() })
}

fn corner_dirs(name: &str) -> Option<(f64, f64)> {
    Some(match name {
        "top_left" => (-1.0, 1.0),
        "top_right" => (1.0, 1.0),
        "bottom_left" => (-1.0, -1.0),
        "bottom_right" => (1.0, -1.0),
        _ => return None,
    })
}

/// A tie, stirrup, crosstie or hoop in its host by cover, in drawn coordinates (plane xz: [x, -z], mapped back by the caller).
pub fn tie(ctx: &mut Ctx, tv: &Value, no: u32, d: f64, xz: bool) -> Option<Tie> {
    let keys = ["in", "cover", "shape", "hook", "corner", "open", "axis", "station", "toward"];
    if !only_keys(ctx, "tie", tv, &keys, TIE_EXAMPLE) {
        return None;
    }
    let zone = tie_zone(ctx, tv, xz)?;
    let cover = ctx.p.field_len("tie", tv, "cover", 1.5)?;
    let shape = ctx.p.field_choice("tie", tv, "shape", &["closed", "u", "crosstie", "hoop"])?;
    let r = d / 2.0;
    let b = Box {
        x0: zone.bx.x0 + cover + r,
        y0: zone.bx.y0 + cover + r,
        x1: zone.bx.x1 - cover - r,
        y1: zone.bx.y1 - cover - r,
    };
    let (dia, _) = hook_dims(no, d, 135, true);
    let rc = dia / 2.0 + r;
    let (w, h) = (b.x1 - b.x0, b.y1 - b.y0);
    let across = if xz { "z" } else { "y" };
    if w < 2.0 * rc + 1e-6 || h < 2.0 * rc + 1e-6 {
        ctx.p.fail(
            "tie",
            format!(
                "'{}' is {} x {} (x by {across}){}: a tie at cover {} with its {} bends does not fit; lower tie.cover or check the host size",
                zone.name,
                fmt_ft_in(zone.bx.width()),
                fmt_ft_in(zone.bx.height()),
                if xz { "; its z extent is the member's 'z' (set e.g. \"z\": [-9, 9] on a pedestal or column so a plan tie knows its depth)" } else { "" },
                fmt_ft_in(cover),
                fmt_ft_in(dia),
            ),
        );
        return None;
    }
    let hook_angle = match tv.get("hook").filter(|x| !x.is_null()) {
        None => 135,
        Some(x) => match x.num() {
            Some(90.0) => 90,
            Some(135.0) => 135,
            Some(a) if a == 180.0 && shape == "u" => 180,
            _ => {
                ctx.p.fail(
                    "tie/hook",
                    format!(
                        "tie.hook must be 135 (seismic hook, the default) or 90{}, got {}",
                        if shape == "u" { " or 180" } else { "" },
                        model::kind_or_text(x)
                    ),
                );
                return None;
            }
        },
    };
    let (_, ext) = hook_dims(no, d, hook_angle, true);
    let out_w = fmt_ft_in(w + d);
    let out_h = fmt_ft_in(h + d);
    let mut anchors = vec![
        NamedAnchor { name: "tie_top_left".into(), p: V2::init(b.x0, b.y1) },
        NamedAnchor { name: "tie_top_right".into(), p: V2::init(b.x1, b.y1) },
        NamedAnchor { name: "tie_bottom_left".into(), p: V2::init(b.x0, b.y0) },
        NamedAnchor { name: "tie_bottom_right".into(), p: V2::init(b.x1, b.y0) },
        NamedAnchor { name: "tie_left".into(), p: V2::init(b.x0, (b.y0 + b.y1) / 2.0) },
        NamedAnchor { name: "tie_right".into(), p: V2::init(b.x1, (b.y0 + b.y1) / 2.0) },
        NamedAnchor { name: "tie_top".into(), p: V2::init((b.x0 + b.x1) / 2.0, b.y1) },
        NamedAnchor { name: "tie_bottom".into(), p: V2::init((b.x0 + b.x1) / 2.0, b.y0) },
    ];
    let mut paths: Vec<Vec<Pt>> = Vec::new();

    let info = match shape {
        "closed" => {
            let cname = tv.get("corner").and_then(Value::str).unwrap_or("top_left");
            let Some((sx, sy)) = corner_dirs(cname) else {
                ctx.p.fail("tie/corner", format!("tie.corner (where the two hooks lap) must be top_left, top_right, bottom_left or bottom_right (got \"{cname}\")"));
                return None;
            };
            let c = V2::init(if sx < 0.0 { b.x0 } else { b.x1 }, if sy < 0.0 { b.y0 } else { b.y1 });
            let (e1, e2) = (V2::init(-sx, 0.0), V2::init(0.0, -sy));
            let (ta, tb) = (c.add(e1.scale(rc)), c.add(e2.scale(rc)));
            let ring = [ta, c.add(e1.scale(w)), c.add(e1.scale(w)).add(e2.scale(h)), c.add(e2.scale(h)), tb];
            paths.push(pathgeom::fillet(&ring, rc));
            let h1 = hook_arc(ta, e1.scale(-1.0), e2, rc, hook_angle, ext);
            let h2 = hook_arc(tb, e2.scale(-1.0), e1, rc, hook_angle, ext);
            anchors.push(NamedAnchor { name: "hook_1_end".into(), p: h1.pts[2].v() });
            anchors.push(NamedAnchor { name: "hook_2_end".into(), p: h2.pts[2].v() });
            anchors.push(NamedAnchor { name: "hook_corner".into(), p: h1.centre });
            paths.push(h1.pts.to_vec());
            paths.push(h2.pts.to_vec());
            format!("closed tie {out_w} x {out_h} out to out, {hook_angle} hooks at {cname}")
        }
        "u" => {
            let open = tv.get("open").and_then(Value::str).unwrap_or("top");
            let (p0, p1, p2, p3) = match open {
                "top" => (V2::init(b.x0, b.y1), V2::init(b.x0, b.y0), V2::init(b.x1, b.y0), V2::init(b.x1, b.y1)),
                "bottom" => (V2::init(b.x0, b.y0), V2::init(b.x0, b.y1), V2::init(b.x1, b.y1), V2::init(b.x1, b.y0)),
                "left" => (V2::init(b.x0, b.y0), V2::init(b.x1, b.y0), V2::init(b.x1, b.y1), V2::init(b.x0, b.y1)),
                "right" => (V2::init(b.x1, b.y0), V2::init(b.x0, b.y0), V2::init(b.x0, b.y1), V2::init(b.x1, b.y1)),
                _ => {
                    ctx.p.fail("tie/open", format!("tie.open (the open side of a U stirrup) must be top, bottom, left or right (got \"{open}\")"));
                    return None;
                }
            };
            let mut body = pathgeom::fillet(&[p0, p1, p2, p3], rc);
            let inward = p3.sub(p0).norm();
            let n = body.len();
            let u_end = body[n - 1].v().sub(body[n - 2].v()).norm();
            let a_end = hook_arc(body[n - 1].v().sub(u_end.scale(rc)), u_end, inward.scale(-1.0), rc, hook_angle, ext);
            body.pop();
            body.extend(a_end.pts);
            let mut rev = pathgeom::reverse_open(&body);
            let m = rev.len();
            let u_st = rev[m - 1].v().sub(rev[m - 2].v()).norm();
            let a_st = hook_arc(rev[m - 1].v().sub(u_st.scale(rc)), u_st, inward, rc, hook_angle, ext);
            rev.pop();
            rev.extend(a_st.pts);
            anchors.push(NamedAnchor { name: "hook_1_end".into(), p: a_st.pts[2].v() });
            anchors.push(NamedAnchor { name: "hook_2_end".into(), p: a_end.pts[2].v() });
            paths.push(pathgeom::reverse_open(&rev));
            format!("U stirrup {out_w} x {out_h} out to out, open {open}, {hook_angle} hooks")
        }
        "crosstie" => {
            let axis = ctx.p.field_choice("tie", tv, "axis", &["x", "y"])?;
            let along_x = axis == "x";
            let (lo, hi) = if along_x { (zone.bx.y0, zone.bx.y1) } else { (zone.bx.x0, zone.bx.x1) };
            let st = match tv.get("station").filter(|s| !s.is_null()) {
                None => (lo + hi) / 2.0,
                Some(s) => match units::parse_length(s) {
                    Some(o) => lo + o,
                    None => {
                        ctx.p.fail("tie/station", format!("tie.station must be a length: the crosstie's offset from the host's {} face as drawn (default: mid-depth), got {}", if along_x { "bottom" } else { "left" }, model::kind_or_text(s)));
                        return None;
                    }
                },
            };
            let tname = tv.get("toward").and_then(Value::str).unwrap_or(if along_x { "down" } else { "left" });
            let tw = match tname {
                "up" if along_x => V2::init(0.0, 1.0),
                "down" if along_x => V2::init(0.0, -1.0),
                "left" if !along_x => V2::init(-1.0, 0.0),
                "right" if !along_x => V2::init(1.0, 0.0),
                _ => {
                    ctx.p.fail("tie/toward", format!("tie.toward (the side both crosstie hooks turn to) must be {} for axis \"{axis}\" (got \"{tname}\")", if along_x { "\"up\" or \"down\"" } else { "\"left\" or \"right\"" }));
                    return None;
                }
            };
            let (p0, p1) = if along_x {
                (V2::init(b.x0, st), V2::init(b.x1, st))
            } else {
                (V2::init(st, b.y0), V2::init(st, b.y1))
            };
            let u = p1.sub(p0).norm();
            let (_, e135) = hook_dims(no, d, 135, true);
            let (_, e90) = hook_dims(no, d, 90, true);
            let a1 = hook_arc(p1.sub(u.scale(rc)), u, tw, rc, 90, e90);
            let a0 = hook_arc(p0.add(u.scale(rc)), u.scale(-1.0), tw, rc, 135, e135);
            let mut path = pathgeom::reverse_open(&a0.pts);
            path.extend(a1.pts);
            anchors.push(NamedAnchor { name: "hook_1_end".into(), p: a0.pts[2].v() });
            anchors.push(NamedAnchor { name: "hook_2_end".into(), p: a1.pts[2].v() });
            anchors.push(NamedAnchor { name: "tie_center".into(), p: p0.mid(p1) });
            paths.push(path);
            format!("crosstie {} out to out along {axis}, 135 and 90 hooks", fmt_ft_in(p0.dist(p1) + d))
        }
        _ => {
            let c = V2::init((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
            let rr = w.min(h) / 2.0;
            let (e, wst) = (V2::init(c.x + rr, c.y), V2::init(c.x - rr, c.y));
            paths.push(vec![Pt::at(e, 1.0), Pt::at(wst, 0.0)]);
            paths.push(vec![Pt::at(wst, 1.0), Pt::at(e, 0.0)]);
            anchors.clear();
            for (name, p) in [
                ("tie_left", wst),
                ("tie_right", e),
                ("tie_top", V2::init(c.x, c.y + rr)),
                ("tie_bottom", V2::init(c.x, c.y - rr)),
                ("tie_center", c),
            ] {
                anchors.push(NamedAnchor { name: name.into(), p });
            }
            format!("hoop {} dia out to out", fmt_ft_in(2.0 * rr + d))
        }
    };
    Some(Tie { paths, anchors, info: format!("{info} in '{}' at {} cover", zone.name, fmt_num(cover)) })
}

pub struct Epoxy {
    pub hole: Vec<Pt>,
    pub face: V2,
    pub tip: V2,
    pub info: String,
}

const EPOXY_EXAMPLE: &str = "{\"in\": \"e_stem\", \"embed\": 6}";

/// An adhesive dowel: the end of the bar that enters 'epoxy.in' is set 'embed' past the face it crosses, and the drilled hole is returned.
pub fn epoxy(ctx: &mut Ctx, ev: &Value, vs: &mut [V2], d: f64) -> Option<Epoxy> {
    if !only_keys(ctx, "epoxy", ev, &["in", "embed", "end", "hole"], EPOXY_EXAMPLE) {
        return None;
    }
    let Some(host_id) = ev.get("in").and_then(Value::str) else {
        ctx.p.fail(
            "epoxy/in",
            format!("epoxy needs \"in\": the concrete or masonry member the bar is drilled into, e.g. {EPOXY_EXAMPLE}"),
        );
        return None;
    };
    let embed = match ev.get("embed").filter(|e| !e.is_null()) {
        None => None,
        Some(e) => match units::parse_length(e).filter(|x| *x > 0.0) {
            Some(x) => Some(x),
            None => {
                ctx.p.fail(
                    "epoxy/embed",
                    format!(
                        "epoxy.embed must be a positive length (the depth of the bar in the drilled hole), got {}",
                        model::kind_or_text(e)
                    ),
                );
                return None;
            }
        },
    };
    let hole_d = ctx.p.field_len("epoxy", ev, "hole", d + 0.125)?;
    let scene = ctx.scene;
    let path = format!("{}/{}/epoxy/in", ctx.p.base, ctx.p.id);
    let Some(host) = scene.find(host_id) else {
        let ids = scene.comp_ids();
        let hint = model::nearest(host_id, &ids)
            .map_or(format!(" Known ids: {}", join_ids(&ids)), |n| format!(" Did you mean '{n}'?"));
        scene.diags.add(
            Level::Error,
            "E_REF_UNKNOWN",
            Some(ctx.p.id),
            Some(&path),
            format!("epoxy.in: no component '{host_id}'.{hint}"),
        );
        ctx.p.ok = false;
        return None;
    };
    if host.state != State::Ok {
        ctx.p.fail("epoxy/in", format!("epoxy.in host '{host_id}' did not build; fix its errors first"));
        return None;
    }
    let xf0 = host.xfs.first()?;
    let outline: Vec<V2> = match host.built.host.as_ref() {
        Some(h) => geom::flatten_loop(&xf0.apply_loop(&h.outline), geom::FLAT_TOL),
        None => {
            let b = world_box(xf0, host.built.box_);
            vec![V2::init(b.x0, b.y0), V2::init(b.x1, b.y0), V2::init(b.x1, b.y1), V2::init(b.x0, b.y1)]
        }
    };
    let (origin, angle) = (ctx.origin, ctx.angle);
    let to_world = |v: V2| origin.add(rot(v, angle));
    let to_local = |w: V2| rot(w.sub(origin), -angle);
    let n = vs.len();
    let hits = |a: V2, u: V2| {
        let mut ts: Vec<f64> = Vec::new();
        for (i, &p) in outline.iter().enumerate() {
            let e = outline[(i + 1) % outline.len()].sub(p);
            let den = u.cross(e);
            if den.abs() < 1e-12 {
                continue;
            }
            let t = p.sub(a).cross(e) / den;
            let s = p.sub(a).cross(u) / den;
            if t > -1e-9 && (-1e-9..=1.0 + 1e-9).contains(&s) && !ts.iter().any(|x| (x - t).abs() < 1e-7) {
                ts.push(t);
            }
        }
        crate::sort::stable(&mut ts, |x, y| x < y);
        ts
    };
    let mut best: Option<(usize, f64, f64, f64, V2, V2)> = None;
    let want = ev.get("end").and_then(Value::str);
    for (ei, adj, name) in [(0usize, 1usize, "start"), (n - 1, n - 2, "end")] {
        if want.is_some_and(|w| w != name) {
            continue;
        }
        let (a, b) = (to_world(vs[adj]), to_world(vs[ei]));
        if geom::point_in_loop_eo(a, &outline) {
            continue;
        }
        let u = b.sub(a).norm();
        let ts = hits(a, u);
        let Some(&t_in) = ts.first() else { continue };
        let t_out = ts.get(1).copied().unwrap_or(f64::INFINITY);
        let t_b = a.dist(b);
        let score = if t_b > t_in { 0.0 } else { t_in - t_b };
        if best.is_none_or(|x| score < x.1 - 1e-9) {
            best = Some((ei, score, t_in, t_out, a, u));
        }
    }
    if let Some(w) = want
        && w != "start"
        && w != "end"
    {
        ctx.p.fail("epoxy/end", format!("epoxy.end must be \"start\" or \"end\" (which end of 'points' is in the hole; default: the end that reaches '{host_id}'), got \"{w}\""));
        return None;
    }
    let Some((ei, _, t_in, t_out, a, u)) = best else {
        ctx.p.fail(
            "epoxy",
            format!("no end of the bar runs into '{host_id}': the first or last leg of 'points' must start outside '{host_id}' and point into one of its faces (end it on the face, e.g. {{\"ref\": \"{host_id}@middle_right\", \"offset\": [0, -4]}}, and give epoxy.embed)"),
        );
        return None;
    };
    let t_end = match embed {
        Some(e) => t_in + e,
        None => a.dist(to_world(vs[ei])),
    };
    if t_end <= t_in + 1e-6 {
        ctx.p.fail(
            "epoxy/embed",
            format!("the bar ends on the face of '{host_id}' (or {} short of it): give the hole depth, e.g. \"epoxy\": {{\"in\": \"{host_id}\", \"embed\": 6}}", fmt_ft_in(t_in - t_end)),
        );
        return None;
    }
    let emb = t_end - t_in;
    if t_end > t_out - 1.0 {
        scene.diags.add_fix(
            Level::Warning,
            "W_EPOXY",
            Some(ctx.p.id),
            Some(&format!("{}/{}/epoxy/embed", ctx.p.base, ctx.p.id)),
            format!("the {} deep hole of '{}' leaves {} of '{host_id}' behind it ({} thick along the bar): a drilled hole needs sound concrete past its end", fmt_ft_in(emb), ctx.p.id, fmt_ft_in((t_out - t_end).max(0.0)), fmt_ft_in(t_out - t_in)),
            format!("lower epoxy.embed to {} or less, or drill into a thicker part of '{host_id}'", fmt_ft_in((t_out - t_in - 1.5).max(1.0))),
        );
    }
    let (face_w, tip_w) = (a.add(u.scale(t_in)), a.add(u.scale(t_end)));
    vs[ei] = to_local(tip_w);
    let (face, tip) = (to_local(face_w), vs[ei]);
    let ul = tip.sub(face).norm();
    let nl = ul.perp().scale(hole_d / 2.0);
    let far = tip.add(ul.scale(0.125));
    let mut hole =
        vec![Pt::at(face.sub(nl), 0.0), Pt::at(far.sub(nl), 0.0), Pt::at(far.add(nl), 0.0), Pt::at(face.add(nl), 0.0)];
    if geom::signed_area(&hole) < 0.0 {
        hole.reverse();
    }
    Some(Epoxy {
        hole,
        face,
        tip,
        info: format!(", epoxied {} into '{host_id}' ({} dia hole)", fmt_ft_in(emb), fmt_ft_in(hole_d)),
    })
}
