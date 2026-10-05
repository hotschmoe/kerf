//! `inspect` queries (SPEC section 14).

use crate::api::Loaded;
use crate::diag::{Diag, nearest};
use crate::geom::*;
use crate::json::{arr_nums, num, obj, s};
use crate::num::fmt_ftin;
use crate::view::{find_view, view_params};
use serde_json::Value;

fn anchors_json(list: &[(String, Pt)]) -> Value {
    Value::Array(
        list.iter()
            .map(|(n, p)| obj(vec![("name", s(n)), ("x", num(p.x)), ("y", num(p.y)), ("x_ft", s(&fmt_ftin(p.x))), ("y_ft", s(&fmt_ftin(p.y)))]))
            .collect(),
    )
}

pub fn inspect(l: &Loaded, q: &Value) -> Result<Value, Diag> {
    let kind = q.get("q").and_then(|x| x.as_str()).unwrap_or("summary");
    match kind {
        "summary" => Ok(obj(vec![("summary", s(&crate::api::summary_of(l)))])),
        "doc" => Ok(obj(vec![("doc", l.canon.clone())])),
        "catalog" => {
            let t = q.get("type").and_then(|x| x.as_str()).ok_or_else(|| Diag::error("E_PARAM", "inspect catalog needs \"type\", e.g. \"truss\""))?;
            let cat = crate::catalog::json();
            let found = cat["types"].as_array().and_then(|a| a.iter().find(|x| x["type"] == t)).cloned();
            found.ok_or_else(|| {
                let names = crate::schema::type_names();
                Diag::error("E_REF_UNKNOWN", format!("unknown component type \"{}\".", t)).fix(match nearest(t, names.iter().copied()) {
                    Some(n) => format!("did you mean \"{}\"? types: {}", n, names.join(", ")),
                    None => format!("types: {}", names.join(", ")),
                })
            })
        }
        "component" | "anchors" => {
            let id = q.get("id").and_then(|x| x.as_str()).ok_or_else(|| Diag::error("E_PARAM", "inspect component/anchors needs \"id\""))?;
            let Some(c) = l.model.comp(id) else {
                let ids: Vec<&str> = l.model.comps.iter().map(|c| c.id.as_str()).collect();
                return Err(Diag::error("E_REF_UNKNOWN", format!("no component \"{}\".", id)).fix(match nearest(id, ids.iter().copied()) {
                    Some(n) => format!("did you mean \"{}\"? components: {}", n, ids.join(", ")),
                    None => format!("components: {}", ids.join(", ")),
                }));
            };
            let Some(inst) = c.insts.first() else {
                return Err(Diag::error("E_PARAM", format!("component \"{}\" could not be built; fix its errors (see check).", id)));
            };
            if kind == "anchors" {
                return Ok(obj(vec![("id", s(id)), ("anchors", anchors_json(&inst.anchors))]));
            }
            let mut params = c.value.clone();
            if let Some(m) = params.as_object_mut() {
                m.shift_remove("id");
                m.shift_remove("type");
            }
            let parts: Vec<Value> = inst
                .parts
                .iter()
                .map(|p| obj(vec![("name", s(&p.name)), ("kind", s(if p.zone { "zone" } else { "prism" })), ("anchors", anchors_json(&p.anchors))]))
                .collect();
            let b = c.bbox();
            Ok(obj(vec![
                ("id", s(id)),
                ("type", s(&c.ctype)),
                ("material", s(&c.material)),
                ("desc", s(&c.desc)),
                ("params", params),
                ("instances", num(c.insts.len() as f64)),
                ("z", arr_nums(&[inst.z0, inst.z1])),
                ("bbox", arr_nums(&[b.x0, b.y0, b.x1, b.y1])),
                ("anchors", anchors_json(&inst.anchors)),
                ("parts", Value::Array(parts)),
            ]))
        }
        "at" => {
            let vid = q.get("view").and_then(|x| x.as_str()).ok_or_else(|| Diag::error("E_PARAM", "inspect at needs \"view\" and \"point\": [x, y]"))?;
            let p = q.get("point").and_then(crate::resolve::lit_pt).ok_or_else(|| Diag::error("E_PARAM", "inspect at needs \"point\": [x, y] in inches"))?;
            let vv = find_view(&l.canon, vid).ok_or_else(|| Diag::error("E_REF_UNKNOWN", format!("unknown view \"{}\".", vid)))?;
            let vp = view_params(vv, &l.model)?;
            let mut d = vec![];
            let base = crate::section::build_section(&l.model, &vp, &l.style, &mut d);
            let mut hits = vec![];
            for vi in &base.vis {
                let inside = vi.shapes.iter().any(|sh| !sh.is_empty() && poly_contains(&sh[0], p) && !sh[1..].iter().any(|h| poly_contains(h, p)));
                if inside {
                    hits.push(obj(vec![
                        ("id", s(l.model.comps[vi.comp].id.as_str())),
                        ("src", s(&vi.src)),
                        ("part", vi.part.as_ref().map(|x| s(x)).unwrap_or(Value::Null)),
                        ("kind", s(if vi.cut { "cut" } else { "beyond" })),
                    ]));
                }
            }
            Ok(obj(vec![("view", s(vid)), ("point", arr_nums(&[p.x, p.y])), ("components", Value::Array(hits))]))
        }
        other => Err(Diag::error("E_PARAM", format!("unknown query \"{}\": use summary, component, anchors, at, catalog, doc", other))),
    }
}
