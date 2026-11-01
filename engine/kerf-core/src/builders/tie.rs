//! Ties and angles placed from the two members they join (SPEC 28.4): a tie wraps the joint (H2.5A, H3, RSP4, LTP4), an
//! angle sits in its inside corner (A34, A35). The joint is found on paper (one member on the other, or side by side);
//! `face` puts the hardware on the members' z faces (seen face-on in a section) or on one side of the joint (seen
//! edge-on). Plates are built in world coordinates like the other framing hardware (`hardware.rs`).

use super::common::Ctx;
use super::connector::{Face, Kind, ModelInfo, Params, Side};
use super::hardware::{Kit, Member, model_text, need};
use crate::clip;
use crate::geom::{Pt, V2};
use crate::scene::Scene;
use crate::json::Value;
use crate::model::{self, Built, NamedAnchor};
use crate::params::ParamEnum;
use crate::pathclip;
use crate::pathgeom;
use crate::sort;
use crate::units::{self, fmt_ft_in};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// The joint on paper: a line at `c` (y when one member sits on the other, x when they are side by side) where the
/// members touch over `lo..hi` along it.
struct Joint {
    horiz: bool,
    c: f64,
    lo: f64,
    hi: f64,
    first_low: bool,
}

impl Joint {
    fn world(&self, l: f64, n: f64) -> V2 {
        if self.horiz { V2::init(l, n) } else { V2::init(n, l) }
    }

    fn pts(&self, ln: &[(f64, f64)]) -> Vec<Pt> {
        ln.iter().map(|&(l, n)| Pt::at(self.world(l, n), 0.0)).collect()
    }
}

/// The lowest and highest point of a member's profile across the joint line at `l`.
fn extent_at(m: &Member, horiz: bool, l: f64) -> Option<(f64, f64)> {
    let v = pathclip::scan(&m.flat, l, horiz);
    Some((*v.first()?, *v.last()?))
}

/// The member's extent along the joint line at `n` (its loops' spans merged), the span holding `lm` or the nearest one.
fn span_at(m: &Member, horiz: bool, n: f64, lm: f64) -> Option<(f64, f64)> {
    let mut iv: Vec<(f64, f64)> = Vec::new();
    for lp in &m.flat {
        let xs = pathclip::scan(core::slice::from_ref(lp), n, !horiz);
        for [lo, hi] in xs.as_chunks::<2>().0 {
            iv.push((*lo, *hi));
        }
    }
    sort::stable(&mut iv, |a, b| a.0 < b.0);
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for s in iv {
        match merged.last_mut() {
            Some(last) if s.0 <= last.1 + 1e-6 => last.1 = last.1.max(s.1),
            _ => merged.push(s),
        }
    }
    let dist = |s: &(f64, f64)| (s.0 - lm).max(lm - s.1).max(0.0);
    merged.into_iter().fold(None, |best: Option<(f64, f64)>, s| match best {
        Some(b) if dist(&b) <= dist(&s) => Some(b),
        _ => Some(s),
    })
}

fn find_joint(a: &Member, b: &Member) -> Option<Joint> {
    for horiz in [true, false] {
        let (a0, a1, b0, b1) =
            if horiz { (a.bx.x0, a.bx.x1, b.bx.x0, b.bx.x1) } else { (a.bx.y0, a.bx.y1, b.bx.y0, b.bx.y1) };
        let (o0, o1) = (a0.max(b0), a1.min(b1));
        if o1 - o0 < 0.05 {
            continue;
        }
        let k = 32;
        let step = (o1 - o0) / f64::from(k);
        let mut hits: [Vec<(f64, f64)>; 2] = [Vec::new(), Vec::new()];
        for i in 0..k {
            let l = o0 + step * (f64::from(i) + 0.5);
            let (Some(ea), Some(eb)) = (extent_at(a, horiz, l), extent_at(b, horiz, l)) else { continue };
            if (ea.0 - eb.1).abs() <= 0.5 {
                hits[1].push((l, eb.1));
            } else if (eb.0 - ea.1).abs() <= 0.5 {
                hits[0].push((l, ea.1));
            }
        }
        let first_low = hits[0].len() > hits[1].len();
        let h = &hits[usize::from(!first_low)];
        let (Some(first), Some(last)) = (h.first(), h.last()) else { continue };
        return Some(Joint {
            horiz,
            c: h[h.len() / 2].1,
            lo: (first.0 - step / 2.0).max(o0),
            hi: (last.0 + step / 2.0).min(o1),
            first_low,
        });
    }
    None
}

/// A member notched for the other (`cuts` notch `by` it: a let-in ledger) is drawn notched only once every member is
/// placed; the joint is found on the notched profile, so the tie sits on the faces that meet in the notch.
fn let_in(scene: &Scene, m: &mut Member, other: &Member) -> bool {
    let names_other = |by: &str| {
        let id = crate::compile::host_id(by);
        other.parts.iter().any(|&(ci, _)| scene.comps.get(ci as usize).is_some_and(|c| c.id == id))
    };
    let notched = m.parts.iter().any(|&(ci, _)| {
        scene.comps.get(ci as usize).is_some_and(|c| crate::cuts::notch_bys(c).iter().any(|(_, by)| names_other(by)))
    });
    if !notched || m.z0 >= other.z1 || other.z0 >= m.z1 {
        return false;
    }
    let cut = clip::boolean(&m.flat, &other.flat, clip::Op::Diff);
    if clip::total_area(&cut) <= 1e-6 {
        return false;
    }
    m.bx = clip::loops_box(&cut);
    m.flat = cut;
    true
}

/// A solid other than the two members at `p` (paper) and depth `z`: a member lying against the face a face-on tie
/// would be nailed to (eave blocking against a truss at the heel). The first one in document order.
fn against_face<'s>(scene: &'s Scene, a: &Member, b: &Member, p: V2, z: f64) -> Option<&'s str> {
    let own = |ci: u32, inst: u32| a.parts.contains(&(ci, inst)) || b.parts.contains(&(ci, inst));
    scene
        .comps
        .iter()
        .filter(|c| c.state == crate::scene::State::Ok && c.visible && !c.dashed)
        .find(|c| {
            c.world.iter().any(|q| {
                q.kind == model::Kind::Body
                    && !q.embedded
                    && !own(c.index, q.instance)
                    && z > q.z0
                    && z < q.z1
                    && clip::from_region(&q.loops, crate::geom::FLAT_TOL)
                        .iter()
                        .any(|l| crate::geom::point_in_loop_eo(p, l))
            })
        })
        .map(|c| c.id.as_str())
}

fn extents(m: &Member) -> String {
    format!(
        "'{}' spans x {}..{}, y {}..{}, z {}..{}",
        m.id,
        fmt_ft_in(m.bx.x0),
        fmt_ft_in(m.bx.x1),
        fmt_ft_in(m.bx.y0),
        fmt_ft_in(m.bx.y1),
        fmt_ft_in(m.z0),
        fmt_ft_in(m.z1)
    )
}

fn legs(ctx: &mut Ctx, cp: &Params, default: f64) -> Option<[f64; 2]> {
    let Some(v) = cp.legs.as_ref() else { return Some([default; 2]) };
    let vals: Vec<Option<f64>> = match v {
        Value::Array(a) if a.len() == 2 => a.iter().map(units::parse_length).collect(),
        _ => Vec::new(),
    };
    match vals.as_slice() {
        [Some(x), Some(y)] if *x > 0.0 && *y > 0.0 => Some([*x, *y]),
        _ => {
            ctx.p.fail(
                "legs",
                format!(
                    "'legs' must be two positive lengths [on the first member, on the second], e.g. [2.75, 2.75] (got {})",
                    model::kind_or_text(v)
                ),
            );
            None
        }
    }
}

pub fn build(ctx: &mut Ctx, cp: &Params, mi: &ModelInfo, kind: Kind, gauge_i: u32, t: f64) -> Option<Built> {
    let angle = kind == Kind::Angle;
    let hint = "the two members it joins, e.g. [\"rafter\", \"plate\"]";
    let list: Vec<Value> = match cp.members.as_ref() {
        Some(Value::Array(a)) if a.len() == 2 => a.clone(),
        other => {
            ctx.p.fail(
                "members",
                format!(
                    "a {} placed from members needs exactly two: {hint} (each an id, id#k or a list of ids drawn as one; got {})",
                    kind.tag(),
                    other.map_or("nothing".to_string(), model::kind_or_text)
                ),
            );
            return None;
        }
    };
    let a = need(ctx, Some(&list[0]), "members", kind, hint);
    let b = need(ctx, Some(&list[1]), "members", kind, hint);
    let default_leg = if angle { 1.4375 } else { mi.table.map_or(2.75, |h| (h.length / 2.0).max(1.5)) };
    let lg = legs(ctx, cp, default_leg);
    let (mut a, mut b, lg) = (a?, b?, lg?);
    let scene = ctx.scene;
    let let_in_joint = let_in(scene, &mut a, &b) | let_in(scene, &mut b, &a);
    if a.parts.iter().any(|p| b.parts.contains(p)) {
        ctx.p.fail(
            "members",
            format!("'members' names '{}' twice; list the two members the {} joins", a.id, kind.tag()),
        );
        return None;
    }
    let w = cp.width.or(mi.strap_width()).unwrap_or(if angle { 2.5 } else { 1.375 });
    let Some(j) = find_joint(&a, &b) else {
        ctx.p.fail(
            "members",
            format!(
                "{}: '{}' and '{}' do not meet on paper: one must sit on the other or beside it, touching within 1/2\" over a common stretch; {}; {}. Fix: name the two members that touch (e.g. the rafter and the plate it bears on)",
                kind.tag(),
                a.id,
                b.id,
                extents(&a),
                extents(&b)
            ),
        );
        return None;
    };
    if a.z0 > b.z1 + 0.5 || b.z0 > a.z1 + 0.5 {
        ctx.p.fail(
            "members",
            format!(
                "{}: '{}' (z {}..{}) and '{}' (z {}..{}) do not overlap along z, so they do not touch; set their z (or name the instance with id#k) so one bears on the other",
                kind.tag(),
                a.id,
                fmt_ft_in(a.z0),
                fmt_ft_in(a.z1),
                b.id,
                fmt_ft_in(b.z0),
                fmt_ft_in(b.z1)
            ),
        );
        return None;
    }
    let (p, q) = if j.first_low { (&a, &b) } else { (&b, &a) };
    let (lp, lq) = if j.first_low { (lg[0], lg[1]) } else { (lg[1], lg[0]) };
    let lm = (j.lo + j.hi) / 2.0;
    let eps = 0.01;
    let (Some(sp), Some(sq)) = (span_at(p, j.horiz, j.c - eps, lm), span_at(q, j.horiz, j.c + eps, lm)) else {
        ctx.p.fail("members", format!("{}: '{}' and '{}' have no profile at their joint", kind.tag(), a.id, b.id));
        return None;
    };
    let long_z = |m: &Member, s: (f64, f64)| m.z1 - m.z0 >= 6.0 && m.z1 - m.z0 >= 2.0 * (s.1 - s.0);
    let face = cp.face.unwrap_or_else(|| {
        if let_in_joint || (j.horiz && !(long_z(p, sp) && long_z(q, sq))) {
            Face::Front
        } else {
            let lo_ok = if angle { (sp.0 - sq.0).abs() > 0.25 } else { (sp.0 - sq.0).abs() <= 0.25 };
            let hi_ok = if angle { (sp.1 - sq.1).abs() > 0.25 } else { (sp.1 - sq.1).abs() <= 0.25 };
            match (j.horiz, lo_ok, hi_ok) {
                (true, true, _) | (true, false, false) => Face::Left,
                (true, false, true) => Face::Right,
                (false, _, true) | (false, false, false) => Face::Top,
                (false, true, false) => Face::Bottom,
            }
        }
    });
    let edge = !matches!(face, Face::Front | Face::Back);
    let valid = if j.horiz { "left or right" } else { "top or bottom" };
    if edge && j.horiz != matches!(face, Face::Left | Face::Right) {
        ctx.p.fail(
            "face",
            format!(
                "{}: '{}' and '{}' meet {}, so the sides of their joint are {valid} (or front/back for their z faces); got {}",
                kind.tag(),
                a.id,
                b.id,
                if j.horiz { "one on the other" } else { "side by side" },
                face.tag()
            ),
        );
    }
    if edge && ctx.p.has("side") {
        ctx.p.fail(
            "side",
            format!(
                "'side' picks the end of the joint a face-on {} wraps; on face {} that face is the side: remove 'side'",
                kind.tag(),
                face.tag()
            ),
        );
    }
    if edge && ctx.p.has("along") {
        ctx.p.fail(
            "along",
            format!(
                "on face {} the {} sits on that side of the joint, so it has no place along it; remove 'along' ('z' moves it along the members)",
                face.tag(),
                kind.tag()
            ),
        );
    }
    if !edge && ctx.p.has("z") {
        ctx.p.fail(
            "z",
            format!(
                "on face {} the {}'s z is the members' face; remove 'z' (with face {valid} it sets the center along z)",
                face.tag(),
                kind.tag()
            ),
        );
    }
    if !ctx.p.ok {
        return None;
    }
    let along = cp.along.unwrap_or(0.0);
    let mut kit = Kit::new();
    let part = kind.tag();
    let nails_on = |l: f64, n0: f64, n1: f64| -> Vec<V2> {
        [1.0 / 3.0, 2.0 / 3.0].iter().map(|k| j.world(l, n0 + (n1 - n0) * k)).collect()
    };
    let rect = |l0: f64, l1: f64, n0: f64, n1: f64| j.pts(&[(l0, n0), (l1, n0), (l1, n1), (l0, n1)]);
    let where_txt = if edge {
        let dir = if matches!(face, Face::Left | Face::Bottom) { -1.0 } else { 1.0 };
        let side_of = |s: (f64, f64)| if dir < 0.0 { s.0 } else { s.1 };
        let (fp, fq) = (side_of(sp), side_of(sq));
        let path: Vec<(f64, f64)> = if angle {
            if dir * (fp - fq) > 0.25 {
                vec![(fq, j.c + lq), (fq, j.c), (fq + dir * lp, j.c)]
            } else if dir * (fq - fp) > 0.25 {
                vec![(fp + dir * lq, j.c), (fp, j.c), (fp, j.c - lp)]
            } else {
                ctx.p.fail(
                    "face",
                    format!(
                        "angle: '{}' and '{}' are flush on the {} side of their joint, so there is no inside corner there; use the other side, face front/back, or kind tie for a flat plate across the joint",
                        a.id,
                        b.id,
                        face.tag()
                    ),
                );
                return None;
            }
        } else if (fp - fq).abs() <= 0.25 {
            let f = if dir < 0.0 { fp.min(fq) } else { fp.max(fq) };
            vec![(f, j.c + lq), (f, j.c - lp)]
        } else {
            vec![(fq, j.c + lq), (fq, j.c), (fp, j.c), (fp, j.c - lp)]
        };
        let center = j.pts(&path);
        let rib = if dir < 0.0 { pathgeom::ribbon(&center, 0.0, t) } else { pathgeom::ribbon(&center, t, 0.0) };
        let zc = (a.z0.max(b.z0) + a.z1.min(b.z1)) / 2.0;
        kit.poly(part, rib, [zc - w / 2.0, zc + w / 2.0], Vec::new());
        if let Some(last) = kit.prisms.last_mut() {
            last.centerline = center;
        }
        format!("{} side of the joint", face.tag())
    } else {
        let s = if face == Face::Front { 1.0 } else { -1.0 };
        let a_face = if s > 0.0 { a.z1 } else { a.z0 };
        let b_face = if s > 0.0 { b.z1 } else { b.z0 };
        let n_is_a = if s > 0.0 { a_face <= b_face } else { a_face >= b_face };
        let (n_m, w_m) = if n_is_a { (&a, &b) } else { (&b, &a) };
        let zf = if n_is_a { a_face } else { b_face };
        let w_face = if s > 0.0 { w_m.z1 } else { w_m.z0 };
        let bend = s * (w_face - zf) > 0.125;
        let n_low = core::ptr::eq(n_m, p);
        let (ln, lw) = if n_low { (lp, lq) } else { (lq, lp) };
        let into_n = |len: f64| if n_low { (j.c - len, j.c) } else { (j.c, j.c + len) };
        let into_w = |len: f64| if n_low { (j.c, j.c + len) } else { (j.c - len, j.c) };
        let zp = [zf, zf + s * t];
        if angle {
            if !bend {
                ctx.p.fail(
                    "face",
                    format!(
                        "angle: neither '{}' nor '{}' passes the other's {} face, so there is no inside corner on it; use kind tie (a flat plate across the joint) or face {valid}",
                        a.id,
                        b.id,
                        face.tag()
                    ),
                );
                return None;
            }
            let lm = lm + along;
            let (l0, l1) = (lm - w / 2.0, lm + w / 2.0);
            let (n0, n1) = into_n(ln);
            kit.poly(part, rect(l0, l1, n0, n1), zp, nails_on(lm, n0, n1));
            let (m0, m1) = if n_low { (j.c - t, j.c) } else { (j.c, j.c + t) };
            kit.poly(part, rect(l0, l1, m0, m1), [zf, zf + s * lw], Vec::new());
        } else if bend {
            if ctx.p.has("along") {
                ctx.p.fail(
                    "along",
                    format!(
                        "the {} bends onto a side face of '{}', so its place along the joint is that face; remove 'along' and choose the face with 'side' (left or right)",
                        kind.tag(),
                        w_m.id
                    ),
                );
                return None;
            }
            let (sw, sn) = if n_low { (sq, sp) } else { (sp, sq) };
            let covers = |f: f64| f >= sn.0 - 0.25 && f <= sn.1 + 0.25;
            let (n0, n1) = into_n(ln);
            let start_of = |dir: f64| {
                let fw = if dir < 0.0 { sw.0 } else { sw.1 };
                (fw, if dir < 0.0 { fw.max(sn.0) } else { fw.min(sn.1) })
            };
            let blocked = |dir: f64| {
                let (_, start) = start_of(dir);
                against_face(scene, &a, &b, j.world(start - dir * w / 2.0, (n0 + n1) / 2.0), zf + s * 0.25)
            };
            let dir = if ctx.p.has("side") {
                if cp.side == Side::Left { -1.0 } else { 1.0 }
            } else {
                let first = if covers(sw.0) || !covers(sw.1) { -1.0 } else { 1.0 };
                if blocked(first).is_some() && blocked(-first).is_none() { -first } else { first }
            };
            let (fw, start) = start_of(dir);
            let lc = start - dir * w / 2.0;
            kit.poly(part, rect(start, start - dir * w, n0, n1), zp, nails_on(lc, n0, n1));
            let (m0, m1) = into_w(lw);
            kit.poly(part, rect(fw, fw + dir * t, m0, m1), [zf, zf + s * w], Vec::new());
        } else {
            let lm = lm + along;
            let (l0, l1) = (lm - w / 2.0, lm + w / 2.0);
            let mut nails = nails_on(lm, j.c - lp, j.c);
            nails.extend(nails_on(lm, j.c, j.c + lq));
            kit.poly(part, rect(l0, l1, j.c - lp, j.c + lq), zp, nails);
        }
        format!("{} face of {}", face.tag(), n_m.id)
    };
    let desc = mi.desc();
    let info = format!(
        "connector {}{}{gauge_i} ga, {} {} to {} on the {where_txt}",
        model_text(mi),
        if desc.is_empty() { String::new() } else { format!("{desc} ") },
        part,
        a.id,
        b.id
    );
    let members = a.parts.iter().chain(&b.parts).copied().collect();
    let joins = [a.id.split('+').map(String::from).collect(), b.id.split('+').map(String::from).collect()];
    let hw = model::HwInfo { members, joins, ..Default::default() };
    let origin = ctx.origin;
    Some(kit.finish(origin, vec![NamedAnchor { name: "joint".into(), p: j.world(lm, j.c) }], info, hw))
}
