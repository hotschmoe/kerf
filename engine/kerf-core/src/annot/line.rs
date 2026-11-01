//! Boundary lines (SPEC 29.2): a dashed or solid polyline through model points, for what is not a component (a no-load
//! zone, an influence line, a property or limit line). Placed exactly where the author puts it, like a weld symbol.

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
    let pen = match non_null(an, "line").map(Value::str) {
        None | Some(Some("dashed")) => Pen::Hidden,
        Some(Some("solid")) => Pen::Beyond,
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
    Some(LineSpec { id, pts, pen, closed })
}

pub fn items(env: &Env, l: &LineSpec) -> Vec<Item> {
    alloc::vec![path_item(env, l.pen, l.id, &l.pts, l.closed)]
}
