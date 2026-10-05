//! Validation diagnostics (SPEC section 9). Messages are written for an LLM reader.

use crate::diag::{Diag, nearest};
use crate::geom::*;
use crate::model::*;
use crate::num::{fmt_ftin, fmt_num};
use crate::poly;
use crate::resolve::{Lookup, parse_ref};
use crate::style::Style;
use serde_json::Value;

struct Pr<'a> {
    comp: &'a Comp,
    p: &'a Prism,
    flat: Vec<Pt>,
    bbox: Rect,
}

fn exempt_type(c: &Comp) -> bool {
    matches!(c.ctype.as_str(), "membrane" | "connector")
}

fn z_overlap(a: &Prism, b: &Prism) -> f64 {
    a.z1.min(b.z1) - a.z0.max(b.z0)
}

fn poly_dist(a: &[Pt], b: &[Pt]) -> f64 {
    // minimum distance between two polygons (0 if they intersect or nest)
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return f64::INFINITY;
    }
    if poly_contains(b, a[0]) || poly_contains(a, b[0]) {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for i in 0..n {
        let sa = Seg::Line(a[i], a[(i + 1) % n]);
        for j in 0..m {
            let sb = Seg::Line(b[j], b[(j + 1) % m]);
            if !intersect(&sa, &sb).is_empty() {
                return 0.0;
            }
            best = best.min(sb.dist(a[i]));
            best = best.min(sa.dist(b[j]));
        }
    }
    best
}

fn bbox_text(r: &Rect) -> String {
    format!("x {}..{}, y {}..{}", fmt_ftin(r.x0), fmt_ftin(r.x1), fmt_ftin(r.y0), fmt_ftin(r.y1))
}

pub fn validate(doc: &Value, model: &Model, style: &Style, diags: &mut Vec<Diag>) {
    let mut prs: Vec<Pr> = vec![];
    for c in &model.comps {
        if c.failed || !c.visible {
            continue;
        }
        for inst in &c.insts {
            for p in &inst.prisms {
                let flat = flatten_loop(&p.region.outer, 0.002);
                let bbox = region_bbox(&p.region);
                prs.push(Pr { comp: c, p, flat, bbox });
            }
        }
    }

    // --- W_OVERLAP
    let mut reported: Vec<(usize, usize)> = vec![];
    for i in 0..prs.len() {
        for j in (i + 1)..prs.len() {
            let (a, b) = (&prs[i], &prs[j]);
            if a.comp.idx == b.comp.idx {
                continue;
            }
            if a.p.embedded || b.p.embedded || exempt_type(a.comp) || exempt_type(b.comp) {
                continue;
            }
            if a.p.only == Only::Solid3d || b.p.only == Only::Solid3d {
                continue;
            }
            if z_overlap(a.p, b.p) <= 1e-6 {
                continue;
            }
            if !a.bbox.overlaps(&b.bbox, -1e-6) {
                continue;
            }
            let key = (a.comp.idx.min(b.comp.idx), a.comp.idx.max(b.comp.idx));
            if reported.contains(&key) {
                continue;
            }
            let shapes = poly::intersect(&[a.flat.clone()], &[b.flat.clone()]);
            let area: f64 = shapes.iter().map(poly::shape_area).sum();
            if area > 0.01 {
                let mut bb = Rect::empty();
                for sh in &shapes {
                    if let Some(o) = sh.first() {
                        bb.union(&poly_bbox(o));
                    }
                }
                reported.push(key);
                diags.push(
                    Diag::warn(
                        "W_OVERLAP",
                        format!(
                            "{} and {} overlap by {} in\u{b2} ({}). Two solid members occupy the same space.",
                            a.p.src,
                            b.p.src,
                            fmt_num(area),
                            bbox_text(&bb)
                        ),
                    )
                    .id(a.comp.id.clone())
                    .fix(format!("move or resize \"{}\" or \"{}\" so they only touch, or mark one embedded if it is meant to sit inside the other", a.comp.id, b.comp.id)),
                );
            }
        }
    }

    // --- W_FLOATING
    for c in &model.comps {
        if c.failed || !c.visible || model.comps.len() < 2 {
            continue;
        }
        if matches!(c.ctype.as_str(), "fill") {
            // fills are backgrounds; they still need to touch something
        }
        let mine: Vec<&Pr> = prs.iter().filter(|p| p.comp.idx == c.idx).collect();
        if mine.is_empty() {
            continue;
        }
        let mut touches = false;
        'outer: for m in &mine {
            for o in prs.iter().filter(|p| p.comp.idx != c.idx) {
                if z_overlap(m.p, o.p) < -1.0 / 32.0 {
                    continue;
                }
                if !m.bbox.overlaps(&o.bbox, 1.0 / 32.0) {
                    continue;
                }
                if poly_dist(&m.flat, &o.flat) <= 1.0 / 32.0 {
                    touches = true;
                    break 'outer;
                }
            }
        }
        if !touches {
            let b = c.bbox();
            diags.push(
                Diag::warn("W_FLOATING", format!("{} ({}, {}) touches no other component: every gap is more than 1/32\".", c.id, c.ctype, bbox_text(&b)))
                    .id(c.id.clone())
                    .fix(format!("place \"{}\" relative to the component it bears on, e.g. at: {{\"anchor\": \"bottom_left\", \"to\": \"<other>@top_left\"}}", c.id)),
            );
        }
    }

    // --- W_COVER
    for c in &model.comps {
        if c.failed || c.ctype != "rebar" || c.value.get("mode").and_then(|m| m.as_str()).unwrap_or("along_z") != "along_z" {
            continue;
        }
        for inst in &c.insts {
            for p in &inst.prisms {
                let Some((ctr, r)) = p.bar else { continue };
                check_cover(model, c, p, ctr, r, diags);
            }
        }
    }

    // --- W_UNTREATED_CONTACT
    for a in prs.iter().filter(|x| x.comp.ctype == "lumber" && x.p.material == "wood") {
        for b in prs.iter().filter(|x| matches!(x.p.material.as_str(), "concrete" | "grout" | "cmu" | "mortar")) {
            if z_overlap(a.p, b.p) < -1.0 / 32.0 || !a.bbox.overlaps(&b.bbox, 1.0 / 32.0) {
                continue;
            }
            if poly_dist(&a.flat, &b.flat) <= 1.0 / 32.0 {
                diags.push(
                    Diag::warn(
                        "W_UNTREATED_CONTACT",
                        format!("{} (untreated wood) touches {} ({}) at {}. Wood in contact with masonry or concrete must be preservative-treated (IRC R317.1).", a.comp.id, b.comp.id, b.p.material, bbox_text(&a.bbox)),
                    )
                    .id(a.comp.id.clone())
                    .fix(format!("set \"treated\": true on \"{}\" (or add a sill seal/barrier and justify)", a.comp.id)),
                );
                break;
            }
        }
    }

    // --- I_SOLID_USED
    for c in &model.comps {
        if c.ctype == "solid" {
            diags.push(
                Diag::info("I_SOLID_USED", format!("{} uses the `solid` escape hatch (material {}); a reviewer should confirm no typed component fits.", c.id, c.material)).id(c.id.clone()),
            );
        }
    }

    // --- annotation references and citations
    let lk = Lookup { comps: &model.comps, all_ids: &[] };
    let mut unverified: Vec<String> = vec![];
    if let Some(views) = doc.get("views").and_then(|v| v.as_array()) {
        for v in views {
            let vid = v.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let Some(anns) = v.get("annotations").and_then(|a| a.as_array()) else { continue };
            let mut seen: Vec<&str> = vec![];
            for an in anns {
                let id = an.get("id").and_then(|i| i.as_str()).unwrap_or("");
                if seen.contains(&id) {
                    diags.push(Diag::error("E_DUP_ID", format!("duplicate annotation id \"{}\" in view {}.", id, vid)).id(id).path(format!("views/{}/annotations/{}", vid, id)).fix("annotation ids must be unique within a view"));
                }
                seen.push(id);
                let path = format!("views/{}/annotations/{}", vid, id);
                for key in ["at", "from", "to"] {
                    if let Some(rv) = an.get(key) {
                        if let Err(d) = lk.point_value(rv) {
                            diags.push(d.id(id).path(format!("{}/{}", path, key)));
                        }
                    }
                }
                if let Some(t) = an.get("target").and_then(|t| t.as_str()) {
                    let (cid, part) = match t.split_once('.') {
                        Some((a, b)) => (a, Some(b)),
                        None => (t, None),
                    };
                    let cid = cid.split('#').next().unwrap_or(cid);
                    match model.comp(cid) {
                        None => {
                            let ids: Vec<&str> = model.comps.iter().map(|c| c.id.as_str()).collect();
                            let fix = match nearest(cid, ids.iter().copied()) {
                                Some(n) => format!("did you mean \"{}\"? components: {}", n, ids.join(", ")),
                                None => format!("components: {}", ids.join(", ")),
                            };
                            diags.push(Diag::error("E_REF_UNKNOWN", format!("note \"{}\" in view {}: target \"{}\" is not a component.", id, vid, t)).id(id).path(format!("{}/target", path)).fix(fix));
                        }
                        Some(c) => {
                            if let (Some(pn), Some(inst)) = (part, c.insts.first()) {
                                if !inst.parts.iter().any(|p| p.name == pn) {
                                    let names: Vec<&str> = inst.parts.iter().map(|p| p.name.as_str()).collect();
                                    diags.push(
                                        Diag::error("E_REF_UNKNOWN", format!("note \"{}\" in view {}: component \"{}\" has no part \"{}\".", id, vid, cid, pn))
                                            .id(id)
                                            .path(format!("{}/target", path))
                                            .fix(format!("parts of {}: {}", cid, if names.is_empty() { "(none)".into() } else { names.join(", ") })),
                                    );
                                }
                            }
                        }
                    }
                }
                if let Some(cites) = an.get("cite").and_then(|c| c.as_array()) {
                    for ct in cites {
                        if ct.get("status").and_then(|s| s.as_str()) != Some("verified") {
                            unverified.push(format!("{}/{}", vid, id));
                        }
                    }
                }
            }
        }
    }
    if !unverified.is_empty() {
        let mut ids: Vec<String> = unverified.clone();
        ids.dedup();
        diags.push(Diag::info(
            "I_UNVERIFIED_CITE",
            format!("{} citation(s) await designer verification (notes: {}). Citations print with a trailing * until verified.", unverified.len(), ids.join(", ")),
        ));
    }
    let _ = style;
}

fn cover_for(cover: &Option<Value>, part: Option<&str>, class: &str) -> f64 {
    let Some(cv) = cover else { return 0.0 };
    if let Some(p) = part {
        if let Some(x) = cv.get("parts").and_then(|pp| pp.get(p)).and_then(|pv| pv.get(class)).and_then(|x| x.as_f64()) {
            return x;
        }
    }
    cv.get(class).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

fn check_cover(model: &Model, bar_comp: &Comp, bar: &Prism, ctr: Pt, r: f64, diags: &mut Vec<Diag>) {
    for host in &model.comps {
        if host.failed || !matches!(host.ctype.as_str(), "concrete" | "cmu_wall") {
            continue;
        }
        let Some(inst) = host.insts.first() else { continue };
        // host region: the part zone containing the bar, else the whole prism
        let mut zone_name: Option<String> = None;
        let mut region: Option<Region> = None;
        // prism containing the center
        let contains = inst.prisms.iter().any(|p| region_locate(&p.region, ctr, 1e-6) != Loc::Outside);
        if !contains {
            continue;
        }
        if host.ctype == "cmu_wall" {
            // bond beam zone first, then the course zone
            for name in ["bond_beam"].iter().map(|s| s.to_string()).chain(inst.parts.iter().filter(|p| p.name.starts_with("course_")).map(|p| p.name.clone())) {
                if let Some(pi) = inst.parts.iter().find(|p| p.name == name && p.zone) {
                    if region_locate(&pi.region, ctr, 1e-6) != Loc::Outside {
                        zone_name = Some(name.clone());
                        region = Some(pi.region.clone());
                        break;
                    }
                }
            }
        } else {
            for pi in inst.parts.iter().filter(|p| p.zone) {
                if region_locate(&pi.region, ctr, 1e-6) != Loc::Outside {
                    zone_name = Some(pi.name.clone());
                    break;
                }
            }
            if let Some(p) = inst.prisms.iter().find(|p| region_locate(&p.region, ctr, 1e-6) != Loc::Outside) {
                region = Some(p.region.clone());
            }
        }
        let Some(region) = region else { continue };
        let mut worst: [(f64, Option<String>); 3] = [(f64::INFINITY, None), (f64::INFINITY, None), (f64::INFINITY, None)];
        let classes = ["bottom", "top", "sides"];
        for seg in loop_segs(&region.outer) {
            let t = seg.tangent(0.5);
            let n = pt(t.y, -t.x);
            let cls = if n.y < -0.5 { 0 } else if n.y > 0.5 { 1 } else { 2 };
            let clear = seg.dist(ctr) - r;
            if clear < worst[cls].0 {
                worst[cls] = (clear, None);
            }
        }
        for (k, class) in classes.iter().enumerate() {
            let (clear, _) = &worst[k];
            if !clear.is_finite() {
                continue;
            }
            let part_key = zone_name.as_deref();
            let req = cover_for(&host.cover, part_key, class);
            if *clear < req - 1e-3 {
                let place_hint = if bar_comp.value.get("place").is_some() {
                    format!("set place.cover to {}", fmt_num(req + if *class == "sides" { 0.0 } else { 0.0 }))
                } else {
                    format!("move the bar at least {} away from the {} face", fmt_ftin(req - clear), class)
                };
                let hostname = match zone_name {
                    Some(ref z) => format!("{}.{}", host.id, z),
                    None => host.id.clone(),
                };
                diags.push(
                    Diag::warn(
                        "W_COVER",
                        format!(
                            "clear cover from {} ({}) to {} {} face is {} < required {} ({}).",
                            match &bar.part { Some(pn) => format!("{}.{}", bar.src, pn), None => bar.src.clone() },
                            bar_comp.value.get("size").and_then(|s| s.as_str()).unwrap_or(""),
                            hostname,
                            class,
                            fmt_ftin(*clear),
                            fmt_ftin(req),
                            if class == &"sides" { "sides" } else { class }
                        ),
                    )
                    .id(bar_comp.id.clone())
                    .fix(place_hint),
                );
            }
        }
    }
}

pub fn ref_ok(model: &Model, s: &str) -> bool {
    let lk = Lookup { comps: &model.comps, all_ids: &[] };
    match parse_ref(s) {
        Ok(r) => lk.point(&r).is_ok(),
        Err(_) => false,
    }
}
