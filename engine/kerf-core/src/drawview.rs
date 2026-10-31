//! Orchestrates a view into a Drawing: scene compile, section/iso geometry, annotations, title.

use crate::annot::{self, AnnBox, AnnKind, Env, Landing, SheetInfo};
use crate::compile;
use crate::drawing::{Drawing, Item, LayerDef};
use crate::font::{self, Font};
use crate::geom::{self, Box};
use crate::iso::{self, Iso};
use crate::json::{self, Value};
use crate::math;
use crate::model::{Diag, Diags, Level, Prism};
use crate::num;
use crate::pen;
use crate::route::{DEFAULT_WORK_LIMIT, Work};
use crate::scene::{self, Scene};
use crate::section::{FillFault, Section};
use crate::style::{Role, Style};
use crate::units;
use crate::view::{self, Kind, NotesSide, NotesStyle, ViewSpec};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn collect_layers(items: &[Item], st: &Style) -> Vec<LayerDef> {
    let mut out = Vec::new();
    for key in pen::DRAW_ORDER {
        let Some(l) = st.layer_by_key(key) else { continue };
        if items.iter().any(|it| it.layer() == l.name) {
            out.push(LayerDef { name: l.name.clone(), lineweight_mm: l.lineweight_mm, linetype: l.linetype.clone() });
        }
    }
    out
}

pub fn meta_string<'a>(doc: &'a Value, key: &str) -> &'a str {
    doc.get("meta").and_then(|m| m.get(key)).and_then(Value::str).unwrap_or("")
}

pub fn code_basis(doc: &Value) -> String {
    let Some(j) = doc.get("meta").and_then(|m| m.get("jurisdiction")) else { return String::new() };
    let code = j.get("code").and_then(Value::str).unwrap_or("");
    if code.is_empty() {
        return String::new();
    }
    if let Some(ed) = j.get("edition").and_then(Value::num).and_then(num::to_int::<i64>) {
        return format!("{code} {ed}");
    }
    code.to_string()
}

/// `x` with exactly `dp` decimals, rounded half up on its shortest decimal digits (the engine's fixed-decimal rule for messages).
fn fixed(x: f64, dp: usize) -> String {
    if !x.is_finite() {
        return "0".to_string();
    }
    let neg = x < 0.0;
    let text = format!("{}", x.abs());
    let (int_part, frac_part) = text.split_once('.').unwrap_or((&text, ""));
    let mut digits: Vec<u8> = int_part.bytes().collect();
    digits.extend(frac_part.bytes().chain(core::iter::repeat(b'0')).take(dp));
    let round_up = frac_part.as_bytes().get(dp).is_some_and(|&d| d >= b'5');
    if round_up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let (ip, fp) = digits.split_at(digits.len() - dp);
    let mut s = String::new();
    if neg && digits.iter().any(|&d| d != b'0') {
        s.push('-');
    }
    s.extend(ip.iter().map(|&d| d as char));
    if dp > 0 {
        s.push('.');
        s.extend(fp.iter().map(|&d| d as char));
    }
    s
}

#[derive(Default)]
struct FitInfo {
    ann: Vec<AnnBox>,
    title: Box,
}

const STANDARD_SCALES: [f64; 7] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0];

/// Paper clearance (inches) kept between a view (its notes, list, title bubble) and the sheet frame or title block.
const FRAME_CLEAR_IN: f64 = 0.125;

/// The sheet area a view with its annotations and title must fit (paper inches): inside the frame and above the title
/// block, `FRAME_CLEAR_IN` clear of both.
fn fit_area(st: &Style) -> (f64, f64) {
    (
        st.sheet_w_in - 2.0 * st.margin_in - 2.0 * FRAME_CLEAR_IN,
        st.sheet_h_in - 2.0 * st.margin_in - st.title_block_h_in - 2.0 * FRAME_CLEAR_IN,
    )
}

struct Culprit {
    name: String,
    ext: f64,
    kind: Option<AnnKind>,
    id: String,
}

/// Who extends furthest beyond the crop on one side (extents in model units, > 0 only).
fn side_culprit(fit: &FitInfo, crop: Box, side: u8) -> Culprit {
    let mut best = Culprit { name: "the crop itself".to_string(), ext: 0.0, kind: None, id: String::new() };
    let mut notes_ext: f64 = 0.0;
    let mut nnotes = 0usize;
    for b in &fit.ann {
        let e = side_ext(b.bbox, crop, side);
        if b.kind == AnnKind::Note {
            nnotes += 1;
            notes_ext = notes_ext.max(e);
            continue;
        }
        if e > best.ext + 1e-9 {
            let name = match b.kind {
                AnnKind::Dim => format!("dimension '{}'", b.id),
                AnnKind::List => "the DETAIL NOTES list".to_string(),
                _ => format!("label '{}'", b.id),
            };
            best = Culprit { name, ext: e, kind: Some(b.kind), id: b.id.clone() };
        }
    }
    if notes_ext > best.ext + 1e-9 {
        best = Culprit {
            name: format!("the notes column ({nnotes} notes)"),
            ext: notes_ext,
            kind: Some(AnnKind::Note),
            id: String::new(),
        };
    }
    let te = side_ext(fit.title, crop, side);
    if te > best.ext + 1e-9 {
        // the title sits under the lowest annotation: an annotation that reaches below the crop is the real culprit
        if best.ext > 1e-9 {
            best.name = format!("{} and the view title under it", best.name);
            best.ext = te;
        } else {
            best = Culprit {
                name: "the view title (bubble, title, scale)".to_string(),
                ext: te,
                kind: None,
                id: "title".to_string(),
            };
        }
    }
    best
}

fn side_ext(b: Box, crop: Box, side: u8) -> f64 {
    if b.is_empty() {
        return 0.0;
    }
    match side {
        b'l' => (crop.x0 - b.x0).max(0.0),
        b'r' => (b.x1 - crop.x1).max(0.0),
        b't' => (b.y1 - crop.y1).max(0.0),
        _ => (crop.y0 - b.y0).max(0.0),
    }
}

/// A paper/model number in a fix hint, rounded to 0.1.
fn fmt_num(n: f64) -> String {
    json::number_string(math::round(n * 10.0) / 10.0)
}

fn dim_offset_fix(fit: &FitInfo, big: &Culprit, need: f64, nfix: &mut usize, fixes: &mut String) {
    for b in &fit.ann {
        if b.kind == AnnKind::Dim && b.id == big.id {
            let sg = if b.offset >= 0.0 { 1.0 } else { -1.0 };
            let no = b.offset - sg * need.min(b.offset.abs() - 1.0);
            *nfix += 1;
            let sep = if *nfix > 1 { "; " } else { "" };
            fixes.push_str(&format!(
                "{sep}{}) change dim '{}' offset from {} to {}",
                *nfix,
                b.id,
                fmt_num(b.offset),
                fmt_num(no)
            ));
        }
    }
}

/// The scale and extents a view was laid out at, and the sheet area it has to fit.
#[derive(Clone, Copy)]
struct Frame<'a> {
    scale_text: &'a str,
    scale: f64,
    crop: Box,
    bounds: Box,
    /// Sheet area (paper inches) inside the frame and above the title block, less the frame clearance.
    aw: f64,
    ah: f64,
}

/// W_VIEW_FIT (SPEC 18): overflow per edge, the culprit on each overflowing axis and the smallest fix first.
fn view_fit(st: &Style, spec: &ViewSpec, frame: &Frame, fit: &FitInfo, diags: &Diags) {
    let Frame { scale_text, scale, crop, bounds, aw, ah } = *frame;
    let view_id = spec.id;
    let leader = spec.notes_style.unwrap_or(st.notes_mode) == NotesStyle::Leader;
    let pw = (bounds.x1 - bounds.x0) / scale;
    let ph = (bounds.y1 - bounds.y0) / scale;
    let ow = (pw - aw).max(0.0);
    let oh = (ph - ah).max(0.0);
    let cw = crop.width() / scale;
    let ch = crop.height() / scale;
    let mut msg = format!(
        "view {view_id} with notes, dimensions and title needs {} x {} paper inches but the sheet area is {} x {} (inside the frame, above the title block, 1/8\" clear of both) at scale {}. Overflow by edge (sheet centered on the view): left {}\", right {}\", top {}\", bottom {}\".",
        fixed(pw, 2),
        fixed(ph, 2),
        fixed(aw, 2),
        fixed(ah, 2),
        scale_text,
        fixed(ow / 2.0, 2),
        fixed(ow / 2.0, 2),
        fixed(oh / 2.0, 2),
        fixed(oh / 2.0, 2)
    );
    let mut fixes = String::new();
    let mut nfix = 0usize;
    let lc = side_culprit(fit, crop, b'l');
    let rc = side_culprit(fit, crop, b'r');
    let tc = side_culprit(fit, crop, b't');
    let bc = side_culprit(fit, crop, b'b');
    let sep = |nfix: usize| if nfix > 1 { "; " } else { "" };
    if ow > 0.0 {
        msg.push_str(&format!(
            " Width: crop {}\" + left side {}\" ({}) + right side {}\" ({}).",
            fixed(cw, 2),
            fixed(lc.ext / scale, 2),
            lc.name,
            fixed(rc.ext / scale, 2),
            rc.name
        ));
        let big = if lc.ext >= rc.ext { &lc } else { &rc };
        let need = ow * scale;
        if cw > ow + 0.5 {
            nfix += 1;
            fixes.push_str(&format!(
                "{nfix}) narrow crop.x by {} in (now [{}, {}])",
                fmt_num(need),
                fmt_num(crop.x0),
                fmt_num(crop.x1)
            ));
        }
        match big.kind {
            Some(AnnKind::Dim) => dim_offset_fix(fit, big, need, &mut nfix, &mut fixes),
            Some(AnnKind::Note) if leader && spec.notes_side == NotesSide::Both => {
                nfix += 1;
                let (keep, drop) = if lc.ext >= rc.ext { ("right", "left") } else { ("left", "right") };
                fixes.push_str(&format!(
                    "{}{nfix}) set notes_side to \"{keep}\" to drop the {drop} notes column (saves about {}\" if the height allows)",
                    sep(nfix),
                    fixed(lc.ext.min(rc.ext) / scale, 2)
                ));
            }
            Some(AnnKind::List) if matches!(spec.notes_side, NotesSide::Left | NotesSide::Right) => {
                nfix += 1;
                fixes.push_str(&format!(
                    "{}{nfix}) set notes_side to \"above\" to put the DETAIL NOTES list above the drawing",
                    sep(nfix)
                ));
            }
            _ => {}
        }
        if matches!(big.kind, Some(AnnKind::Note | AnnKind::List)) {
            nfix += 1;
            fixes.push_str(&format!(
                "{}{nfix}) shorten the longest notes (they wrap at {} characters)",
                sep(nfix),
                st.wrap_cols()
            ));
        }
    }
    if oh > 0.0 {
        msg.push_str(&format!(
            " Height: crop {}\" + above {}\" ({}) + below {}\" ({}).",
            fixed(ch, 2),
            fixed(tc.ext / scale, 2),
            tc.name,
            fixed(bc.ext / scale, 2),
            bc.name
        ));
        let need = oh * scale;
        if ch > oh + 0.5 {
            nfix += 1;
            fixes.push_str(&format!(
                "{}{nfix}) shorten crop.y by {} in (now [{}, {}])",
                sep(nfix),
                fmt_num(need),
                fmt_num(crop.y0),
                fmt_num(crop.y1)
            ));
        }
        let big = if tc.ext >= bc.ext { &tc } else { &bc };
        match big.kind {
            Some(AnnKind::Dim) => dim_offset_fix(fit, big, need, &mut nfix, &mut fixes),
            Some(AnnKind::List) if !matches!(spec.notes_side, NotesSide::Left | NotesSide::Right) => {
                nfix += 1;
                fixes.push_str(&format!(
                    "{}{nfix}) set notes_side to \"right\" or \"left\" to put the DETAIL NOTES list beside the drawing",
                    sep(nfix)
                ));
            }
            Some(AnnKind::Note) if leader && spec.notes_side != NotesSide::Both => {
                nfix += 1;
                fixes.push_str(&format!(
                    "{}{nfix}) set notes_side to \"both\" to split the notes over two columns (adds about {}\" of width)",
                    sep(nfix),
                    fixed(0.0 + (st.gutter_in + st.wrap_chars * st.text_height_in * 0.75), 2)
                ));
            }
            _ => {}
        }
        if big.kind.is_none() && big.id == "title" {
            nfix += 1;
            fixes.push_str(&format!(
                "{}{nfix}) the view title (bubble, title, scale) sits below the lowest annotation: shorten the crop or the notes column, raise the lowest dimension, or use a smaller scale",
                sep(nfix)
            ));
        }
    }
    // a scale change only when the overflow is greater than 15% of the frame
    if ow > 0.15 * aw || oh > 0.15 * ah {
        let extra_w = pw - cw;
        let extra_h = ph - ch;
        let pick = STANDARD_SCALES
            .iter()
            .copied()
            .find(|&s2| s2 > scale && crop.width() / s2 + extra_w <= aw && crop.height() / s2 + extra_h <= ah);
        let sc = pick.unwrap_or_else(|| {
            math::ceil((crop.width() / (aw - extra_w).max(0.5)).max(crop.height() / (ah - extra_h).max(0.5)))
        });
        nfix += 1;
        let lab = if pick.is_some() { units::scale_label(sc) } else { format!("1:{}", fixed(sc, 0)) };
        fixes.push_str(&format!("{}{nfix}) use a smaller scale (e.g. {lab})", sep(nfix)));
    }
    if nfix == 0 {
        fixes.push_str("shorten the notes or shrink the crop");
    }
    diags.add_fix(Level::Warning, "W_VIEW_FIT", Some(view_id), Some(&format!("views/{view_id}")), msg, fixes);
}

/// One complete section pass at a fixed (resolved) crop and scale: geometry, annotations, title.
struct SecPass {
    items: Vec<Item>,
    bounds: Box,
    detail: Box,
    fit: FitInfo,
    unverified: bool,
}

const GLYPH_INFO: &str = "some characters are not in the plotter font and were replaced by '?' (dashes, quotes, x-sign and fractions are folded automatically)";

/// What every pass over a section view shares.
#[derive(Clone, Copy)]
struct SectionInputs<'a> {
    st: &'a Style,
    scene: &'a Scene<'a>,
    prisms: &'a [Prism],
    font: &'a Font,
    doc: &'a Value,
    diags: &'a Diags,
    work: &'a Work,
}

/// `W_CUT_FILL` (SPEC 25.1): a cut part that lost its fill to an embedded part's hole, once per host and hole.
fn fill_fault_diags(faults: &[FillFault], view_id: &str, diags: &Diags) {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for f in faults {
        let key = (f.host.as_str(), f.hole.as_str());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let (host, hole) = key;
        let (msg, fix) = if f.lost {
            (
                format!(
                    "'{host}': subtracting embedded '{hole}' from its cut fill in view {view_id} removed more area than '{hole}' has (a polygon boolean fault), so '{host}' keeps its whole fill and '{hole}' draws over it"
                ),
                format!(
                    "nothing is missing from the drawing; please report this file. If '{hole}' must read as a hole in '{host}', move '{hole}' 1/64\" off the edge it shares with '{host}'"
                ),
            )
        } else {
            (
                format!("'{host}' draws no cut fill in view {view_id}: embedded '{hole}' covers all of its cut"),
                format!("move or resize '{host}' or '{hole}' so '{host}' shows, or delete '{host}' if '{hole}' replaces it"),
            )
        };
        diags.add_fix(Level::Warning, "W_CUT_FILL", Some(host), Some(&format!("views/{view_id}")), msg, fix);
    }
}

fn section_pass(inp: SectionInputs, spec: &ViewSpec) -> SecPass {
    let SectionInputs { st, scene, prisms, font, doc, diags, work } = inp;
    let scale = spec.scale;
    let mut sec = Section::new(scene, spec, prisms);
    sec.build();
    fill_fault_diags(&sec.fill_faults, spec.id, diags);
    let mut base_items = sec.finish();
    let regions = sec.region_items();
    let mut env = Env::new(st, font, scene, spec, scale, spec.crop, diags, Landing::Section(&mut sec));
    env.work = Some(work);
    let ann = annot::annotate(&mut env, &mut base_items);
    let unverified = env.unverified;
    let mut all: Vec<Item> = base_items;
    all.extend(regions);
    all.extend(ann);
    // title below the lowest annotation
    let mut tcrop = spec.crop;
    let ab = annot::items_box(font, &all);
    if !ab.is_empty() && ab.y0 < tcrop.y0 {
        tcrop.y0 = ab.y0;
    }
    let scale_text = units::scale_label(if spec.scale == 0.0 { 0.0 } else { scale });
    let info = SheetInfo {
        number: &spec.number,
        title: spec.title,
        scale_text: &scale_text,
        sheet: meta_string(doc, "sheet"),
        unverified,
    };
    let mut detail = annot::items_box(font, &all);
    detail.add_box(spec.crop);
    let title_from = all.len();
    annot::title_items(&mut env, &info, tcrop, &mut all);
    let fit = FitInfo { ann: env.ann_boxes.clone(), title: annot::items_box(font, &all[title_from..]) };
    if env.unknown_glyph {
        diags.add(Level::Info, "I_GLYPH", Some(spec.id), None, GLYPH_INFO.to_string());
    }
    let mut bounds = annot::items_box(font, &all);
    bounds.add_box(spec.crop);
    SecPass { items: all, bounds, detail, fit, unverified }
}

/// Auto crop (SPEC 19): bounding box of the non-fill components visible in the section, plus 6".
/// Fills (earth, gravel, sand, compacted fill) are left out and get clipped by the resulting crop.
pub fn auto_crop(prisms: &[Prism], cut_z: f64) -> Box {
    let mut solid = Box::default();
    let mut any = Box::default();
    for p in prisms {
        let is_cut = p.z0 < cut_z - 1e-9 && p.z1 > cut_z + 1e-9;
        if !is_cut && p.z1 > cut_z + 1e-9 {
            continue;
        }
        let mut b = Box::default();
        for l in &p.loops {
            b.add_box(geom::points_box(l));
        }
        b.add_box(geom::points_box(&p.line_pts));
        b.add_box(geom::points_box(&p.centerline));
        if b.is_empty() {
            continue;
        }
        any.add_box(b);
        if p.role != Role::Soil {
            solid.add_box(b);
        }
    }
    let mut b = if solid.is_empty() { any } else { solid };
    if b.is_empty() {
        b = Box { x0: -6.0, y0: -6.0, x1: 6.0, y1: 6.0 };
    }
    let m = 6.0;
    Box { x0: b.x0 - m, y0: b.y0 - m, x1: b.x1 + m, y1: b.y1 + m }
}

struct SecResolved {
    scale: f64,
    crop: Box,
    scale_text: String,
    pass: SecPass,
}

/// Resolve an omitted crop and/or scale (SPEC 19) and run the section pass. The scale is the first of the
/// standard scales (largest first) at which view + notes + dimensions + title fit the sheet frame; every
/// candidate is evaluated in order, so the result is stable. Explicit crop and scale are used as given.
fn resolve_section(inp: SectionInputs, spec: &ViewSpec) -> SecResolved {
    let SectionInputs { st, prisms, diags, .. } = inp;
    let mut rs = spec.clone();
    if !spec.has_crop {
        rs.crop = auto_crop(prisms, spec.cut_z);
    }
    if spec.has_scale {
        let pass = section_pass(inp, &rs);
        return SecResolved { scale: rs.scale, crop: rs.crop, scale_text: rs.scale_text.to_string(), pass };
    }
    let (aw, ah) = fit_area(st);
    let mut last: Option<SecResolved> = None;
    let mut last_diags: Vec<Diag> = Vec::new();
    for (k, &sc) in STANDARD_SCALES.iter().enumerate() {
        let is_last = k + 1 == STANDARD_SCALES.len();
        // the crop alone must fit (necessary condition; skips hopeless candidates without a full pass)
        if !is_last && (rs.crop.width() / sc > aw + 1e-6 || rs.crop.height() / sc > ah + 1e-6) {
            continue;
        }
        rs.scale = sc;
        let td = Diags::new();
        let pass = section_pass(SectionInputs { diags: &td, ..inp }, &rs);
        last_diags = td.take();
        let pw = (pass.bounds.x1 - pass.bounds.x0) / sc;
        let ph = (pass.bounds.y1 - pass.bounds.y0) / sc;
        let fits = pw <= aw + 1e-6 && ph <= ah + 1e-6;
        last = Some(SecResolved { scale: sc, crop: rs.crop, scale_text: units::scale_label(sc), pass });
        if fits {
            break;
        }
    }
    diags.extend(last_diags);
    if let Some(r) = last {
        r
    } else {
        let pass = section_pass(inp, &rs);
        SecResolved { scale: rs.scale, crop: rs.crop, scale_text: rs.scale_text.to_string(), pass }
    }
}

/// The crop and scale a view's drawing actually uses once an omitted crop/scale is resolved.
#[derive(Clone, Copy, Debug)]
pub struct Resolved {
    pub crop: Box,
    pub scale: f64,
}

/// Section views only; other views and fully explicit specs come back unchanged.
pub fn resolve_spec(doc: &Value, st: &Style, scene: &Scene, spec: &ViewSpec) -> Resolved {
    let same = Resolved { crop: spec.crop, scale: spec.scale };
    if spec.kind != Kind::Section || (spec.has_crop && spec.has_scale) {
        return same;
    }
    let prisms = compile::view_prisms(scene, &spec.omit);
    let font = font::embedded();
    let d = Diags::new();
    let work = Work::new(DEFAULT_WORK_LIMIT);
    let r = resolve_section(SectionInputs { st, scene, prisms: &prisms, font, doc, diags: &d, work: &work }, spec);
    Resolved { crop: r.crop, scale: r.scale }
}

/// Build the Drawing for `view_id`. Returns None (with diagnostics) if the view does not exist or is invalid.
pub fn build<'a>(doc: &Value, st: &'a Style, view_id: &str, diags: &Diags) -> Option<Drawing<'a>> {
    let Some(vnode) = view::find_view(doc, view_id) else {
        let ids = view::view_ids(doc);
        diags.add(
            Level::Error,
            "E_REF_UNKNOWN",
            Some(view_id),
            Some("views"),
            format!("no view with id '{view_id}'. Views: {}", scene::join_ids(&ids)),
        );
        return None;
    };
    let idx = doc
        .get("views")
        .and_then(Value::arr)
        .and_then(|vs| vs.iter().rposition(|v| v.get("id").and_then(Value::str) == Some(view_id)))
        .unwrap_or(0);
    let spec = view::parse(vnode, idx, diags)?;
    let scene = compile::compile(doc, st, diags);
    build_from_scene(doc, st, &scene, &spec, diags)
}

/// Build a view's Drawing from an already compiled scene. `diags` receives view-level diagnostics.
pub fn build_from_scene<'a>(
    doc: &Value,
    st: &'a Style,
    scene: &Scene,
    spec: &ViewSpec,
    diags: &Diags,
) -> Option<Drawing<'a>> {
    let view_id = spec.id;
    for o in &spec.omit {
        if scene.find(o).is_none() {
            diags.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(view_id),
                Some(&format!("views/{view_id}/omit")),
                format!(
                    "view {view_id} omits '{o}', which is not a component id. Components: {}",
                    scene::join_ids(&scene.comp_ids())
                ),
            );
        }
    }
    let prisms = compile::view_prisms(scene, &spec.omit);
    let work = Work::new(DEFAULT_WORK_LIMIT);
    let font = font::embedded();
    let items: Vec<Item>;
    let mut scale: f64;
    let bounds: Box;
    let unverified: bool;
    let crop_out: Box;
    let detail: Box;
    let fit: FitInfo;
    let scale_text: String;
    if spec.kind == Kind::Section {
        let rs = resolve_section(SectionInputs { st, scene, prisms: &prisms, font, doc, diags, work: &work }, spec);
        scale = rs.scale;
        crop_out = rs.crop;
        scale_text = rs.scale_text;
        let pass = rs.pass;
        unverified = pass.unverified;
        items = pass.items;
        bounds = pass.bounds;
        detail = pass.detail;
        fit = pass.fit;
    } else {
        scale_text = spec.scale_text.to_string();
        let mut trial = 0usize;
        let mut fitted = 0.0;
        let mut need = 0.0f64;
        loop {
            let tdiags = Diags::new();
            let mut iso = Iso::default();
            let factor = if trial == 0 { 0.0 } else { (fitted + 0.5 * trial as f64).max(need) };
            let res = iso::build(&mut iso, scene, spec, st, factor);
            if trial == 0 {
                fitted = res.scale;
            }
            scale = res.scale;
            let mut res = res;
            let regions = iso.region_items();
            let mut env = Env::new(st, font, scene, spec, scale, res.crop, &tdiags, Landing::Iso(&iso));
            env.work = Some(&work);
            let ann = annot::annotate(&mut env, &mut res.items);
            let un = env.unverified;
            let mut all: Vec<Item> = res.items;
            all.extend(regions);
            all.extend(ann);
            let mut tcrop = res.crop;
            let ab = annot::items_box(font, &all);
            if !ab.is_empty() && ab.y0 < tcrop.y0 {
                tcrop.y0 = ab.y0;
            }
            let label = units::scale_label(if spec.scale == 0.0 { 0.0 } else { scale });
            let info = SheetInfo {
                number: &spec.number,
                title: spec.title,
                scale_text: &label,
                sheet: meta_string(doc, "sheet"),
                unverified: un,
            };
            let mut det = annot::items_box(font, &all);
            det.add_box(res.crop);
            let title_from = all.len();
            annot::title_items(&mut env, &info, tcrop, &mut all);
            let fit_t = FitInfo { ann: env.ann_boxes.clone(), title: annot::items_box(font, &all[title_from..]) };
            if env.unknown_glyph {
                tdiags.add(Level::Info, "I_GLYPH", Some(view_id), None, GLYPH_INFO.to_string());
            }
            let mut b = annot::items_box(font, &all);
            b.add_box(res.crop);
            // NTS views grow their (internal) fit factor until notes and title fit the sheet frame
            let pw = (b.x1 - b.x0) / scale;
            let ph = (b.y1 - b.y0) / scale;
            let (aw, ah) = fit_area(st);
            let fits = pw <= aw + 1e-6 && ph <= ah + 1e-6;
            if !fits {
                // the geometry shrinks with the factor, the annotations keep their paper size
                let (gw, gh) = (res.crop.width(), res.crop.height());
                let (room_w, room_h) = (aw - (pw - gw / scale), ah - (ph - gh / scale));
                if room_w > 0.5 && room_h > 0.5 {
                    need = (gw / room_w).max(gh / room_h);
                }
            }
            if fits || spec.scale != 0.0 || trial >= 16 {
                diags.extend(tdiags.take());
                items = all;
                bounds = b;
                unverified = un;
                crop_out = res.crop;
                detail = det;
                fit = fit_t;
                break;
            }
            trial += 1;
        }
    }
    // fit check against the sheet frame
    {
        let pw = (bounds.x1 - bounds.x0) / scale;
        let ph = (bounds.y1 - bounds.y0) / scale;
        let (aw, ah) = fit_area(st);
        if pw > aw + 1e-6 || ph > ah + 1e-6 {
            view_fit(st, spec, &Frame { scale_text: &scale_text, scale, crop: crop_out, bounds, aw, ah }, &fit, diags);
        }
    }
    let layers = collect_layers(&items, st);
    let label = units::scale_label(if spec.scale == 0.0 { 0.0 } else { scale });
    let mut d = Drawing::new(
        doc.get("id").and_then(Value::str).unwrap_or("").to_string(),
        spec.id.to_string(),
        spec.kind,
        scale,
        [bounds.x0, bounds.y0, bounds.x1, bounds.y1],
        st,
    );
    d.items = items;
    d.layers = layers;
    d.diagnostics = diags.to_vec();
    d.number = spec.number.clone();
    d.title = spec.title.to_string();
    d.scale_label = label;
    d.sheet_no = meta_string(doc, "sheet").to_string();
    d.date = meta_string(doc, "date").to_string();
    d.code_basis = code_basis(doc);
    d.crop = crop_out;
    d.detail_bounds = detail;
    d.has_unverified = unverified;
    d.author = meta_string(doc, "author").to_string();
    d.project = meta_string(doc, "project").to_string();
    d.doc_title = doc.get("title").and_then(Value::str).unwrap_or("").to_string();
    Some(d)
}
