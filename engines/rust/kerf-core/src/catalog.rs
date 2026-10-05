//! Component catalog as JSON and markdown (the text the LLM system prompt embeds).

use crate::json::{obj, s};
use crate::schema::*;
use serde_json::Value;

pub fn json() -> Value {
    let mut types = vec![];
    for t in TYPES {
        let params: Vec<Value> = all_params(t)
            .iter()
            .map(|p| {
                obj(vec![
                    ("name", s(p.name)),
                    ("type", s(&kind_name(&p.kind))),
                    ("default", s(p.def)),
                    ("required", Value::Bool(p.req)),
                    ("doc", s(p.doc)),
                ])
            })
            .collect();
        types.push(obj(vec![
            ("type", s(t.name)),
            ("summary", s(t.summary)),
            ("params", Value::Array(params)),
            ("parts", s(t.parts)),
            ("anchors", s(t.anchors)),
            ("draws", s(t.draws)),
        ]));
    }
    obj(vec![("kerf_catalog", s("0.1")), ("types", Value::Array(types))])
}

pub fn markdown() -> String {
    let mut out = String::from("# Kerf component catalog\n\nEvery component has `id`, `type`, optional `label`, `material`, placement (`at`, `rotate`, `slope`, `mirror`), `z`, `array`, `embedded`, `visible` plus the type parameters below. Lengths are inches (numbers) or text such as \"7-5/8\".\n\n");
    for t in TYPES {
        out.push_str(&format!("## {}\n{}.\n\n", t.name, t.summary));
        out.push_str("| param | type | default | notes |\n|---|---|---|---|\n");
        for p in t.params {
            out.push_str(&format!("| {}{} | {} | {} | {} |\n", p.name, if p.req { " *" } else { "" }, kind_name(&p.kind).replace('|', "\\|"), if p.req { "required".to_string() } else { p.def.to_string() }, p.doc));
        }
        out.push_str(&format!("\nParts: {}\n\nAnchors: {}\n\nDraws: {}\n\n", t.parts, t.anchors, t.draws));
    }
    out
}
