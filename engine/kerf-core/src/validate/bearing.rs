//! `W_BEARING` (SPEC 27.1): a member with a minimum bearing (`Traits::min_bearing`, the I-joist) laid along x on
//! something under it must bear at least that length at each end support and at each intermediate support.

use super::{Drawn, Pf, axis_aligned, z_gap};
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
