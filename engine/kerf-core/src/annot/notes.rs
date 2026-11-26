use super::text::{ascii_fold, ascii_fold_flag, layer_name, path_item, text_item, upper_if, wrap};
use super::{Env, Landing};
use crate::catalog::Type;
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
    if !comp.built.nails.is_empty() {
        let marks: Vec<V2> = match &env.landing {
            Landing::Section(sec) => sec.nail_marks.iter().filter(|m| m.0 == comp.index).map(|m| m.1).collect(),
            Landing::Iso(iso) => iso.nail_marks.iter().filter(|m| m.0 == comp.index).map(|m| m.1).collect(),
        };
        return nail_candidates(&marks, env.crop);
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
    let mut lp = shape::banded_label_point(&shape::shapes_of(&shapes_src), env.crop, h)?;
    let mut moved_from: Option<V2> = None;
    if let Landing::Section(sec) = &mut env.landing
        && let Some((q, cleared)) = off_embedded(sec, comp.index, &shapes_src, lp.p, h)
    {
        moved_from = Some(lp.p);
        lp.p = q;
        shapes_src = cleared;
    }
    if let Landing::Section(sec) = &mut env.landing
        && let Some(c) = thinland::candidates(sec, comp, inst, part, lp.inset, h, lp.p)
    {
        return Some(c);
    }
    let mut cands = shape::candidates_for(&shape::shapes_of(&shapes_src), lp.inset, lp.p, h);
    if let Some(o) = moved_from
        && let Some(k) = cands.iter().skip(1).position(|c| c.sub(o).dot(lp.p.sub(o)) < 0.0)
    {
        let c = cands.remove(k + 1);
        cands.insert(1, c);
    }
    Some(cands)
}

fn off_embedded(
    sec: &mut crate::section::Section<'_>,
    comp: u32,
    region: &[Vec<V2>],
    p: V2,
    h: f64,
) -> Option<(V2, Vec<Vec<V2>>)> {
    let bx = clip::loops_box(region);
    let prisms = Rc::clone(&sec.prisms);
    let mut emb: Vec<Vec<V2>> = Vec::new();
    for (i, q) in prisms.iter().enumerate() {
        if q.comp == comp || !q.embedded || q.kind != Kind::Body || sec.cls[i] == Class::Drop {
            continue;
        }
        let r = sec.visible_region(i);
        if !r.is_empty() && clip::loops_box(&r).overlaps(bx, 1e-9) {
            emb.extend(r);
        }
    }
    let near = |l: &Vec<V2>| {
        geom::point_in_loop_eo(p, l)
            || l.iter().enumerate().any(|(k, &a)| geom::dist_point_seg(p, a, l[(k + 1) % l.len()]) < 0.5 * h)
    };
    if !emb.iter().any(near) {
        return None;
    }
    let mut cleared = region.to_vec();
    for l in &emb {
        let mut o = l.clone();
        clip::orient_ccw(&mut o);
        cleared = clip::boolean(&cleared, &[o], clip::Op::Diff);
    }
    let q = shape::clear_point_near(&shape::shapes_of(&cleared), h, p, 0.5 * h)?;
    Some((q, cleared))
}

fn nail_candidates(marks: &[V2], crop: crate::geom::Box) -> Option<Vec<V2>> {
    let inside: Vec<V2> = marks.iter().copied().filter(|m| crop.contains(*m)).collect();
    if inside.is_empty() {
        return None;
    }
    let n = inside.len() as f64;
    let c = inside.iter().fold(V2::init(0.0, 0.0), |a, m| a.add(m.scale(1.0 / n)));
    let mut out = inside;
    crate::sort::stable(&mut out, |a, b| a.dist(c) < b.dist(c));
    out.dedup_by(|a, b| a.eql(*b, 1e-6));
    out.truncate(12);
    Some(out)
}

#[derive(Clone, Debug)]
pub struct NoteIn<'a> {
    pub id: &'a str,
    pub text: String,
    pub body: String,
    pub cands: Vec<V2>,
    pub movable: bool,
    pub place: Option<V2>,
    pub column: Option<route::Side>,
    pub key: Option<u32>,
    pub member: &'a str,
}

#[derive(Clone, Debug)]
pub struct Obstacle {
    pub poly: [V2; 4],
    pub id: String,
    pub kind: route::ObstKind,
    pub text: String,
    pub owner: usize,
    pub axis: V2,
    pub off: f64,
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
    pub owner: usize,
}

#[derive(Clone, Debug)]
pub struct LabelSpec<'a> {
    pub k: usize,
    pub id: &'a str,
    pub text: &'a str,
    pub align: crate::drawing::Align,
    pub base: Option<V2>,
    pub off: V2,
    pub pin: bool,
}

fn num2(x: f64) -> String {
    json::number_string(math::round(x * 100.0) / 100.0)
}

pub fn fmt_pt(p: V2) -> String {
    format!("[{}, {}]", num2(p.x), num2(p.y))
}

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
        route::ObstKind::Member => {}
    }
    None
}

fn note_fix_text(id: &str, fa: Option<V2>, fp: Option<V2>) -> Option<String> {
    if let Some(p) = fa {
        return Some(format!("set note '{id}' \"at\": {} (another point inside its target)", fmt_pt(p)));
    }
    fp.map(|p| format!("set note '{id}' \"place\": {}", fmt_pt(p)))
}

#[derive(Clone, Copy)]
enum NoteFix {
    At(V2),
    Place(V2),
    Column(route::Side),
    Side(route::Side),
}

struct Verifier<'v, 'a, 'e> {
    env: &'v Env<'a, 'e>,
    prep: &'v NotePrep,
    notes: &'v [NoteIn<'a>],
    room: (Box, &'v [Obstacle], &'v [[V2; 2]]),
    base: usize,
    work: route::Work,
    tries: usize,
}

const VERIFY_TRIES: usize = 12;
const VERIFY_WORK: u64 = route::DEFAULT_WORK_LIMIT / 4;

impl Verifier<'_, '_, '_> {
    fn hits_after(&mut self, i: usize, f: NoteFix) -> Option<usize> {
        if self.tries == 0 || self.work.spent() {
            return None;
        }
        self.tries -= 1;
        let mut ns: Vec<NoteIn> = self.notes.to_vec();
        let n = ns.get_mut(i)?;
        match f {
            NoteFix::At(p) => {
                n.cands = alloc::vec![p];
                n.movable = false;
            }
            NoteFix::Place(p) => n.place = Some(p),
            NoteFix::Column(s) => n.column = Some(s),
            NoteFix::Side(s) => {
                for m in &mut ns {
                    m.column.get_or_insert(s);
                }
            }
        }
        let r = route_with(self.env, self.prep, &ns, self.room, false, Some(&self.work));
        (!r.budget_exhausted && r.hits.len() < self.base).then_some(r.hits.len())
    }

    fn first(&mut self, cands: &[(usize, NoteFix)]) -> Option<String> {
        for &(i, f) in cands {
            let Some(k) = self.hits_after(i, f) else { continue };
            let id = self.notes.get(i)?.id;
            let what = match f {
                NoteFix::At(p) => format!("set note '{id}' \"at\": {} (another point inside its target)", fmt_pt(p)),
                NoteFix::Place(p) => format!("set note '{id}' \"place\": {}", fmt_pt(p)),
                NoteFix::Column(s) => {
                    format!("set note '{id}' \"column\": \"{}\"", if s == route::Side::Left { "left" } else { "right" })
                }
                NoteFix::Side(s) => format!(
                    "set \"{}\": \"{}\" on view {}",
                    if self.prep.keyed { "tags_side" } else { "notes_side" },
                    if s == route::Side::Left { "left" } else { "right" },
                    self.env.spec.id
                ),
            };
            let after = if k == 0 {
                "the view then has no leader hits".to_string()
            } else {
                format!("the view then has {k} leader hit{} instead of {}", if k == 1 { "" } else { "s" }, self.base)
            };
            return Some(format!("{what} (checked with the whole view laid out again: {after})"));
        }
        None
    }
}

fn note_cands(i: usize, fa: Option<V2>, fp: Option<V2>, r: &route::Layout, out: &mut Vec<(usize, NoteFix)>) {
    out.extend(fa.map(|p| (i, NoteFix::At(p))));
    out.extend(fp.map(|p| (i, NoteFix::Place(p))));
    if let Some(&left) = r.left.get(i) {
        out.push((i, NoteFix::Column(if left { route::Side::Right } else { route::Side::Left })));
    }
}

const CASCADE_FIX: &str = "no single change to one note lowers this view's leader hits (each moves the collision to another pair): put the tags on one side (\"tags_side\" on a keyed view, \"notes_side\" on a leader view), set \"column\" on several crowded notes at once, shorten or merge notes, or split the view";

pub fn report_hits(
    env: &Env,
    notes: &[NoteIn],
    obsts: &[Obstacle],
    r: &route::Layout,
    prep: &NotePrep,
    ext: Box,
    soft: &[[V2; 2]],
) {
    let g = &prep.g;
    use crate::model::Level;
    let vid = env.spec.id;
    let maxn = 8usize;
    let mut ver = Verifier {
        env,
        prep,
        notes,
        room: (ext, obsts, soft),
        base: r.hits.len(),
        work: route::Work::new(VERIFY_WORK),
        tries: if env.verify_fixes { VERIFY_TRIES } else { 0 },
    };
    let mut side_fix: Option<Option<String>> = None;
    for ht in r.hits.iter().take(maxn) {
        let Some(me) = notes.get(ht.note) else { continue };
        let rel = if ht.dist <= 1e-9 {
            "crosses".to_string()
        } else {
            format!("comes within {} in (paper) of", fixed(ht.dist / env.s, 2))
        };
        let what;
        let mut fix: Option<String> = None;
        let mut cands: Vec<(usize, NoteFix)> = Vec::new();
        let mut rule = "leaders must stay at least one text height ({h} in paper) clear of other leaders, notes, dimension text and labels";
        match ht.kind {
            route::HitKind::Member => {
                let Some(m) = ht.other.checked_sub(obsts.len()).and_then(|k| prep.member_names.get(k)) else {
                    continue;
                };
                what = m.clone();
                rule = "a leader that crosses another note's member reads as pointing at it";
                note_cands(ht.note, ht.fix_at, ht.fix_place, r, &mut cands);
            }
            route::HitKind::Through => {
                let Some(m) = prep.through_names.get(ht.note) else { continue };
                let left = r.left.get(ht.note).copied().unwrap_or(false);
                what = format!("the cut {m} to reach '{}' on its far side", me.member);
                rule = if left {
                    "from the right column the leader reaches that point without crossing a cut member"
                } else {
                    "from the left column the leader reaches that point without crossing a cut member"
                };
                let other = if left { route::Side::Right } else { route::Side::Left };
                if me.place.is_none() {
                    cands.push((ht.note, NoteFix::Column(other)));
                    fix = ver.first(&cands);
                    if fix.is_none() {
                        fix = Some(format!(
                            "set note '{}' \"column\": \"{}\"",
                            me.id,
                            if left { "right" } else { "left" }
                        ));
                    }
                } else {
                    fix = Some(format!(
                        "move the text of note '{}' to the {} of its target (\"place\"), or remove \"place\" so the layout picks the free side",
                        me.id,
                        if left { "right" } else { "left" }
                    ));
                }
            }
            route::HitKind::Leader => {
                let Some(o) = notes.get(ht.other) else { continue };
                what = format!("the leader of note '{}'", o.id);
                note_cands(ht.other, ht.other_fix_at, ht.other_fix_place, r, &mut cands);
                note_cands(ht.note, ht.fix_at, ht.fix_place, r, &mut cands);
            }
            route::HitKind::Note => {
                let Some(o) = notes.get(ht.other) else { continue };
                what = format!("the text of note '{}'", o.id);
                note_cands(ht.note, ht.fix_at, ht.fix_place, r, &mut cands);
                note_cands(ht.other, None, None, r, &mut cands);
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
                    note_cands(ht.note, ht.fix_at, ht.fix_place, r, &mut cands);
                }
            }
        }
        if fix.is_none() && !cands.is_empty() {
            fix = ver.first(&cands);
            if fix.is_none() && ver.tries > 0 && !ver.work.spent() {
                let both = if prep.keyed {
                    env.spec.tags_side == NotesSide::Both
                } else {
                    matches!(env.spec.notes_side, NotesSide::Both | NotesSide::Above)
                };
                if both && side_fix.is_none() {
                    let sides =
                        [(ht.note, NoteFix::Side(route::Side::Right)), (ht.note, NoteFix::Side(route::Side::Left))];
                    side_fix = Some(ver.first(&sides));
                }
                fix = Some(side_fix.clone().flatten().unwrap_or_else(|| CASCADE_FIX.to_string()));
            }
        }
        if fix.is_none() {
            fix = note_fix_text(me.id, ht.fix_at, ht.fix_place);
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
                "view {vid}: the leader (or arrowhead) of note '{}' {rel} {what}; {}",
                me.id,
                rule.replace("{h}", &fixed(env.style.text_height_in, 3))
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

pub struct NotePrep {
    pub g: route::Geo,
    pub gutter: f64,
    pub tag_r: f64,
    pub keyed: bool,
    pub lines_of: Vec<Vec<String>>,
    pub sizes: Vec<(f64, f64)>,
    pub members: Vec<route::Obst>,
    pub member_names: Vec<String>,
    pub groups: Vec<usize>,
    /// Per note (SPEC 30.3): the column whose leader would run through a cut member, those members' loops and their name.
    pub avoid: Vec<Option<route::Side>>,
    pub through: Vec<Vec<Vec<V2>>>,
    pub through_names: Vec<String>,
}

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
    let widest = numbers.iter().map(|k| env.font.width(k.to_string(), h)).fold(0.0, f64::max);
    let tag_r = match st.key_tag {
        KeyTag::Plain => 0.0,
        KeyTag::Circle => (0.5 * widest + 0.35 * h).max(0.8 * h),
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
    NotePrep {
        g,
        gutter: st.gutter_in * sc,
        tag_r,
        keyed: !numbers.is_empty(),
        lines_of,
        sizes,
        members: Vec::new(),
        member_names: Vec::new(),
        groups: alloc::vec![usize::MAX; notes.len()],
        avoid: alloc::vec![None; notes.len()],
        through: alloc::vec![Vec::new(); notes.len()],
        through_names: alloc::vec![String::new(); notes.len()],
    }
}

/// How many notes per view the layout tries on the other side to keep a leader out of a cut member (SPEC 30.3).
pub const AUTO_SIDE_TRIES: usize = 4;

/// The first note, not tried yet and free to change sides, whose leader runs through a cut member; with the other side.
pub fn through_free(
    env: &Env,
    prep: &NotePrep,
    notes: &[NoteIn],
    r: &route::Layout,
    tried: &[usize],
) -> Option<(usize, route::Side)> {
    let both = if prep.keyed {
        env.spec.tags_side == NotesSide::Both
    } else {
        matches!(env.spec.notes_side, NotesSide::Both | NotesSide::Above)
    };
    if !both {
        return None;
    }
    r.hits.iter().filter(|h| h.kind == route::HitKind::Through).find_map(|h| {
        let n = notes.get(h.note)?;
        let left = *r.left.get(h.note)?;
        (n.column.is_none() && n.place.is_none() && !tried.contains(&h.note))
            .then_some((h.note, if left { route::Side::Right } else { route::Side::Left }))
    })
}

pub fn through_members(env: &mut Env, notes: &[NoteIn], prep: &mut NotePrep) {
    let scene = env.scene;
    let crop = env.crop;
    let Landing::Section(sec) = &mut env.landing else { return };
    if notes.iter().all(|n| n.member.is_empty()) {
        return;
    }
    let prisms = Rc::clone(&sec.prisms);
    let mut walls: Vec<(u32, crate::section::Flat, Box)> = Vec::new();
    for (k, p) in prisms.iter().enumerate() {
        if sec.cls[k] != Class::Cut || p.kind != Kind::Body || p.embedded || p.dashed {
            continue;
        }
        let framing = scene
            .comps
            .get(p.comp as usize)
            .is_some_and(|c| matches!(c.ty.ty, Type::Lumber | Type::Ijoist | Type::Steel | Type::Concrete | Type::CmuWall));
        if framing {
            let f = sec.flat_of(k);
            let bx = clip::loops_box(&f);
            walls.push((p.comp, f, bx));
        }
    }
    let min = route::through_min(prep.g.h);
    for (i, n) in notes.iter().enumerate() {
        let (Some(&l), false) = (n.cands.first(), n.member.is_empty()) else { continue };
        let target = scene.find(crate::compile::host_id(n.member)).map(|c| c.index);
        let ray = |x: f64| -> (f64, Vec<usize>) {
            let mut total = 0.0;
            let mut crossed = Vec::new();
            for (w, (comp, f, bx)) in walls.iter().enumerate() {
                if Some(*comp) == target || l.y < bx.y0 || l.y > bx.y1 {
                    continue;
                }
                let len = route::seg_inside_len(l, V2::init(x, l.y), f);
                if len > 1e-6 {
                    total += len;
                    crossed.push(w);
                }
            }
            (total, crossed)
        };
        let (ll, lw) = ray(crop.x0.min(l.x) - 1.0);
        let (rl, rw) = ray(crop.x1.max(l.x) + 1.0);
        let (side, ws) = if ll >= min && rl < 0.25 * min {
            (route::Side::Left, lw)
        } else if rl >= min && ll < 0.25 * min {
            (route::Side::Right, rw)
        } else {
            continue;
        };
        prep.avoid[i] = Some(side);
        prep.through[i] = ws.iter().flat_map(|&w| walls[w].1.iter().cloned()).collect();
        let mut names: Vec<String> = Vec::new();
        for &w in &ws {
            if let Some(c) = scene.comps.get(walls[w].0 as usize) {
                let nm = format!("{} '{}'", c.ty.name(), c.id);
                if !names.contains(&nm) {
                    names.push(nm);
                }
            }
        }
        prep.through_names[i] = names.join(" and ");
    }
}

pub fn member_obstacles(env: &mut Env, notes: &[NoteIn], prep: &mut NotePrep) {
    let scene = env.scene;
    let Landing::Section(sec) = &mut env.landing else { return };
    let mut ids: Vec<&str> = Vec::new();
    for (i, n) in notes.iter().enumerate() {
        if n.member.is_empty() || n.cands.is_empty() {
            continue;
        }
        let g = match ids.iter().position(|m| *m == n.member) {
            Some(g) => g,
            None => {
                ids.push(n.member);
                ids.len() - 1
            }
        };
        prep.groups[i] = g;
    }
    let prisms = Rc::clone(&sec.prisms);
    for (g, id) in ids.iter().enumerate() {
        let Some(c) = scene.find(id) else { continue };
        let mine: Vec<usize> = prisms
            .iter()
            .enumerate()
            .filter(|(_, p)| p.comp == c.index && p.kind == Kind::Body)
            .map(|(k, _)| k)
            .collect();
        let compact = match c.ty.ty {
            Type::AnchorBolt | Type::Bolt => true,
            Type::Rebar => !c.built.points_mode,
            _ => false,
        };
        if mine.is_empty() || !compact {
            continue;
        }
        let first = notes.iter().position(|n| n.member == *id).map_or("", |k| notes[k].id);
        for k in mine {
            for l in sec.visible_region(k) {
                let a = geom::signed_area_v(&l);
                if l.len() < 3 || a <= 0.0 {
                    continue;
                }
                let bb = clip::loops_box(core::slice::from_ref(&l));
                if c.ty.ty == Type::Rebar && bb.width().max(bb.height()) > 2.0 * bb.width().min(bb.height()) {
                    continue;
                }
                let poly = if l.len() == 4 {
                    [l[0], l[1], l[2], l[3]]
                } else if a >= 0.5 * bb.width() * bb.height() {
                    [V2::init(bb.x0, bb.y0), V2::init(bb.x1, bb.y0), V2::init(bb.x1, bb.y1), V2::init(bb.x0, bb.y1)]
                } else {
                    continue;
                };
                prep.members.push(route::Obst { kind: route::ObstKind::Member, poly, group: g });
                prep.member_names.push(format!("{} '{}' (the target of note '{first}')", c.ty.name(), c.id));
            }
        }
    }
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
    route_with(env, prep, notes, (ext, obsts, soft), light, env.work)
}

fn route_with(
    env: &Env,
    prep: &NotePrep,
    notes: &[NoteIn],
    (ext, obsts, soft): (Box, &[Obstacle], &[[V2; 2]]),
    light: bool,
    work: Option<&route::Work>,
) -> route::Layout {
    let crop = env.crop;
    let mut ro: Vec<route::Obst> =
        obsts.iter().map(|o| route::Obst { kind: o.kind, poly: o.poly, group: usize::MAX }).collect();
    ro.extend(prep.members.iter().copied());
    let rin: Vec<route::NoteIn> = notes
        .iter()
        .zip(&prep.sizes)
        .zip(&prep.groups)
        .enumerate()
        .map(|(i, ((n, &(w, hgt)), &group))| route::NoteIn {
            w,
            hgt,
            cands: &n.cands,
            movable: n.movable,
            place: n.place,
            column: n.column,
            group,
            avoid: prep.avoid.get(i).copied().flatten(),
            through: prep.through.get(i).map_or(&[], Vec::as_slice),
        })
        .collect();
    let side = match (prep.keyed, env.spec.tags_side, env.spec.notes_side) {
        (true, NotesSide::Left, _) => route::Side::Left,
        (true, NotesSide::Right, _) => route::Side::Right,
        (true, _, _) => route::Side::Both,
        (false, _, NotesSide::Left) => route::Side::Left,
        (false, _, NotesSide::Right) => route::Side::Right,
        (false, _, NotesSide::Both | NotesSide::Above) => route::Side::Both,
    };
    let mut p = route::Params::new(prep.g, crop, crop.x0.min(ext.x0), crop.x1.max(ext.x1), prep.gutter, side);
    p.light = light;
    p.work = work;
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

pub struct ListEntry<'a> {
    pub num: u32,
    pub text: &'a str,
    pub body: &'a str,
    pub first: &'a str,
}

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

const ONE_COLUMN_LINES: usize = 8;

pub struct ListLook<'a> {
    pub st: &'a Style,
    pub font: &'a Font,
    pub sc: f64,
    pub side: NotesSide,
    pub title: Option<&'a str>,
}

pub fn list_items(env: &mut Env, entries: &[ListEntry], ext: Box) -> Vec<Item> {
    let look =
        ListLook { st: env.style, font: env.font, sc: env.s, side: env.spec.notes_side, title: env.spec.notes_title };
    list_block(&look, entries, ext, &mut env.unknown_glyph)
}

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
