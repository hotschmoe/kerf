use crate::builders::world_box;
use crate::clip::{self, Op};
use crate::geom::{self, Loc, Pt, V2, Xf};
use crate::json::Value;
use crate::limits;
use crate::math;
use crate::model::{self, Cover, Diag, Diags, HostInfo, Kind, Level, Prism};
use crate::scene::{Comp, Scene, State, join_ids};
use crate::style::Role;
use crate::units::{self, fmt_ft_in};
use crate::view::Members;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::f64::consts::PI;

mod bearing;

const FLAT_TOL: f64 = geom::FLAT_TOL;

struct Pf<'a> {
    prism: &'a Prism,
    flat: Vec<Vec<V2>>,
    box_: geom::Box,
    comp: &'a Comp<'a>,
}

fn box_text(b: geom::Box) -> String {
    format!("x {}..{}, y {}..{}", fmt_ft_in(b.x0), fmt_ft_in(b.x1), fmt_ft_in(b.y0), fmt_ft_in(b.y1))
}

fn two_places(x: f64) -> String {
    let n = crate::num::to_int_clamped::<u64>(math::round(x.abs() * 100.0), 0, u64::MAX);
    format!("{}{}.{:02}", if x < 0.0 && n != 0 { "-" } else { "" }, n / 100, n % 100)
}

fn seg_dist(p: V2, q: V2, r: V2, s: V2) -> f64 {
    let mut ta = [0.0; 2];
    let mut tb = [0.0; 2];
    if geom::seg_seg(p, q, r, s, &mut ta, &mut tb) > 0 {
        return 0.0;
    }
    geom::dist_point_seg(p, r, s)
        .min(geom::dist_point_seg(q, r, s))
        .min(geom::dist_point_seg(r, p, q).min(geom::dist_point_seg(s, p, q)))
}

fn region_dist(a: &[Vec<V2>], b: &[Vec<V2>], limit: f64) -> f64 {
    let mut best = f64::INFINITY;
    for la in a {
        for lb in b {
            for (i, &p) in la.iter().enumerate() {
                let q = la[(i + 1) % la.len()];
                let bx0 = p.x.min(q.x) - limit;
                let bx1 = p.x.max(q.x) + limit;
                let by0 = p.y.min(q.y) - limit;
                let by1 = p.y.max(q.y) + limit;
                for (k, &r) in lb.iter().enumerate() {
                    let s = lb[(k + 1) % lb.len()];
                    if r.x.min(s.x) > bx1 || r.x.max(s.x) < bx0 || r.y.min(s.y) > by1 || r.y.max(s.y) < by0 {
                        continue;
                    }
                    let d = seg_dist(p, q, r, s);
                    if d < best {
                        best = d;
                        if best == 0.0 {
                            return 0.0;
                        }
                    }
                }
            }
        }
    }
    if let (Some(a0), Some(b0)) = (a.first().and_then(|l| l.first()), b.first().and_then(|l| l.first())) {
        if !b.is_empty() && geom::locate(*a0, b, 0.0) == Loc::Inside {
            return 0.0;
        }
        if !a.is_empty() && geom::locate(*b0, a, 0.0) == Loc::Inside {
            return 0.0;
        }
    }
    best
}

fn contact_len(a: &[Vec<V2>], b: &[Vec<V2>], gap: f64) -> f64 {
    let sin_tol = 0.0175;
    let mut total = 0.0;
    for la in a {
        for (i, &p) in la.iter().enumerate() {
            let q = la[(i + 1) % la.len()];
            let d = q.sub(p);
            let len = d.len();
            if len <= 1e-9 {
                continue;
            }
            for lb in b {
                for (k, &r) in lb.iter().enumerate() {
                    let s = lb[(k + 1) % lb.len()];
                    let e = s.sub(r);
                    let elen = e.len();
                    if elen <= 1e-9 || (d.cross(e) / (len * elen)).abs() > sin_tol {
                        continue;
                    }
                    if (d.cross(r.sub(p)) / len).abs() > gap || (d.cross(s.sub(p)) / len).abs() > gap {
                        continue;
                    }
                    let (t0, t1) = (d.dot(r.sub(p)) / len, d.dot(s.sub(p)) / len);
                    total += (t0.max(t1).min(len) - t0.min(t1).max(0.0)).max(0.0);
                }
            }
        }
    }
    total
}

fn z_gap(a: &Prism, b: &Prism) -> f64 {
    (a.z0.max(b.z0) - a.z1.min(b.z1)).max(0.0)
}

pub fn run(scene: &Scene<'_>, doc: &Value, diags: &Diags) {
    let mut items: Vec<Pf<'_>> = Vec::new();
    let mut hidden: Vec<Pf<'_>> = Vec::new();
    for c in &scene.comps {
        if c.state != State::Ok {
            continue;
        }
        for p in &c.world {
            if p.kind == Kind::Ghost && !p.dashed {
                continue;
            }
            let flat = clip::from_region(&p.loops, FLAT_TOL);
            let box_ = clip::loops_box(&flat);
            let pf = Pf { prism: p, flat, box_, comp: c };
            if p.kind == Kind::Ghost {
                hidden.push(pf);
            } else {
                items.push(pf);
            }
        }
    }
    let (sets, ids) = drawn_sets(scene, doc, &items, &hidden);
    each_set(&sets, &ids, diags, overlaps);
    each_set(&sets, &ids, diags, floating);
    each_set(&sets, &ids, diags, untreated);
    each_set(&sets, &ids, diags, cover);
    each_set(&sets, &ids, diags, near_miss);
    each_set(&sets, &ids, diags, bearing::check);
    let seated: Vec<u32> = sets.iter().flat_map(bearing::seated).collect();
    each_set(&sets, &ids, diags, |s, d| bearing::unsupported(s, &seated, d));
    let crop = explicit_crop(doc);
    each_set(&sets, &ids, diags, |s, d| short_slope(s, crop, d));
    each_set(&sets, &ids, diags, |s, d| crate::steelcheck::drawn(&s.comps, d));
    each_set(&sets, &ids, diags, |s, d| crate::hwcheck::drawn(&s.comps, d));
    crate::steelcheck::run(scene, doc, diags);
    crate::hwcheck::run(scene, diags);
    for c in &scene.comps {
        if c.state == State::Ok && c.ty.traits.escape_hatch {
            let mat = c.world.first().map_or("?", |p| p.material.as_str());
            diags.add(
                Level::Info,
                "I_SOLID_USED",
                Some(&c.id),
                None,
                format!("{} uses the `solid` escape hatch (material {}); a reviewer should confirm no typed component fits.", c.id, mat),
            );
        }
    }
    unverified_count(doc, diags);
}

struct Drawn<'p, 'a> {
    comps: Vec<&'a Comp<'a>>,
    items: Vec<&'p Pf<'a>>,
    hidden: Vec<&'p Pf<'a>>,
    views: Vec<usize>,
}

fn drawn_sets<'p, 'a>(
    scene: &'a Scene<'a>,
    doc: &'a Value,
    items: &'p [Pf<'a>],
    hidden: &'p [Pf<'a>],
) -> (Vec<Drawn<'p, 'a>>, Vec<&'a str>) {
    let mut groups: Vec<(Vec<bool>, Vec<usize>)> = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    let va = doc.get("views").and_then(Value::arr).filter(|va| va.len() <= limits::MAX_VIEWS).unwrap_or(&[]);
    for v in va {
        let Some(id) = v.get("id").and_then(Value::str) else { continue };
        if ids.contains(&id) {
            continue;
        }
        let members = Members::of(v);
        if members.none {
            continue;
        }
        let mask: Vec<bool> =
            scene.comps.iter().map(|c| c.state == State::Ok && c.visible && members.draws(c)).collect();
        match groups.iter_mut().find(|g| g.0 == mask) {
            Some(g) => g.1.push(ids.len()),
            None => groups.push((mask, alloc::vec![ids.len()])),
        }
        ids.push(id);
    }
    if groups.is_empty() {
        groups.push((scene.comps.iter().map(|c| c.state == State::Ok).collect(), Vec::new()));
    }
    let sets = groups
        .into_iter()
        .map(|(mask, views)| Drawn {
            comps: scene.comps.iter().filter(|c| mask[c.index as usize]).collect(),
            items: items.iter().filter(|p| mask[p.comp.index as usize]).collect(),
            hidden: hidden.iter().filter(|p| mask[p.comp.index as usize]).collect(),
            views,
        })
        .collect();
    (sets, ids)
}

fn each_set<'p, 'a>(sets: &[Drawn<'p, 'a>], ids: &[&str], diags: &Diags, check: impl Fn(&Drawn<'p, 'a>, &Diags)) {
    let mut found: Vec<(Diag, Vec<usize>)> = Vec::new();
    for s in sets {
        let sd = Diags::new();
        check(s, &sd);
        for d in sd.into_vec() {
            match found.iter_mut().find(|(f, vs)| *f == d && !s.views.iter().any(|v| vs.contains(v))) {
                Some((_, vs)) => vs.extend(&s.views),
                None => found.push((d, s.views.clone())),
            }
        }
    }
    for (mut d, mut vs) in found {
        anchor_hint(sets, &mut d);
        if vs.len() < ids.len() {
            vs.sort_unstable();
            let names: Vec<&str> = vs.iter().map(|&v| ids[v]).collect();
            d.message.push_str(&format!(" (in view{} {})", if names.len() == 1 { "" } else { "s" }, names.join(", ")));
        }
        diags.push(d);
    }
}

fn anchor_hint(sets: &[Drawn<'_, '_>], d: &mut Diag) {
    if !matches!(d.code, "W_OVERLAP" | "W_FLOATING" | "W_COVER" | "W_BAR_OUTSIDE" | "W_NEAR_MISS") {
        return;
    }
    let mut named: Vec<&Comp<'_>> = Vec::new();
    for c in sets.iter().flat_map(|s| s.comps.iter()) {
        if c.anchor_shift.is_some()
            && d.message.contains(&format!("'{}'", c.id))
            && !named.iter().any(|n| n.index == c.index)
        {
            named.push(c);
        }
    }
    for c in named {
        let Some((anchor, v)) = &c.anchor_shift else { continue };
        d.message.push_str(&format!(
            " ('{}' is a point-list shape placed by at.anchor \"{anchor}\", which moves its points by [{}, {}] from where at.to puts them; kerf before beta.2 ignored that anchor: if the points were written from at.to, remove it: {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"at\":{{\"anchor\":null}}}}}})",
            c.id,
            model::fmt_num(math::round(v.x * 10000.0) / 10000.0),
            model::fmt_num(math::round(v.y * 10000.0) / 10000.0),
            c.id
        ));
    }
}

fn has_solid_body(c: &Comp<'_>) -> bool {
    c.world.iter().any(|p| p.kind == Kind::Body && !matches!(p.role, Role::Soil | Role::Void))
}

fn overlaps(s: &Drawn<'_, '_>, diags: &Diags) {
    let mut reported: Vec<[u32; 2]> = Vec::new();
    for (i, p) in s.items.iter().enumerate() {
        if p.prism.embedded || p.prism.kind != Kind::Body || p.comp.ty.traits.hardware {
            continue;
        }
        for q in &s.items[i + 1..] {
            if q.prism.embedded || q.prism.kind != Kind::Body || q.comp.ty.traits.hardware {
                continue;
            }
            if p.comp.index == q.comp.index && p.prism.instance == q.prism.instance {
                continue;
            }
            if (p.prism.role == Role::Soil) != (q.prism.role == Role::Soil) {
                continue;
            }
            if p.prism.role == Role::Soil && (has_solid_body(p.comp) || has_solid_body(q.comp)) {
                continue;
            }
            if z_gap(p.prism, q.prism) > 0.0 || (p.prism.z1.min(q.prism.z1) - p.prism.z0.max(q.prism.z0)) < 1e-6 {
                continue;
            }
            if !p.box_.overlaps(q.box_, 0.0) {
                continue;
            }
            let inter = clip::boolean(&p.flat, &q.flat, Op::Intersect);
            let area = clip::total_area(&inter);
            if area <= 0.01 {
                continue;
            }
            let key = [p.comp.index.min(q.comp.index), p.comp.index.max(q.comp.index)];
            if reported.contains(&key) {
                continue;
            }
            reported.push(key);
            let b = clip::loops_box(&inter);
            diags.add_fix(
                Level::Warning,
                "W_OVERLAP",
                Some(&p.comp.id),
                None,
                format!(
                    "'{}' and '{}' overlap by {} sq in ({}) within z {}..{}",
                    p.comp.id,
                    q.comp.id,
                    two_places(area),
                    box_text(b),
                    fmt_ft_in(p.prism.z0.max(q.prism.z0)),
                    fmt_ft_in(p.prism.z1.min(q.prism.z1)),
                ),
                crate::cuts::birdmouth_overlap_fix(p.comp, q.comp, &s.comps, b.height())
                    .or_else(|| crate::cuts::birdmouth_overlap_fix(q.comp, p.comp, &s.comps, b.height()))
                    .unwrap_or_else(|| overlap_fix(p, q)),
            );
        }
    }
}

fn overlap_fix(p: &Pf<'_>, q: &Pf<'_>) -> String {
    const PLAIN: &str = "move or resize one of them so they only touch, or mark the inner one embedded:true if it is reinforcement or hardware";
    let (host, inner) = match (p.comp.ty.traits.takes_voids, q.comp.ty.traits.takes_voids) {
        (true, false) => (p.comp, q.comp),
        (false, true) => (q.comp, p.comp),
        (true, true) if q.prism.role == Role::Grout && p.prism.role != Role::Grout => (p.comp, q.comp),
        (true, true) if p.prism.role == Role::Grout && q.prism.role != Role::Grout => (q.comp, p.comp),
        _ => return PLAIN.to_string(),
    };
    if host.world.iter().any(|w| w.fp_proxy) && host.node.get("shape").and_then(Value::str) != Some("round") {
        return format!(
            "if '{}' is cast into '{}' (an angled footprint takes no voids), mark it embedded: {{\"op\": \"update\", \"path\": \"components/{}\", \"value\": {{\"embedded\": true}}}}; else {PLAIN}",
            inner.id, host.id, inner.id
        );
    }
    if inner.ty.traits.passes_through {
        return format!(
            "if '{}' passes through '{}', cut its hole: {{\"op\": \"update\", \"path\": \"components/{}\", \"value\": {{\"through\": \"{}\"}}}}; else {PLAIN}",
            inner.id, host.id, inner.id, host.id
        );
    }
    let mut list: Vec<Value> = host.node.get("voids").and_then(Value::arr).map(<[Value]>::to_vec).unwrap_or_default();
    list.push(Value::String(inner.id.clone()));
    let mut buf = Vec::new();
    crate::json::write_compact(&mut buf, &Value::Array(list));
    format!(
        "if '{}' is cast into '{}', subtract it (the hole follows the member): {{\"op\": \"update\", \"path\": \"components/{}\", \"value\": {{\"voids\": {}}}}}; else {PLAIN}",
        inner.id,
        host.id,
        host.id,
        String::from_utf8_lossy(&buf)
    )
}

fn cmu_reinforcement_contact(wall: &Comp<'_>, bar: &Comp<'_>, p: &Prism) -> Option<(usize, bool)> {
    if wall.ty.ty != crate::catalog::Type::CmuWall || !bar.ty.traits.reinforcement {
        return None;
    }
    let c = p.loops.first().map(|l| geom::points_box(l).center())?;
    let vertical = bar.world.iter().any(|q| {
        q.centerline.windows(2).any(|w| (w[0].y - w[1].y).abs() > (w[0].x - w[1].x).abs() + 1e-6)
            || q.path3
                .windows(2)
                .any(|w| (w[0][1] - w[1][1]).abs() > (w[0][0] - w[1][0]).abs() + (w[0][2] - w[1][2]).abs() + 1e-6)
    });
    for (xf, z) in wall.xfs.iter().zip(&wall.zs) {
        if p.z0 > z[1] || p.z1 < z[0] {
            continue;
        }
        let v = c.sub(V2::init(xf.tx, xf.ty));
        let v = V2::init((xf.d * v.x - xf.c * v.y) / xf.det(), (-xf.b * v.x + xf.a * v.y) / xf.det());
        if !wall.built.box_.contains(v) {
            continue;
        }
        let course = crate::num::to_int_clamped::<usize>(math::floor(v.y / 8.0) + 1.0, 1, 200);
        let grout = wall.node.get("grout").and_then(Value::str).unwrap_or("reinforced");
        let courses = wall.node.get("grout_courses").and_then(Value::arr).unwrap_or(&[]);
        let listed = courses.iter().any(|v| match v.arr() {
            Some([a, b]) => a.num().zip(b.num()).is_some_and(|(a, b)| course as f64 >= a && course as f64 <= b),
            _ => v.num() == Some(course as f64),
        });
        let grouted = grout == "solid"
            || (grout == "reinforced" && vertical)
            || listed
            || crate::builders::cmu_wall::beam_at(wall.node, course, v.x);
        return Some((course, grouted));
    }
    None
}

const JOINT_GAP: f64 = 1.0;

fn floating(s: &Drawn<'_, '_>, diags: &Diags) {
    let gap = 1.0 / 32.0;
    let n_comps = s.comps.iter().filter(|c| !c.dashed).count();
    if n_comps < 2 {
        return;
    }
    let bar_diagram = !s.comps.iter().any(|c| c.built.host.is_some());
    for c in &s.comps {
        if c.dashed || c.world.is_empty() || c.existing == Some(true) {
            continue;
        }
        if bar_diagram && c.ty.ty == crate::catalog::Type::Rebar {
            continue;
        }
        let touching_in = |p: &&&Pf<'_>, any_instance: bool| {
            let reinforced = s.comps.iter().any(|other| {
                cmu_reinforcement_contact(other, c, p.prism).is_some_and(|(_, g)| g)
                    || (c.ty.ty == crate::catalog::Type::CmuWall
                        && other.world.iter().any(|bp| cmu_reinforcement_contact(c, other, bp).is_some_and(|(_, g)| g)))
            });
            reinforced
                || s.items.iter().chain(&s.hidden).any(|q| {
                    let reach =
                        if p.prism.role.is_masonry() && q.prism.role.is_masonry() { JOINT_GAP - 1e-9 } else { gap };
                    let other = q.comp.index != c.index || (any_instance && q.prism.instance != p.prism.instance);
                    other
                        && z_gap(p.prism, q.prism) <= gap
                        && p.box_.expand(reach).overlaps(q.box_, 0.0)
                        && region_dist(&p.flat, &q.flat, reach) <= reach
                })
        };
        let touching = |p: &&&Pf<'_>| touching_in(p, false);
        let mine: Vec<&&Pf<'_>> = s.items.iter().filter(|p| p.comp.index == c.index).collect();
        let touches = mine.iter().any(touching);
        if touches && c.arr_count > 1 {
            let mut loose: Vec<u32> = Vec::new();
            for p in &mine {
                if !loose.contains(&p.prism.instance)
                    && !mine.iter().filter(|q| q.prism.instance == p.prism.instance).any(|q| touching_in(q, true))
                {
                    loose.push(p.prism.instance);
                }
            }
            if !loose.is_empty() {
                loose.sort_unstable();
                let ids: Vec<String> = loose.iter().map(|k| format!("{}#{k}", c.id)).collect();
                let zs: Vec<String> = loose
                    .iter()
                    .filter_map(|k| mine.iter().find(|p| p.prism.instance == *k))
                    .map(|p| format!("{}..{}", fmt_ft_in(p.prism.z0), fmt_ft_in(p.prism.z1)))
                    .collect();
                diags.add_fix(
                    Level::Warning,
                    "W_FLOATING",
                    Some(&c.id),
                    None,
                    format!(
                        "array instance(s) {} of '{}' touch nothing (z {}) while instance 0 or others do; the array runs past the members it should sit on",
                        join_ids(&ids),
                        c.id,
                        zs.join(", ")
                    ),
                    "an array starts at instance 0 (where `at` and `z` put it) and steps by spacing: lower array.count, or for n instances centered on z = c set \"z\" to c - (n-1)*spacing/2".to_string(),
                );
            }
        }
        if !touches {
            if let Some((wall, course)) = s.comps.iter().find_map(|h| {
                c.world.iter().find_map(|p| cmu_reinforcement_contact(h, c, p).filter(|(_, g)| !g).map(|(k, _)| (h, k)))
            }) {
                let mut beams = wall.node.get("bond_beams").and_then(Value::arr).unwrap_or(&[]).to_vec();
                beams.push(Value::Number(course as f64));
                let mut bytes = Vec::new();
                crate::json::write_compact(&mut bytes, &Value::Array(beams));
                diags.add_fix(Level::Warning,"W_FLOATING",Some(&c.id),None,
                    format!("bar '{}' lies in an ungrouted cell of '{}' at course {course}; reinforcement needs grout around it",c.id,wall.id),
                    format!("if this course carries horizontal reinforcement, grout it as a bond beam: {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"bond_beams\":{}}}}}; vertical reinforcement uses grout reinforced or solid",wall.id,String::from_utf8_lossy(&bytes)));
                continue;
            }
            let (msg, fix) = match face_host(s, c, &mine) {
                Some((host, face, gap)) => (
                    format!(
                        "component '{}' touches nothing: it floats {} off the {face} of '{host}' (hardware placed with a hand-set `z`)",
                        c.id,
                        fmt_ft_in(gap)
                    ),
                    format!(
                        "mount it on that face: {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"on\":\"{host}.{face}\",\"z\":null}}}}",
                        c.id
                    ),
                ),
                None => (
                    format!(
                        "component '{}' touches nothing (gap > 1/32\" to every other component, in plan and in z); it may be mis-placed",
                        c.id
                    ),
                    "check its `at` anchor/offset, or `z` range if it should bear on another member".to_string(),
                ),
            };
            diags.add_fix(Level::Warning, "W_FLOATING", Some(&c.id), None, msg, fix);
        } else if c.ty.ty == crate::catalog::Type::Panel {
            panel_faces(s, c, &mine, gap, diags);
        } else if c.ty.traits.hardware {
            strap_faces(s, c, &mine, gap, diags);
        }
    }
}

fn strap_faces(s: &Drawn<'_, '_>, c: &Comp<'_>, mine: &[&&Pf<'_>], gap: f64, diags: &Diags) {
    let Some((host, face)) = c.node.get("on").and_then(Value::str).and_then(|o| o.rsplit_once('.')) else { return };
    if !matches!(face, "face_front" | "face_back") {
        return;
    }
    use crate::catalog::Type;
    let host_id = host.split('#').next().unwrap_or(host);
    let framing = |q: &Pf<'_>| {
        matches!(q.comp.ty.ty, Type::Lumber | Type::Ijoist | Type::Truss | Type::Steel)
            && q.prism.kind == Kind::Body
            && !q.prism.embedded
    };
    let mut best: Option<(f64, f64, &str)> = None;
    for p in mine.iter().filter(|p| p.prism.kind == Kind::Body) {
        for q in s.items.iter() {
            if q.comp.index == c.index || q.comp.id == host_id || !framing(q) || !p.box_.overlaps(q.box_, 0.0) {
                continue;
            }
            let inter = clip::boolean(&p.flat, &q.flat, Op::Intersect);
            let area = clip::total_area(&inter);
            if area <= 0.25 {
                continue;
            }
            let (g0, g1) = if face == "face_back" { (p.prism.z1, q.prism.z0) } else { (q.prism.z1, p.prism.z0) };
            let between = s.items.iter().any(|r| {
                r.comp.index != c.index
                    && r.comp.index != q.comp.index
                    && r.prism.kind == Kind::Body
                    && r.prism.z1.min(g1) - r.prism.z0.max(g0) > 1e-6
                    && clip::total_area(&clip::boolean(&inter, &r.flat, Op::Intersect)) > 0.5 * area
            });
            if between {
                continue;
            }
            let off = if face == "face_back" { q.prism.z0 - p.prism.z1 } else { p.prism.z0 - q.prism.z1 };
            let inside = p.prism.z1.min(q.prism.z1) - p.prism.z0.max(q.prism.z0);
            let d = if off > gap {
                off
            } else if inside > gap && off < -gap {
                -inside
            } else {
                continue;
            };
            if d > 2.0 {
                continue;
            }
            if best.is_none_or(|b| area > b.1) {
                best = Some((d, area, q.comp.id.as_str()));
            }
        }
    }
    let Some((d, _, other)) = best else { return };
    let side = if face == "face_back" { "back" } else { "front" };
    let how = if d > 0.0 {
        format!("it stands {} off the {side} face of '{other}'", fmt_ft_in(d))
    } else {
        format!("it runs {} inside '{other}', whose {side} face stands proud of '{host}'", fmt_ft_in(-d))
    };
    diags.add_fix(
        Level::Warning,
        "W_FLOATING",
        Some(&c.id),
        None,
        format!(
            "strap '{}' lies on the {side} face of '{host}' (`on`), but where it crosses '{other}' {how}: a strap must lie on both members it joins",
            c.id
        ),
        format!(
            "join the two members with a member tie that follows both faces: {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"kind\":\"tie\",\"members\":[\"{host}\",\"{other}\"],\"face\":\"{side}\",\"points\":null,\"on\":null,\"lay\":null}}}}; or bring the faces flush (a filler on '{other}', or the members' z)",
            c.id
        ),
    );
}

fn panel_faces(s: &Drawn<'_, '_>, c: &Comp<'_>, mine: &[&&Pf<'_>], gap: f64, diags: &Diags) {
    for p in mine.iter().filter(|p| p.prism.kind == Kind::Body && p.prism.instance == 0) {
        let [l] = p.flat.as_slice() else { continue };
        if l.len() != 4 {
            continue;
        }
        let edges: Vec<(V2, V2)> = (0..4).map(|i| (l[i], l[(i + 1) % 4])).collect();
        let Some(&(a, b)) = edges.iter().max_by(|x, y| x.1.sub(x.0).len().total_cmp(&y.1.sub(y.0).len())) else {
            continue;
        };
        let len = b.sub(a).len();
        let t = clip::total_area(&p.flat) / len.max(1e-9);
        if t <= 1e-6 || len < 4.0 * t {
            continue;
        }
        let faces: Vec<Vec<V2>> =
            edges.iter().filter(|(e0, e1)| e1.sub(*e0).len() > 2.0 * t).map(|(e0, e1)| alloc::vec![*e0, *e1]).collect();
        let near = |q: &&&Pf<'_>| q.comp.index != c.index && z_gap(p.prism, q.prism) <= gap;
        let others: Vec<&&Pf<'_>> = s.items.iter().chain(&s.hidden).filter(near).collect();
        if others.iter().any(|q| contact_len(&faces, &q.flat, gap) > 1e-3) {
            continue;
        }
        let ends: Vec<Vec<V2>> = edges
            .iter()
            .filter(|(e0, e1)| e1.sub(*e0).len() <= 2.0 * t)
            .map(|(e0, e1)| alloc::vec![*e0, *e1])
            .collect();
        if ends.len() == 2
            && ends.iter().all(|e| others.iter().any(|q| contact_len(core::slice::from_ref(e), &q.flat, gap) > 1e-3))
        {
            continue;
        }
        let u = b.sub(a).norm();
        let n = V2::init(-u.y, u.x);
        let mid = V2::init(l.iter().map(|v| v.x).sum::<f64>() / 4.0, l.iter().map(|v| v.y).sum::<f64>() / 4.0);
        let mut best: Option<(f64, f64, &str)> = None;
        for q in &others {
            if q.comp.ty.traits.hardware || q.prism.role == Role::Soil {
                continue;
            }
            let (mut u0, mut u1, mut n0, mut n1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
            for v in q.flat.iter().flatten() {
                let d = v.sub(mid);
                (u0, u1) = (u0.min(d.dot(u)), u1.max(d.dot(u)));
                (n0, n1) = (n0.min(d.dot(n)), n1.max(d.dot(n)));
            }
            if u1.min(len / 2.0) - u0.max(-len / 2.0) <= t {
                continue;
            }
            let (g, side) = if n0 >= t / 2.0 - 1e-6 {
                (n0 - t / 2.0, 1.0)
            } else if n1 <= -t / 2.0 + 1e-6 {
                (-t / 2.0 - n1, -1.0)
            } else {
                continue;
            };
            if g > gap && g <= 6.0 && best.is_none_or(|bb| g < bb.0) {
                best = Some((g, side, q.comp.id.as_str()));
            }
        }
        let (msg, fix) = match best {
            Some((g, side, host)) => {
                let m = n.scale(side);
                let (from, to) = if m.y.abs() >= m.x.abs() {
                    if m.y > 0.0 { ("below", "up") } else { ("above", "down") }
                } else if m.x > 0.0 {
                    ("left of", "right")
                } else {
                    ("right of", "left")
                };
                (
                    format!(
                        "panel '{}' touches other components only at its ends or edges: nothing lies against either face; it floats {} {from} '{host}'",
                        c.id,
                        fmt_ft_in(g)
                    ),
                    format!(
                        "move it {} {to} onto '{host}' (a sheet lies on the framing face to face): check its `at` anchor and offset (an offset left from an earlier placement?), or `z`",
                        fmt_ft_in(g)
                    ),
                )
            }
            None => (
                format!(
                    "panel '{}' touches other components only at its ends or edges: nothing lies against either face, so nothing holds it",
                    c.id
                ),
                "put it on the framing face to face: check its `at` anchor and offset, or `z`".to_string(),
            ),
        };
        diags.add_fix(Level::Warning, "W_FLOATING", Some(&c.id), None, msg, fix);
        return;
    }
}

fn face_host<'a>(s: &Drawn<'_, 'a>, c: &Comp<'a>, mine: &[&&Pf<'_>]) -> Option<(String, &'static str, f64)> {
    if !c.ty.traits.hardware || c.node.get("on").is_some() || !mine.iter().all(|p| p.prism.face_tie) {
        return None;
    }
    let mut best: Option<(f64, String, &'static str)> = None;
    for p in mine {
        for q in s
            .items
            .iter()
            .filter(|q| q.comp.index != c.index && !q.comp.ty.traits.hardware && q.prism.role != Role::Soil)
        {
            if !p.box_.overlaps(q.box_, 0.0)
                || clip::total_area(&clip::boolean(&p.flat, &q.flat, Op::Intersect)) <= 0.01
            {
                continue;
            }
            let (gap, face) = if p.prism.z0 >= q.prism.z1 {
                (p.prism.z0 - q.prism.z1, "face_front")
            } else {
                (q.prism.z0 - p.prism.z1, "face_back")
            };
            if gap > 0.0 && gap < 1.0 && best.as_ref().is_none_or(|b| gap < b.0) {
                best = Some((gap, q.comp.id.clone(), face));
            }
        }
    }
    best.map(|(g, id, f)| (id, f, g))
}

const UNTREATED_MIN_CONTACT: f64 = 0.125;

fn untreated(s: &Drawn<'_, '_>, diags: &Diags) {
    let mut reported: Vec<[u32; 2]> = Vec::new();
    let gap = 1.0 / 32.0;
    for p in &s.items {
        if p.prism.role != Role::Wood || p.comp.existing == Some(true) {
            continue;
        }
        for q in &s.items {
            if !q.prism.role.is_masonry() || z_gap(p.prism, q.prism) > gap {
                continue;
            }
            if !p.box_.expand(gap).overlaps(q.box_, 0.0) {
                continue;
            }
            let dist = region_dist(&p.flat, &q.flat, gap);
            if dist > gap || (dist > 1e-6 && contact_len(&p.flat, &q.flat, gap) < UNTREATED_MIN_CONTACT) {
                continue;
            }
            let key = [p.comp.index, q.comp.index];
            if reported.contains(&key) {
                continue;
            }
            reported.push(key);
            diags.add_fix(
                Level::Warning,
                "W_UNTREATED_CONTACT",
                Some(&p.comp.id),
                None,
                format!(
                    "untreated wood '{}' touches {} '{}' (IRC R317.1 requires preservative-treated wood or a barrier where wood contacts concrete or masonry)",
                    p.comp.id, q.prism.material, q.comp.id
                ),
                "set \"treated\": true on the wood member (or add a sill seal/barrier and note it)".to_string(),
            );
        }
    }
}

struct ZoneBox<'a> {
    name: &'a str,
    box_: geom::Box,
}

#[derive(Clone, Copy)]
struct Edge {
    a: V2,
    b: V2,
    class: usize,
    outward: V2,
}

fn host_edges(outline: &[Pt], xf: &Xf) -> Vec<Edge> {
    let world = xf.apply_loop(outline);
    let flat = geom::flatten_loop(&world, FLAT_TOL);
    let ccw = geom::signed_area_v(&flat) >= 0.0;
    let mut out = Vec::new();
    for (i, &p) in flat.iter().enumerate() {
        let q = flat[(i + 1) % flat.len()];
        let d = q.sub(p);
        let l = d.len();
        if l < 1e-9 {
            continue;
        }
        let nrm = if ccw { V2::init(d.y / l, -d.x / l) } else { V2::init(-d.y / l, d.x / l) };
        let class = if nrm.y < -0.5 {
            0
        } else if nrm.y > 0.5 {
            1
        } else {
            2
        };
        out.push(Edge { a: p, b: q, class, outward: nrm });
    }
    out
}

fn void_edges(l: &[V2]) -> Vec<Edge> {
    let mut out = Vec::new();
    for (i, &p) in l.iter().enumerate() {
        let q = l[(i + 1) % l.len()];
        let d = q.sub(p);
        let len = d.len();
        if len < 1e-9 {
            continue;
        }
        let ny = d.x / len;
        let class = if ny < -0.5 {
            0
        } else if ny > 0.5 {
            1
        } else {
            2
        };
        out.push(Edge { a: p, b: q, class, outward: V2::init(-d.y / len, d.x / len) });
    }
    out
}

fn host_section(h: &Comp<'_>, xf: &Xf, instance: usize, z: f64) -> Vec<Vec<V2>> {
    let Some(host) = h.built.host.as_ref() else { return Vec::new() };
    if h.node.get("shape").and_then(Value::str) == Some("round")
        && let (Some(d), Some(hz)) = (h.node.get("diameter").and_then(units::parse_length), h.zs.get(instance))
    {
        let r = d / 2.0;
        let dz = z - (hz[0] + hz[1]) / 2.0;
        if dz.abs() > r {
            return Vec::new();
        }
        let dx = math::sqrt((r * r - dz * dz).max(0.0));
        let bx = world_box(xf, h.built.box_);
        let x = bx.center().x;
        return clip::from_region(&[model::rect_loop(x - dx, bx.y0, x + dx, bx.y1)], FLAT_TOL);
    }
    if h.node.get("shape").and_then(Value::str).is_some_and(|s| matches!(s, "footprint" | "round")) {
        if let Some(p) = h.world.iter().find(|p| p.instance as usize == instance && p.footprint.len() >= 3) {
            let xs = crate::pathclip::scan(core::slice::from_ref(&p.footprint), z, false);
            return xs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|x| model::rect_loop(x[0], p.fp_y[0], x[1], p.fp_y[1]).into_iter().map(|v| v.v()).collect())
                .collect();
        }
        let mut region = Vec::new();
        for p in h.world.iter().filter(|p| p.instance as usize == instance && z >= p.z0 - 1e-6 && z <= p.z1 + 1e-6) {
            let flat = clip::from_region(&p.loops, FLAT_TOL);
            region = if region.is_empty() { flat } else { clip::boolean(&region, &flat, Op::Unite) };
        }
        return region;
    }
    clip::from_region(&[xf.apply_loop(&host.outline)], FLAT_TOL)
}

fn segment_in_region(a: V2, b: V2, region: &[Vec<V2>]) -> Vec<[V2; 2]> {
    let mut ts = alloc::vec![0.0, 1.0];
    for l in region {
        for (i, &p) in l.iter().enumerate() {
            let mut ta = [0.0; 2];
            let mut tb = [0.0; 2];
            let n = geom::seg_seg(a, b, p, l[(i + 1) % l.len()], &mut ta, &mut tb);
            ts.extend_from_slice(&ta[..n]);
        }
    }
    crate::sort::stable(&mut ts, |a, b| a < b);
    ts.windows(2)
        .filter(|w| w[1] - w[0] > 1e-9 && geom::locate(a.lerp(b, (w[0] + w[1]) / 2.0), region, 1e-6) != Loc::Outside)
        .map(|w| [a.lerp(b, w[0]), a.lerp(b, w[1])])
        .collect()
}

fn exposed_edges(s: &Drawn<'_, '_>, h: &Comp<'_>, bp: &Prism, edges: Vec<Edge>) -> Vec<Edge> {
    let z = (bp.z0 + bp.z1) / 2.0;
    let mut others = Vec::new();
    for c in s.comps.iter().filter(|c| c.index != h.index && c.built.host.is_some()) {
        for (i, (xf, hz)) in c.xfs.iter().zip(&c.zs).enumerate() {
            if z >= hz[0] - 1e-6 && z <= hz[1] + 1e-6 {
                let mut region = host_section(c, xf, i, z);
                for v in c.voids.iter().filter(|v| z > v.z[0] && z < v.z[1]) {
                    region = clip::boolean(&region, &v.loops, Op::Diff);
                }
                others.push(region);
            }
        }
    }
    if others.is_empty() {
        return edges;
    }
    let mut out = Vec::new();
    for e in edges {
        let mut ts = alloc::vec![0.0, 1.0];
        for region in &others {
            for l in region {
                for (i, &a) in l.iter().enumerate() {
                    let mut ta = [0.0; 2];
                    let mut tb = [0.0; 2];
                    let n = geom::seg_seg(e.a, e.b, a, l[(i + 1) % l.len()], &mut ta, &mut tb);
                    ts.extend_from_slice(&ta[..n]);
                }
            }
        }
        crate::sort::stable(&mut ts, |a, b| a < b);
        let normal = e.outward.scale(1e-5);
        for w in ts.windows(2).filter(|w| w[1] - w[0] > 1e-9) {
            let m = e.a.lerp(e.b, (w[0] + w[1]) / 2.0);
            let covered = others.iter().any(|r| geom::locate(m.add(normal), r, 1e-7) == Loc::Inside);
            if !covered {
                out.push(Edge { a: e.a.lerp(e.b, w[0]), b: e.a.lerp(e.b, w[1]), class: e.class, outward: e.outward });
            }
        }
    }
    out
}

fn cover_for<'z>(h: &HostInfo, zones_world: &[ZoneBox<'z>], c: V2) -> (Cover, Option<&'z str>) {
    for pc in &h.part_cover {
        if let Some(z) = zones_world.iter().find(|z| z.name == pc.part && z.box_.contains(c)) {
            return (pc.cover, Some(z.name));
        }
    }
    (h.cover, zones_world.iter().find(|z| z.box_.contains(c)).map(|z| z.name))
}

fn cover_json(c: Cover, face: Option<usize>, value: f64) -> String {
    let mut v = [c.bottom, c.top, c.sides];
    if let Some(k) = face {
        v[k] = value;
    }
    format!(
        "\"bottom\": {}, \"top\": {}, \"sides\": {}",
        model::num_text(v[0]),
        model::num_text(v[1]),
        model::num_text(v[2])
    )
}

fn cover_op(host_id: &str, h: &HostInfo, part: Option<&str>, face: usize, value: f64) -> String {
    let base = cover_json(h.cover, if part.is_none() { Some(face) } else { None }, value);
    let mut parts: Vec<String> = h
        .part_cover
        .iter()
        .map(|pc| {
            let f = if part == Some(pc.part.as_str()) { Some(face) } else { None };
            format!("\"{}\": {{{}}}", pc.part, cover_json(pc.cover, f, value))
        })
        .collect();
    if let Some(p) = part.filter(|p| !h.part_cover.iter().any(|pc| pc.part == *p)) {
        parts.push(format!("\"{p}\": {{{}}}", cover_json(h.cover, Some(face), value)));
    }
    let parts = if parts.is_empty() { String::new() } else { format!(", \"parts\": {{{}}}", parts.join(", ")) };
    format!("{{\"op\": \"update\", \"path\": \"components/{host_id}\", \"value\": {{\"cover\": {{{base}{parts}}}}}}}")
}

fn move_bar_op(bar: &Comp<'_>, delta: V2, leg: Option<usize>) -> Option<String> {
    let points = bar.node.get("points")?.arr()?;
    if bar.node.get("plane").and_then(Value::str).is_some_and(|p| p != "xy") {
        return None;
    }
    let xf = bar.xfs.first()?;
    let local = V2::init((xf.d * delta.x - xf.c * delta.y) / xf.det(), (-xf.b * delta.x + xf.a * delta.y) / xf.det());
    let mut out = points.to_vec();
    for (i, p) in out.iter_mut().enumerate() {
        if leg.is_some_and(|k| i + 1 != k && i != k) {
            continue;
        }
        if let Some(a) = p.arr().filter(|a| a.len() >= 2) {
            let (x, y) = units::parse_length(&a[0]).zip(units::parse_length(&a[1]))?;
            let mut a = a.to_vec();
            a[0] = Value::Number(x + local.x);
            a[1] = Value::Number(y + local.y);
            *p = Value::Array(a);
        } else {
            let v = if matches!(p, Value::String(_)) {
                Value::Object(alloc::vec![crate::json::Member { key: "ref".into(), value: p.clone() }])
            } else {
                p.clone()
            };
            let old = v.get("offset").and_then(Value::arr);
            let x = old.and_then(|a| a.first()).and_then(units::parse_length).unwrap_or(0.0);
            let y = old.and_then(|a| a.get(1)).and_then(units::parse_length).unwrap_or(0.0);
            let patch = Value::Object(alloc::vec![crate::json::Member {
                key: "offset".into(),
                value: Value::Array(alloc::vec![Value::Number(x + delta.x), Value::Number(y + delta.y)])
            }]);
            *p = crate::json::merge_patch(&v, &patch);
        }
    }
    let mut bytes = Vec::new();
    crate::json::write_compact(&mut bytes, &Value::Array(out));
    Some(format!(
        "{{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"points\":{}}}}}",
        bar.id,
        String::from_utf8_lossy(&bytes)
    ))
}

struct Piece {
    segs: Vec<[V2; 2]>,
    leg: usize,
    is_bend: bool,
    p0: V2,
    p1: V2,
}

fn line_intersect(p: V2, d: V2, q: V2, e: V2) -> Option<V2> {
    let den = d.cross(e);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = q.sub(p).cross(e) / den;
    Some(p.add(d.scale(t)))
}

fn bar_pieces(cl: &[Pt]) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut legs = 0;
    for w in cl.windows(2) {
        let mut fl = alloc::vec![w[0].v()];
        geom::flatten_seg_into(&mut fl, w[0].v(), w[1].v(), w[0].b, FLAT_TOL);
        let segs: Vec<[V2; 2]> = fl.windows(2).map(|s| [s[0], s[1]]).collect();
        let bend = w[0].b != 0.0;
        if !bend {
            legs += 1;
        }
        out.push(Piece { segs, leg: legs, is_bend: bend, p0: w[0].v(), p1: w[1].v() });
    }
    for i in 0..out.len() {
        if out[i].is_bend || out[i].p0.dist(out[i].p1) < 1e-6 {
            continue;
        }
        let d = out[i].p1.sub(out[i].p0).norm();
        if i >= 2 && out[i - 1].is_bend {
            let prev = &out[i - 2];
            if !prev.is_bend
                && prev.p0.dist(prev.p1) > 1e-6
                && let Some(x) = line_intersect(out[i].p0, d, prev.p0, prev.p1.sub(prev.p0).norm())
            {
                out[i].p0 = x;
            }
        }
        if i + 2 < out.len() && out[i + 1].is_bend {
            let next = &out[i + 2];
            if !next.is_bend
                && next.p0.dist(next.p1) > 1e-6
                && let Some(x) = line_intersect(out[i].p0, d, next.p0, next.p1.sub(next.p0).norm())
            {
                out[i].p1 = x;
            }
        }
    }
    out
}

fn piece_text(pieces: &[Piece], k: usize) -> String {
    let Some(pc) = pieces.get(k) else { return "?".to_string() };
    let nlegs = pieces.iter().filter(|q| !q.is_bend).count();
    if pc.is_bend {
        let m = pc.segs.get(pc.segs.len() / 2).map_or(pc.p0, |s| s[0]);
        return format!(
            "the bend after segment {} of {} (near x {}, y {})",
            pc.leg,
            nlegs,
            fmt_ft_in(m.x),
            fmt_ft_in(m.y)
        );
    }
    format!(
        "segment {} of {} (x {}, y {} to x {}, y {})",
        pc.leg,
        nlegs,
        fmt_ft_in(pc.p0.x),
        fmt_ft_in(pc.p0.y),
        fmt_ft_in(pc.p1.x),
        fmt_ft_in(pc.p1.y)
    )
}

fn cover(s: &Drawn<'_, '_>, diags: &Diags) {
    for bar in &s.comps {
        if !bar.ty.traits.reinforcement {
            continue;
        }
        let r = bar.built.bar_d / 2.0;
        let mut outside: Vec<(V2, &str)> = Vec::new();
        let xz = bar.world.iter().any(|p| p.xz_bar);
        let mut worst: Vec<(u32, usize, f64, String, String)> = Vec::new();
        for bp in bar.world.iter().filter(|p| p.role == Role::Rebar) {
            let mut segs: Vec<[V2; 2]> = Vec::new();
            let mut pieces: Vec<Piece> = Vec::new();
            let centre = if bp.centerline.len() >= 2 {
                pieces = bar_pieces(&bp.centerline);
                let flat = geom::flatten_polyline(&bp.centerline, false, FLAT_TOL);
                segs = flat.windows(2).map(|s| [s[0], s[1]]).collect();
                let mut longest = 0;
                let mut ll = -1.0;
                for (i, s) in segs.iter().enumerate() {
                    let l = s[0].dist(s[1]);
                    if l > ll {
                        ll = l;
                        longest = i;
                    }
                }
                let Some(ls) = segs.get(longest) else { continue };
                ls[0].mid(ls[1])
            } else {
                let Some(l0) = bp.loops.first() else { continue };
                geom::loop_box(l0).center()
            };
            let mut hosted = false;
            for h in &s.comps {
                let Some(host) = h.built.host.as_ref() else { continue };
                let Some(hz0) = h.zs.first() else { continue };
                if bp.z0 < hz0[0] - 1e-6 && bp.z1 < hz0[0] - 1e-6 {
                    continue;
                }
                let mut found = false;
                for (hi, xf) in h.xfs.iter().enumerate() {
                    let Some(hz) = h.zs.get(hi) else { break };
                    if bp.z1 < hz[0] - 1e-6 || bp.z0 > hz[1] + 1e-6 {
                        continue;
                    }
                    let region = host_section(h, xf, hi, (bp.z0 + bp.z1) / 2.0);
                    if h.ty.ty != crate::catalog::Type::CmuWall
                        && h.world.first().is_some_and(|p| matches!(p.material.as_str(), "cmu" | "grout" | "mortar"))
                        && s.comps.iter().any(|wall| {
                            wall.ty.ty == crate::catalog::Type::CmuWall
                                && wall.xfs.iter().zip(&wall.zs).enumerate().any(|(i, (wxf, wz))| {
                                    let z = (bp.z0 + bp.z1) / 2.0;
                                    let wr = host_section(wall, wxf, i, z);
                                    z >= wz[0]
                                        && z <= wz[1]
                                        && region.iter().flatten().all(|&v| geom::locate(v, &wr, 1e-6) != Loc::Outside)
                                })
                        })
                    {
                        continue;
                    }
                    let centre = if geom::locate(centre, &region, 1e-6) != Loc::Outside {
                        centre
                    } else {
                        let Some(clipped) =
                            segs.iter().find_map(|sg| segment_in_region(sg[0], sg[1], &region).first().copied())
                        else {
                            continue;
                        };
                        clipped[0].mid(clipped[1])
                    };
                    found = true;
                    if h.existing == Some(true) || bar.existing == Some(true) {
                        break;
                    }
                    let mut edges =
                        if h.node.get("shape").and_then(Value::str).is_some_and(|s| matches!(s, "footprint" | "round"))
                        {
                            region
                                .iter()
                                .flat_map(|l| {
                                    let pts: Vec<Pt> = l.iter().map(|&p| Pt::at(p, 0.0)).collect();
                                    host_edges(&pts, &Xf::IDENTITY)
                                })
                                .collect()
                        } else {
                            host_edges(&host.outline, xf)
                        };
                    for v in h.voids.iter().filter(|v| v.cover_face && v.z[0] < bp.z1 && v.z[1] > bp.z0) {
                        for l in &v.loops {
                            let ve = void_edges(l);
                            let ve = ve.into_iter().filter(|e| {
                                if h.ty.ty != crate::catalog::Type::CmuWall {
                                    return true;
                                }
                                let m = e.a.mid(e.b).add(e.outward.scale(1e-5));
                                !s.items.iter().any(|p| {
                                    p.prism.role == Role::Steel
                                        && z_gap(p.prism, bp) < 1e-6
                                        && geom::locate(m, &p.flat, 1e-6) != Loc::Outside
                                })
                            });
                            edges.extend(ve);
                        }
                    }
                    let edges = exposed_edges(s, h, bp, edges);
                    let zw: Vec<ZoneBox<'_>> =
                        h.built.zones.iter().map(|z| ZoneBox { name: &z.name, box_: world_box(xf, z.box_) }).collect();
                    let (req, part) = cover_for(host, &zw, centre);
                    let mut min_clear = [f64::INFINITY; 3];
                    let mut min_piece = [0usize; 3];
                    let mut min_normal = [V2::init(0.0, 0.0); 3];
                    for e in &edges {
                        let dist;
                        if !segs.is_empty() {
                            let mut d = f64::INFINITY;
                            let mut best_key = f64::INFINITY;
                            let mut best_pi = 0;
                            for (pi, pc) in pieces.iter().enumerate() {
                                let mut pd = f64::INFINITY;
                                for sg in &pc.segs {
                                    if !pc.is_bend && sg[1].sub(sg[0]).norm().dot(e.outward).abs() > 0.7 {
                                        continue;
                                    }
                                    for sg in segment_in_region(sg[0], sg[1], &region) {
                                        pd = pd.min(seg_dist(sg[0], sg[1], e.a, e.b));
                                    }
                                }
                                let key = pd + if pc.is_bend { 1e-4 } else { 0.0 };
                                if key < best_key - 1e-12 {
                                    best_key = key;
                                    best_pi = pi;
                                }
                                d = d.min(pd);
                            }
                            dist = d;
                            if dist - r < min_clear[e.class] {
                                min_piece[e.class] = best_pi;
                            }
                        } else {
                            dist = geom::dist_point_seg(centre, e.a, e.b);
                        }
                        let clear = dist - r;
                        if clear < min_clear[e.class] {
                            min_clear[e.class] = clear;
                            min_normal[e.class] = e.outward;
                        }
                    }
                    let reqs = [req.bottom, req.top, req.sides];
                    let names = ["bottom", "top", "sides"];
                    let depth = geom::loop_box(&host.outline).height();
                    for k in 0..3 {
                        if min_clear[k] < reqs[k] - 1e-3 {
                            let actual = (math::floor(min_clear[k].max(0.0) * 16.0 + 1e-6) / 16.0).max(0.0);
                            let part = part.filter(|p| host.part_cover.iter().any(|pc| pc.part == *p));
                            let knob = match part {
                                Some(p) => format!("cover.parts.{p}.{}", names[k]),
                                None => format!("cover.{}", names[k]),
                            };
                            let mut fix = format!(
                                "move the bar inward (increase place.cover / side_cover or the offset), or if {} is the intended cover set {knob} of '{}' to {}: {}",
                                fmt_ft_in(actual),
                                h.id,
                                model::num_text(actual),
                                cover_op(&h.id, host, part, k, actual)
                            );
                            if min_clear[k] < 0.0 {
                                let delta = min_normal[k].scale(-(reqs[k] - min_clear[k] + 1.0 / 64.0));
                                fix = format!(
                                    "the bar surface crosses an exposed face; setting cover to 0 cannot fix it. Move the failing leg inward by at least {}{}",
                                    fmt_ft_in(reqs[k] - min_clear[k]),
                                    move_bar_op(bar, delta, pieces.get(min_piece[k]).map(|p| p.leg))
                                        .map_or_else(String::new, |op| format!(": {op}"))
                                );
                            }
                            if !host.cover_set && req.bottom + req.top + 2.0 * r > depth + 1e-6 {
                                fix.push_str(&format!(
                                    "; '{}' uses the default cover (bottom {}, top {}, sides {}), which no bar fits in its {} depth: give it its own \"cover\" (slab on grade with mid-depth bars, e.g. {{\"bottom\": 1.5, \"top\": 1.5}})",
                                    h.id,
                                    fmt_ft_in(req.bottom),
                                    fmt_ft_in(req.top),
                                    fmt_ft_in(req.sides),
                                    fmt_ft_in(depth)
                                ));
                            }
                            let src = match part.filter(|p| host.part_cover.iter().any(|pc| pc.part == *p)) {
                                Some(p) => format!("cover.parts.{p}.{}", names[k]),
                                None => format!("cover.{}{}", names[k], if host.cover_set { "" } else { ", default" }),
                            };
                            let message = if !pieces.is_empty() {
                                format!(
                                    "clear cover from {}, {}, to the {} of '{}' is {} < required {} ({src})",
                                    bar.built.info,
                                    piece_text(&pieces, min_piece[k]),
                                    names[k],
                                    h.id,
                                    fmt_ft_in(min_clear[k]),
                                    fmt_ft_in(reqs[k]),
                                )
                            } else {
                                format!(
                                    "clear cover from {} bar to the {} of '{}' is {} < required {} ({src}); bar center at x {}, y {}",
                                    bar.built.info,
                                    names[k],
                                    h.id,
                                    fmt_ft_in(min_clear[k]),
                                    fmt_ft_in(reqs[k]),
                                    fmt_ft_in(centre.x),
                                    fmt_ft_in(centre.y),
                                )
                            };
                            if xz {
                                match worst.iter_mut().find(|w| w.0 == h.index && w.1 == k) {
                                    Some(w) if w.2 <= min_clear[k] => {}
                                    Some(w) => *w = (h.index, k, min_clear[k], message, fix),
                                    None => worst.push((h.index, k, min_clear[k], message, fix)),
                                }
                            } else {
                                diags.add_fix(Level::Warning, "W_COVER", Some(&bar.id), None, message, fix);
                            }
                        }
                    }
                    break;
                }
                if found {
                    hosted = true;
                }
            }
            if pieces.is_empty() {
                if !hosted {
                    outside.push((centre, "its centre"));
                }
            } else if let Some(o) = path_outside(s, bp, &pieces, centre) {
                outside.push(o);
            }
        }
        for (_, _, _, message, fix) in worst {
            diags.add_fix(Level::Warning, "W_COVER", Some(&bar.id), None, message, fix);
        }
        bar_outside(s, bar, &outside, diags);
    }
}

fn in_host(s: &Drawn<'_, '_>, bp: &Prism, p: V2) -> bool {
    s.comps.iter().any(|h| {
        if h.built.host.is_none() {
            return false;
        }
        h.xfs.iter().zip(&h.zs).any(|(xf, hz)| {
            if bp.z1 < hz[0] - 1e-6 || bp.z0 > hz[1] + 1e-6 {
                return false;
            }
            let instance = h.xfs.iter().position(|q| core::ptr::eq(q, xf)).unwrap_or(0);
            let region = host_section(h, xf, instance, (bp.z0 + bp.z1) / 2.0);
            geom::locate(p, &region, 1e-6) != Loc::Outside
        })
    })
}

fn path_outside(s: &Drawn<'_, '_>, bp: &Prism, pieces: &[Piece], centre: V2) -> Option<(V2, &'static str)> {
    for pc in pieces.iter().filter(|pc| pc.is_bend) {
        let m = pc.segs.get(pc.segs.len() / 2).map_or(pc.p0, |sg| sg[0]);
        if !in_host(s, bp, m) {
            return Some((m, "a bend"));
        }
    }
    let any =
        pieces.iter().flat_map(|pc| pc.segs.iter()).any(|sg| in_host(s, bp, sg[0]) || in_host(s, bp, sg[0].mid(sg[1])));
    let last = pieces.last().and_then(|pc| pc.segs.last()).is_some_and(|sg| in_host(s, bp, sg[1]));
    if any || last { None } else { Some((centre, "the middle of its longest leg")) }
}

fn bar_outside(s: &Drawn<'_, '_>, bar: &Comp<'_>, outside: &[(V2, &str)], diags: &Diags) {
    let Some(&(c, what)) = outside.first() else { return };
    let mut nearest: Option<(&str, f64)> = None;
    for h in &s.comps {
        let Some(host) = h.built.host.as_ref() else { continue };
        for xf in &h.xfs {
            let fl = geom::flatten_loop(&xf.apply_loop(&host.outline), FLAT_TOL);
            let d = (0..fl.len())
                .map(|i| geom::dist_point_seg(c, fl[i], fl[(i + 1) % fl.len()]))
                .fold(f64::INFINITY, f64::min);
            if nearest.is_none_or(|(_, nd)| d < nd) {
                nearest = Some((h.id.as_str(), d));
            }
        }
    }
    let Some((host, dist)) = nearest else { return };
    let total = bar.world.len();
    let which = if total == 1 {
        format!("{what} (x {}, y {})", fmt_ft_in(c.x), fmt_ft_in(c.y))
    } else if bar.world.iter().any(|p| p.xz_bar) {
        format!(
            "{} of the {total} pieces of its xz path (the first at x {}, y {})",
            outside.len(),
            fmt_ft_in(c.x),
            fmt_ft_in(c.y)
        )
    } else {
        format!("{} of its {total} bars (the first at x {}, y {})", outside.len(), fmt_ft_in(c.x), fmt_ft_in(c.y))
    };
    diags.add_fix(
        Level::Warning,
        "W_BAR_OUTSIDE",
        Some(&bar.id),
        None,
        format!(
            "rebar '{}' is not inside concrete or masonry: {which} lies in no host; the nearest host is '{host}', {} away",
            bar.id,
            fmt_ft_in(dist),
        ),
        format!(
            "move the bar into '{host}' (\"place\": {{\"in\": \"{host}\", \"face\": \"bottom\", \"cover\": 3}} puts bars at cover inside its outline) or correct its at/points; acknowledge W_BAR_OUTSIDE only if the bar really stands outside the concrete (a dowel into future work)"
        ),
    );
}

fn unverified_count(doc: &Value, diags: &Diags) {
    let mut n = 0usize;
    let mut ids: Vec<String> = Vec::new();
    if let Some(va) = doc.get("views").and_then(Value::arr) {
        for v in va {
            let vid = v.get("id").map_or("?", |x| x.str().unwrap_or("?"));
            let Some(aa) = v.get("annotations").and_then(Value::arr) else { continue };
            for x in aa {
                let mut mine = false;
                if let Some(ca) = x.get("cite").and_then(Value::arr) {
                    for c in ca {
                        let st = c.get("status").map_or("suggested", |s| s.str().unwrap_or("suggested"));
                        if st != "verified" {
                            n += 1;
                            mine = true;
                        }
                    }
                }
                if mine {
                    ids.push(format!("{}/{}", vid, x.get("id").map_or("?", |i| i.str().unwrap_or("?"))));
                }
            }
        }
    }
    if n > 0 {
        diags.add(
            Level::Info,
            "I_UNVERIFIED_CITE",
            None,
            None,
            format!("{} citation(s) await designer verification (notes: {}). Citations print with a trailing * until verified.", n, join_ids(&ids)),
        );
    }
}

#[derive(Clone, Copy)]
struct CompBox<'a> {
    c: &'a Comp<'a>,
    instance: u32,
    box_: geom::Box,
    z0: f64,
    z1: f64,
}

const SIDE_BY_SIDE_GAP: f64 = 1.0;

fn near_miss(s: &Drawn<'_, '_>, diags: &Diags) {
    let mut boxes: Vec<CompBox<'_>> = Vec::new();
    for &c in &s.comps {
        if !c.ty.traits.near_miss || !axis_aligned(c) {
            continue;
        }
        let first = boxes.len();
        for p in &c.world {
            if p.kind == Kind::Ghost {
                continue;
            }
            let mut b = geom::Box::default();
            for l in &p.loops {
                b.add_box(geom::loop_box(l));
            }
            if b.is_empty() {
                continue;
            }
            match boxes[first..].iter_mut().find(|cb| cb.instance == p.instance) {
                Some(cb) => {
                    cb.box_.add_box(b);
                    cb.z0 = cb.z0.min(p.z0);
                    cb.z1 = cb.z1.max(p.z1);
                }
                None => boxes.push(CompBox { c, instance: p.instance, box_: b, z0: p.z0, z1: p.z1 }),
            }
        }
    }
    let mut reported: Vec<[u32; 3]> = Vec::new();
    let min_gap = 1.0 / 32.0;
    let max_gap = 3.0;
    let hung = |a: &str, b: &str| {
        s.comps.iter().any(|h| {
            let [j0, j1] = &h.built.hw.joins;
            (j0.iter().any(|x| x == a) && j1.iter().any(|x| x == b))
                || (j0.iter().any(|x| x == b) && j1.iter().any(|x| x == a))
        })
    };
    for (i, &ca) in boxes.iter().enumerate() {
        for &cb in &boxes[i + 1..] {
            let both_existing = ca.c.existing == Some(true) && cb.c.existing == Some(true);
            if ca.c.index == cb.c.index || both_existing || hung(&ca.c.id, &cb.c.id) {
                continue;
            }
            let zlo = ca.z0.max(cb.z0);
            let zhi = ca.z1.min(cb.z1);
            if zhi - zlo <= 1e-6 {
                continue;
            }
            for on_x in [false, true] {
                let sel = |b: geom::Box| if on_x { [b.x0, b.x1, b.y0, b.y1] } else { [b.y0, b.y1, b.x0, b.x1] };
                let [a0, a1, o0, o1] = sel(ca.box_);
                let [b0, b1, q0, q1] = sel(cb.box_);
                let overlap = o1.min(q1) - o0.max(q0);
                let gap = a0.max(b0) - a1.min(b1);
                if !(overlap > 1e-6 && gap >= min_gap - 1e-9 && gap <= max_gap + 1e-9) {
                    continue;
                }
                let g0 = a1.min(b1);
                let g1 = a0.max(b0);
                let lo = o0.max(q0);
                let hi = o1.min(q1);
                let blocked = s.items.iter().any(|it| {
                    let own = |b: &CompBox<'_>| it.comp.index == b.c.index && it.prism.instance == b.instance;
                    if own(&ca) || own(&cb) || it.comp.dashed {
                        return false;
                    }
                    if it.prism.z1.min(zhi) - it.prism.z0.max(zlo) <= 1e-6 {
                        return false;
                    }
                    let [c0, c1, d0, d1] = sel(it.box_);
                    c1.min(g1) - c0.max(g0) > 1e-6 && d1.min(hi) - d0.max(lo) > 1e-6
                });
                if blocked {
                    continue;
                }
                let a_len = a1 - a0;
                let b_len = b1 - b0;
                let a_across = o1 - o0;
                let b_across = q1 - q0;
                let side_by_side = a_len < a_across && b_len < b_across;
                if side_by_side && on_x && gap >= SIDE_BY_SIDE_GAP - 1e-9 {
                    continue;
                }
                let key = [ca.c.index.min(cb.c.index), ca.c.index.max(cb.c.index), u32::from(on_x)];
                if reported.contains(&key) {
                    continue;
                }
                reported.push(key);
                let (grow, other) =
                    if b_len / b_across.max(1e-9) > a_len / a_across.max(1e-9) { (cb, ca) } else { (ca, cb) };
                let grow_is_a = grow.c.index == ca.c.index && grow.instance == ca.instance;
                let grow_len = if grow_is_a { a_len } else { b_len };
                let grow_below_other = if grow_is_a { a1 <= b0 } else { b1 <= a0 };
                let edge_name = if on_x {
                    if grow_below_other { "left" } else { "right" }
                } else if grow_below_other {
                    "bottom"
                } else {
                    "top"
                };
                let anchor = if on_x {
                    if grow_below_other { "middle_left" } else { "middle_right" }
                } else if grow_below_other {
                    "bottom_left"
                } else {
                    "top_left"
                };
                if side_by_side {
                    let toward = if on_x {
                        if grow_below_other { "right" } else { "left" }
                    } else if grow_below_other {
                        "up"
                    } else {
                        "down"
                    };
                    diags.add_fix(
                        Level::Warning,
                        "W_NEAR_MISS",
                        Some(&grow.c.id),
                        None,
                        format!(
                            "'{}' and '{}' run parallel with a {} gap along {} ({}..{}) between their long faces; they probably should bear on each other",
                            grow.c.id,
                            other.c.id,
                            fmt_ft_in(gap),
                            if on_x { "x" } else { "y" },
                            fmt_ft_in(g0),
                            fmt_ft_in(g1),
                        ),
                        format!(
                            "move '{}' {} {} (its `at` offset) or place it against '{}' with an anchor Ref; ignore this warning if the gap is intended",
                            grow.c.id,
                            fmt_ft_in(gap),
                            toward,
                            other.c.id
                        ),
                    );
                    continue;
                }
                diags.add_fix(
                    Level::Warning,
                    "W_NEAR_MISS",
                    Some(&grow.c.id),
                    None,
                    format!(
                        "'{}' and '{}' leave a {} gap along {} ({}..{}) between facing edges while overlapping along {}; '{}' probably should extend to the {} edge of '{}'",
                        grow.c.id,
                        other.c.id,
                        fmt_ft_in(gap),
                        if on_x { "x" } else { "y" },
                        fmt_ft_in(g0),
                        fmt_ft_in(g1),
                        if on_x { "y" } else { "x" },
                        grow.c.id,
                        edge_name,
                        other.c.id,
                    ),
                    format!(
                        "set the length of '{}' to {}, or use \"until\": \"{}@{}\" so it follows '{}'; ignore this warning if the gap is intended",
                        grow.c.id,
                        fmt_ft_in(grow_len + gap),
                        other.c.id,
                        anchor,
                        other.c.id
                    ),
                );
            }
        }
    }
}

fn explicit_crop(doc: &Value) -> Option<geom::Box> {
    for v in doc.get("views")?.arr()? {
        if v.get("kind").and_then(Value::str) == Some("iso") {
            continue;
        }
        let Some(cv) = v.get("crop").filter(|c| !c.is_null()) else { continue };
        let Some(xr) = range2(cv.get("x")) else { continue };
        let Some(yr) = range2(cv.get("y")) else { continue };
        return Some(geom::Box { x0: xr[0], x1: xr[1], y0: yr[0], y1: yr[1] });
    }
    None
}

fn range2(v: Option<&Value>) -> Option<[f64; 2]> {
    let arr = v?.arr()?;
    if arr.len() != 2 {
        return None;
    }
    Some([units::parse_length(&arr[0])?, units::parse_length(&arr[1])?])
}

fn host_angle(h: &Comp<'_>) -> Option<f64> {
    h.ty.traits.slope_host?;
    Some(if h.ty.traits.has_pitch { h.angle + h.pitch_angle } else { h.angle })
}

fn same_slope(a1: f64, a2: f64) -> bool {
    let x = a1 - a2;
    let mut d = x % PI;
    if d < 0.0 {
        d += PI;
    }
    if d > PI / 2.0 {
        d = PI - d;
    }
    d < 0.5 * (PI / 180.0)
}

fn proj_extent(items: &[&Pf<'_>], c: &Comp<'_>, part: Option<&str>, u: V2) -> Option<[f64; 2]> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for it in items {
        if it.comp.index != c.index || it.prism.instance != 0 {
            continue;
        }
        if part.is_some_and(|pn| it.prism.part != pn) {
            continue;
        }
        for l in &it.flat {
            for v in l {
                let t = v.x * u.x + v.y * u.y;
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
    }
    (lo <= hi).then_some([lo, hi])
}

fn short_slope(s: &Drawn<'_, '_>, crop: Option<geom::Box>, diags: &Diags) {
    let items = &s.items;
    let slack = 0.5;
    let touch = 1.0 / 16.0;
    for &c in &s.comps {
        if c.dashed || !c.ty.traits.sheet {
            continue;
        }
        let mut u = V2::init(math::cos(c.angle), math::sin(c.angle));
        if u.y.abs() < math::sin(0.5 * (PI / 180.0)) {
            continue;
        }
        if u.y < 0.0 {
            u = V2::init(-u.x, -u.y);
        }
        let mut pmax = f64::NEG_INFINITY;
        let mut vtx = V2::init(0.0, 0.0);
        for it in items {
            if it.comp.index != c.index || it.prism.instance != 0 {
                continue;
            }
            for l in &it.flat {
                for v in l {
                    let t = v.x * u.x + v.y * u.y;
                    if t > pmax {
                        pmax = t;
                        vtx = *v;
                    }
                }
            }
        }
        if pmax == f64::NEG_INFINITY {
            continue;
        }
        let mut best: Option<&Comp<'_>> = None;
        let mut best_end = 0.0;
        for &h in &s.comps {
            if h.index == c.index || h.dashed {
                continue;
            }
            if c.ty.traits.rigid_sheet && h.ty.traits.rigid_sheet {
                continue;
            }
            let Some(ha) = host_angle(h) else { continue };
            if !same_slope(ha, c.angle) {
                continue;
            }
            let part = h.ty.traits.slope_host.map(|s| s.part).filter(|p| !p.is_empty());
            let mut touching = false;
            for sp in items.iter().filter(|s| s.comp.index == c.index && s.prism.instance == 0) {
                for hp in items.iter().filter(|s| s.comp.index == h.index && s.prism.instance == 0) {
                    if part.is_some_and(|pn| hp.prism.part != pn) {
                        continue;
                    }
                    if z_gap(sp.prism, hp.prism) > touch || !sp.box_.expand(touch).overlaps(hp.box_, 0.0) {
                        continue;
                    }
                    if region_dist(&sp.flat, &hp.flat, touch) <= touch {
                        touching = true;
                    }
                }
            }
            if !touching {
                continue;
            }
            let Some(ext) = proj_extent(items, h, part, u) else { continue };
            if best.is_none() || ext[1] > best_end {
                best = Some(h);
                best_end = ext[1];
            }
        }
        let Some(h) = best else { continue };
        let mut crop_room = f64::INFINITY;
        if let Some(cb) = crop {
            if u.x > 1e-9 {
                crop_room = crop_room.min((cb.x1 - vtx.x) / u.x);
            } else if u.x < -1e-9 {
                crop_room = crop_room.min((cb.x0 - vtx.x) / u.x);
            }
            if u.y > 1e-9 {
                crop_room = crop_room.min((cb.y1 - vtx.y) / u.y);
            }
        }
        let gap_host = best_end - pmax;
        let short = gap_host.min(crop_room);
        if short <= slack {
            continue;
        }
        let by_crop = crop_room < gap_host;
        let end_anchor = h.ty.traits.slope_host.map_or("", |s| s.end_anchor);
        let anchor = if !end_anchor.is_empty() {
            end_anchor
        } else if u.x >= 0.0 {
            "top_right"
        } else {
            "top_left"
        };
        let what = if by_crop {
            format!("the view crop (where '{}' continues)", h.id)
        } else {
            format!("the end of '{}'", h.id)
        };
        diags.add_fix(
            Level::Warning,
            "W_SHORT_SLOPE",
            Some(&c.id),
            None,
            format!("'{}' rests on the sloped '{}' (same slope) but its upper end stops {} short of {}", c.id, h.id, fmt_ft_in(short), what),
            format!(
                "replace the literal length of '{}' (it does not follow the pitch) with \"until\": \"{}@{}\" (grows along the slope to the member's end; pair it with \"slope\": \"@{}\"), or lengthen it by {}; acknowledge W_SHORT_SLOPE if it should stop there",
                c.id, h.id, anchor, h.id, fmt_ft_in(short)
            ),
        );
    }
}

fn axis_aligned(c: &Comp<'_>) -> bool {
    for p in &c.world {
        if p.kind == Kind::Ghost {
            continue;
        }
        if p.embedded || p.loops.len() != 1 || p.loops[0].len() != 4 {
            return false;
        }
        let l = &p.loops[0];
        for (i, v) in l.iter().enumerate() {
            let w = l[(i + 1) % 4];
            if v.b != 0.0 {
                return false;
            }
            if (v.x - w.x).abs() > 1e-9 && (v.y - w.y).abs() > 1e-9 {
                return false;
            }
        }
    }
    true
}
