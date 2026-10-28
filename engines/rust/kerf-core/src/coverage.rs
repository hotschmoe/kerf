//! Which components and notes answer each line of `meta.requested`.

use crate::json::Value;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Item {
    pub text: String,
    pub found: Vec<String>,
}

const STOP_WORDS: [&str; 13] = ["w", "with", "and", "at", "of", "the", "a", "an", "to", "for", "in", "on", "per"];

fn tokens(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && !b[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_alphanumeric() {
            i += 1;
        }
        if i == start {
            break;
        }
        let mut w = b[start..i].to_ascii_lowercase();
        if w.len() > 3 && w[w.len() - 1] == b's' && w[w.len() - 2] != b's' {
            w.pop();
        }
        let Ok(w) = String::from_utf8(w) else { continue };
        if !STOP_WORDS.contains(&w.as_str()) {
            out.push(w);
        }
    }
    out
}

fn covers(item: &[String], cand: &[String]) -> bool {
    !item.is_empty() && item.iter().all(|t| cand.contains(t))
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::str).unwrap_or("")
}

fn target_comp(v: &Value) -> &str {
    let mut t = str_of(v, "target");
    if t.is_empty() {
        t = str_of(v, "at");
    }
    let end = t.find(['.', '@', '#']).unwrap_or(t.len());
    &t[..end]
}

fn has_component(doc: &Value, id: &str) -> bool {
    doc.get("components").and_then(Value::arr).is_some_and(|cs| cs.iter().any(|c| str_of(c, "id") == id))
}

fn add_found(list: &mut Vec<String>, id: String) {
    if !list.contains(&id) {
        list.push(id);
    }
}

pub fn requested_list(doc: &Value) -> &[Value] {
    doc.get("meta").and_then(|m| m.get("requested")).and_then(Value::arr).unwrap_or(&[])
}

pub fn compute(doc: &Value) -> Vec<Item> {
    let mut out = Vec::new();
    for rv in requested_list(doc) {
        let Some(s) = rv.str() else { continue };
        let text = s.trim_matches([' ', '\t', '\r', '\n']);
        if text.is_empty() {
            continue;
        }
        let want = tokens(text);
        let mut found = Vec::new();
        if let Some(ca) = doc.get("components").and_then(Value::arr) {
            for c in ca {
                let joined = format!("{} {} {} {} {}", str_of(c, "id"), str_of(c, "type"), str_of(c, "label"), str_of(c, "model"), str_of(c, "size"));
                if covers(&want, &tokens(&joined)) {
                    add_found(&mut found, String::from(str_of(c, "id")));
                }
            }
        }
        if let Some(va) = doc.get("views").and_then(Value::arr) {
            for v in va {
                let Some(anns) = v.get("annotations").and_then(Value::arr) else { continue };
                for an in anns {
                    let ty = str_of(an, "type");
                    if ty != "note" && ty != "label" {
                        continue;
                    }
                    if !covers(&want, &tokens(str_of(an, "text"))) {
                        continue;
                    }
                    let tc = target_comp(an);
                    if !tc.is_empty() && has_component(doc, tc) {
                        add_found(&mut found, String::from(tc));
                    } else {
                        add_found(&mut found, format!("note {}", str_of(an, "id")));
                    }
                }
            }
        }
        out.push(Item { text: String::from(text), found });
    }
    out
}
