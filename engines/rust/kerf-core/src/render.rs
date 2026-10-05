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
    let lk = Lookup { comps: &model.comps };
    let pending = annotate(&vp, &base.vis, model, &lk, style, &crop, s, &mut diags, &mut unverified);
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
    layout_notes(&notes, style, crop, s, &vp.notes_side, &mut note_items);
    let mut out = vec![];
    for (k, its) in per_annot.into_iter().enumerate() {
        out.extend(its);
        if let Some(sl) = note_slot[k] {
            out.extend(note_items[sl].drain(..));
        }
    }
    out
}
