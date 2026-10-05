//! View rendering: section/iso base + annotations + title -> Drawing.

use crate::annot::*;
use crate::diag::Diag;
use crate::drawing::{Drawing, Item, SheetInfo};
use crate::geom::*;
use crate::model::Model;
use crate::resolve::Lookup;
use crate::style::Style;
use crate::view::{ViewParams, find_view, view_ids, view_params};
use serde_json::Value;

pub fn render_view(doc: &Value, model: &Model, view_id: &str, style: &Style) -> Result<Drawing, Diag> {
    let Some(vv) = find_view(doc, view_id) else {
        let ids = view_ids(doc);
        let mut d = Diag::error("E_REF_UNKNOWN", format!("unknown view \"{}\".", view_id));
        d = d.fix(format!("views in this document: {}", if ids.is_empty() { "(none)".to_string() } else { ids.join(", ") }));
        return Err(d);
    };
    let vp = view_params(vv, model)?;
    let mut diags: Vec<Diag> = vec![];
    let base = if vp.kind == "iso" { crate::iso::build_iso(model, &vp, style, &mut diags) } else { crate::section::build_section(model, &vp, style, &mut diags) };
    let crop = base.crop;
    let s = base.s;
    let mut items = base.items;

    // sheet info
    let meta = doc.get("meta");
    let mget = |k: &str| meta.and_then(|m| m.get(k)).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let jur = meta.and_then(|m| m.get("jurisdiction"));
    let code_basis = match jur {
        Some(j) => format!(
            "{} {}",
            j.get("code").and_then(|x| x.as_str()).unwrap_or(""),
            j.get("edition").and_then(|x| x.as_f64()).map(crate::num::fmt_num).unwrap_or_default()
        )
        .trim()
        .to_string(),
        None => String::new(),
    };
    let mut unverified = false;
    let lk = Lookup { comps: &model.comps, all_ids: &[] };
    let pending = annotate(&vp, &base.vis, model, &lk, style, &crop, s, &mut diags, &mut unverified, &mut items);
    items.extend(pending);

    let info = SheetInfo {
        number: vp.number.clone(),
        title: vp.title.clone(),
        scale_text: vp.scale_text.clone(),
        sheet: mget("sheet"),
        date: mget("date"),
        author: mget("author"),
        project: mget("project"),
        code_basis,
        unverified,
        doc_title: model.title.clone(),
    };
    let ab = crate::drawing::items_bounds(&items);
    let mut tcrop = crop;
    if !ab.is_empty() && ab.y0 < crop.y0 {
        tcrop.y0 = ab.y0;
    }
    title_items(style, &info, &tcrop, s, &vp.id, &mut items);

    {
        let mut texts: Vec<(String, String)> = vec![("view title".to_string(), vp.title.clone()), ("document title".to_string(), model.title.clone())];
        for an in &vp.annotations {
            let id = an.get("id").and_then(|x| x.as_str()).unwrap_or("");
            if let Some(t) = an.get("text").and_then(|x| x.as_str()) {
                texts.push((format!("annotation {}", id), t.to_string()));
            }
        }
        for (what, t) in texts {
            for c in crate::font::fold_report(&t).1 {
                diags.push(Diag::info("I_GLYPH", format!("{} contains '{}' (U+{:04X}), which the stroke font lacks; it is drawn as '?'. Use plain ASCII.", what, c, c as u32)).path(format!("views/{}", vp.id)));
            }
        }
    }
    let mut d = Drawing {
        doc: model.id.clone(),
        view: vp.id.clone(),
        kind: vp.kind.clone(),
        scale: s,
        bounds: crop,
        crop,
        items,
        diagnostics: diags,
        info,
    };
    d.compute_bounds();
    // fit check
    let pw = d.bounds.w() / s;
    let ph = d.bounds.h() / s;
    let aw = style.sheet_w - 2.0 * style.margin;
    let ah = style.sheet_h - 2.0 * style.margin - style.title_block_h;
    if pw > aw + 1e-6 || ph > ah + 1e-6 {
        d.diagnostics.push(
            Diag::warn(
                "W_VIEW_FIT",
                format!(
                    "view {} with notes and title needs {} x {} paper inches but the sheet area is {} x {} at scale {}.",
                    vp.id,
                    crate::num::fmt_num(pw),
                    crate::num::fmt_num(ph),
                    crate::num::fmt_num(aw),
                    crate::num::fmt_num(ah),
                    vp.scale_text
                ),
            )
            .path(format!("views/{}/scale", vp.id))
            .fix(if vp.kind == "iso" { "shrink the crop or use fewer/shorter notes".to_string() } else { "use a smaller scale (e.g. 3/4\"=1'-0\") or shrink the crop / notes".to_string() }),
        );
    }
    Ok(d)
}

fn annotate(
    vp: &ViewParams,
    vis: &[crate::section::VisInfo],
    model: &Model,
    lk: &Lookup,
    style: &Style,
    crop: &Rect,
    s: f64,
    diags: &mut Vec<Diag>,
    unverified: &mut bool,
    base_items: &mut Vec<Item>,
) -> Vec<Item> {
    let mut notes: Vec<NoteIn> = vec![];
    let mut note_slot: Vec<Option<usize>> = vec![]; // per annotation: index into notes
    let mut per_annot: Vec<Vec<Item>> = vec![];
    for (k, an) in vp.annotations.iter().enumerate() {
        let id = an.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let ty = an.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let apath = format!("views/{}/annotations/{}", vp.id, id);
        let mut its: Vec<Item> = vec![];
        let mut slot = None;
        match ty {
            "note" => {
                let text = an.get("text").and_then(|x| x.as_str()).unwrap_or("");
                let cites: Vec<Value> = an.get("cite").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                let (t, unv) = note_text(style, text, &cites);
                if unv {
                    *unverified = true;
                }
                let target = an.get("target").and_then(|x| x.as_str()).unwrap_or("");
                let landing: Option<Pt> = if let Some(at) = an.get("at") {
                    match lk.point_value(at) {
                        Ok(p) => Some(p),
                        Err(d) => {
                            diags.push(d.path(format!("{}/at", apath)).id(id.clone()));
                            None
                        }
                    }
                } else if vp.kind == "iso" {
                    crate::iso::iso_landing(target, vis, model, crop)
                } else {
                    target_landing(target, vis, model, crop)
                };
                if !target.is_empty() && an.get("at").is_none() {
                    // unknown component?
                    let cid = target.split(['.', '#']).next().unwrap_or(target);
                    if model.comp(cid).is_none() {
                        let ids: Vec<&str> = model.comps.iter().map(|c| c.id.as_str()).collect();
                        let mut d = Diag::error("E_REF_UNKNOWN", format!("note \"{}\" target \"{}\" is not a component.", id, target)).id(id.clone()).path(format!("{}/target", apath));
                        d = d.fix(match crate::diag::nearest(cid, ids.iter().copied()) {
                            Some(n) => format!("did you mean \"{}\"?", n),
                            None => format!("components: {}", ids.join(", ")),
                        });
                        diags.push(d);
                        per_annot.push(its);
                        note_slot.push(None);
                        continue;
                    }
                }
                let place = an.get("place").and_then(crate::resolve::lit_pt);
                match landing {
                    Some(l) => {
                        slot = Some(notes.len());
                        notes.push(NoteIn { id: id.clone(), text: t, landing: l, place });
                    }
                    None => {
                        if an.get("at").is_none() {
                            diags.push(warn_target(&id, &vp.id, target));
                        }
                    }
                }
            }
            "dim" => {
                let from = an.get("from").map(|v| lk.point_value(v));
                let to = an.get("to").map(|v| lk.point_value(v));
                match (from, to) {
                    (Some(Ok(a)), Some(Ok(b))) => {
                        let dir = an.get("dir").and_then(|x| x.as_str()).unwrap_or("h");
                        let off = an.get("offset").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        let text = an.get("text").and_then(|x| x.as_str());
                        if vp.kind == "iso" {
                            // dimensions are 2D-view annotations
                        } else {
                            dim_items(style, &id, a, b, dir, off, text, s, &mut its);
                        }
                    }
                    (Some(Err(d)), _) => diags.push(d.id(id.clone()).path(format!("{}/from", apath))),
                    (_, Some(Err(d))) => diags.push(d.id(id.clone()).path(format!("{}/to", apath))),
                    _ => {}
                }
            }
            "label" => {
                let text = an.get("text").and_then(|x| x.as_str()).unwrap_or("");
                match an.get("at").map(|v| lk.point_value(v)) {
                    Some(Ok(p)) => {
                        let off = an.get("offset").and_then(crate::resolve::lit_pt).unwrap_or(pt(0.0, 0.0));
                        let q = p + off;
                        if vp.kind == "iso" {
                            if let Some(pp) = crate::iso::project_point(vp, model, q) {
                                label_items(style, &id, text, pp, s, &mut its);
                            }
                        } else {
                            label_items(style, &id, text, q, s, &mut its);
                        }
                    }
                    Some(Err(d)) => diags.push(d.id(id.clone()).path(format!("{}/at", apath))),
                    None => {}
                }
            }
            _ => {}
        }
        let _ = k;
        per_annot.push(its);
        note_slot.push(slot);
    }
    let mut note_items: Vec<Vec<Item>> = vec![];
    let mut per_annot = per_annot;
    for its in per_annot.iter_mut() {
        slide_dim_texts(its, base_items, s);
    }
    let mut ext = *crop;
    let mut knock: Vec<Vec<Pt>> = vec![];
    let mut legend: Vec<(String, String, String)> = vec![];
    let mut obstacles: Vec<Vec<Pt>> = vec![];
    for its in &per_annot {
        ext.union(&crate::drawing::items_bounds(its));
        for it in its {
            if let Some(poly) = text_poly(it, 0.03 * s) {
                obstacles.push(poly);
            }
            if let Some(poly) = text_poly(it, 0.02 * s) {
                knock.push(poly);
            }
        }
    }
    let keynote = style.notes_mode == "keynote";
    let mut laid = notes.iter().map(|n| NoteIn { id: n.id.clone(), text: n.text.clone(), landing: n.landing, place: n.place }).collect::<Vec<_>>();
    if keynote {
        for (k, n) in laid.iter_mut().enumerate() {
            n.text = (k + 1).to_string();
        }
    }
    layout_notes(&laid, style, crop, &ext, &obstacles, s, &vp.notes_side, if keynote { Some(0.14 * s) } else { None }, &mut note_items);
    if keynote {
        legend = notes.iter().enumerate().map(|(k, n)| ((k + 1).to_string(), n.text.clone(), n.id.clone())).collect();
    }
    knock_hatch(base_items, &knock);
    let mut out = vec![];
    for (k, its) in per_annot.into_iter().enumerate() {
        out.extend(its);
        if let Some(sl) = note_slot[k] {
            out.extend(note_items[sl].drain(..));
        }
    }
    if !legend.is_empty() {
        // legend block under the view: "<n>  TEXT" lines, wrapped at twice the note width
        let h = style.text_height * s;
        let pitch = h * style.line_spacing;
        let mut b = crate::drawing::items_bounds(&out);
        b.union(crop);
        let mut y = b.y0 - 0.3 * s - h;
        let x = crop.x0;
        out.push(text_item(style, "notes", "anno", "legend", "KEYNOTES", x, y, h * 1.15, 0.0, "left", "baseline"));
        y -= pitch * 1.3;
        for (num, text, _id) in &legend {
            let lines = crate::font::wrap(text, style.wrap_chars * 2);
            for (j, line) in lines.iter().enumerate() {
                let t = if j == 0 { format!("{}  {}", num, line) } else { format!("    {}", line) };
                out.push(text_item(style, "notes", "anno", "legend", &t, x, y, h, 0.0, "left", "baseline"));
                y -= pitch;
            }
        }
    }
    out
}


/// Remove hatch line parts that fall inside text boxes (dimension text and labels read cleanly over hatch).
fn knock_hatch(items: &mut [Item], boxes: &[Vec<Pt>]) {
    if boxes.is_empty() {
        return;
    }
    for it in items.iter_mut() {
        if let Item::Hatch { lines, .. } = it {
            let mut out: Vec<[f64; 4]> = Vec::with_capacity(lines.len());
            'line: for l in lines.iter() {
                let (a, b) = (pt(l[0], l[1]), pt(l[2], l[3]));
                let mut cuts: Vec<(f64, f64)> = vec![];
                for bx in boxes {
                    if let Some(iv) = convex_interval(a, b, bx) {
                        cuts.push(iv);
                    }
                }
                if cuts.is_empty() {
                    out.push(*l);
                    continue;
                }
                cuts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
                let mut t = 0.0;
                let degenerate = a.near(b, 1e-12);
                for (c0, c1) in cuts {
                    if degenerate {
                        continue 'line;
                    }
                    if c0 > t + 1e-9 {
                        let (p, q) = (a.lerp(b, t), a.lerp(b, c0));
                        out.push([p.x, p.y, q.x, q.y]);
                    }
                    t = t.max(c1);
                }
                if degenerate {
                    continue;
                }
                if t < 1.0 - 1e-9 {
                    let (p, q) = (a.lerp(b, t), b);
                    out.push([p.x, p.y, q.x, q.y]);
                }
            }
            *lines = out;
        }
    }
}

/// Parameter interval of segment a->b inside a convex polygon (any winding); dots test containment.
fn convex_interval(a: Pt, b: Pt, poly: &[Pt]) -> Option<(f64, f64)> {
    let area = poly_area(poly);
    let sgn = if area >= 0.0 { 1.0 } else { -1.0 };
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = b - a;
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let e = q - p;
        // inside: cross(e, x - p) * sgn >= 0
        let num = e.cross(a - p) * sgn;
        let den = e.cross(d) * sgn;
        if den.abs() < 1e-15 {
            if num < 0.0 {
                return None;
            }
        } else {
            let t = -num / den;
            if den > 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, t1))
}

/// Slide dimension text along its dimension line until it no longer sits on drawing linework.
fn slide_dim_texts(its: &mut [Item], base: &[Item], s: f64) {
    let mut strokes: Vec<(Pt, Pt)> = vec![];
    for it in base {
        if let Item::Path { pen, pts, closed, .. } = it {
            if !matches!(pen.as_str(), "cut" | "profile" | "beyond" | "steel" | "rebar" | "frame") {
                continue;
            }
            let fl = flatten_poly(pts, 0.01);
            for w in fl.windows(2) {
                strokes.push((w[0], w[1]));
            }
            if *closed && fl.len() > 2 {
                strokes.push((fl[fl.len() - 1], fl[0]));
            }
        }
    }
    let hits = |poly: &[Pt]| -> bool {
        let mut bb = Rect::empty();
        for q in poly {
            bb.add(*q);
        }
        strokes.iter().any(|(a, b)| {
            let sb = Rect::new(a.x, a.y, b.x, b.y);
            sb.overlaps(&bb, 0.0) && {
                let sg = Seg::Line(*a, *b);
                let n = poly.len();
                (0..n).any(|i| !intersect(&sg, &Seg::Line(poly[i], poly[(i + 1) % n])).is_empty()) || poly_contains(poly, *a)
            }
        })
    };
    for it in its.iter_mut() {
        let (is_dim, rot, h) = match it {
            Item::Text { pen, rot, h, .. } => (pen == "dim", *rot, *h),
            _ => (false, 0.0, 0.0),
        };
        if !is_dim {
            continue;
        }
        let pad = 0.015 * s;
        match text_poly(it, pad) {
            Some(p) if hits(&p) => {}
            _ => continue,
        }
        let u = pt(rot.to_radians().cos(), rot.to_radians().sin());
        let step = h * 1.2;
        let mut found: Option<(f64, f64)> = None;
        if let Item::Text { x, y, .. } = it {
            let (ox, oy) = (*x, *y);
            'search: for k in 1..=10 {
                for sign in [1.0, -1.0] {
                    let (nx, ny) = (ox + u.x * step * k as f64 * sign, oy + u.y * step * k as f64 * sign);
                    let mut tmp = Item::Text { layer: String::new(), pen: String::new(), src: String::new(), s: String::new(), x: nx, y: ny, h, rot, align: String::new(), valign: String::new() };
                    if let (Item::Text { s: ts, align: ta, valign: tv, .. }, Item::Text { s: ss, align: sa, valign: sv, .. }) = (&mut tmp, &*it) {
                        *ts = ss.clone();
                        *ta = sa.clone();
                        *tv = sv.clone();
                    }
                    if let Some(poly) = text_poly(&tmp, pad) {
                        if !hits(&poly) {
                            found = Some((nx, ny));
                            break 'search;
                        }
                    }
                }
            }
        }
        if let (Some((nx, ny)), Item::Text { x, y, .. }) = (found, it) {
            *x = nx;
            *y = ny;
        }
    }
}
