//! Which components and annotations answer each line of `meta.requested` (SPEC 21, 25.3). Words match after the house
//! normalization (`house::Words`: abbreviations both ways, case, punctuation, plural s, lengths); every candidate is one
//! component or one annotation, and an item is covered when one candidate offers all of its words.

use crate::house::{self, Words};
use crate::json::Value;
use crate::lint;
use crate::scene::Scene;
use crate::style::Style;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The candidate that offers most of an uncovered item's words.
pub struct Closest {
    /// Where it is, e.g. `note 'n4' in view A`.
    pub source: String,
    pub text: String,
    /// The item's words it lacks, as a person reads them.
    pub missing: Vec<String>,
    pub have: usize,
    pub total: usize,
}

pub struct Item {
    pub text: String,
    pub found: Vec<String>,
    pub closest: Option<Closest>,
}

struct Cand {
    found: String,
    source: String,
    text: String,
    words: Vec<String>,
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

fn add_found(list: &mut Vec<String>, id: &str) {
    if !list.iter().any(|x| x == id) {
        list.push(String::from(id));
    }
}

pub fn requested_list(doc: &Value) -> &[Value] {
    doc.get("meta").and_then(|m| m.get("requested")).and_then(Value::arr).unwrap_or(&[])
}

/// What a weld symbol says, in words: `3/16 fillet weld all around`, `cjp complete joint penetration groove weld`.
fn weld_text(an: &Value) -> String {
    let kind = match str_of(an, "weld") {
        "cjp" => "cjp complete joint penetration groove",
        "pjp" => "pjp partial joint penetration groove",
        _ => "fillet",
    };
    let size = match an.get("size") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => crate::annot::weld::size_text(*n),
        _ => String::new(),
    };
    let mut t = format!("{size} {kind} weld");
    if matches!(an.get("all_around"), Some(Value::Bool(true))) {
        t.push_str(" all around");
    }
    if matches!(an.get("field"), Some(Value::Bool(true))) {
        t.push_str(" field weld");
    }
    if str_of(an, "sides") == "both" {
        t.push_str(" both sides");
    }
    let tail = str_of(an, "tail");
    if !tail.is_empty() {
        t.push(' ');
        t.push_str(tail);
    }
    t
}

fn note_words_text(an: &Value) -> String {
    let mut t = String::from(str_of(an, "text"));
    for c in an.get("cite").and_then(Value::arr).unwrap_or(&[]) {
        for k in ["code", "section", "title"] {
            t.push(' ');
            t.push_str(str_of(c, k));
        }
    }
    t
}

/// A dimension's words: its `text`, or else the length it measures (needs the compiled scene).
fn dim_text(scene: Option<&Scene>, an: &Value) -> (String, Option<String>) {
    let text = str_of(an, "text");
    if !text.is_empty() {
        return (String::from(text), None);
    }
    let Some(sc) = scene else { return (String::new(), None) };
    let (Some(fv), Some(tv)) = (an.get("from"), an.get("to")) else { return (String::new(), None) };
    let (Some(a), Some(b)) = (lint::quiet_point(sc, fv), lint::quiet_point(sc, tv)) else {
        return (String::new(), None);
    };
    let dir = match str_of(an, "dir") {
        "" => lint::dominant_dir(a, b),
        d => d,
    };
    let len = lint::dim_measure(a, b, dir);
    (crate::units::fmt_ft_in(len), Some(house::length_word(len)))
}

/// A table offers its title with its column headers, units and header rows as one candidate, each body row with that
/// heading as another, and each of its notes (SPEC 26.1).
fn table_candidates(an: &Value, aid: &str, vid: &str, w: &Words, out: &mut Vec<Cand>) {
    let texts = crate::annot::table::texts(an);
    let heading: Vec<&str> =
        texts.iter().filter(|(p, _)| !p.starts_with("rows/") && !p.starts_with("notes/")).map(|(_, t)| *t).collect();
    let heading = heading.join(" ");
    let mut push = |text: String, what: &str| {
        let words = w.offered(&text);
        if !words.is_empty() {
            out.push(Cand { found: format!("table {aid}"), source: format!("table '{aid}' {what}in view {vid}"), text, words });
        }
    };
    push(heading.clone(), "");
    for (r, row) in crate::annot::table::row_texts(an).into_iter().enumerate() {
        push(format!("{heading} {row}"), &format!("row {r} "));
    }
    for (p, t) in texts.iter().filter(|(p, _)| p.starts_with("notes/")) {
        push(String::from(*t), &format!("{} ", p.replace('/', " ")));
    }
}

fn candidates(doc: &Value, scene: Option<&Scene>, w: &Words) -> Vec<Cand> {
    let mut out = Vec::new();
    for c in doc.get("components").and_then(Value::arr).unwrap_or(&[]) {
        let id = str_of(c, "id");
        let joined =
            format!("{} {} {} {} {}", id, str_of(c, "type"), str_of(c, "label"), str_of(c, "model"), str_of(c, "size"));
        let text = joined.get(id.len() + 1..).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
        out.push(Cand {
            found: String::from(id),
            source: format!("component '{id}'"),
            text,
            words: w.offered(&joined),
        });
    }
    for v in doc.get("views").and_then(Value::arr).unwrap_or(&[]) {
        let vid = str_of(v, "id");
        let measure = if str_of(v, "kind") == "iso" { None } else { scene };
        let title = str_of(v, "title");
        if !title.is_empty() {
            out.push(Cand {
                found: format!("view {vid}"),
                source: format!("view {vid} title"),
                text: String::from(title),
                words: w.offered(title),
            });
        }
        for an in v.get("annotations").and_then(Value::arr).unwrap_or(&[]) {
            let aid = str_of(an, "id");
            let ty = str_of(an, "type");
            if ty == "table" {
                table_candidates(an, aid, vid, w, &mut out);
                continue;
            }
            let (text, words) = match ty {
                "note" | "label" => {
                    let t = note_words_text(an);
                    let words = w.offered(&t);
                    (String::from(str_of(an, "text")), words)
                }
                "dim" => {
                    let (t, len) = dim_text(measure, an);
                    let mut words = w.offered(&t);
                    if let Some(l) = len {
                        words.push(l);
                        crate::sort::stable(&mut words, |a, b| a < b);
                        words.dedup();
                    }
                    (t, words)
                }
                "weld" => {
                    let t = weld_text(an);
                    let words = w.offered(&t);
                    (t, words)
                }
                _ => continue,
            };
            if words.is_empty() {
                continue;
            }
            let tc = target_comp(an);
            let found = if (ty == "note" || ty == "label") && !tc.is_empty() && has_component(doc, tc) {
                String::from(tc)
            } else {
                format!("{ty} {aid}")
            };
            out.push(Cand { found, source: format!("{ty} '{aid}' in view {vid}"), text, words });
        }
    }
    out
}

fn has(words: &[String], w: &str) -> bool {
    words.binary_search_by(|x| x.as_str().cmp(w)).is_ok()
}

/// One entry per non-empty `meta.requested` string: the components / annotations that cover it, and for an uncovered
/// item the closest candidate. `scene` (when given) lets dimensions without `text` offer the length they measure.
pub fn compute(doc: &Value, st: &Style, scene: Option<&Scene>) -> Vec<Item> {
    let items: Vec<&str> = requested_list(doc)
        .iter()
        .filter_map(Value::str)
        .map(|s| s.trim_matches([' ', '\t', '\r', '\n']))
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return Vec::new();
    }
    let w = Words::new(st);
    let cands = candidates(doc, scene, &w);
    let mut out = Vec::new();
    for text in items {
        let want = w.wanted(text);
        let mut found = Vec::new();
        let mut best: Option<(usize, &Cand)> = None;
        for c in &cands {
            let n = want.iter().filter(|t| has(&c.words, t)).count();
            if n == want.len() && n > 0 {
                add_found(&mut found, &c.found);
            } else if n > 0 && best.is_none_or(|(b, _)| n > b) {
                best = Some((n, c));
            }
        }
        let closest = match best {
            Some((n, c)) if found.is_empty() => Some(Closest {
                source: c.source.clone(),
                text: c.text.clone(),
                missing: want.iter().filter(|t| !has(&c.words, t)).map(|t| house::display_word(t)).collect(),
                have: n,
                total: want.len(),
            }),
            _ => None,
        };
        out.push(Item { text: String::from(text), found, closest });
    }
    out
}
