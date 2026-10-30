//! Layout repair (SPEC 20): re-rendering the dimension labels and the repair moves for a dimension or label that sits on a note leader.

use super::dims::{DimSpec, base_hits, label_items, polys_overlap, stack_dims};
use super::notes::{LabelSpec, Meta, MetaKind, Obstacle, clear_of_leaders, dirs8};
use super::text::text_poly;
use super::{Env, items_box};
use crate::drawing::Item;
use crate::geom::{Box, V2};
use crate::pen::Pen;
use crate::route;
use alloc::vec::Vec;

/// One rendering of the dimensions and labels (stacked, with the repair pushes applied) plus what the router needs.
pub struct Render<'a> {
    pub per: Vec<Vec<Item>>,
    pub eff: Vec<f64>,
    pub obstacles: Vec<Obstacle>,
    pub soft: Vec<[V2; 2]>,
    pub knock: Vec<[V2; 4]>,
    pub ext: Box,
    pub meta: Vec<Meta<'a>>,
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
    lpush: &[V2],
) -> Render<'a> {
    let mut per: Vec<Vec<Item>> = per_in.to_vec();
    let mut meta: Vec<Meta<'a>> = meta_in.to_vec();
    let mut eff = alloc::vec![0.0; dl.dspecs.len()];
    let mut ditems: Vec<Vec<Item>> = (0..dl.dspecs.len()).map(|_| Vec::new()).collect();
    let mut label_polys: Vec<[V2; 4]> = Vec::new();
    for (i, l) in dl.lspecs.iter().enumerate() {
        let mut its = Vec::new();
        let push = lpush.get(i).copied().unwrap_or_default();
        if let Some(p) = l.base {
            label_items(env, l.id, l.text, p.add(push), &mut its);
        }
        for it in &its {
            if let Item::Text(t) = it {
                label_polys.push(text_poly(env.font, t, 0.02 * env.s));
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
    stack_dims(env, dl.dspecs, dpush, dl.base_segs, &label_polys, &mut ditems, &mut eff);
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
    Render { per, eff, obstacles, soft, knock, ext, meta }
}

/// Push a dimension out by whole dimension spacings (0.25 paper inch) until its text clears every leader.
pub fn repair_dim(env: &Env, o: &Obstacle, leaders: &[[V2; 3]], h: f64, dpush: &mut [f64], dspecs: &[DimSpec]) -> bool {
    let step = 0.25 * env.s;
    let cap = 8.0 * step;
    let (Some(cur), Some(spec)) = (dpush.get_mut(o.owner), dspecs.get(o.owner)) else { return false };
    if *cur + step > cap + 1e-9 {
        return false;
    }
    let sgn = spec.sgn();
    let mut chosen = 1.0;
    let mut k = 1.0;
    while k <= 6.0 {
        if *cur + k * step > cap + 1e-9 {
            break;
        }
        if clear_of_leaders(leaders, &o.poly, o.axis.scale(sgn * k * step), h) {
            chosen = k;
            break;
        }
        k += 1.0;
    }
    *cur += chosen * step;
    true
}

/// Move a label (smallest move first, up/down before sideways) to where no leader comes within a text height
/// and it overprints no other label or dimension text; prefers a spot that does not sit on drawn outlines.
pub fn repair_label(
    o: &Obstacle,
    obsts: &[Obstacle],
    leaders: &[[V2; 3]],
    h: f64,
    lpush: &mut [V2],
    base_segs: &[[V2; 2]],
) -> bool {
    let dirs = dirs8();
    let cap = 12.0 * h;
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
                if !clear_of_leaders(leaders, &o.poly, dn, h) {
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
}
