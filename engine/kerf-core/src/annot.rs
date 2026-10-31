//! Annotations (SPEC 6, 16, 23): leader or keyed notes with deterministic column layout, dimensions, labels, and the title
//! block under a view.
//!
//! This file holds the layout context (`Env`) and the main entry `annotate` (parse, route, repair, assemble); the parts live in
//! `annot/`: `text` (glyph folding, wrapping, text boxes), `notes` (landing, obstacles, routing, citations), `dims` (one
//! dimension and the stacking of colliding ones), `repair` (the bounded repair loop), `knockout` (hatch under text) and
//! `title` (the title bubble). `shape` has the landing geometry shared with the iso view.

pub mod dims;
pub mod knockout;
pub mod notes;
pub mod repair;
pub mod table;
pub mod text;
pub mod title;
pub mod weld;

use crate::drawing::Item;
use crate::font::Font;
use crate::geom::{Box, V2};
use crate::iso::Iso;
use crate::json::Value;
use crate::model::{self, Diags, Level};
use crate::num;
use crate::route;
use crate::scene::{self, Scene};
use crate::section::Section;
use crate::style::Style;
use crate::view::{Kind, NotesSide, NotesStyle, ViewSpec};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub use text::{ascii_fold, items_box};
pub use title::{SheetInfo, title_items};

use dims::DimSpec;
use notes::{LabelSpec, Meta, MetaKind, NoteIn};
use repair::DimLab;

pub enum Landing<'a, 's> {
    Section(&'a mut Section<'s>),
    Iso(&'a Iso),
}

pub struct Env<'a, 's> {
    pub style: &'a Style,
    pub font: &'a Font,
    pub scene: &'a Scene<'a>,
    pub spec: &'a ViewSpec<'a>,
    /// Drawing scale factor (`S` in the layout notes).
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        style: &'a Style,
        font: &'a Font,
        scene: &'a Scene<'a>,
        spec: &'a ViewSpec<'a>,
        s: f64,
        crop: Box,
        diags: &'a Diags,
        landing: Landing<'a, 's>,
    ) -> Env<'a, 's> {
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

    /// The view's `notes_style`, else the style's `notes.mode`.
    pub fn notes_style(&self) -> NotesStyle {
        if self.spec.kind == Kind::Schedule {
            return NotesStyle::List;
        }
        self.spec.notes_style.unwrap_or(self.style.notes_mode)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnnKind {
    Note,
    Dim,
    Label,
    /// The keyed notes list (one box per view).
    List,
    Table,
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

fn str_member<'v>(an: &'v Value, key: &str) -> Option<&'v str> {
    an.get(key).and_then(Value::str)
}

fn non_null<'v>(an: &'v Value, key: &str) -> Option<&'v Value> {
    an.get(key).filter(|v| !v.is_null())
}

/// What a view's annotation list yields: the notes, dimensions and labels to lay out, and per annotation (in view order)
/// its items so far, its note slot and its bookkeeping.
struct Gathered<'a> {
    notes: Vec<NoteIn<'a>>,
    note_slot: Vec<Option<usize>>,
    per: Vec<Vec<Item>>,
    meta: Vec<Meta<'a>>,
    dspecs: Vec<DimSpec<'a>>,
    lspecs: Vec<LabelSpec<'a>>,
    welds: Vec<(usize, Vec<Item>)>,
    tables: Vec<table::Table<'a>>,
    /// Per note: drawn only in the DETAIL NOTES list (list views; keyed notes without `target` or `at`).
    listed: Vec<bool>,
}

/// The id of annotation `k`, or a diagnostic and `None` when it has none or repeats an earlier one.
fn annotation_id<'a>(env: &Env<'a, '_>, an: &'a Value, k: usize, seen: &mut Vec<&'a str>) -> Option<&'a str> {
    let vid = env.spec.id;
    let Some(id) = str_member(an, "id") else {
        env.diags.add(
            Level::Error,
            "E_PARAM",
            None,
            Some(&format!("views/{vid}/annotations/{k}")),
            format!("annotation {k} of view {vid} needs a string \"id\""),
        );
        return None;
    };
    if seen.contains(&id) {
        env.diags.add(
            Level::Error,
            "E_DUP_ID",
            Some(id),
            Some(&format!("views/{vid}/annotations/{id}")),
            format!("duplicate annotation id '{id}' in view {vid}"),
        );
        return None;
    }
    seen.push(id);
    Some(id)
}

/// The component (index, instance, part) an `at` point is given relative to: a Ref, or the `ref` of `{ref, offset}`;
/// for a literal `[x, y]` (or `@origin`), the note's `target` (`comp`, `comp.part`, `comp#k`) when it has one.
fn at_owner<'v>(env: &Env, atv: &'v Value, target: &'v str) -> Option<(u32, u32, Option<&'v str>)> {
    let text = atv.str().or_else(|| atv.get("ref").and_then(Value::str));
    if let Some(r) = text.and_then(|t| scene::parse_ref(t).ok()).filter(|r| !r.origin) {
        let c = env.scene.find(r.comp)?;
        return Some((c.index, r.inst, r.part));
    }
    if target.is_empty() {
        return None;
    }
    let (head, part) = match target.split_once('.') {
        Some((h, p)) => (h, Some(p)),
        None => (target, None),
    };
    let (cid, inst) = match head.split_once('#') {
        Some((c, k)) => (c, k.parse::<u32>().ok()?),
        None => (head, 0),
    };
    Some((env.scene.find(cid)?.index, inst, part))
}

/// Drawing position of the model point `p` resolved from `atv`: itself in a section; in an iso, projected from the
/// face nearest the viewer of the component it refers to (SPEC 25.2).
fn at_drawn(env: &Env, atv: &Value, target: &str, p: V2) -> V2 {
    match &env.landing {
        Landing::Section(_) => p,
        Landing::Iso(iso) => iso.project_on(at_owner(env, atv, target), p),
    }
}

/// A note's `key` (keyed views): a whole number from 1 to 999; anything else warns and is ignored.
fn read_key(env: &Env, an: &Value, id: &str, apath: &str) -> Option<u32> {
    let kv = non_null(an, "key")?;
    let whole = kv.num().and_then(|k| num::to_int::<u32>(k).filter(|&n| f64::from(n) == k));
    if let Some(n) = whole.filter(|n| (1..=999).contains(n)) {
        return Some(n);
    }
    env.diags.add_fix(
        Level::Warning,
        "W_PARAM",
        Some(id),
        Some(&format!("{apath}/key")),
        format!(
            "note '{id}' in view {}: \"key\" must be a whole number from 1 to 999 (got {}); ignored, the note is numbered in order",
            env.spec.id,
            model::kind_or_text(kv)
        ),
        "use e.g. \"key\": 3, or remove it".to_string(),
    );
    None
}

/// The first note in the view with key `key` that has a text (keyed views: a note with a shared key may omit its text,
/// and then takes that note's text and, unless it has its own, its `cite`).
fn shared_key_note<'a>(env: &Env<'a, '_>, key: u32) -> Option<&'a Value> {
    env.spec
        .annotations
        .iter()
        .find(|an| non_null(an, "key").and_then(Value::num) == Some(f64::from(key)) && str_member(an, "text").is_some())
}

/// A note, or `None` (with a diagnostic) when it cannot be drawn.
fn read_note<'a>(env: &mut Env<'a, '_>, an: &'a Value, id: &'a str, apath: &str) -> Option<NoteIn<'a>> {
    let vid = env.spec.id;
    let style = env.notes_style();
    let key = if style == NotesStyle::Leader { None } else { read_key(env, an, id, apath) };
    let shared = if str_member(an, "text").is_none() { key.and_then(|k| shared_key_note(env, k)) } else { None };
    let Some(text0) = str_member(an, "text").or_else(|| shared.and_then(|sn| str_member(sn, "text"))) else {
        let alt = if style == NotesStyle::Leader { "" } else { " (or a \"key\" shared with a note that has one)" };
        env.diags.add(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(apath),
            format!("note '{id}' needs a string \"text\"{alt}"),
        );
        return None;
    };
    if notes::text_too_long(env, "note", id, apath, text0) {
        return None;
    }
    let target = an.get("target").map_or("", |x| x.str().unwrap_or(""));
    let text = scene::where_occurs_text(env.scene, target, text0);
    let cite_src = if an.get("cite").is_some() { an } else { shared.unwrap_or(an) };
    let cites: &[Value] = cite_src.get("cite").and_then(Value::arr).unwrap_or(&[]);
    let full = notes::note_text(env, &text, cites);
    let body = text::upper_if(env, &text).into_owned();
    let untargeted = str_member(an, "target").is_none() && non_null(an, "at").is_none();
    if style == NotesStyle::List || (style == NotesStyle::Keyed && untargeted) {
        return Some(NoteIn {
            id,
            text: full,
            body,
            cands: Vec::new(),
            movable: false,
            place: None,
            column: None,
            key,
        });
    }
    let target = str_member(an, "target").unwrap_or("");
    let mut landing: Option<Vec<V2>> = None;
    let mut movable = true;
    let at_v = non_null(an, "at");
    if let Some(atv) = at_v {
        if let Some(p) = env.scene.point(atv, id, &format!("{apath}/at")) {
            landing = Some(alloc::vec![at_drawn(env, atv, target, p)]);
        }
        movable = false;
    } else if target.is_empty() {
        env.diags.add(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(apath),
            format!("note '{id}' needs a \"target\" (component id or comp.part) or an \"at\" point; a general note without a leader belongs in a view with \"notes_style\": \"keyed\" (it is then listed with no tag) or \"list\""),
        );
        return None;
    } else {
        let cid = target.find(['.', '#']).map_or(target, |d| &target[..d]);
        if env.scene.find(cid).is_none() {
            let ids = env.scene.comp_ids();
            let hint = match model::nearest(cid, &ids) {
                Some(n) => format!(" Did you mean '{n}'?"),
                None => format!(" Components: {}", scene::join_ids(&ids)),
            };
            env.diags.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(id),
                Some(&format!("{apath}/target")),
                format!("note '{id}' target '{target}' is not a component.{hint}"),
            );
            return None;
        }
        landing = notes::target_landing(env, target);
    }
    let mut place: Option<V2> = None;
    if let Some(pv) = non_null(an, "place") {
        place = Some(model::offset_pair_or_diag(env.diags, Some(id), &format!("{apath}/place"), "place", pv)?);
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
                    "use \"column\": \"left\" or \"right\", or omit it to let the layout choose (view notes_side)"
                        .to_string(),
                );
            }
        }
    }
    if let Some(cands) = landing {
        return Some(NoteIn { id, text: full, body, cands, movable, place, column, key });
    }
    let base = target.split(['.', '#']).next().unwrap_or("");
    if at_v.is_none()
        && let Some(c) = env.scene.find(base).filter(|c| !env.spec.members.draws(c))
    {
        env.diags.add_fix(
            Level::Warning,
            "W_NOTE_TARGET",
            Some(id),
            Some(apath),
            format!("note '{id}' in view {vid}: target '{target}' is not drawn in this view (its \"views\" or the view's \"omit\" / \"only\" exclude '{}'). The note was not drawn.", c.id),
            format!("move the note to a view that draws '{}', target a member of view {vid}, or include '{}' in view {vid}", c.id, c.id),
        );
    } else if at_v.is_none() {
        env.diags.add_fix(
            Level::Warning,
            "W_NOTE_TARGET",
            Some(id),
            Some(apath),
            format!("note '{id}' in view {vid}: target '{target}' is not visible in this view (outside the crop, behind the cut plane, or hidden). The note was not drawn."),
            "move the view crop or cut_z so the target is visible, change target, or give the note an explicit \"at\" Ref".to_string(),
        );
    }
    None
}

/// `pin` on a dimension or label: true keeps it exactly where the author put it; anything but a boolean warns.
fn read_pin(env: &Env, an: &Value, id: &str, apath: &str) -> bool {
    match non_null(an, "pin") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(v) => {
            env.diags.add_fix(
                Level::Warning,
                "W_PARAM",
                Some(id),
                Some(&format!("{apath}/pin")),
                format!(
                    "'{id}' in view {}: \"pin\" must be true or false (got {}); ignored",
                    env.spec.id,
                    model::kind_or_text(v)
                ),
                "use \"pin\": true to keep it exactly where you put it, or remove it".to_string(),
            );
            false
        }
    }
}

fn read_dim<'a>(env: &Env<'a, '_>, an: &'a Value, k: usize, id: &'a str, apath: &str) -> Option<DimSpec<'a>> {
    let (Some(fv), Some(tv)) = (an.get("from"), an.get("to")) else {
        env.diags.add(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(apath),
            format!("dim '{id}' needs \"from\" and \"to\" points (Refs)"),
        );
        return None;
    };
    let from = env.scene.point(fv, id, &format!("{apath}/from"))?;
    let to = env.scene.point(tv, id, &format!("{apath}/to"))?;
    let dir_s = str_member(an, "dir").unwrap_or("h");
    let dir = match dir_s {
        "h" => DimDir::H,
        "v" => DimDir::V,
        "aligned" => DimDir::Aligned,
        _ => {
            env.diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/dir")),
                format!("dim dir must be \"h\", \"v\" or \"aligned\" (got \"{dir_s}\")"),
            );
            return None;
        }
    };
    let off =
        model::length_or_diag(env.diags, Some(id), &format!("{apath}/offset"), an.get("offset"), "dim offset", 0.0)?;
    let pin = read_pin(env, an, id, apath);
    Some(DimSpec { k, id, from, to, dir, off0: off, text: str_member(an, "text"), pin })
}

/// A label and its offset from the `at` point.
fn read_label<'a>(
    env: &mut Env<'a, '_>,
    an: &'a Value,
    k: usize,
    id: &'a str,
    apath: &str,
) -> Option<(LabelSpec<'a>, V2)> {
    let Some(text) = str_member(an, "text") else {
        env.diags.add(Level::Error, "E_PARAM", Some(id), Some(apath), format!("label '{id}' needs a string \"text\""));
        return None;
    };
    if notes::text_too_long(env, "label", id, apath, text) {
        return None;
    }
    let Some(atv) = an.get("at") else {
        env.diags.add(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(apath),
            format!("label '{id}' needs \"at\": a Ref or [x, y]"),
        );
        return None;
    };
    let mut p = env.scene.point(atv, id, &format!("{apath}/at"))?;
    let mut off = V2::init(0.0, 0.0);
    if let Some(ov) = non_null(an, "offset") {
        off = model::offset_pair_or_diag(env.diags, Some(id), &format!("{apath}/offset"), "label offset", ov)?;
        p = p.add(off);
    }
    let align = match non_null(an, "align").map(|v| v.str()) {
        None | Some(Some("center")) => crate::drawing::Align::Center,
        Some(Some("left")) => crate::drawing::Align::Left,
        Some(Some("right")) => crate::drawing::Align::Right,
        Some(_) => {
            env.diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/align")),
                format!(
                    "label '{id}': \"align\" must be \"left\", \"center\" or \"right\" (got {})",
                    model::kind_or_text(an.get("align").unwrap_or(&Value::Null))
                ),
                "left puts the start of each line on `at`, right its end, center (the default) its middle".to_string(),
            );
            return None;
        }
    };
    let base = Some(at_drawn(env, atv, "", p));
    let pin = read_pin(env, an, id, apath);
    Some((LabelSpec { k, id, text, align, base, off, pin }, off))
}

/// Reads every annotation of the view; the invalid ones are diagnosed and skipped.
fn gather<'a>(env: &mut Env<'a, '_>) -> Gathered<'a> {
    let spec: &'a ViewSpec<'a> = env.spec;
    let vid = spec.id;
    let mut g = Gathered {
        notes: Vec::new(),
        note_slot: Vec::new(),
        per: Vec::new(),
        meta: Vec::new(),
        dspecs: Vec::new(),
        lspecs: Vec::new(),
        welds: Vec::new(),
        tables: Vec::new(),
        listed: Vec::new(),
    };
    let mut seen: Vec<&'a str> = Vec::new();
    let types = ["note", "dim", "label", "weld", "table"];
    for (k, an) in spec.annotations.iter().enumerate() {
        let mut slot: Option<usize> = None;
        let mut cur_meta = Meta::default();
        if let Some(id) = annotation_id(env, an, k, &mut seen) {
            let apath = format!("views/{vid}/annotations/{id}");
            match str_member(an, "type").unwrap_or("") {
                ty @ ("dim" | "weld") if spec.kind == Kind::Schedule => {
                    env.diags.add_fix(
                        Level::Error,
                        "E_PARAM",
                        Some(id),
                        Some(&format!("{apath}/type")),
                        format!("{ty} '{id}' is in schedule view {vid}, which draws no components: a {ty} needs a section view"),
                        format!("move '{id}' to a section view, or remove it (a schedule view holds tables, notes and labels)"),
                    );
                }
                "table" => {
                    if let Some(t) = table::read(env, an, k, id, &apath) {
                        g.tables.push(t);
                    }
                }
                "note" => {
                    if let Some(note) = read_note(env, an, id, &apath) {
                        slot = Some(g.notes.len());
                        let ns = env.notes_style();
                        let untargeted = str_member(an, "target").is_none() && non_null(an, "at").is_none();
                        g.listed.push(ns == NotesStyle::List || (ns == NotesStyle::Keyed && untargeted));
                        g.notes.push(note);
                    }
                }
                "dim" if spec.kind != Kind::Iso => {
                    if let Some(ds) = read_dim(env, an, k, id, &apath) {
                        cur_meta = Meta {
                            id,
                            kind: MetaKind::Dim,
                            axis: ds.axis(),
                            off: ds.off0,
                            off2: 0.0,
                            owner: g.dspecs.len(),
                        };
                        g.dspecs.push(ds);
                    }
                }
                "dim" => {}
                "weld" if spec.kind != Kind::Iso => {
                    if let Some(ws) = weld::read(env, an, id, &apath) {
                        let its = weld::items(env, &ws);
                        env.labels_box.add_box(items_box(env.font, &its));
                        g.welds.push((k, its));
                    }
                }
                "weld" => {}
                "label" => {
                    if let Some((ls, off)) = read_label(env, an, k, id, &apath) {
                        cur_meta = Meta {
                            id,
                            kind: MetaKind::Label,
                            axis: V2::default(),
                            off: off.x,
                            off2: off.y,
                            owner: g.lspecs.len(),
                        };
                        g.lspecs.push(ls);
                    }
                }
                ty => {
                    env.diags.add(
                        Level::Error,
                        "E_PARAM",
                        Some(id),
                        Some(&format!("{apath}/type")),
                        format!("annotation '{id}' has type \"{ty}\"; use one of {}", model::join_quoted(&types)),
                    );
                }
            }
        }
        g.per.push(Vec::new());
        g.note_slot.push(slot);
        g.meta.push(cur_meta);
    }
    g
}

/// Annotate a view. Returns the annotation items (notes interleaved after their annotation's own
/// items, in view order). Hatch lines in `base_items` are knocked out under dimension text and labels.
///
/// SPEC 20: dimensions are stacked (`stack_dims`), notes routed (`route::route`), and when leaders still hit
/// dimension text or labels, a bounded repair loop (at most 8 passes) pushes those dimensions out and
/// moves those labels, re-routing after each pass; the pass with the fewest hits wins.
pub fn annotate<'a>(env: &mut Env<'a, '_>, base_items: &mut [Item]) -> Vec<Item> {
    let Gathered { notes, note_slot, per, meta, dspecs, lspecs, mut welds, tables, listed } = gather(env);
    let style = env.notes_style();
    if style == NotesStyle::Leader && env.spec.notes_side == NotesSide::Above {
        let vid = env.spec.id;
        env.diags.add_fix(
            Level::Warning,
            "W_PARAM",
            Some(vid),
            Some(&format!("views/{vid}/notes_side")),
            format!("view {vid}: notes_side \"above\" places the DETAIL NOTES list of keyed notes; leader notes use \"both\" here"),
            "set \"notes_style\": \"keyed\" on the view, or notes_side to right, left or both".to_string(),
        );
    }
    let numbers = if style == NotesStyle::Leader { Vec::new() } else { notes::key_numbers(&notes) };
    let routed_idx: Vec<usize> = (0..notes.len()).filter(|&i| !listed[i]).collect();
    let routed_notes: Vec<NoteIn<'a>> = routed_idx.iter().map(|&i| notes[i].clone()).collect();
    let routed_numbers: Vec<u32> = routed_idx.iter().filter_map(|&i| numbers.get(i).copied()).collect();
    let routed: &[NoteIn<'a>] = &routed_notes;
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
    let prep = notes::prep_notes(env, routed, &routed_numbers);

    let best = repair::solve(env, routed, &prep, &dl, &per, &meta);
    let Some(fin) = best else { return Vec::new() };
    repair::report_moves(env, routed, &dl, &fin);
    repair::report_overlaps(env, routed, &prep, &dl, &fin);
    let rd = fin.rd;
    notes::report_hits(env, routed, &rd.obstacles, &fin.r, &prep.g);
    notes::check_landings(env, routed, &fin.r, &base_segs);
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
    let mut outs: Vec<Vec<Item>> = (0..routed.len()).map(|_| Vec::new()).collect();
    notes::emit_notes(env, routed, &prep, &fin.r, &mut outs);
    for (o, n) in outs.iter().zip(routed) {
        let bx = items_box(env.font, o);
        env.notes_box.add_box(bx);
        env.ann_boxes.push(AnnBox { id: n.id.to_string(), kind: AnnKind::Note, bbox: bx, offset: 0.0 });
    }
    knockout::knock_hatch(base_items, &rd.knock);
    let mut result: Vec<Item> = Vec::new();
    for (k, its) in rd.per.into_iter().enumerate() {
        result.extend(its);
        if let Some(w) = welds.iter_mut().find(|w| w.0 == k) {
            result.append(&mut w.1);
        }
        if let Some(Some(sl)) = note_slot.get(k)
            && let Some(r) = routed_idx.iter().position(|i| i == sl)
            && let Some(o) = outs.get_mut(r)
        {
            result.append(o);
        }
    }
    let schedule = env.spec.kind == Kind::Schedule;
    let mut drawn: Vec<(usize, Vec<Item>)> = Vec::new();
    let mut cursor: Option<f64> = None;
    for t in tables.iter().filter(|t| t.at.is_some() || schedule) {
        let mut its = table::render(env, t);
        let local = items_box(env.font, &its);
        let d = match t.at {
            Some(at) => at,
            None => {
                let y = cursor.map_or(0.0, |c| c - local.y1);
                cursor = Some(y + local.y0 - env.style.gutter_in * env.s);
                V2::init(0.0, y)
            }
        };
        table::translate(&mut its, d);
        drawn.push((t.k, its));
    }
    if !numbers.is_empty() {
        let entries = notes::list_entries(env, &notes, &numbers);
        let mut ext = items_box(env.font, &result);
        ext.add_box(env.crop);
        for (_, its) in &drawn {
            ext.add_box(items_box(env.font, its));
        }
        let list = notes::list_items(env, &entries, ext);
        let bx = items_box(env.font, &list);
        env.notes_box.add_box(bx);
        env.ann_boxes.push(AnnBox { id: "notes_list".to_string(), kind: AnnKind::List, bbox: bx, offset: 0.0 });
        result.extend(list);
    }
    let mut ext = items_box(env.font, &result);
    ext.add_box(env.crop);
    for (_, its) in &drawn {
        ext.add_box(items_box(env.font, its));
    }
    for t in tables.iter().filter(|t| t.at.is_none() && !schedule) {
        let mut its = table::render(env, t);
        let local = items_box(env.font, &its);
        let d = V2::init(ext.x0 - local.x0, ext.y0 - env.style.gutter_in * env.s - local.y1);
        table::translate(&mut its, d);
        ext.add_box(items_box(env.font, &its));
        drawn.push((t.k, its));
    }
    crate::sort::stable(&mut drawn, |a, b| a.0 < b.0);
    let boxes: Vec<(usize, Box)> = drawn.iter().map(|(k, its)| (*k, items_box(env.font, its))).collect();
    let regions: Vec<Vec<V2>> = base_items
        .iter()
        .flat_map(|it| match it {
            Item::Hatch(h) => h.loops.as_slice(),
            Item::Fill(f) => f.loops.as_slice(),
            _ => &[],
        })
        .map(|lp| lp.iter().map(|q| q.v()).collect())
        .collect();
    table_overlaps(env, &tables, &boxes, &base_segs, &regions);
    for ((_, its), (k, bx)) in drawn.into_iter().zip(&boxes) {
        let id = tables.iter().find(|t| t.k == *k).map_or("", |t| t.id);
        env.ann_boxes.push(AnnBox { id: id.to_string(), kind: AnnKind::Table, bbox: *bx, offset: 0.0 });
        result.extend(its);
    }
    result
}

/// `W_TABLE_OVERLAP` (SPEC 26.1): a table placed with `at` overprints another table, a note, dimension, label or the
/// notes list, or the drawing's lines.
fn table_overlaps(
    env: &Env,
    tables: &[table::Table],
    boxes: &[(usize, Box)],
    base_segs: &[[V2; 2]],
    regions: &[Vec<V2>],
) {
    let vid = env.spec.id;
    let pad = 0.05 * env.s;
    for (i, &(k, bx)) in boxes.iter().enumerate() {
        let Some(t) = tables.iter().find(|t| t.k == k).filter(|t| t.at.is_some()) else { continue };
        let inner = bx.expand(-pad);
        let mut hits: Vec<String> = Vec::new();
        for (j, &(k2, b2)) in boxes.iter().enumerate() {
            if j != i && inner.overlaps(b2, 0.0) {
                hits.push(format!("table '{}'", tables.iter().find(|t| t.k == k2).map_or("", |t| t.id)));
            }
        }
        for a in env.ann_boxes.iter().filter(|a| a.kind != AnnKind::Table && inner.overlaps(a.bbox, 0.0)) {
            hits.push(match a.kind {
                AnnKind::Note => format!("note '{}'", a.id),
                AnnKind::Dim => format!("dimension '{}'", a.id),
                AnnKind::Label => format!("label '{}'", a.id),
                _ => format!("the {} list", env.spec.notes_title.unwrap_or("DETAIL NOTES")),
            });
        }
        let on_region = regions.iter().any(|lp| {
            crate::geom::point_in_loop_eo(inner.center(), lp)
                || (0..lp.len()).any(|i| crate::pathclip::clip_seg(lp[i], lp[(i + 1) % lp.len()], inner).is_some())
        });
        if on_region || base_segs.iter().any(|sg| crate::pathclip::clip_seg(sg[0], sg[1], inner).is_some()) {
            hits.push("the drawing (its lines or hatched regions)".to_string());
        }
        if hits.is_empty() {
            continue;
        }
        let at = t.at.unwrap_or_default();
        let fix = if env.spec.kind == Kind::Iso {
            "remove \"at\" to place the table under the drawing, or move \"at\" clear of them".to_string()
        } else {
            let mut clear = env.crop;
            for (j, &(_, b2)) in boxes.iter().enumerate() {
                if j != i {
                    clear.add_box(b2);
                }
            }
            for a in env.ann_boxes.iter().filter(|a| a.kind != AnnKind::Table) {
                clear.add_box(a.bbox);
            }
            let below = V2::init(at.x, at.y - (bx.y1 - clear.y0) - env.style.gutter_in * env.s);
            format!(
                "remove \"at\" to place the table under the drawing, or set \"at\": {} (below everything else)",
                notes::fmt_pt(below)
            )
        };
        env.diags.add_fix(
            Level::Warning,
            "W_TABLE_OVERLAP",
            Some(t.id),
            Some(&format!("views/{vid}/annotations/{}/at", t.id)),
            format!("table '{}' in view {vid} overprints {}", t.id, hits.join(", ")),
            fix,
        );
    }
}
