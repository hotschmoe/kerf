//! W_CROP_STALE: a component whose visible extent mostly falls outside an explicit section crop. A crop that cuts a
//! member on purpose (break lines) is not stale: the warning needs an edit that moved or resized the member out of a
//! crop it sat in, or a member of which less than 10% stays inside every view that draws it.

use crate::geom::{self, Box};
use crate::json::{self, Value};
use crate::model::{Diags, Level};
use crate::scene::{Comp, Scene, State};
use crate::units;
use crate::view::{self, Kind, ViewSpec};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;

const STALE_FRACTION: f64 = 0.25;
/// Outside fraction past which a member is lost from the view rather than cut by it.
const LOST_FRACTION: f64 = 0.9;

/// The scene and document as they were before an edit, so an edit that newly pushes a component out of the crop is reported.
pub struct Before<'a> {
    pub scene: &'a Scene<'a>,
    pub doc: &'a Value,
}

fn visible_extent(c: &Comp<'_>, cut_z: f64) -> Box {
    let mut b = Box::default();
    for p in &c.world {
        if p.role == crate::style::Role::Soil {
            continue;
        }
        let is_cut = p.z0 < cut_z - 1e-9 && p.z1 > cut_z + 1e-9;
        if !is_cut && p.z1 > cut_z + 1e-9 {
            continue;
        }
        for l in &p.loops {
            b.add_box(geom::points_box(l));
        }
        b.add_box(geom::points_box(&p.line_pts));
        b.add_box(geom::points_box(&p.centerline));
    }
    b
}

fn outside_fraction(b: Box, crop: Box) -> f64 {
    let eps = 1e-6;
    let fx = inside1(b.x0, b.x1, crop.x0, crop.x1, eps);
    let fy = inside1(b.y0, b.y1, crop.y0, crop.y1, eps);
    1.0 - fx * fy
}

fn inside1(lo: f64, hi: f64, c0: f64, c1: f64, eps: f64) -> f64 {
    if hi - lo < eps {
        return if lo >= c0 - eps && lo <= c1 + eps { 1.0 } else { 0.0 };
    }
    (hi.min(c1) - lo.max(c0)).max(0.0) / (hi - lo)
}

/// The cut plane W_CROP_STALE measures a view's members against: a section's cut_z, nothing for an elevation (every
/// member shows); plan and iso views are not checked (their crops are not in model x/y).
fn crop_cut(spec: &ViewSpec<'_>) -> Option<f64> {
    match spec.kind {
        Kind::Section => Some(spec.cut_z),
        Kind::Elevation => Some(f64::INFINITY),
        _ => None,
    }
}

fn explicit_crop(doc: &Value, id: &str) -> Option<Box> {
    let node = view::find_view(doc, id)?;
    let spec = view::parse(node, 0, &Diags::new())?;
    (crop_cut(&spec).is_some() && spec.has_crop).then_some(spec.crop)
}

fn shown_somewhere(c: &Comp<'_>, vs: &[Value], max_outside: f64) -> bool {
    for (vi, vnode) in vs.iter().enumerate() {
        let Some(spec) = view::parse(vnode, vi, &Diags::new()) else { continue };
        if !spec.members.draws(c) {
            continue;
        }
        let Some(cut) = crop_cut(&spec).filter(|_| spec.has_crop) else {
            return true;
        };
        let ext = visible_extent(c, cut);
        if !ext.is_empty() && outside_fraction(ext, spec.crop) <= max_outside {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq)]
enum Prior {
    None,
    Added,
    /// Mostly inside the crop before the edit, with this extent.
    Inside(Box),
    Cut,
}

fn prior(before: Option<&Before<'_>>, c: &Comp<'_>, spec: &ViewSpec<'_>) -> Prior {
    let Some(bf) = before else { return Prior::None };
    let Some(old) = bf.scene.find(&c.id) else { return Prior::Added };
    let old_ext = visible_extent(old, crop_cut(spec).unwrap_or(spec.cut_z));
    if old_ext.is_empty() || view::find_view(bf.doc, spec.id).is_none() {
        return Prior::None;
    }
    let Some(old_crop) = explicit_crop(bf.doc, spec.id) else { return Prior::Inside(old_ext) };
    if outside_fraction(old_ext, old_crop) > STALE_FRACTION { Prior::Cut } else { Prior::Inside(old_ext) }
}

fn same_box(a: Box, b: Box) -> bool {
    let e = 1e-6;
    (a.x0 - b.x0).abs() < e && (a.x1 - b.x1).abs() < e && (a.y0 - b.y0).abs() < e && (a.y1 - b.y1).abs() < e
}

fn num(x: f64) -> String {
    json::number_string(x)
}

pub fn run(scene: &Scene<'_>, doc: &Value, before: Option<&Before<'_>>, diags: &Diags) {
    let Some(vs) = doc.get("views").and_then(Value::arr) else { return };
    let mut reported = vec![false; scene.comps.len()];
    for (vi, vnode) in vs.iter().enumerate() {
        let Some(spec) = view::parse(vnode, vi, &Diags::new()) else { continue };
        let Some(cut) = crop_cut(&spec).filter(|_| spec.has_crop) else {
            continue;
        };
        for c in &scene.comps {
            if c.state != State::Ok || !spec.members.draws(c) {
                continue;
            }
            let ext = visible_extent(c, cut);
            if ext.is_empty() {
                continue;
            }
            let frac = outside_fraction(ext, spec.crop);
            if frac <= STALE_FRACTION {
                continue;
            }
            let was = reported.get(c.index as usize).copied().unwrap_or(false);
            let moved = match prior(before, c, &spec) {
                Prior::Cut => continue,
                Prior::Inside(old) => !same_box(old, ext),
                Prior::Added | Prior::None => false,
            };
            if !moved && (was || frac <= LOST_FRACTION || shown_somewhere(c, vs, LOST_FRACTION)) {
                continue;
            }
            if let Some(r) = reported.get_mut(c.index as usize) {
                *r = true;
            }
            let mut need = spec.crop;
            need.add_box(ext);
            let fit = format!(
                "{{\"x\":[{},{}],\"y\":[{},{}]}}",
                num(crate::math::floor(need.x0)),
                num(crate::math::ceil(need.x1)),
                num(crate::math::floor(need.y0)),
                num(crate::math::ceil(need.y1))
            );
            let mut buf = [0u8; 40];
            let pct = json::fmt_fixed(&mut buf, frac * 100.0, 0).to_string();
            let lead = if before.is_some() { "after this edit, " } else { "" };
            let message = format!(
                "{lead}'{}' has {pct}% of its extent outside the crop of view {}: it spans x {}..{}, y {}..{}, the crop is x {}..{}, y {}..{}",
                c.id,
                spec.id,
                units::fmt_ft_in(ext.x0),
                units::fmt_ft_in(ext.x1),
                units::fmt_ft_in(ext.y0),
                units::fmt_ft_in(ext.y1),
                units::fmt_ft_in(spec.crop.x0),
                units::fmt_ft_in(spec.crop.x1),
                units::fmt_ft_in(spec.crop.y0),
                units::fmt_ft_in(spec.crop.y1),
            );
            let fix = format!(
                "remove \"crop\" (and \"scale\", if you set one) from view {}: the engine then re-fits the view to all non-fill components + 6\"; or set \"crop\" to {fit} to contain '{}'",
                spec.id, c.id
            );
            diags.add_fix(
                Level::Warning,
                "W_CROP_STALE",
                Some(&c.id),
                Some(&format!("views/{}/crop", spec.id)),
                message,
                fix,
            );
        }
    }
}
