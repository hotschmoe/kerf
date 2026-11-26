use super::{Drawn, Pf, axis_aligned, z_gap};
use crate::catalog::Type;
use crate::geom::{self, V2};
use crate::model::{Diags, Kind, Level, fmt_num};
use crate::style::Role;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const TOUCH: f64 = 1.0 / 32.0;

struct Support {
    a: f64,
    b: f64,
    ids: Vec<String>,
}

fn is_support(p: &Pf<'_>) -> bool {
    p.prism.kind == Kind::Body
        && !p.prism.embedded
        && !p.prism.dashed
        && !p.comp.ty.traits.hardware
        && !p.comp.ty.traits.line_like
        && p.prism.role != Role::Soil
}

pub(super) fn check(s: &Drawn<'_, '_>, diags: &Diags) {
    for c in &s.comps {
        let [end_min, mid_min] = c.ty.traits.min_bearing;
        if end_min <= 0.0 || c.dashed || !axis_aligned(c) {
            continue;
        }
        let flanges: Vec<&&Pf<'_>> =
            s.items.iter().filter(|p| p.comp.index == c.index && p.prism.part == "bottom_flange").collect();
        let mut reported = false;
        for fl in flanges {
            if reported || !fl.prism.lengthwise {
                continue;
            }
            let (x0, x1, y) = (fl.box_.x0, fl.box_.x1, fl.box_.y0);
            let mut spans: Vec<Support> = Vec::new();
            for q in &s.items {
                if q.comp.index == c.index || !is_support(q) || z_gap(fl.prism, q.prism) > 0.0 {
                    continue;
                }
                if (q.prism.z1.min(fl.prism.z1) - q.prism.z0.max(fl.prism.z0)) < 1e-6 {
                    continue;
                }
                if (q.box_.y1 - y).abs() > TOUCH || q.box_.y0 >= y {
                    continue;
                }
                let a = q.box_.x0.max(x0);
                let b = q.box_.x1.min(x1);
                if b - a <= 1e-6 {
                    continue;
                }
                spans.push(Support { a, b, ids: alloc::vec![q.comp.id.clone()] });
            }
            crate::sort::stable(&mut spans, |p, q| p.a < q.a);
            let mut merged: Vec<Support> = Vec::new();
            for sp in spans {
                match merged.last_mut() {
                    Some(m) if sp.a <= m.b + TOUCH => {
                        m.b = m.b.max(sp.b);
                        for id in sp.ids {
                            if !m.ids.contains(&id) {
                                m.ids.push(id);
                            }
                        }
                    }
                    _ => merged.push(sp),
                }
            }
            for m in &merged {
                let len = m.b - m.a;
                let left = m.a <= x0 + TOUCH;
                let right = m.b >= x1 - TOUCH;
                let need = if left || right { end_min } else { mid_min };
                if len >= need - 1e-6 || (left && right) {
                    continue;
                }
                let on = m.ids.join("', '");
                let (where_, fix) = if left || right {
                    let side = if left { "left" } else { "right" };
                    let short = need - len;
                    let fix = format!(
                        "lengthen '{}' by {} at its {side} end so it reaches x = {} over '{on}' (its bottom flange then bears {}), or support that end with a hanger; acknowledge W_BEARING if the joist is detailed short on purpose",
                        c.id,
                        fmt_ft_in(short),
                        fmt_num(if left { m.b - need } else { m.a + need }),
                        fmt_ft_in(need)
                    );
                    (format!("at its {side} end"), fix)
                } else {
                    let fix = format!(
                        "widen the support under '{}' to at least {} (e.g. a 2x4 or wider plate), or add squash blocks / a bearing block per the manufacturer; acknowledge W_BEARING if this is not a bearing point",
                        c.id,
                        fmt_ft_in(need)
                    );
                    (format!("at an intermediate support (x {}..{})", fmt_num(m.a), fmt_num(m.b)), fix)
                };
                diags.add_fix(
                    Level::Warning,
                    "W_BEARING",
                    Some(&c.id),
                    None,
                    format!(
                        "'{}' bears {} on '{on}' {where_}; an I-joist needs {} of bearing there (manufacturer minimum: {} at ends, {} at intermediate supports)",
                        c.id,
                        fmt_ft_in(len),
                        fmt_ft_in(need),
                        fmt_ft_in(end_min),
                        fmt_ft_in(mid_min)
                    ),
                    fix,
                );
                reported = true;
            }
        }
    }
}

const PROBE: f64 = 1.0 / 16.0;
const PROBES: usize = 9;

fn inside(q: &Pf<'_>, p: V2) -> bool {
    q.box_.expand(1e-6).contains(p) && geom::locate_even_odd(p, &q.flat, 0.0) != geom::Loc::Outside
}

fn beside(p: &Pf<'_>, q: &Pf<'_>) -> bool {
    q.comp.index != p.comp.index
        && q.prism.kind != Kind::Ghost
        && q.prism.z1.min(p.prism.z1) - q.prism.z0.max(p.prism.z0) > 1e-6
}

fn along(p: &Pf<'_>, q: &Pf<'_>, y: f64) -> bool {
    let b = p.box_;
    (0..PROBES).any(|k| inside(q, V2::init(b.x0 + b.width() * (k as f64 + 0.5) / PROBES as f64, y)))
}

fn flat_plates<'s, 'p, 'a>(s: &'s Drawn<'p, 'a>) -> impl Iterator<Item = &'s &'p Pf<'a>> {
    s.items.iter().filter(|p| {
        p.comp.ty.ty == Type::Lumber
            && !p.comp.dashed
            && axis_aligned(p.comp)
            && p.prism.kind == Kind::Body
            && !p.prism.quads.is_empty()
            && !p.prism.blocking
            && !p.prism.embedded
            && p.box_.height() < p.box_.width()
    })
}

/// The flat plates of this drawn set with something under them: a plate seated in one view is seated (SPEC 30.4).
pub(super) fn seated(s: &Drawn<'_, '_>) -> Vec<u32> {
    flat_plates(s)
        .filter(|p| s.items.iter().chain(&s.hidden).any(|q| beside(p, q) && along(p, q, p.box_.y0 - PROBE)))
        .map(|p| p.comp.index)
        .collect()
}

/// A flat plate cut end-on that hangs off the face of one member, carries a wood member and has nothing under it.
pub(super) fn unsupported(s: &Drawn<'_, '_>, seated: &[u32], diags: &Diags) {
    let mut told: Vec<u32> = Vec::new();
    for p in flat_plates(s) {
        let (c, b) = (p.comp, p.box_);
        if seated.contains(&c.index) || told.contains(&c.index) {
            continue;
        }
        let wood = |q: &Pf<'_>| matches!(q.comp.ty.ty, Type::Lumber | Type::Truss | Type::Ijoist) && !q.prism.dashed;
        let Some(load) = s.items.iter().find(|q| beside(p, q) && wood(q) && along(p, q, b.y1 + PROBE)) else { continue };
        let mid = (b.y0 + b.y1) / 2.0;
        let at_edge = |x: f64| s.items.iter().find(|q| beside(p, q) && inside(q, V2::init(x, mid)));
        let (n, edge) = match (at_edge(b.x0 - PROBE), at_edge(b.x1 + PROBE)) {
            (Some(n), None) => (n, "left"),
            (None, Some(n)) => (n, "right"),
            _ => continue,
        };
        let (n, runs_past) = (n.comp, n.box_.y1 > b.y1 + TOUCH);
        let seat = if runs_past {
            let views = c.node.get("views").map_or(String::new(), |v| {
                let mut buf = Vec::new();
                crate::json::write_compact(&mut buf, v);
                format!(",\"views\":{}", String::from_utf8_lossy(&buf))
            });
            format!(
                "'{}' runs past it, so give it a ledger or block under it on that face: {{\"op\":\"add\",\"path\":\"components\",\"value\":{{\"id\":\"{}_ledger\",\"type\":\"lumber\",\"size\":\"2x4\",\"at\":{{\"anchor\":\"top_{edge}\",\"to\":\"{}@bottom_{edge}\"}}{views}}}}} (and its fasteners in a note); or draw",
                n.id, c.id, c.id
            )
        } else {
            format!(
                "set it on top of '{}': {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"at\":{{\"anchor\":\"bottom_{edge}\",\"to\":\"{}@top_{edge}\"}}}}}}; or draw",
                n.id, c.id, n.id
            )
        };
        told.push(c.index);
        diags.add_fix(
            Level::Warning,
            "W_UNSUPPORTED",
            Some(&c.id),
            Some(&format!("components/{}/at", c.id)),
            format!(
                "'{}' (x {}..{}, y {}..{}) carries '{}' but nothing is drawn under it: it only touches '{}' along its {edge} edge, and a plate does not bear through its edge",
                c.id,
                fmt_ft_in(b.x0),
                fmt_ft_in(b.x1),
                fmt_ft_in(b.y0),
                fmt_ft_in(b.y1),
                load.comp.id,
                n.id
            ),
            format!(
                "{seat} what carries it under it (a stud, post, beam or ledger; a hanger or angle seat); acknowledge W_UNSUPPORTED if it hangs from '{}'",
                load.comp.id
            ),
        );
    }
}
