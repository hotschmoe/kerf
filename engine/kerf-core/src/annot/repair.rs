//! Layout repair (SPEC 20): re-rendering the dimension labels and the repair moves for a dimension or label that sits on a note leader.

use super::dims::{DimSpec, base_hits, label_items, polys_overlap, stack_dims};
use super::notes::{self, LabelSpec, Meta, MetaKind, NotePrep, Obstacle, clear_of_leaders, dirs8};
use super::text::text_poly;
use super::{Env, Landing, NoteIn, items_box};
use crate::drawing::Item;
use crate::geom::{Box, V2};
use crate::json;
use crate::math;
use crate::model::Level;
use crate::pen::Pen;
use crate::route;
use crate::units;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// One rendering of the dimensions and labels (stacked, with the repair pushes applied) plus what the router needs.
pub struct Render<'a> {
    pub per: Vec<Vec<Item>>,
    pub obstacles: Vec<Obstacle>,
    pub soft: Vec<[V2; 2]>,
    pub knock: Vec<[V2; 4]>,
    pub ext: Box,
    pub meta: Vec<Meta<'a>>,
    /// Per dimension: what it collides with at its authored offset (the stacking moved it out).
    pub dim_blockers: Vec<Vec<String>>,
}

pub struct DimLab<'d, 'a> {
    pub dspecs: &'d [DimSpec<'a>],
    pub lspecs: &'d [LabelSpec<'a>],
    pub base_segs: &'d [[V2; 2]],
}

pub fn render_dim_labels<'a>(
    env: &mut Env,
    dl: &DimLab<'_, 'a>,
    per_in: &[Vec<Item>],
    meta_in: &[Meta<'a>],
    dpush: &[f64],
    dslide: &[f64],
    lpush: &[V2],
) -> Render<'a> {
    let mut per: Vec<Vec<Item>> = per_in.to_vec();
    let mut meta: Vec<Meta<'a>> = meta_in.to_vec();
    let mut eff = alloc::vec![0.0; dl.dspecs.len()];
    let mut ditems: Vec<Vec<Item>> = (0..dl.dspecs.len()).map(|_| Vec::new()).collect();
    let mut label_polys: Vec<(&str, [V2; 4])> = Vec::new();
    for (i, l) in dl.lspecs.iter().enumerate() {
        let mut its = Vec::new();
        let push = lpush.get(i).copied().unwrap_or_default();
        if let Some(p) = l.base {
            label_items(env, l.id, l.text, p.add(push), l.align, &mut its);
        }
        for it in &its {
            if let Item::Text(t) = it {
                label_polys.push((l.id, text_poly(env.font, t, 0.02 * env.s)));
            }
        }
        if let Some(slot) = per.get_mut(l.k) {
            *slot = its;
        }
        if let Some(m) = meta.get_mut(l.k) {
            m.off = l.off.x + push.x;
            m.off2 = l.off.y + push.y;
        }
    }
    let mut dim_blockers: Vec<Vec<String>> = (0..dl.dspecs.len()).map(|_| Vec::new()).collect();
    stack_dims(env, dl.dspecs, dpush, dslide, dl.base_segs, &label_polys, &mut ditems, &mut eff, &mut dim_blockers);
    for (i, d) in dl.dspecs.iter().enumerate() {
        if let Some(slot) = per.get_mut(d.k) {
            *slot = core::mem::take(&mut ditems[i]);
        }
        if let Some(m) = meta.get_mut(d.k) {
            m.off = eff[i];
        }
    }
    let mut ext = env.crop;
    let mut obstacles = Vec::new();
    let mut soft = Vec::new();
    let mut knock = Vec::new();
    for (its, m) in per.iter().zip(&meta) {
        ext.add_box(items_box(env.font, its));
        for it in its {
            match it {
                Item::Text(t) => {
                    knock.push(text_poly(env.font, t, 0.02 * env.s));
                    obstacles.push(Obstacle {
                        poly: text_poly(env.font, t, 0.0),
                        id: t.src.clone(),
                        kind: if m.kind == MetaKind::Label { route::ObstKind::Label } else { route::ObstKind::Dim },
                        text: t.s.clone(),
                        owner: m.owner,
                        axis: m.axis,
                        off: m.off,
                        off2: m.off2,
                    });
                }
                Item::Path(p) if m.kind == MetaKind::Dim && p.pen == Pen::Dim && p.pts.len() == 2 => {
                    soft.push([p.pts[0].v(), p.pts[1].v()]);
                }
                _ => {}
            }
        }
    }
    Render { per, obstacles, soft, knock, ext, meta, dim_blockers }
}

/// Where a moved dimension text or label may go: at least a text height `h` from every leader, and off every note
/// text block (or key tag).
pub struct Clearance<'l> {
    pub leaders: &'l [[V2; 3]],
    pub notes: Vec<[V2; 4]>,
    pub h: f64,
}

impl Clearance<'_> {
    fn new<'l>(r: &'l route::Layout, prep: &NotePrep) -> Clearance<'l> {
        let notes =
            r.x.iter()
                .zip(&r.top)
                .zip(&prep.sizes)
                .map(|((&x, &top), &(w, hgt))| {
                    [V2::init(x, top - hgt), V2::init(x + w, top - hgt), V2::init(x + w, top), V2::init(x, top)]
                })
                .collect();
        Clearance { leaders: &r.leaders, notes, h: prep.g.h }
    }

    fn ok(&self, poly: &[V2; 4], shift: V2) -> bool {
        if !clear_of_leaders(self.leaders, poly, shift, self.h) {
            return false;
        }
        let q = poly.map(|p| p.add(shift));
        !self.notes.iter().any(|b| polys_overlap(&q, b))
    }
}

/// Clear a dimension's text off the note leaders that hit it. First the text slides along the dimension line (in
/// half text heights, staying between the extension lines), so the line stays where the author put it; text that does
/// not fit moves to the other end first (`slide` +1 / -1 then marks the end it avoids). When neither clears it, the
/// dimension is pushed out by whole dimension spacings (0.25 paper inch). A pinned dimension stays.
pub fn repair_dim(
    env: &Env,
    o: &Obstacle,
    cl: &Clearance,
    dpush: &mut [f64],
    dslide: &mut [f64],
    dspecs: &[DimSpec],
) -> bool {
    let h = cl.h;
    let step = 0.25 * env.s;
    let cap = 8.0 * step;
    let (Some(cur), Some(slide), Some(spec)) = (dpush.get_mut(o.owner), dslide.get_mut(o.owner), dspecs.get(o.owner))
    else {
        return false;
    };
    if spec.pin {
        return false;
    }
    if *slide == 0.0 {
        let u = spec.along();
        let (lo, hi) = o.poly.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            let t = p.dot(u);
            (lo.min(t), hi.max(t))
        });
        let tick = env.style.tick_len_in * env.s;
        if hi - lo + 2.0 * env.style.dim_text_gap_in * env.s > spec.span() - tick {
            let mid = spec.from.mid(spec.to).dot(u);
            *slide = if 0.5 * (lo + hi) > mid { 1.0 } else { -1.0 };
            return true;
        }
        let room = 0.5 * (spec.span() - (hi - lo)) - tick;
        let mut k = 1.0;
        while k * 0.5 * h <= room {
            for sg in [1.0, -1.0] {
                let d = sg * k * 0.5 * h;
                if cl.ok(&o.poly, u.scale(d)) {
                    *slide = d;
                    return true;
                }
            }
            k += 1.0;
        }
    }
    if *cur + step > cap + 1e-9 {
        return false;
    }
    let sgn = spec.sgn();
    let mut k = 1.0;
    while *cur + k * step <= cap + 1e-9 {
        if cl.ok(&o.poly, o.axis.scale(sgn * k * step)) {
            *cur += k * step;
            return true;
        }
        k += 1.0;
    }
    false
}

/// Labels move at most this many text heights from where the author put them (SPEC 25.2).
const LABEL_MOVE_CAP: f64 = 6.0;

/// Move a label (smallest move first, up/down before sideways) to where no leader comes within a text height
/// and it overprints no other label or dimension text; prefers a spot that does not sit on drawn outlines.
/// A pinned label never moves.
pub fn repair_label(o: &Obstacle, obsts: &[Obstacle], cl: &Clearance, lpush: &mut [V2], dl: &DimLab) -> bool {
    let h = cl.h;
    let dirs = dirs8();
    let cap = LABEL_MOVE_CAP * h;
    if dl.lspecs.get(o.owner).is_none_or(|l| l.pin) {
        return false;
    }
    let base_segs = dl.base_segs;
    let Some(cur) = lpush.get_mut(o.owner) else { return false };
    for relax in 0..2 {
        let mut k = 1.0f64;
        while k * 0.5 * h <= cap {
            let delta = crate::math::ceil(k * 0.5 * h * 4.0) / 4.0;
            for dv in dirs {
                let dn = dv.norm().scale(delta);
                if (cur.x + dn.x).abs() > cap || (cur.y + dn.y).abs() > cap {
                    continue;
                }
                if !cl.ok(&o.poly, dn) {
                    continue;
                }
                let q = o.poly.map(|p| p.add(dn));
                let blocked = obsts.iter().any(|ob| {
                    !(ob.kind == route::ObstKind::Label && ob.owner == o.owner) && polys_overlap(&q, &ob.poly)
                });
                if blocked {
                    continue;
                }
                if relax == 0 && base_hits(base_segs, &q) > 0 {
                    continue;
                }
                *cur = cur.add(dn);
                return true;
            }
            k += 1.0;
        }
    }
    false
}

pub struct Best<'a> {
    pub r: route::Layout,
    pub rd: Render<'a>,
    /// The repair pushes of this pass (dimension: outward distance; label: shift) and, per pushed item, the note whose
    /// leader it was moved away from.
    pub dpush: Vec<f64>,
    pub lpush: Vec<V2>,
    pub dnote: Vec<Option<usize>>,
    pub lnote: Vec<Option<usize>>,
}

/// Routes the notes, then repairs what hits dimension text or labels: pushes those dimensions out and moves those
/// labels, re-routing after each pass (at most `MAX_REPAIR_PASSES`, stopping when clean or after two passes in a row
/// that did not get better). The pass with the fewest hits wins.
/// Passes of re-routing the repair loop makes at most.
const MAX_REPAIR_PASSES: usize = 8;

pub fn solve<'a>(
    env: &mut Env<'a, '_>,
    notes: &[NoteIn<'a>],
    prep: &NotePrep,
    dl: &DimLab<'_, 'a>,
    per: &[Vec<Item>],
    meta: &[Meta<'a>],
) -> Option<Best<'a>> {
    let mut dpush = alloc::vec![0.0f64; dl.dspecs.len()];
    let mut dslide = alloc::vec![0.0f64; dl.dspecs.len()];
    let mut lpush = alloc::vec![V2::init(0.0, 0.0); dl.lspecs.len()];
    let mut dnote: Vec<Option<usize>> = alloc::vec![None; dl.dspecs.len()];
    let mut lnote: Vec<Option<usize>> = alloc::vec![None; dl.lspecs.len()];
    // route, then repair what hits dimension text or labels (bounded, deterministic)
    let mut best: Option<Best<'a>> = None;
    let mut stale = 0usize;
    for pass in 0..MAX_REPAIR_PASSES {
        let rd = render_dim_labels(env, dl, per, meta, &dpush, &dslide, &lpush);
        let r = notes::route_notes(env, prep, notes, rd.ext, &rd.obstacles, &rd.soft, pass > 0);
        let better = best.as_ref().is_none_or(|b| r.hits.len() < b.r.hits.len());
        if better {
            stale = 0;
        } else {
            stale += 1;
        }
        // stop when clean, or when two passes in a row did not get better
        let stop = r.hits.is_empty() || stale >= 2;
        let mut changed = false;
        let snap = better.then(|| (dpush.clone(), lpush.clone(), dnote.clone(), lnote.clone()));
        if !stop {
            let mut touched_d = alloc::vec![false; dl.dspecs.len()];
            let mut touched_l = alloc::vec![false; dl.lspecs.len()];
            let cl = Clearance::new(&r, prep);
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
                    if repair_dim(env, o, &cl, &mut dpush, &mut dslide, dl.dspecs) {
                        changed = true;
                        if let Some(dn) = dnote.get_mut(o.owner) {
                            dn.get_or_insert(ht.note);
                        }
                    }
                } else {
                    let Some(t) = touched_l.get_mut(o.owner) else { continue };
                    if *t {
                        continue;
                    }
                    *t = true;
                    if repair_label(o, &rd.obstacles, &cl, &mut lpush, dl) {
                        changed = true;
                        if let Some(ln) = lnote.get_mut(o.owner) {
                            ln.get_or_insert(ht.note);
                        }
                    }
                }
            }
        }
        if let Some((dpush, lpush, dnote, lnote)) = snap {
            best = Some(Best { r, rd, dpush, lpush, dnote, lnote });
        }
        if stop || !changed {
            break;
        }
    }
    best
}

/// Moves below this many paper inches are not reported (SPEC 25.2).
const MOVE_REPORT_IN: f64 = 0.125;

fn num2(x: f64) -> String {
    json::number_string(math::round(x * 100.0) / 100.0)
}

fn paper(x: f64, s: f64) -> String {
    json::number_string(math::round(x / s * 100.0) / 100.0)
}

fn and_list(items: &[String]) -> String {
    match items {
        [] => "other annotations".to_string(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// I_ANNOT_MOVED (SPEC 25.2): every dimension the stacking or the repair drew more than 1/8" (paper) away from its
/// authored offset, and every label the repair moved that far, with what it was moved away from and how to keep it.
pub fn report_moves(env: &Env, notes: &[NoteIn], dl: &DimLab, best: &Best) {
    let vid = env.spec.id;
    let s = env.s;
    let tol = MOVE_REPORT_IN * s + 1e-9;
    for (i, d) in dl.dspecs.iter().enumerate() {
        let Some(eff) = best.rd.meta.get(d.k).map(|m| m.off) else { continue };
        let moved = (eff - d.off0).abs();
        if moved <= tol {
            continue;
        }
        let mut why = best.rd.dim_blockers.get(i).cloned().unwrap_or_default();
        if best.dpush.get(i).is_some_and(|&p| p > 0.0)
            && let Some(n) = best.dnote.get(i).copied().flatten().and_then(|n| notes.get(n))
        {
            why.push(format!("the leader of note '{}'", n.id));
        }
        let chained: Vec<String> =
            dl.dspecs.iter().filter(|o| o.chain == d.chain && o.id != d.id).map(|o| format!("'{}'", o.id)).collect();
        let from = if chained.is_empty() || (d.off0 - d.authored).abs() < 1e-9 {
            format!("its offset {}", num2(d.off0))
        } else {
            format!(
                "offset {} (its chain with {} lines up there; written {})",
                num2(d.off0),
                and_list(&chained),
                num2(d.authored)
            )
        };
        let reason = if why.is_empty() && !chained.is_empty() {
            format!("together with its chain ({})", and_list(&chained))
        } else {
            format!("to clear {}", and_list(&why))
        };
        env.diags.add_fix(
            Level::Info,
            "I_ANNOT_MOVED",
            Some(d.id),
            Some(&format!("views/{vid}/annotations/{}/offset", d.id)),
            format!(
                "view {vid}: dimension '{}' is drawn at offset {} instead of {from}: moved {} ({}\" paper) outward {reason}",
                d.id,
                num2(eff),
                units::fmt_ft_in(moved),
                paper(moved, s),
            ),
            format!(
                "keep it there: set \"offset\": {}; or keep {} by moving what it collides with, or set \"pin\": true (drawn exactly at {}, overlaps then warn W_LABEL_OVERLAP)",
                num2(eff),
                num2(d.off0),
                num2(d.off0)
            ),
        );
    }
    let iso = matches!(env.landing, Landing::Iso(_));
    for (i, l) in dl.lspecs.iter().enumerate() {
        let Some(push) = best.lpush.get(i).copied() else { continue };
        if push.len() <= tol {
            continue;
        }
        let note = best.lnote.get(i).copied().flatten().and_then(|n| notes.get(n));
        let why = note.map_or_else(|| "note leaders".to_string(), |n| format!("the leader of note '{}'", n.id));
        let keep = if iso {
            String::new()
        } else {
            format!(
                "keep it there: set \"offset\": [{}, {}] (now [{}, {}]); ",
                num2(l.off.x + push.x),
                num2(l.off.y + push.y),
                num2(l.off.x),
                num2(l.off.y)
            )
        };
        let mover = note.map_or_else(|| "the note".to_string(), |n| format!("note '{}'", n.id));
        env.diags.add_fix(
            Level::Info,
            "I_ANNOT_MOVED",
            Some(l.id),
            Some(&format!("views/{vid}/annotations/{}", l.id)),
            format!(
                "view {vid}: label '{}' ('{}') is drawn {} ({}\" paper) from where its at and offset put it, to clear {why}",
                l.id,
                l.text,
                units::fmt_ft_in(push.len()),
                paper(push.len(), s)
            ),
            format!(
                "{keep}or keep it where you put it: move {mover} (\"place\" or \"at\") or set \"pin\": true on the label (a leader through it then warns W_LEADER_HIT)"
            ),
        );
    }
}

/// W_LABEL_OVERLAP (SPEC 25.2): a label whose text overprints another label or a dimension's text, a pinned
/// dimension whose text overprints another dimension's text, and a note's text (or key tag) on a label or dimension
/// text. Proposes a label offset that clears it.
pub fn report_overlaps(env: &Env, notes: &[NoteIn], prep: &NotePrep, dl: &DimLab, best: &Best) {
    let rd = &best.rd;
    let vid = env.spec.id;
    let h = env.style.label_height_in * env.s;
    let iso = matches!(env.landing, Landing::Iso(_));
    let obs = &rd.obstacles;
    let pinned_dim = |o: &Obstacle| o.kind == route::ObstKind::Dim && dl.dspecs.get(o.owner).is_some_and(|d| d.pin);
    let mut reported = 0usize;
    for (i, a) in obs.iter().enumerate() {
        for b in &obs[i + 1..] {
            let a_label = a.kind == route::ObstKind::Label;
            let b_label = b.kind == route::ObstKind::Label;
            if !(a_label || b_label || pinned_dim(a) || pinned_dim(b)) || a.id == b.id {
                continue;
            }
            if !polys_overlap(&a.poly, &b.poly) {
                continue;
            }
            reported += 1;
            if reported > 8 {
                continue;
            }
            let (mv, other) = if b_label && !a_label { (b, a) } else { (a, b) };
            let name = |o: &Obstacle| {
                if o.kind == route::ObstKind::Label {
                    format!("label '{}' ('{}')", o.id, o.text)
                } else {
                    format!("the text '{}' of dimension '{}'", o.text, o.id)
                }
            };
            let mut fix = String::new();
            if mv.kind == route::ObstKind::Label && !iso {
                'search: for k in 1..=24u32 {
                    let delta = math::ceil(f64::from(k) * 0.5 * h * 4.0) / 4.0;
                    for dv in dirs8() {
                        let dn = dv.norm().scale(delta);
                        let q = mv.poly.map(|p| p.add(dn));
                        if obs.iter().all(|o| o.id == mv.id || !polys_overlap(&q, &o.poly)) {
                            fix = format!(
                                "set label '{}' \"offset\": [{}, {}] (now [{}, {}])",
                                mv.id,
                                num2(mv.off + dn.x),
                                num2(mv.off2 + dn.y),
                                num2(mv.off),
                                num2(mv.off2)
                            );
                            break 'search;
                        }
                    }
                }
            }
            if fix.is_empty() {
                fix = format!(
                    "move one of them: a label with its \"offset\", a dimension with its \"offset\" (or drop \"pin\" on '{}')",
                    mv.id
                );
            }
            env.diags.add_fix(
                Level::Warning,
                "W_LABEL_OVERLAP",
                Some(mv.id.as_str()),
                Some(&format!("views/{vid}/annotations/{}", mv.id)),
                format!("view {vid}: {} overprints {}", name(mv), name(other)),
                fix,
            );
        }
    }
    let blocks = Clearance::new(&best.r, prep).notes;
    for (n, b) in notes.iter().zip(&blocks) {
        for o in obs.iter().filter(|o| polys_overlap(b, &o.poly)) {
            reported += 1;
            if reported > 8 {
                continue;
            }
            let what = if o.kind == route::ObstKind::Label {
                format!("label '{}' ('{}')", o.id, o.text)
            } else {
                format!("the text '{}' of dimension '{}'", o.text, o.id)
            };
            let fix = if n.place.is_some() {
                format!(
                    "move note '{}' (its \"place\") or {}",
                    n.id,
                    if o.kind == route::ObstKind::Label {
                        "the label (its \"offset\")"
                    } else {
                        "the dimension (its \"offset\")"
                    }
                )
            } else {
                format!(
                    "give note '{}' a \"place\" clear of it, or move {} (its \"offset\")",
                    n.id,
                    if o.kind == route::ObstKind::Label { "the label" } else { "the dimension" }
                )
            };
            env.diags.add_fix(
                Level::Warning,
                "W_LABEL_OVERLAP",
                Some(n.id),
                Some(&format!("views/{vid}/annotations/{}", n.id)),
                format!("view {vid}: the text of note '{}' overprints {what}", n.id),
                fix,
            );
        }
    }
    if reported > 8 {
        env.diags.add(
            Level::Warning,
            "W_LABEL_OVERLAP",
            None,
            Some(&format!("views/{vid}")),
            format!("view {vid}: {} more overprinting labels not listed; fix the ones above first", reported - 8),
        );
    }
}
