use super::text::path_item;
use super::{Env, at_drawn, non_null};
use crate::drawing::Item;
use crate::geom::V2;
use crate::json::Value;
use crate::model::{self, Level};
use crate::pen::Pen;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;

pub struct LineSpec<'a> {
    pub id: &'a str,
    pub pts: Vec<V2>,
    pub pen: Pen,
    pub closed: bool,
    pub layer: Pen,
}

const EXAMPLE: &str = "{\"type\":\"line\",\"points\":[\"heel@bottom_right\",{\"ref\":\"heel@bottom_right\",\"offset\":[48,24]}],\"line\":\"dashed\"}";

pub fn read<'a>(env: &mut Env<'a, '_>, an: &'a Value, id: &'a str, apath: &str) -> Option<LineSpec<'a>> {
    let vid = env.spec.id;
    let list = match non_null(an, "points").and_then(Value::arr) {
        Some(a) if a.len() >= 2 => a,
        _ => {
            env.diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/points")),
                format!(
                    "line '{id}' in view {vid} needs \"points\": two or more points (\"comp@anchor\", {{\"ref\":...,\"offset\":[dx,dy]}} or [x, y])"
                ),
                format!("e.g. {EXAMPLE}"),
            );
            return None;
        }
    };
    let heavy = match non_null(an, "weight").map(Value::str) {
        None | Some(Some("light")) => false,
        Some(Some("heavy")) => true,
        Some(_) => {
            env.diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/weight")),
                format!(
                    "line '{id}' in view {vid}: \"weight\" must be \"light\" or \"heavy\" (got {})",
                    model::kind_or_text(an.get("weight").unwrap_or(&Value::Null))
                ),
                "light (the default: the hidden pen dashed, the beyond pen solid) or heavy (twice as wide, for a zone boundary that must read across a hatched fill)"
                    .to_string(),
            );
            return None;
        }
    };
    let (pen, layer) = match non_null(an, "line").map(Value::str) {
        None | Some(Some("dashed")) => (if heavy { Pen::Vapor } else { Pen::Hidden }, Pen::Hidden),
        Some(Some("solid")) => (if heavy { Pen::Profile } else { Pen::Beyond }, Pen::Beyond),
        Some(_) => {
            env.diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/line")),
                format!(
                    "line '{id}' in view {vid}: \"line\" must be \"dashed\" or \"solid\" (got {})",
                    model::kind_or_text(an.get("line").unwrap_or(&Value::Null))
                ),
                "dashed (the default) for a zone or limit that is not built, solid for a line drawn like a visible edge"
                    .to_string(),
            );
            return None;
        }
    };
    let closed = match non_null(an, "closed") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(v) => {
            env.diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/closed")),
                format!("line '{id}' in view {vid}: \"closed\" must be true or false (got {})", model::kind_or_text(v)),
            );
            return None;
        }
    };
    let mut pts = Vec::with_capacity(list.len());
    for (k, pv) in list.iter().enumerate() {
        let p = env.scene.point(pv, id, &format!("{apath}/points/{k}"))?;
        pts.push(at_drawn(env, pv, "", p));
    }
    Some(LineSpec { id, pts, pen, closed, layer })
}

pub fn items(env: &Env, l: &LineSpec) -> Vec<Item> {
    let mut it = path_item(env, l.pen, l.id, &l.pts, l.closed);
    if let Item::Path(p) = &mut it {
        p.layer = env.style.layer_for_pen(l.layer).into();
    }
    alloc::vec![it]
}

/// Hatch is cut this far (paper inches) each side of a line annotation (SPEC 30.1).
const HALO_IN: f64 = 0.04;

pub fn masks(env: &Env, l: &LineSpec) -> Vec<[V2; 4]> {
    let r = HALO_IN * env.s;
    let n = l.pts.len();
    let segs = if l.closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(segs);
    for i in 0..segs {
        let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
        let d = b.sub(a);
        let len = d.len();
        if len < 1e-9 {
            continue;
        }
        let u = V2::init(d.x / len * r, d.y / len * r);
        let w = V2::init(-u.y, u.x);
        out.push([a.sub(u).sub(w), b.add(u).sub(w), b.add(u).add(w), a.sub(u).add(w)]);
    }
    out
}
