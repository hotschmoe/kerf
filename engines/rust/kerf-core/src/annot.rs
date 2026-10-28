//! Annotations (SPEC 6, 16): leader notes with deterministic column layout, dimensions, labels, and the title block under a view.
//!
//! This file holds the layout context (`Env`) and the main entry `annotate` (parse, route, repair, assemble); the parts live in
//! `annot/`: `text` (glyph folding, wrapping, text boxes), `notes` (landing, obstacles, routing, citations), `dims` (one
//! dimension and the stacking of colliding ones), `repair` (the bounded repair loop), `knockout` (hatch under text) and
//! `title` (the title bubble). `shape` has the landing geometry shared with the iso view.

pub mod dims;
pub mod knockout;
pub mod notes;
pub mod repair;
pub mod text;
pub mod title;

use crate::drawing::Item;
use crate::font::Font;
use crate::geom::{Box, V2};
use crate::iso::Iso;
use crate::json::Value;
use crate::model::{self, Diags, Level};
use crate::route;
use crate::scene::{self, Scene};
use crate::section::Section;
use crate::style::Style;
use crate::view::{Kind, ViewSpec};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub use crate::shape::Shape;
pub use dims::polys_overlap;
pub use notes::report_hits;
pub use text::{ascii_fold, items_box, text_poly};
pub use title::{title_items, SheetInfo};

use dims::DimSpec;
use notes::{LabelSpec, Meta, MetaKind, NoteIn};
use repair::{Best, DimLab};

pub enum Landing<'a, 's> {
    Section(&'a mut Section<'s>),
    Iso(&'a Iso),
}

pub struct Env<'a, 's> {
    pub style: &'a Style,
    pub font: &'a Font,
    pub scene: &'a Scene<'a>,
    pub spec: &'a ViewSpec<'a>,
    /// Drawing scale factor (the Zig `S`).
    pub s: f64,
    pub crop: Box,
    pub diags: &'a Diags,
    pub landing: Landing<'a, 's>,
    pub unverified: bool,
    pub unknown_glyph: bool,
    /// Extents of the annotation groups (model units), filled by `annotate` (used by the view-fit check).
    pub notes_box: Box,
    pub dims_box: Box,
    pub labels_box: Box,
    /// Per-annotation extents (model units) for the view-fit diagnostic.
    pub ann_boxes: Vec<AnnBox>,
    /// Layout effort shared by every pass and trial of one view build (None: each routing call gets its own default budget).
    pub work: Option<&'a route::Work>,
}

impl<'a, 's> Env<'a, 's> {
    pub fn new(style: &'a Style, font: &'a Font, scene: &'a Scene<'a>, spec: &'a ViewSpec<'a>, s: f64, crop: Box, diags: &'a Diags, landing: Landing<'a, 's>) -> Env<'a, 's> {
        Env {
            style,
            font,
            scene,
            spec,
            s,
            crop,
            diags,
            landing,
            unverified: false,
            unknown_glyph: false,
            notes_box: Box::default(),
            dims_box: Box::default(),
            labels_box: Box::default(),
            ann_boxes: Vec::new(),
            work: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnnKind {
    Note,
    Dim,
    Label,
}

#[derive(Clone, Debug)]
pub struct AnnBox {
    pub id: String,
    pub kind: AnnKind,
    pub bbox: Box,
    pub offset: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimDir {
    H,
    V,
    Aligned,
}

pub const DIM_VARIANTS: usize = 6;

const MAX_REPAIR_PASSES: usize = 8;

fn str_member<'v>(an: &'v Value, key: &str) -> Option<&'v str> {
    an.get(key).and_then(Value::str)
}

fn non_null<'v>(an: &'v Value, key: &str) -> Option<&'v Value> {
    an.get(key).filter(|v| !v.is_null())
}

/// Annotate a view. Returns the annotation items (notes interleaved after their annotation's own
/// items, in view order). Hatch lines in `base_items` are knocked out under dimension text and labels.
///
/// SPEC 20: dimensions are stacked (`stack_dims`), notes routed (`route::route`), and when leaders still hit
/// dimension text or labels, a bounded repair loop (at most 8 passes) pushes those dimensions out and
/// moves those labels, re-routing after each pass; the pass with the fewest hits wins.
pub fn annotate<'a>(env: &mut Env<'a, '_>, base_items: &mut [Item]) -> Vec<Item> {
    let spec: &'a ViewSpec<'a> = env.spec;
    let vid = spec.id;
    let mut notes: Vec<NoteIn<'a>> = Vec::new();
    let mut note_slot: Vec<Option<usize>> = Vec::new();
    let mut per: Vec<Vec<Item>> = Vec::new();
    let mut meta: Vec<Meta<'a>> = Vec::new();
    let mut dspecs: Vec<DimSpec<'a>> = Vec::new();
    let mut lspecs: Vec<LabelSpec<'a>> = Vec::new();
    let mut seen: Vec<&'a str> = Vec::new();
    let types = ["note", "dim", "label"];
    for (k, an) in spec.annotations.iter().enumerate() {
        let mut slot: Option<usize> = None;
        let mut cur_meta = Meta::default();
        'body: {
            let Some(id) = str_member(an, "id") else {
                env.diags.add(Level::Error, "E_PARAM", None, Some(&format!("views/{vid}/annotations/{k}")), format!("annotation {k} of view {vid} needs a string \"id\""));
                break 'body;
            };
            let apath = format!("views/{vid}/annotations/{id}");
            if seen.contains(&id) {
                env.diags.add(Level::Error, "E_DUP_ID", Some(id), Some(&apath), format!("duplicate annotation id '{id}' in view {vid}"));
                break 'body;
            }
            seen.push(id);
            let ty = str_member(an, "type").unwrap_or("");
            match ty {
                "note" => {
                    let Some(text0) = str_member(an, "text") else {
                        env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&apath), format!("note '{id}' needs a string \"text\""));
                        break 'body;
                    };
                    if notes::text_too_long(env, "note", id, &apath, text0) {
                        break 'body;
                    }
                    let target = an.get("target").map_or("", |x| x.str().unwrap_or(""));
                    let text = scene::where_occurs_text(env.scene, target, text0);
                    let cites: &[Value] = an.get("cite").and_then(Value::arr).unwrap_or(&[]);
                    let full = notes::note_text(env, &text, cites);
                    let target = str_member(an, "target").unwrap_or("");
                    let mut landing: Option<Vec<V2>> = None;
                    let mut movable = true;
                    let at_v = non_null(an, "at");
                    if let Some(atv) = at_v {
                        if let Some(p) = env.scene.point(atv, id, &format!("{apath}/at")) {
                            landing = Some(alloc::vec![p]);
                        }
                        movable = false;
                    } else if target.is_empty() {
                        env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&apath), format!("note '{id}' needs a \"target\" (component id or comp.part) or an \"at\" point"));
                        break 'body;
                    } else {
                        let cid = target.find(['.', '#']).map_or(target, |d| &target[..d]);
                        if env.scene.find(cid).is_none() {
                            let ids = env.scene.comp_ids();
                            let hint = match model::nearest(cid, &ids) {
                                Some(n) => format!(" Did you mean '{n}'?"),
                                None => format!(" Components: {}", scene::join_ids(&ids)),
                            };
                            env.diags.add(Level::Error, "E_REF_UNKNOWN", Some(id), Some(&format!("{apath}/target")), format!("note '{id}' target '{target}' is not a component.{hint}"));
                            break 'body;
                        }
                        landing = notes::target_landing(env, target);
                    }
                    let mut place: Option<V2> = None;
                    if let Some(pv) = non_null(an, "place") {
                        match model::offset_pair_or_diag(env.diags, Some(id), &format!("{apath}/place"), "place", pv) {
                            Some(p) => place = Some(p),
                            None => break 'body,
                        }
                    }
                    let mut column: Option<route::Side> = None;
                    if let Some(cv) = non_null(an, "column") {
                        match cv.str().unwrap_or("") {
                            "left" => column = Some(route::Side::Left),
                            "right" => column = Some(route::Side::Right),
                            _ => {
                                let got = cv.str().map_or_else(|| "a non-string value".to_string(), |sv| format!("\"{sv}\""));
                                env.diags.add_fix(
                                    Level::Warning,
                                    "W_PARAM",
                                    Some(id),
                                    Some(&format!("{apath}/column")),
                                    format!("note '{id}' in view {vid}: \"column\" must be \"left\" or \"right\" (got {got}); ignored"),
                                    "use \"column\": \"left\" or \"right\", or omit it to let the layout choose (view notes_side)".to_string(),
                                );
                            }
                        }
                    }
                    if let Some(l) = landing {
                        slot = Some(notes.len());
                        notes.push(NoteIn { id, text: full, cands: l, movable, place, column });
                    } else if at_v.is_none() {
                        env.diags.add_fix(
                            Level::Warning,
                            "W_NOTE_TARGET",
                            Some(id),
                            Some(&apath),
                            format!("note '{id}' in view {vid}: target '{target}' is not visible in this view (outside the crop, behind the cut plane, or hidden). The note was not drawn."),
                            "move the view crop or cut_z so the target is visible, change target, or give the note an explicit \"at\" Ref".to_string(),
                        );
                    }
                }
                "dim" => {
                    if spec.kind == Kind::Iso {
                        break 'body;
                    }
                    let (Some(fv), Some(tv)) = (an.get("from"), an.get("to")) else {
                        env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&apath), format!("dim '{id}' needs \"from\" and \"to\" points (Refs)"));
                        break 'body;
                    };
                    let Some(from) = env.scene.point(fv, id, &format!("{apath}/from")) else { break 'body };
                    let Some(to) = env.scene.point(tv, id, &format!("{apath}/to")) else { break 'body };
                    let dir_s = str_member(an, "dir").unwrap_or("h");
                    let dir = match dir_s {
                        "h" => DimDir::H,
                        "v" => DimDir::V,
                        "aligned" => DimDir::Aligned,
                        _ => {
                            env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{apath}/dir")), format!("dim dir must be \"h\", \"v\" or \"aligned\" (got \"{dir_s}\")"));
                            break 'body;
                        }
                    };
                    let Some(off) = model::length_or_diag(env.diags, Some(id), &format!("{apath}/offset"), an.get("offset"), "dim offset", 0.0) else { break 'body };
                    let ds = DimSpec { k, id, from, to, dir, off0: off, text: str_member(an, "text") };
                    cur_meta = Meta { id, kind: MetaKind::Dim, axis: ds.axis(), off, off2: 0.0, owner: dspecs.len() };
                    dspecs.push(ds);
                }
                "label" => {
                    let Some(text) = str_member(an, "text") else {
                        env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&apath), format!("label '{id}' needs a string \"text\""));
                        break 'body;
                    };
                    if notes::text_too_long(env, "label", id, &apath, text) {
                        break 'body;
                    }
                    let Some(atv) = an.get("at") else {
                        env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&apath), format!("label '{id}' needs \"at\": a Ref or [x, y]"));
                        break 'body;
                    };
                    let Some(mut p) = env.scene.point(atv, id, &format!("{apath}/at")) else { break 'body };
                    let mut loff = V2::init(0.0, 0.0);
                    if let Some(ov) = non_null(an, "offset") {
                        let Some(o) = model::offset_pair_or_diag(env.diags, Some(id), &format!("{apath}/offset"), "label offset", ov) else { break 'body };
                        loff = o;
                        p = p.add(loff);
                    }
                    cur_meta = Meta { id, kind: MetaKind::Label, axis: V2::default(), off: loff.x, off2: loff.y, owner: lspecs.len() };
                    let base = match &mut env.landing {
                        Landing::Section(_) => Some(p),
                        Landing::Iso(iso) => iso.project(p),
                    };
                    lspecs.push(LabelSpec { k, id, text, base, off: loff });
                }
                _ => {
                    env.diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{apath}/type")), format!("annotation '{id}' has type \"{ty}\"; use one of {}", model::join_quoted(&types)));
                }
            }
        }
        per.push(Vec::new());
        note_slot.push(slot);
        meta.push(cur_meta);
    }
    let mut base_segs: Vec<[V2; 2]> = Vec::new();
    for it in base_items.iter() {
        if let Item::Path(p) = it {
            let pts = &p.pts;
            if pts.len() < 2 {
                continue;
            }
            for w in pts.windows(2) {
                base_segs.push([w[0].v(), w[1].v()]);
            }
            if p.closed {
                base_segs.push([pts[pts.len() - 1].v(), pts[0].v()]);
            }
        }
    }
    let dl = DimLab { dspecs: &dspecs, lspecs: &lspecs, base_segs: &base_segs };
    let mut dpush = alloc::vec![0.0f64; dspecs.len()];
    let mut lpush = alloc::vec![V2::init(0.0, 0.0); lspecs.len()];
    let prep = notes::prep_notes(env, &notes);

    // route, then repair what hits dimension text or labels (bounded, deterministic)
    let mut best: Option<Best<'a>> = None;
    let mut stale = 0usize;
    for pass in 0..MAX_REPAIR_PASSES {
        let rd = repair::render_dim_labels(env, &dl, &per, &meta, &dpush, &lpush);
        let r = notes::route_notes(env, &prep, &notes, rd.ext, &rd.obstacles, &rd.soft, pass > 0);
        let better = best.as_ref().is_none_or(|b| r.hits.len() < b.r.hits.len());
        if better {
            stale = 0;
        } else {
            stale += 1;
        }
        // stop when clean, or when two passes in a row did not get better
        let stop = r.hits.is_empty() || stale >= 2;
        let mut changed = false;
        if !stop {
            let mut touched_d = alloc::vec![false; dspecs.len()];
            let mut touched_l = alloc::vec![false; lspecs.len()];
            for ht in &r.hits {
                if ht.kind != route::HitKind::Dim && ht.kind != route::HitKind::Label {
                    continue;
                }
                let Some(o) = rd.obstacles.get(ht.other) else { continue };
                if o.kind == route::ObstKind::Dim {
                    let Some(t) = touched_d.get_mut(o.owner) else { continue };
                    if *t {
                        continue;
                    }
                    *t = true;
                    if repair::repair_dim(env, o, &r.leaders, prep.g.h, &mut dpush, &dspecs) {
                        changed = true;
                    }
                } else {
                    let Some(t) = touched_l.get_mut(o.owner) else { continue };
                    if *t {
                        continue;
                    }
                    *t = true;
                    if repair::repair_label(o, &rd.obstacles, &r.leaders, prep.g.h, &mut lpush, &base_segs) {
                        changed = true;
                    }
                }
            }
        }
        if better {
            best = Some(Best { r, rd });
        }
        if stop || !changed {
            break;
        }
    }
    let Some(fin) = best else { return Vec::new() };
    let rd = fin.rd;
    notes::report_hits(env, &notes, &rd.obstacles, &fin.r, &prep.g);
    for (its, m) in rd.per.iter().zip(&rd.meta) {
        let bx = items_box(env.font, its);
        match m.kind {
            MetaKind::Dim => {
                env.dims_box.add_box(bx);
                env.ann_boxes.push(AnnBox { id: m.id.to_string(), kind: AnnKind::Dim, bbox: bx, offset: m.off });
            }
            MetaKind::Label => {
                env.labels_box.add_box(bx);
                env.ann_boxes.push(AnnBox { id: m.id.to_string(), kind: AnnKind::Label, bbox: bx, offset: 0.0 });
            }
            MetaKind::None => {}
        }
    }
    let mut outs: Vec<Vec<Item>> = (0..notes.len()).map(|_| Vec::new()).collect();
    notes::emit_notes(env, &notes, &prep, &fin.r, &mut outs);
    for (o, n) in outs.iter().zip(&notes) {
        let bx = items_box(env.font, o);
        env.notes_box.add_box(bx);
        env.ann_boxes.push(AnnBox { id: n.id.to_string(), kind: AnnKind::Note, bbox: bx, offset: 0.0 });
    }
    knockout::knock_hatch(base_items, &rd.knock);
    let mut result: Vec<Item> = Vec::new();
    for (k, its) in rd.per.into_iter().enumerate() {
        result.extend(its);
        if let Some(Some(sl)) = note_slot.get(k) {
            if let Some(o) = outs.get_mut(*sl) {
                result.append(o);
            }
        }
    }
    if env.style.notes_mode_keynote && !notes.is_empty() {
        notes::legend_items(env, &notes, &mut result);
    }
    result
}

