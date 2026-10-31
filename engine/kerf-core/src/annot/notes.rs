//! Leader notes (SPEC 6.3) and keyed notes (SPEC 24): landing points, obstacles, routing of the note column, key tags and
//! the DETAIL NOTES list, hit reporting, and note text with citations.

use super::text::{ascii_fold, ascii_fold_flag, layer_name, path_item, text_item, upper_if, wrap};
use super::{Env, Landing};
use crate::clip;
use crate::drawing::{Align, FillItem, Item, PathItem, TextItem, VAlign};
use crate::font::Font;
use crate::geom::{self, Box, Pt, V2};
use crate::house;
use crate::json::{self, Value};
use crate::limits;
use crate::math;
use crate::model::Kind;
use crate::num;
use crate::pen::{LayerKey, Pen};
use crate::route;
use crate::scene::Scene;
use crate::section::Class;
use crate::shape;
use crate::sort;
use crate::style::KeyTag;
use crate::style::Style;
use crate::thinland;
use crate::view::NotesSide;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// E_LIMIT for a note/label text over `limits::MAX_TEXT_CHARS` (reports and returns true).
pub fn text_too_long(env: &Env, what: &str, id: &str, apath: &str, text: &str) -> bool {
    if text.len() <= limits::MAX_TEXT_CHARS {
        return false;
    }
    let msg = limits::message(
        &format!("characters in the text of {what} '{id}'"),
        text.len(),
        limits::MAX_TEXT_CHARS,
        "A drawing note is a short phrase, not a paragraph.",
    );
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
    if !spec.members.draws(comp) {
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
                for (i, p) in prisms.iter().enumerate() {
                    if !(spec.hidden_lines || p.hw_dash)
                        || p.comp != comp.index
                        || inst.is_some_and(|k| p.instance != k)
                        || part.is_some_and(|pn| p.part != pn)
                        || sec.cls[i] == Class::Drop
                        || p.kind != Kind::Body
                    {
                        continue;
                    }
                    let f = sec.flat_of(i);
                    shapes_src.extend(clip::boolean(&f, core::slice::from_ref(&sec.crop_loop), clip::Op::Intersect));
                }
            }
            if shapes_src.is_empty()
                && let Some(pn) = part
                && let Some(lb) = comp
                    .plan
                    .get(inst.unwrap_or(0) as usize)
                    .and_then(|pi| pi.parts.iter().find(|(n, _)| n == pn).map(|(_, b)| *b))
            {
                let mut pts =
                    [V2::init(lb.x0, lb.y0), V2::init(lb.x1, lb.y0), V2::init(lb.x1, lb.y1), V2::init(lb.x0, lb.y1)];
                clip::orient_ccw(&mut pts);
                let reg = clip::boolean(&[pts.to_vec()], core::slice::from_ref(&sec.crop_loop), clip::Op::Intersect);
                shapes_src.extend(reg);
            }
            if shapes_src.is_empty()
                && comp.plan.is_empty()
                && let Some(pn) = part
                && let Some(lb) = Scene::part_box(comp, pn)
            {
                let xf = comp.xfs.get(inst.unwrap_or(0) as usize)?;
                let cs =
                    [V2::init(lb.x0, lb.y0), V2::init(lb.x1, lb.y0), V2::init(lb.x1, lb.y1), V2::init(lb.x0, lb.y1)];
                let mut pts = cs.map(|c| xf.apply(c));
                clip::orient_ccw(&mut pts);
                let reg = clip::boolean(&[pts.to_vec()], core::slice::from_ref(&sec.crop_loop), clip::Op::Intersect);
                shapes_src.extend(reg);
            }
        }
        Landing::Iso(iso) => return iso.landing(comp, inst, part, env.crop, h),
    }
    let shapes = shape::shapes_of(&shapes_src);
    let lp = shape::banded_label_point(&shapes, env.crop, h)?;
    if let Landing::Section(sec) = &mut env.landing
        && let Some(c) = thinland::candidates(sec, comp, inst, part, lp.inset, h, lp.p)
    {
        return Some(c);
    }
    Some(shape::candidates_for(&shapes, lp.inset, lp.p, h))
}

#[derive(Clone, Debug)]
pub struct NoteIn<'a> {
    pub id: &'a str,
    pub text: String,
    /// The text without its citations (keyed lists compare it to say which field of a shared-key note differs).
    pub body: String,
    pub cands: Vec<V2>,
    /// False when the note has an explicit `at` (never nudged).
    pub movable: bool,
    pub place: Option<V2>,
    /// `column` hint of the note.
    pub column: Option<route::Side>,
    /// Explicit `key` (keyed notes): the note's number in the list.
    pub key: Option<u32>,
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
    /// Which end (or the middle) of each line sits on `at`.
    pub align: crate::drawing::Align,
    /// Drawing-space position (author offset applied); None when an iso label does not project.
    pub base: Option<V2>,
    pub off: V2,
    /// `pin`: never moved by the layout repair (overlaps then warn W_LABEL_OVERLAP).
    pub pin: bool,
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

const DIRS8: [(f64, f64); 8] =
    [(0.0, 1.0), (0.0, -1.0), (1.0, 0.0), (-1.0, 0.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];

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
                        return Some(format!(
                            "set dim '{}' \"offset\": {} (now {})",
                            o.id,
                            json::number_string(no),
                            json::number_string(o.off)
                        ));
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
        let rel = if ht.dist <= 1e-9 {
            "crosses".to_string()
        } else {
            format!("comes within {} in (paper) of", fixed(ht.dist / env.s, 2))
        };
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
        let fx = fix.unwrap_or_else(|| {
            "move the note with \"place\", give it another \"at\" point, or change notes_side".to_string()
        });
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

/// W_NOTE_LANDING (SPEC 25.2): a note with an explicit `at` must put its arrowhead on the drawing. In a section that is
/// anywhere inside the crop (an arrow may point into an opening or the air above grade); in an iso it is a visible face
/// or within half a text height of a drawn line, since the space between iso faces is not part of any member.
pub fn check_landings(env: &Env, notes: &[NoteIn], r: &route::Layout, base_segs: &[[V2; 2]]) {
    let vid = env.spec.id;
    let tol = 0.5 * env.style.text_height_in * env.s;
    let iso = match &env.landing {
        Landing::Iso(iso) => Some(*iso),
        Landing::Section(_) => None,
    };
    for (n, &land) in notes.iter().zip(&r.landing) {
        if n.movable {
            continue;
        }
        let gap = base_segs.iter().map(|sg| geom::dist_point_seg(land, sg[0], sg[1])).fold(f64::INFINITY, f64::min);
        let where_ = match iso {
            None if env.crop.expand(tol).contains(land) => continue,
            None => "outside the view crop".to_string(),
            Some(iso) => {
                let on_face =
                    iso.vis.iter().flat_map(|v| &v.shapes).any(|sh| shape::shape_contains(&sh.as_shape(), land));
                if on_face || gap <= tol {
                    continue;
                }
                "off every visible face of the iso".to_string()
            }
        };
        let an = env.spec.annotations.iter().find(|a| a.get("id").and_then(Value::str) == Some(n.id));
        let at_text = an.and_then(|a| a.get("at")).map_or_else(String::new, |v| {
            let mut buf = Vec::new();
            json::write_compact(&mut buf, v);
            String::from_utf8_lossy(&buf).into_owned()
        });
        let target = an.and_then(|a| a.get("target")).and_then(Value::str).unwrap_or("");
        let far = if gap.is_finite() {
            format!("{} in (paper) from the nearest drawn line", fixed(gap / env.s, 2))
        } else {
            "and nothing is drawn in the view".to_string()
        };
        let fix = if target.is_empty() {
            "point \"at\" at the member the note describes: a Ref such as \"<component>@center\", or {\"ref\": ..., \"offset\": [dx, dy]} with an offset that stays on the member".to_string()
        } else {
            format!(
                "remove \"at\" so the arrow lands inside target '{target}', or point it at a Ref on '{target}' (e.g. \"{}@center\")",
                target.split(['.', '#']).next().unwrap_or(target)
            )
        };
        env.diags.add_fix(
            crate::model::Level::Warning,
            "W_NOTE_LANDING",
            Some(n.id),
            Some(&format!("views/{vid}/annotations/{}/at", n.id)),
            format!(
                "view {vid}: the arrow of note '{}' (\"at\": {at_text}) lands on empty paper, {where_}, {far}",
                n.id
            ),
            fix,
        );
    }
}

/// Note text blocks (word wrap, sizes) and the routing parameters; independent of where dimensions end up.
pub struct NotePrep {
    pub g: route::Geo,
    pub gutter: f64,
    /// Circle/hex key tag radius (keyed views).
    pub tag_r: f64,
    pub keyed: bool,
    pub lines_of: Vec<Vec<String>>,
    /// (w, hgt) of each note's text block.
    pub sizes: Vec<(f64, f64)>,
}

/// `numbers` holds each note's key number in a keyed view and is empty otherwise.
pub fn prep_notes(env: &Env, notes: &[NoteIn], numbers: &[u32]) -> NotePrep {
    let st = env.style;
    let sc = env.s;
    let h = st.text_height_in * sc;
    let g = route::Geo {
        h,
        pitch: h * st.line_spacing,
        gap: st.note_gap_in * sc,
        shoulder: st.shoulder_in * sc,
        pad: 0.04 * sc,
    };
    let wrap_n = st.wrap_cols();
    // circle and hex tags share one size, set by the widest number
    let widest = numbers.iter().map(|k| env.font.width(k.to_string(), h)).fold(0.0, f64::max);
    let tag_r = match st.key_tag {
        KeyTag::Plain => 0.0,
        KeyTag::Circle => (0.5 * widest + 0.35 * h).max(0.8 * h),
        // the hexagon's inscribed circle holds the number
        KeyTag::Hex => (0.5 * widest + 0.35 * h).max(0.8 * h) * 2.0 / math::sqrt(3.0),
    };
    let mut lines_of = Vec::with_capacity(notes.len());
    let mut sizes = Vec::with_capacity(notes.len());
    for (i, n) in notes.iter().enumerate() {
        if let Some(&num) = numbers.get(i) {
            let label = num.to_string();
            sizes.push(match st.key_tag {
                KeyTag::Plain => (env.font.width(&label, h), h),
                KeyTag::Circle | KeyTag::Hex => (2.0 * tag_r, 2.0 * tag_r),
            });
            lines_of.push(alloc::vec![label]);
        } else {
            let lines = wrap(&n.text, wrap_n);
            let mut w: f64 = 0.0;
            for l in &lines {
                w = w.max(env.font.width(ascii_fold(l), h));
            }
            sizes.push((w, h + (lines.len() as f64 - 1.0) * g.pitch));
            lines_of.push(lines);
        }
    }
    NotePrep { g, gutter: st.gutter_in * sc, tag_r, keyed: !numbers.is_empty(), lines_of, sizes }
}

pub fn route_notes(
    env: &Env,
    prep: &NotePrep,
    notes: &[NoteIn],
    ext: Box,
    obsts: &[Obstacle],
    soft: &[[V2; 2]],
    light: bool,
) -> route::Layout {
    let crop = env.crop;
    let ro: Vec<route::Obst> = obsts.iter().map(|o| route::Obst { kind: o.kind, poly: o.poly }).collect();
    let rin: Vec<route::NoteIn> = notes
        .iter()
        .zip(&prep.sizes)
        .map(|(n, &(w, hgt))| route::NoteIn {
            w,
            hgt,
            cands: &n.cands,
            movable: n.movable,
            place: n.place,
            column: n.column,
        })
        .collect();
    // key tags go to the side nearer their landing point; notes_side places the list instead
    let side = match env.spec.notes_side {
        _ if prep.keyed => route::Side::Both,
        NotesSide::Left => route::Side::Left,
        NotesSide::Right => route::Side::Right,
        NotesSide::Both | NotesSide::Above => route::Side::Both,
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
    let tag_r = prep.tag_r;
    for (i, n) in notes.iter().enumerate() {
        let (Some(lines), Some(o), Some(&px), Some(&ptop), Some(&left), Some(l), Some(&land)) = (
            prep.lines_of.get(i),
            out.get_mut(i),
            r.x.get(i),
            r.top.get(i),
            r.left.get(i),
            r.leaders.get(i),
            r.landing.get(i),
        ) else {
            continue;
        };
        let mut start = l[0];
        if prep.keyed {
            let (bw, bh) = prep.sizes.get(i).copied().unwrap_or_default();
            let c = V2::init(px + 0.5 * bw, ptop - 0.5 * bh);
            match st.key_tag {
                KeyTag::Plain => {}
                KeyTag::Circle => {
                    // the leader starts on the circle, not a text gap away from it
                    start.x += if left { -g.pad } else { g.pad };
                    let quarter = core::f64::consts::SQRT_2 - 1.0;
                    let pts = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
                        .map(|(dx, dy)| Pt::at(V2::init(c.x + dx * tag_r, c.y + dy * tag_r), quarter));
                    o.push(Item::Path(PathItem {
                        layer: String::from(env.style.layer_for_pen(Pen::Anno)),
                        pen: Pen::Anno,
                        src: String::from(n.id),
                        closed: true,
                        pts: pts.to_vec(),
                    }));
                }
                KeyTag::Hex => {
                    start.x += if left { -g.pad } else { g.pad };
                    let mut hex = [V2::default(); 6];
                    for (k, hp) in hex.iter_mut().enumerate() {
                        let ang = k as f64 * math::PI / 3.0;
                        *hp = V2::init(c.x + tag_r * math::cos(ang), c.y + tag_r * math::sin(ang));
                    }
                    o.push(path_item(env, Pen::Anno, n.id, &hex, true));
                }
            }
            let num = lines.first().map_or("", String::as_str);
            o.push(text_item(
                env,
                LayerKey::Notes,
                Pen::Anno,
                n.id,
                num,
                c.x,
                c.y,
                h,
                0.0,
                Align::Center,
                VAlign::Middle,
            ));
        } else {
            let right_aligned = n.place.is_some() && left;
            let bw = prep.sizes.get(i).map_or(0.0, |s| s.0);
            for (j, line) in lines.iter().enumerate() {
                let ty = ptop - h - j as f64 * g.pitch;
                if right_aligned {
                    o.push(text_item(
                        env,
                        LayerKey::Notes,
                        Pen::Anno,
                        n.id,
                        line,
                        px + bw,
                        ty,
                        h,
                        0.0,
                        Align::Right,
                        VAlign::Baseline,
                    ));
                } else {
                    o.push(text_item(
                        env,
                        LayerKey::Notes,
                        Pen::Anno,
                        n.id,
                        line,
                        px,
                        ty,
                        h,
                        0.0,
                        Align::Left,
                        VAlign::Baseline,
                    ));
                }
            }
        }
        let d = land.sub(l[1]).norm();
        let alen = st.arrow_len_in * sc;
        let aw = st.arrow_width_in * sc;
        let base = land.sub(d.scale(alen));
        let perp = d.perp();
        o.push(path_item(env, Pen::Anno, n.id, &[start, l[1], base], false));
        let tri = alloc::vec![
            Pt::at(land, 0.0),
            Pt::at(base.add(perp.scale(aw)), 0.0),
            Pt::at(base.sub(perp.scale(aw)), 0.0)
        ];
        o.push(Item::Fill(FillItem {
            layer: String::from(layer_name(env, LayerKey::Notes)),
            src: String::from(n.id),
            loops: alloc::vec![tri],
        }));
    }
}

/// One entry of the keyed notes list: its number, its text, and the first note with that number (the src of its text).
pub struct ListEntry<'a> {
    pub num: u32,
    pub text: &'a str,
    pub body: &'a str,
    pub first: &'a str,
}

/// Keyed numbering (SPEC 24): a note's `key` is its number; the other notes take the lowest unused numbers in note order.
pub fn key_numbers(notes: &[NoteIn]) -> Vec<u32> {
    let mut used: Vec<u32> = notes.iter().filter_map(|n| n.key).collect();
    let mut next = 1u32;
    notes
        .iter()
        .map(|n| {
            n.key.unwrap_or_else(|| {
                while used.contains(&next) {
                    next += 1;
                }
                used.push(next);
                next
            })
        })
        .collect()
}

/// Layout numbering (SPEC 26.2): each note takes the number the layout gave its id; a note the layout did not number
/// (none should remain) takes the lowest number no other note uses.
pub fn shared_numbers(notes: &[NoteIn], keys: &[(String, u32)]) -> Vec<u32> {
    let mut used: Vec<u32> = keys.iter().map(|k| k.1).collect();
    let mut next = 1u32;
    notes
        .iter()
        .map(|n| match keys.iter().find(|k| k.0 == n.id) {
            Some(k) => k.1,
            None => {
                while used.contains(&next) {
                    next += 1;
                }
                used.push(next);
                next
            }
        })
        .collect()
}

/// The list entries in number order; notes sharing a key share the first one's text (a different text warns).
pub fn list_entries<'a>(env: &Env, notes: &'a [NoteIn], numbers: &[u32]) -> Vec<ListEntry<'a>> {
    let vid = env.spec.id;
    let mut out: Vec<ListEntry<'a>> = Vec::new();
    for (n, &num) in notes.iter().zip(numbers) {
        match out.iter().find(|e| e.num == num) {
            Some(e) if e.text != n.text && e.body == n.body => env.diags.add_fix(
                crate::model::Level::Warning,
                "W_PARAM",
                Some(n.id),
                Some(&format!("views/{vid}/annotations/{}/cite", n.id)),
                format!(
                    "note '{}' in view {vid} shares key {num} and its text with note '{}' but its \"cite\" differs (it reads \"{}\", '{}' reads \"{}\"); the DETAIL NOTES list shows the entry of '{}'",
                    n.id, e.first, n.text, e.first, e.text, e.first
                ),
                format!(
                    "remove \"text\" and \"cite\" from '{}' (a note with a shared key and no text takes the key's text and cite), copy the \"cite\" of '{}', or give it its own \"key\"",
                    n.id, e.first
                ),
            ),
            Some(e) if e.text != n.text => env.diags.add_fix(
                crate::model::Level::Warning,
                "W_PARAM",
                Some(n.id),
                Some(&format!("views/{vid}/annotations/{}/text", n.id)),
                format!(
                    "note '{}' in view {vid} shares key {num} with note '{}' but its \"text\" differs (\"{}\" vs \"{}\"); the DETAIL NOTES list shows the text of '{}'",
                    n.id, e.first, n.body, e.body, e.first
                ),
                format!(
                    "remove \"text\" from '{}' (a note with a shared key and no text takes the key's text and cite), or give it its own \"key\"",
                    n.id
                ),
            ),
            Some(_) => {}
            None => out.push(ListEntry { num, text: &n.text, body: &n.body, first: n.id }),
        }
    }
    sort::stable(&mut out, |a, b| a.num < b.num);
    out
}

/// Lines a list keeps in one column (above the drawing, or beside one shorter than that).
const ONE_COLUMN_LINES: usize = 8;

/// How a DETAIL NOTES list is drawn: style, font, scale factor and where it goes relative to the drawing.
pub struct ListLook<'a> {
    pub st: &'a Style,
    pub font: &'a Font,
    pub sc: f64,
    pub side: NotesSide,
    /// The list heading; `None` is the style's `notes.list_title`.
    pub title: Option<&'a str>,
}

/// The DETAIL NOTES list (SPEC 24) of a view.
pub fn list_items(env: &mut Env, entries: &[ListEntry], ext: Box) -> Vec<Item> {
    let look =
        ListLook { st: env.style, font: env.font, sc: env.s, side: env.spec.notes_side, title: env.spec.notes_title };
    list_block(&look, entries, ext, &mut env.unknown_glyph)
}

/// The DETAIL NOTES list (SPEC 24): heading, rule, then `<n>.` and the wrapped text with a hanging indent. It sits beside
/// (`notes_side` left/right) or above (`above`, `both`) `ext`, the drawing with its tags, dimensions and labels, and
/// splits into two columns past 8 lines, or beside a taller drawing, once one column would be taller than the drawing.
/// `unknown` is set when a character is not in the stroke font.
pub fn list_block(look: &ListLook, entries: &[ListEntry], ext: Box, unknown: &mut bool) -> Vec<Item> {
    let ListLook { st, font, sc, side, title } = *look;
    let h = st.text_height_in * sc;
    let pitch = h * st.line_spacing;
    let gap = st.gutter_in * sc;
    let wrap_n = num::to_int_clamped::<usize>(st.wrap_chars * 1.25, 5, 250);
    let widest = entries.iter().map(|e| font.width(format!("{}.", e.num), h)).fold(0.0, f64::max);
    let indent = widest + font.width("  ", h);
    let wrapped: Vec<Vec<String>> = entries.iter().map(|e| wrap(e.text, wrap_n)).collect();
    let text_w = wrapped.iter().flatten().map(|l| font.width(ascii_fold(l), h)).fold(0.0, f64::max);
    let title = title.unwrap_or(&st.list_title);
    let title = if st.text_case_upper { house::upper(title, st) } else { title.to_string() };
    let col_w = (indent + text_w).max(font.width(ascii_fold(&title), h));
    let total: usize = wrapped.iter().map(Vec::len).sum();
    let beside = matches!(side, NotesSide::Left | NotesSide::Right);
    let two = total > ONE_COLUMN_LINES && (!beside || total as f64 * pitch > ext.height());
    // the first column ends at the entry that makes the taller column shortest (ties: the longer first column)
    let mut split = entries.len();
    if two {
        let mut best = usize::MAX;
        let mut acc = 0;
        for (k, w) in wrapped.iter().enumerate().take(entries.len() - 1) {
            acc += w.len();
            let tall = acc.max(total - acc);
            if tall <= best {
                best = tall;
                split = k + 1;
            }
        }
    }
    let ncols = if split < entries.len() { 2.0 } else { 1.0 };
    let block_w = ncols * col_w + (ncols - 1.0) * gap;
    let first: usize = wrapped[..split].iter().map(Vec::len).sum();
    let rows = first.max(total - first) as f64;
    // heading, its rule half a text height under the baseline, the first entry's cap 0.6 text heights under the rule
    let (rule_drop, lead) = (1.5 * h, 1.6 * h);
    let block_h = rule_drop + lead + (rows - 1.0) * pitch;
    let (x0, top) = match side {
        NotesSide::Left => (ext.x0 - gap - block_w, ext.y1),
        NotesSide::Right => (ext.x1 + gap, ext.y1),
        NotesSide::Both | NotesSide::Above => (ext.x0, ext.y1 + gap + block_h),
    };
    let rule_y = top - rule_drop;
    let col_x = |c: usize| x0 + c as f64 * (col_w + gap);
    let layer = String::from(st.layer_name(LayerKey::Notes));
    let mut text = |src: &str, s: &str, x: f64, y: f64| {
        let folded = ascii_fold_flag(s, Some(&mut *unknown)).into_owned();
        Item::Text(TextItem::new(layer.clone(), Pen::Anno, String::from(src), folded, x, y, h))
    };
    let mut out = Vec::new();
    out.push(text("notes_list", &title, x0, top - h));
    for c in 0..ncols as usize {
        out.push(Item::Path(PathItem {
            layer: String::from(st.layer_for_pen(Pen::Anno)),
            pen: Pen::Anno,
            src: String::from("notes_list"),
            closed: false,
            pts: alloc::vec![Pt::at(V2::init(col_x(c), rule_y), 0.0), Pt::at(V2::init(col_x(c) + col_w, rule_y), 0.0)],
        }));
    }
    let mut y = [rule_y - lead; 2];
    for (i, (e, lines)) in entries.iter().zip(&wrapped).enumerate() {
        let c = usize::from(i >= split);
        let cx = col_x(c);
        let label = format!("{}.", e.num);
        out.push(text(e.first, &label, cx, y[c]));
        for line in lines {
            out.push(text(e.first, line, cx + indent, y[c]));
            y[c] -= pitch;
        }
    }
    out
}

/// A citation `section` under the case transform: words with a digit (`1926.755(a)(1)`, `R602.3(5)`) print as written,
/// since paragraph letters are significant; plain words (`Table`) take the transform.
fn section_text(env: &Env, section: &str) -> String {
    let mut out = String::with_capacity(section.len());
    for (i, w) in section.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if w.bytes().any(|c| c.is_ascii_digit()) {
            out.push_str(w);
        } else {
            out.push_str(&upper_if(env, w));
        }
    }
    out
}

/// The printed note: the text under the case transform, then each citation per `citations.format`. A citation's
/// `section` keeps the case of its numbered parts (`section_text`) and `edition` prints as written; its code and the
/// format's literal text take the case transform.
pub fn note_text(env: &mut Env, text: &str, cites: &[Value]) -> String {
    let mut s = upper_if(env, text).into_owned();
    for c in cites {
        let code = c.get("code").map_or("", |x| x.str().unwrap_or(""));
        let section_s = c.get("section").map_or("", |x| x.str().unwrap_or(""));
        let ed = c.get("edition").and_then(Value::num).map_or_else(String::new, json::number_string);
        let status = c.get("status").map_or("suggested", |x| x.str().unwrap_or("suggested"));
        let fmt = env.style.cite_format.as_str();
        let mut rest = fmt;
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("{code}") {
                s.push_str(&upper_if(env, code));
                rest = r;
            } else if let Some(r) = rest.strip_prefix("{section}") {
                s.push_str(&section_text(env, section_s));
                rest = r;
            } else if let Some(r) = rest.strip_prefix("{edition}") {
                s.push_str(&ed);
                rest = r;
            } else if let Some(ch) = rest.chars().next() {
                s.push_str(&upper_if(env, &rest[..ch.len_utf8()]));
                rest = &rest[ch.len_utf8()..];
            }
        }
        if status != "verified" && env.style.cite_flag_unverified {
            s.push_str(&env.style.cite_flag);
            env.unverified = true;
        }
    }
    s
}
