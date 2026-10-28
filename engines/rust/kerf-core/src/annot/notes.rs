//! Leader notes (SPEC 6.3): landing points, obstacles, routing of the note column and its legend, hit reporting, and note text with citations.

use super::text::{ascii_fold, layer_name, path_item, text_item, upper_if, wrap};
use super::{items_box, Env, Landing};
use crate::clip;
use crate::compile;
use crate::drawing::{Align, FillItem, Item, VAlign};
use crate::geom::{Box, Pt, V2};
use crate::json::{self, Value};
use crate::limits;
use crate::math;
use crate::num;
use crate::pen::{LayerKey, Pen};
use crate::route;
use crate::scene::Scene;
use crate::shape;
use crate::thinland;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// E_LIMIT for a note/label text over `limits::MAX_TEXT_CHARS` (reports and returns true).
pub fn text_too_long(env: &Env, what: &str, id: &str, apath: &str, text: &str) -> bool {
    if text.len() <= limits::MAX_TEXT_CHARS {
        return false;
    }
    let msg = limits::message(&format!("characters in the text of {what} '{id}'"), text.len(), limits::MAX_TEXT_CHARS, "A drawing note is a short phrase, not a paragraph.");
    env.diags.add_fix(
        crate::model::Level::Error,
        "E_LIMIT",
        Some(id),
        Some(apath),
        msg,
        "shorten the text to one phrase (about 130 characters or fewer) or split it into several notes".to_string(),
    );
    true
}

pub fn target_landing(env: &mut Env, target: &str) -> Option<Vec<V2>> {
    let mut comp_id = target;
    let mut part: Option<&str> = None;
    let mut inst: Option<u32> = None;
    if let Some(d) = target.find('.') {
        comp_id = &target[..d];
        part = Some(&target[d + 1..]);
    }
    if let Some(h) = comp_id.find('#') {
        inst = comp_id[h + 1..].parse::<u32>().ok();
        comp_id = &comp_id[..h];
    }
    let scene = env.scene;
    let spec = env.spec;
    let comp = scene.find(comp_id)?;
    if comp.state != crate::scene::State::Ok {
        return None;
    }
    if compile::is_omitted(&spec.omit, &comp.id) {
        return None;
    }
    let mut shapes_src: Vec<Vec<V2>> = Vec::new();
    let h = env.style.text_height_in * env.s;
    match &mut env.landing {
        Landing::Section(sec) => {
            let prisms = Rc::clone(&sec.prisms);
            for (i, p) in prisms.iter().enumerate() {
                if p.comp != comp.index {
                    continue;
                }
                if inst.is_some_and(|k| p.instance != k) {
                    continue;
                }
                if part.is_some_and(|pn| p.part != pn) {
                    continue;
                }
                shapes_src.extend(sec.visible_region(i));
            }
            if shapes_src.is_empty() {
                if let Some(pn) = part {
                    if let Some(lb) = Scene::part_box(comp, pn) {
                        let xf = comp.xfs.get(inst.unwrap_or(0) as usize)?;
                        let cs = [V2::init(lb.x0, lb.y0), V2::init(lb.x1, lb.y0), V2::init(lb.x1, lb.y1), V2::init(lb.x0, lb.y1)];
                        let mut pts = cs.map(|c| xf.apply(c));
                        clip::orient_ccw(&mut pts);
                        let reg = clip::boolean(&[pts.to_vec()], &[sec.crop_loop.clone()], clip::Op::Intersect);
                        shapes_src.extend(reg);
                    }
                }
            }
        }
        Landing::Iso(iso) => {
            let p = iso.landing(comp, inst, part)?;
            return Some(alloc::vec![p]);
        }
    }
    let shapes = shape::shapes_of(&shapes_src);
    let lp = shape::banded_label_point(&shapes, env.crop, h)?;
    if let Landing::Section(sec) = &mut env.landing {
        if let Some(c) = thinland::candidates(sec, comp, inst, part, lp.inset, h, lp.p) {
            return Some(c);
        }
    }
    Some(shape::candidates_for(&shapes, lp.inset, lp.p, h))
}

#[derive(Clone, Debug)]
pub struct NoteIn<'a> {
    pub id: &'a str,
    pub text: String,
    pub cands: Vec<V2>,
    /// False when the note has an explicit `at` (never nudged).
    pub movable: bool,
    pub place: Option<V2>,
    /// `column` hint of the note.
    pub column: Option<route::Side>,
}

/// A dimension text or label box that leaders must keep away from, with what is needed to propose a fix.
#[derive(Clone, Debug)]
pub struct Obstacle {
    pub poly: [V2; 4],
    pub id: String,
    pub kind: route::ObstKind,
    pub text: String,
    /// Index into the dimension specs (kind dim) or label specs (kind label).
    pub owner: usize,
    /// dim: unit vector along which |offset| grows (before the sign of offset); label: unused
    pub axis: V2,
    /// dim: current offset; label: current dx
    pub off: f64,
    /// label: current dy
    pub off2: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MetaKind {
    #[default]
    None,
    Dim,
    Label,
}

#[derive(Clone, Debug, Default)]
pub struct Meta<'a> {
    pub id: &'a str,
    pub kind: MetaKind,
    pub axis: V2,
    pub off: f64,
    pub off2: f64,
    /// Index into the dim / label specs.
    pub owner: usize,
}

#[derive(Clone, Debug)]
pub struct LabelSpec<'a> {
    pub k: usize,
    pub id: &'a str,
    pub text: &'a str,
    /// Drawing-space position (author offset applied); None when an iso label does not project.
    pub base: Option<V2>,
    pub off: V2,
}

fn num2(x: f64) -> String {
    json::number_string(math::round(x * 100.0) / 100.0)
}

pub fn fmt_pt(p: V2) -> String {
    format!("[{}, {}]", num2(p.x), num2(p.y))
}

/// `x` with exactly `decimals` places (rounded half away from zero), integer arithmetic only.
fn fixed(x: f64, decimals: u32) -> String {
    let scale = 10u64.pow(decimals);
    let n = num::to_int_clamped::<u64>(math::round(x.abs() * scale as f64), 0, u64::MAX);
    let sign = if x < 0.0 && n != 0 { "-" } else { "" };
    format!("{sign}{}.{:0width$}", n / scale, n % scale, width = decimals as usize)
}

pub fn clear_of_leaders(leaders: &[[V2; 3]], poly: &[V2; 4], shift: V2, h: f64) -> bool {
    let q = poly.map(|p| p.add(shift));
    leaders.iter().all(|l| route::poly_box_dist(l, &q) >= h)
}

const DIRS8: [(f64, f64); 8] = [(0.0, 1.0), (0.0, -1.0), (1.0, 0.0), (-1.0, 0.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];

pub fn dirs8() -> [V2; 8] {
    DIRS8.map(|(x, y)| V2::init(x, y))
}

/// Offset proposals for a dimension or label that a leader runs into (what the repair loop could not fix).
pub fn obstacle_fix(o: &Obstacle, leaders: &[[V2; 3]], h: f64) -> Option<String> {
    match o.kind {
        route::ObstKind::Dim => {
            let sgn = if o.off >= 0.0 { 1.0 } else { -1.0 };
            for k in 1..=24u32 {
                let delta = math::ceil(f64::from(k) * 0.5 * h * 4.0) / 4.0;
                for dirn in [1.0f64, -1.0] {
                    let no = o.off + sgn * dirn * delta;
                    if dirn < 0.0 && no.abs() < 2.0 * h {
                        continue;
                    }
                    if clear_of_leaders(leaders, &o.poly, o.axis.scale(sgn * dirn * delta), h) {
                        return Some(format!("set dim '{}' \"offset\": {} (now {})", o.id, json::number_string(no), json::number_string(o.off)));
                    }
                }
            }
        }
        route::ObstKind::Label => {
            for k in 1..=24u32 {
                let delta = math::ceil(f64::from(k) * 0.5 * h * 4.0) / 4.0;
                for dv in dirs8() {
                    let dn = dv.norm().scale(delta);
                    if clear_of_leaders(leaders, &o.poly, dn, h) {
                        return Some(format!(
                            "set label '{}' \"offset\": [{}, {}] (now [{}, {}])",
                            o.id,
                            json::number_string(math::round((o.off + dn.x) * 100.0) / 100.0),
                            json::number_string(math::round((o.off2 + dn.y) * 100.0) / 100.0),
                            json::number_string(o.off),
                            json::number_string(o.off2)
                        ));
                    }
                }
            }
        }
    }
    None
}

fn note_fix_text(id: &str, fa: Option<V2>, fp: Option<V2>) -> Option<String> {
    if let Some(p) = fa {
        return Some(format!("set note '{id}' \"at\": {} (another point inside its target)", fmt_pt(p)));
    }
    fp.map(|p| format!("set note '{id}' \"place\": {}", fmt_pt(p)))
}

pub fn report_hits(env: &Env, notes: &[NoteIn], obsts: &[Obstacle], r: &route::Layout, g: &route::Geo) {
    use crate::model::Level;
    let vid = env.spec.id;
    let maxn = 8usize;
    for ht in r.hits.iter().take(maxn) {
        let Some(me) = notes.get(ht.note) else { continue };
        let rel = if ht.dist <= 1e-9 { "crosses".to_string() } else { format!("comes within {} in (paper) of", fixed(ht.dist / env.s, 2)) };
        let what;
        let mut fix: Option<String>;
        match ht.kind {
            route::HitKind::Leader => {
                let Some(o) = notes.get(ht.other) else { continue };
                what = format!("the leader of note '{}'", o.id);
                fix = note_fix_text(o.id, ht.other_fix_at, ht.other_fix_place);
                if fix.is_none() {
                    fix = note_fix_text(me.id, ht.fix_at, ht.fix_place);
                }
            }
            route::HitKind::Note => {
                let Some(o) = notes.get(ht.other) else { continue };
                what = format!("the text of note '{}'", o.id);
                fix = note_fix_text(me.id, ht.fix_at, ht.fix_place);
                if fix.is_none() {
                    fix = note_fix_text(o.id, None, None);
                }
            }
            route::HitKind::Dim | route::HitKind::Label => {
                let Some(o) = obsts.get(ht.other) else { continue };
                what = if ht.kind == route::HitKind::Dim {
                    format!("the text '{}' of dimension '{}'", o.text, o.id)
                } else {
                    format!("label '{}' ('{}')", o.id, o.text)
                };
                fix = obstacle_fix(o, &r.leaders, g.h);
                if fix.is_none() {
                    fix = note_fix_text(me.id, ht.fix_at, ht.fix_place);
                }
            }
        }
        let fx = fix.unwrap_or_else(|| "move the note with \"place\", give it another \"at\" point, or change notes_side".to_string());
        env.diags.add_fix(
            Level::Warning,
            "W_LEADER_HIT",
            Some(me.id),
            Some(&format!("views/{vid}/annotations/{}", me.id)),
            format!(
                "view {vid}: the leader (or arrowhead) of note '{}' {rel} {what}; leaders must stay at least one text height ({} in paper) clear of other leaders, notes, dimension text and labels",
                me.id,
                fixed(env.style.text_height_in, 3)
            ),
            fx,
        );
    }
    if r.budget_exhausted {
        env.diags.add_fix(
            Level::Warning,
            "W_LEADER_HIT",
            None,
            Some(&format!("views/{vid}")),
            format!(
                "view {vid}: the layout effort limit was reached ({} notes, {} leader hits left): the notes were placed by the basic rule (sorted by landing height, de-crossed) and some leaders may touch each other or other annotations",
                notes.len(),
                r.hits.len()
            ),
            "fewer notes per view: split the detail into two views, shorten or merge notes, or set \"place\" on the crowded ones".to_string(),
        );
    }
    if r.hits.len() > maxn {
        env.diags.add(
            Level::Warning,
            "W_LEADER_HIT",
            None,
            Some(&format!("views/{vid}")),
            format!("view {vid}: {} more leader hits not listed; fix the ones above first (dense notes: split the view, shorten notes, or set \"place\" on some)", r.hits.len() - maxn),
        );
    }
}

/// Note text blocks (word wrap, sizes) and the routing parameters; independent of where dimensions end up.
pub struct NotePrep {
    pub g: route::Geo,
    pub gutter: f64,
    pub tag_r: f64,
    pub lines_of: Vec<Vec<String>>,
    /// (w, hgt) of each note's text block.
    pub sizes: Vec<(f64, f64)>,
}

pub fn prep_notes(env: &Env, notes: &[NoteIn]) -> NotePrep {
    let st = env.style;
    let sc = env.s;
    let h = st.text_height_in * sc;
    let g = route::Geo { h, pitch: h * st.line_spacing, gap: st.note_gap_in * sc, shoulder: st.shoulder_in * sc, pad: 0.04 * sc };
    let wrap_n = st.wrap_cols();
    let keynote = st.notes_mode_keynote;
    let tag_r = 0.14 * sc;
    let mut lines_of = Vec::with_capacity(notes.len());
    let mut sizes = Vec::with_capacity(notes.len());
    for (i, n) in notes.iter().enumerate() {
        let mut w = 2.0 * tag_r;
        let mut hgt = 2.0 * tag_r;
        if keynote {
            lines_of.push(alloc::vec![(i + 1).to_string()]);
        } else {
            let lines = wrap(&n.text, wrap_n);
            w = 0.0;
            for l in &lines {
                w = w.max(env.font.width(ascii_fold(l), h));
            }
            hgt = h + (lines.len() as f64 - 1.0) * g.pitch;
            lines_of.push(lines);
        }
        sizes.push((w, hgt));
    }
    NotePrep { g, gutter: st.gutter_in * sc, tag_r, lines_of, sizes }
}

pub fn route_notes(env: &Env, prep: &NotePrep, notes: &[NoteIn], ext: Box, obsts: &[Obstacle], soft: &[[V2; 2]], light: bool) -> route::Layout {
    let crop = env.crop;
    let ro: Vec<route::Obst> = obsts.iter().map(|o| route::Obst { kind: o.kind, poly: o.poly }).collect();
    let rin: Vec<route::NoteIn> = notes
        .iter()
        .zip(&prep.sizes)
        .map(|(n, &(w, hgt))| route::NoteIn { w, hgt, cands: &n.cands, movable: n.movable, place: n.place, column: n.column })
        .collect();
    let side = match env.spec.notes_side {
        crate::view::NotesSide::Left => route::Side::Left,
        crate::view::NotesSide::Right => route::Side::Right,
        crate::view::NotesSide::Both => route::Side::Both,
    };
    let mut p = route::Params::new(prep.g, crop, crop.x0.min(ext.x0), crop.x1.max(ext.x1), prep.gutter, side);
    p.light = light;
    p.work = env.work;
    route::route(p, &rin, &ro, soft)
}

pub fn emit_notes(env: &mut Env, notes: &[NoteIn], prep: &NotePrep, r: &route::Layout, out: &mut [Vec<Item>]) {
    let st = env.style;
    let sc = env.s;
    let h = st.text_height_in * sc;
    let g = prep.g;
    let keynote = st.notes_mode_keynote;
    let tag_r = prep.tag_r;
    for (i, n) in notes.iter().enumerate() {
        let (Some(lines), Some(o), Some(&px), Some(&ptop), Some(&left), Some(l), Some(&land)) =
            (prep.lines_of.get(i), out.get_mut(i), r.x.get(i), r.top.get(i), r.left.get(i), r.leaders.get(i), r.landing.get(i))
        else {
            continue;
        };
        if keynote {
            let cx = px + tag_r;
            let cy = ptop - tag_r;
            let mut hex = [V2::default(); 6];
            for (k, hp) in hex.iter_mut().enumerate() {
                let ang = math::PI / 6.0 + k as f64 * math::PI / 3.0;
                *hp = V2::init(cx + tag_r * math::cos(ang), cy + tag_r * math::sin(ang));
            }
            o.push(path_item(env, Pen::Anno, n.id, &hex, true));
            let num = lines.first().map_or("", String::as_str);
            o.push(text_item(env, LayerKey::Notes, Pen::Anno, n.id, num, cx, cy, h, 0.0, Align::Center, VAlign::Middle));
        } else {
            let right_aligned = n.place.is_some() && left;
            let bw = prep.sizes.get(i).map_or(0.0, |s| s.0);
            for (j, line) in lines.iter().enumerate() {
                let ty = ptop - h - j as f64 * g.pitch;
                if right_aligned {
                    o.push(text_item(env, LayerKey::Notes, Pen::Anno, n.id, line, px + bw, ty, h, 0.0, Align::Right, VAlign::Baseline));
                } else {
                    o.push(text_item(env, LayerKey::Notes, Pen::Anno, n.id, line, px, ty, h, 0.0, Align::Left, VAlign::Baseline));
                }
            }
        }
        let d = land.sub(l[1]).norm();
        let alen = st.arrow_len_in * sc;
        let aw = st.arrow_width_in * sc;
        let base = land.sub(d.scale(alen));
        let perp = d.perp();
        o.push(path_item(env, Pen::Anno, n.id, &[l[0], l[1], base], false));
        let tri = alloc::vec![Pt::at(land, 0.0), Pt::at(base.add(perp.scale(aw)), 0.0), Pt::at(base.sub(perp.scale(aw)), 0.0)];
        o.push(Item::Fill(FillItem { layer: String::from(layer_name(env, LayerKey::Notes)), src: String::from(n.id), loops: alloc::vec![tri] }));
    }
}

/// Keynote legend: numbered full texts in a block under the view (left aligned with the crop).
pub fn legend_items(env: &mut Env, notes: &[NoteIn], result: &mut Vec<Item>) {
    let st = env.style;
    let sc = env.s;
    let h = st.text_height_in * sc;
    let pitch = h * st.line_spacing;
    let mut bx = items_box(env.font, result);
    bx.add_box(env.crop);
    let top = env.crop.y1;
    let wrap_n = num::to_int_clamped::<usize>(st.wrap_chars * 1.25, 5, 250);
    let x0 = bx.x1 + 0.35 * sc;
    let mut y = top;
    result.push(text_item(env, LayerKey::Notes, Pen::Anno, "legend", "KEYNOTES", x0, y - h, h, 0.0, Align::Left, VAlign::Baseline));
    y -= pitch * 1.4;
    for (i, n) in notes.iter().enumerate() {
        let lines = wrap(&n.text, wrap_n);
        let num = (i + 1).to_string();
        result.push(text_item(env, LayerKey::Notes, Pen::Anno, "legend", &num, x0, y - h, h, 0.0, Align::Left, VAlign::Baseline));
        for (j, line) in lines.iter().enumerate() {
            result.push(text_item(env, LayerKey::Notes, Pen::Anno, "legend", line, x0 + 0.35 * sc, y - h - j as f64 * pitch, h, 0.0, Align::Left, VAlign::Baseline));
        }
        y -= pitch * lines.len() as f64 + 0.25 * pitch;
    }
}

pub fn note_text(env: &mut Env, text: &str, cites: &[Value]) -> String {
    let mut s = String::from(text);
    for c in cites {
        let code = c.get("code").map_or("", |x| x.str().unwrap_or(""));
        let section_s = c.get("section").map_or("", |x| x.str().unwrap_or(""));
        let ed = c.get("edition").and_then(Value::num).map_or_else(String::new, json::number_string);
        let status = c.get("status").map_or("suggested", |x| x.str().unwrap_or("suggested"));
        let fmt = env.style.cite_format.as_str();
        let mut rest = fmt;
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("{code}") {
                s.push_str(code);
                rest = r;
            } else if let Some(r) = rest.strip_prefix("{section}") {
                s.push_str(section_s);
                rest = r;
            } else if let Some(r) = rest.strip_prefix("{edition}") {
                s.push_str(&ed);
                rest = r;
            } else if let Some(ch) = rest.chars().next() {
                s.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
        if status != "verified" && env.style.cite_flag_unverified {
            s.push_str(&env.style.cite_flag);
            env.unverified = true;
        }
    }
    upper_if(env, &s).into_owned()
}
