//! Document lints (SPEC 19): W_UNKNOWN_KEY, W_NOTE_STYLE, W_DIM_ZERO, `acknowledge` handling (I_ACK),
//! and the dim `dir` default. They look at the document and the compiled scene, not at geometry output;
//! field names come from schema_fields / catalog (the same tables the parsers and `kerf schema` use).

use crate::compile;
use crate::coverage;
use crate::geom::V2;
use crate::json::{Member, Value};
use crate::model::{self, Diag, Diags, Level};
use crate::references;
use crate::scene::Scene;
use crate::schema_fields::{self as schema, Object};
use crate::style::Style;
use crate::units;
use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn run(scene: &Scene, doc: &Value, diags: &Diags) {
    if !matches!(doc, Value::Object(_)) {
        return;
    }
    unknown_keys(doc, diags);
    note_style(doc, diags);
    dim_zero(scene, doc, diags);
    ack_shape(doc, diags);
    requested_missing(doc, diags);
    references::lint(doc, diags);
}

// ---- unknown keys ---------------------------------------------------------------------------------------

struct Syn {
    key: &'static str,
    to: &'static str,
    /// Object names the row applies to (empty = any).
    objs: &'static [&'static str],
    /// Full replacement for the suggestion text.
    hint: &'static str,
}

const ANNOTATION_OBJS: &[&str] = &["note", "dim", "label"];

const fn syn(key: &'static str, to: &'static str) -> Syn {
    Syn { key, to, objs: &[], hint: "" }
}

static SYNONYMS: [Syn; 12] = [
    syn("citations", "cite"),
    syn("citation", "cite"),
    syn("cites", "cite"),
    Syn { key: "side", to: "notes_side", objs: &["view"], hint: "" },
    Syn {
        key: "side",
        to: "notes_side",
        objs: ANNOTATION_OBJS,
        hint: "the note column side is the VIEW's \"notes_side\" (right|left|both, at views/<view>/notes_side); to put one note's text somewhere use \"place\": [x, y], to move its arrow use \"at\"",
    },
    syn("point", "at"),
    syn("target_point", "at"),
    syn("arrow", "at"),
    Syn { key: "kind", to: "type", objs: ANNOTATION_OBJS, hint: "" },
    syn("pos", "place"),
    syn("position", "place"),
    Syn { key: "ref", to: "to", objs: &["at"], hint: "" },
];

struct Suggestion<'k> {
    to: &'k str,
    hint: &'static str,
}

fn suggest_key<'k>(o: &'k Object, key: &str) -> Option<Suggestion<'k>> {
    for s in &SYNONYMS {
        if s.key != key {
            continue;
        }
        if !s.objs.is_empty() && !s.objs.contains(&o.name) {
            continue;
        }
        return Some(Suggestion { to: s.to, hint: s.hint });
    }
    let mut best: Option<&str> = None;
    let mut bd = usize::MAX;
    for f in o.fields {
        let d = model::edit_distance(key, f.name);
        if d < bd {
            bd = d;
            best = Some(f.name);
        }
    }
    let limit = if key.len() <= 4 { 1 } else { 2 };
    best.filter(|_| bd <= limit).map(|to| Suggestion { to, hint: "" })
}

fn key_list(o: &Object) -> String {
    let mut out = String::new();
    for (i, f) in o.fields.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(f.name);
    }
    out
}

fn lint_object(diags: &Diags, v: &Value, o: &Object, path: &str, id: Option<&str>) {
    let Value::Object(members) = v else { return };
    for m in members {
        if schema::has_field(o, &m.key) {
            continue;
        }
        let kpath = format!("{path}/{}", m.key);
        let valid_list = || {
            (
                format!(
                    "unknown key \"{}\" in {path} ({}): it is ignored (kept in the file). Valid keys: {}",
                    m.key,
                    o.name,
                    key_list(o)
                ),
                format!("remove \"{}\" or use one of the valid keys (`kerf schema {}`)", m.key, o.name),
            )
        };
        let (message, fix) = match suggest_key(o, &m.key) {
            Some(sg) if !sg.hint.is_empty() => (
                format!(
                    "unknown key \"{}\" in {path} ({}): it is ignored (kept in the file). {}",
                    m.key, o.name, sg.hint
                ),
                format!("remove \"{}\" or follow the hint above (`kerf schema {}`)", m.key, o.name),
            ),
            Some(sg) if schema::has_field(o, sg.to) => (
                format!(
                    "unknown key \"{}\" in {path} ({}): it is ignored (kept in the file). Did you mean \"{}\"?",
                    m.key, o.name, sg.to
                ),
                format!("rename \"{}\" to \"{}\" (`kerf schema {}`)", m.key, sg.to, o.name),
            ),
            Some(_) => (
                format!(
                    "unknown key \"{}\" in {path} ({}): it is ignored (kept in the file). Valid keys: {}",
                    m.key,
                    o.name,
                    key_list(o)
                ),
                format!("remove \"{}\" (`kerf schema {}`)", m.key, o.name),
            ),
            None => valid_list(),
        };
        diags.add_fix(Level::Warning, "W_UNKNOWN_KEY", id, Some(&kpath), message, fix);
    }
}

fn str_of<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).map_or("", |x| x.str().unwrap_or(""))
}

fn arr_of<'v>(v: &'v Value, key: &str) -> Option<&'v [Value]> {
    v.get(key).and_then(Value::arr)
}

fn unknown_keys(doc: &Value, diags: &Diags) {
    lint_object(diags, doc, &schema::DOC, "doc", None);
    for c in arr_of(doc, "components").unwrap_or(&[]) {
        if !matches!(c, Value::Object(_)) {
            continue;
        }
        let cid = str_of(c, "id");
        if cid.is_empty() {
            continue;
        }
        let base = format!("components/{cid}");
        if let Some(at) = c.get("at") {
            lint_object(diags, at, &schema::AT, &format!("{base}/at"), Some(cid));
        }
        if let Some(ar) = c.get("array") {
            lint_object(diags, ar, &schema::ARRAY, &format!("{base}/array"), Some(cid));
        }
        for (i, it) in arr_of(c, "acknowledge").unwrap_or(&[]).iter().enumerate() {
            lint_object(diags, it, &schema::ACK, &format!("{base}/acknowledge/{i}"), Some(cid));
        }
    }
    for v in arr_of(doc, "views").unwrap_or(&[]) {
        if !matches!(v, Value::Object(_)) {
            continue;
        }
        let vid = str_of(v, "id");
        if vid.is_empty() {
            continue;
        }
        let vbase = format!("views/{vid}");
        lint_object(diags, v, &schema::VIEW, &vbase, Some(vid));
        for an in arr_of(v, "annotations").unwrap_or(&[]) {
            if !matches!(an, Value::Object(_)) {
                continue;
            }
            let aid = str_of(an, "id");
            if aid.is_empty() {
                continue;
            }
            let o = match str_of(an, "type") {
                "dim" => &schema::DIM,
                "label" => &schema::LABEL,
                _ => &schema::NOTE,
            };
            let apath = format!("{vbase}/annotations/{aid}");
            lint_object(diags, an, o, &apath, Some(aid));
            for (i, ci) in arr_of(an, "cite").unwrap_or(&[]).iter().enumerate() {
                lint_object(diags, ci, &schema::CITE, &format!("{apath}/cite/{i}"), Some(aid));
            }
        }
    }
}

// ---- note style -----------------------------------------------------------------------------------------

struct Abbr {
    word: &'static str,
    abbr: &'static str,
}

/// Spelled-out word(s) that the house style abbreviates (matched as whole words, case-insensitive).
static SPELLED_OUT: [Abbr; 10] = [
    Abbr { word: "GYPSUM BOARD", abbr: "GYP. BD." },
    Abbr { word: "PRESSURE TREATED", abbr: "PT" },
    Abbr { word: "ON CENTER", abbr: "O.C." },
    Abbr { word: "CONCRETE", abbr: "CONC." },
    Abbr { word: "CONTINUOUS", abbr: "CONT." },
    Abbr { word: "EACH", abbr: "EA." },
    Abbr { word: "BOTTOM", abbr: "BOTT." },
    Abbr { word: "REINFORCING", abbr: "REINF." },
    Abbr { word: "MINIMUM", abbr: "MIN." },
    Abbr { word: "DIAMETER", abbr: "DIA." },
];

/// Abbreviations that legitimately end a note with a period (no trailing-period warning).
const PERIOD_ABBRS: [&str; 40] = [
    "MIN", "MAX", "EA", "CONT", "TYP", "DIA", "GA", "SIM", "BOTT", "CONC", "EMBED", "MFR", "FTG", "GRD", "HDR", "DBL",
    "STL", "GALV", "VERT", "HORIZ", "GYP", "BD", "REINF", "SHTG", "CLR", "PLY", "NO", "STD", "SQ", "EQ", "NOM",
    "APPROX", "REQD", "SPEC", "DWG", "DWGS", "EXIST", "THK", "ALT", "UNO",
];

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// Index of the next whole-word, case-insensitive match of `phrase` in `text` at or after `from`.
fn find_word(text: &[u8], phrase: &str, from: usize) -> Option<usize> {
    let p = phrase.as_bytes();
    let mut i = from;
    while i + p.len() <= text.len() {
        let hit = text[i..i + p.len()].eq_ignore_ascii_case(p)
            && !(i > 0 && is_word_char(text[i - 1]))
            && !(i + p.len() < text.len() && is_word_char(text[i + p.len()]));
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[derive(Clone, Debug)]
pub struct NoteStyle {
    /// Human-readable list of the problems ("" = fine).
    pub issues: String,
    /// The text with every problem that can be fixed mechanically fixed.
    pub fixed: String,
    /// Some problem needs a rewrite (commentary, a dangling end, length): `fixed` does not cure it.
    pub rewrite: bool,
}

/// Longest note, in characters: one idea per note. SPEC 21 says 120; the reference flush-beam strap note is 121 characters and
/// the reference details must stay clean, so the limit is 130 (recorded in NOTES.md).
const MAX_NOTE_LEN: usize = 130;

/// Words that make a note read as a person talking (matched as whole words, case-insensitive).
const CHATTER_WORDS: [&str; 2] = ["WE", "PLEASE"];
const CHATTER_PHRASES: [&str; 2] = ["SHOULD BE", "NOTE:"];

/// A note must not end on one of these: the thing it connects to is missing.
const CONNECTOR_WORDS: [&str; 10] = ["W/", "AND", "OR", "TO", "@", "&", "PER", "WITH", "FOR", "OF"];

/// Commentary in a note, as a short description ("" = none): first person, "NOTE:", "PLEASE", "SHOULD BE", a question.
fn commentary(text: &[u8]) -> &'static str {
    if text.contains(&b'?') {
        return "a question mark";
    }
    for ph in CHATTER_PHRASES {
        if find_word(text, ph, 0).is_some() {
            return ph;
        }
    }
    for w in CHATTER_WORDS {
        if find_word(text, w, 0).is_some() {
            return w;
        }
    }
    let mut from = 0;
    while let Some(at) = find_word(text, "I", from) {
        if matches!(text.get(at + 1), Some(b' ' | b'\'')) {
            return "I";
        }
        from = at + 1;
    }
    ""
}

const WS: [u8; 4] = *b" \t\r\n";

fn trim_end<'t>(t: &'t [u8], set: &[u8]) -> &'t [u8] {
    let mut e = t.len();
    while e > 0 && set.contains(&t[e - 1]) {
        e -= 1;
    }
    &t[..e]
}

fn last_token(text: &[u8]) -> &[u8] {
    let t = trim_end(text, &WS);
    match t.iter().rposition(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'(')) {
        Some(sp) => &t[sp + 1..],
        None => t,
    }
}

/// The connector word a note ends on ("" = none), ignoring a trailing period.
fn dangling_end(text: &[u8]) -> &'static str {
    let tok = trim_end(last_token(trim_end(text, b" \t\r\n.")), b".");
    CONNECTOR_WORDS.into_iter().find(|w| tok.eq_ignore_ascii_case(w.as_bytes())).unwrap_or("")
}

/// The text with case and runs of whitespace ignored, and no trailing period (to spot a note that repeats another).
fn normalized_text(text: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    let bytes = trim_end(text.as_bytes(), b" \t\r\n.");
    let mut start = 0;
    while start < bytes.len() && matches!(bytes[start], b' ' | b'\t' | b'\r' | b'\n' | b'.') {
        start += 1;
    }
    let mut buf: Vec<u8> = Vec::new();
    for &c in &bytes[start..] {
        if c.is_ascii_whitespace() {
            space = true;
            continue;
        }
        if space && !buf.is_empty() {
            buf.push(b' ');
        }
        space = false;
        buf.push(c.to_ascii_uppercase());
    }
    out.push_str(&String::from_utf8_lossy(&buf));
    out
}

fn has_dash_fraction(text: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < text.len() {
        if text[i].is_ascii_digit() && text[i + 1] == b'-' {
            let mut j = i + 2;
            let s = j;
            while j < text.len() && text[j].is_ascii_digit() {
                j += 1;
            }
            if j > s && j < text.len() && text[j] == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn add_issue(list: &mut String, what: &str) {
    if !list.is_empty() {
        list.push_str("; ");
    }
    list.push_str(what);
}

/// Check one note text against the house style (SPEC 19). Pure.
pub fn check_note_text(text: &str) -> NoteStyle {
    let tb = text.as_bytes();
    let mut issues = String::new();
    if tb.iter().any(u8::is_ascii_lowercase) {
        add_issue(&mut issues, "lowercase letters (notes are UPPERCASE)");
    }
    if tb.windows(3).any(|w| w == b" x ") {
        add_issue(&mut issues, "\" x \" as a size separator (use \" X \")");
    }
    if has_dash_fraction(tb) {
        add_issue(&mut issues, "dashed fraction like 1-1/2\" (house style is 1 1/2\")");
    }
    for sp in &SPELLED_OUT {
        if find_word(tb, sp.word, 0).is_some() {
            add_issue(&mut issues, &format!("\"{}\" spelled out (use \"{}\")", sp.word, sp.abbr));
        }
    }
    let trimmed = trim_end(tb, &WS);
    let mut trailing = false;
    if trimmed.len() > 1 && trimmed[trimmed.len() - 1] == b'.' {
        let tok = last_token(trimmed);
        let base = trim_end(tok, b".");
        let mut ok = base.contains(&b'.');
        if !ok {
            ok = PERIOD_ABBRS.iter().any(|ab| ab.as_bytes().eq_ignore_ascii_case(base));
        }
        if !ok && base.last().is_some_and(|&c| !c.is_ascii_alphabetic()) {
            ok = true;
        }
        if !ok {
            trailing = true;
        }
    }
    if trailing {
        add_issue(&mut issues, "trailing period (notes are not sentences)");
    }
    let mut rewrite = false;
    if tb.len() > MAX_NOTE_LEN {
        rewrite = true;
        add_issue(
            &mut issues,
            &format!("{} characters (notes stay under {MAX_NOTE_LEN}: one idea per note, split it)", tb.len()),
        );
    }
    let chat = commentary(tb);
    if !chat.is_empty() {
        rewrite = true;
        add_issue(
            &mut issues,
            &format!("commentary ({chat}): a note names the thing and how it is installed, it does not talk"),
        );
    }
    let end = dangling_end(tb);
    if !end.is_empty() {
        rewrite = true;
        add_issue(
            &mut issues,
            &format!("ends on \"{end}\" with nothing after it (finish the thought or drop the connector)"),
        );
    }
    if issues.is_empty() {
        return NoteStyle { issues, fixed: text.to_string(), rewrite: false };
    }

    let mut work: Vec<u8> = tb.to_vec();
    for sp in &SPELLED_OUT {
        let mut guard = 0;
        while let Some(at) = find_word(&work, sp.word, 0) {
            if guard > 50 {
                break;
            }
            guard += 1;
            let mut next = Vec::with_capacity(work.len());
            next.extend_from_slice(&work[..at]);
            next.extend_from_slice(sp.abbr.as_bytes());
            next.extend_from_slice(&work[at + sp.word.len()..]);
            work = next;
        }
    }
    let up = work.to_ascii_uppercase();
    let mut out: Vec<u8> = Vec::with_capacity(up.len());
    for k in 0..up.len() {
        if up[k] == b'-' && k > 0 && k + 1 < up.len() && up[k - 1].is_ascii_digit() && up[k + 1].is_ascii_digit() {
            let mut j = k + 1;
            while j < up.len() && up[j].is_ascii_digit() {
                j += 1;
            }
            if j < up.len() && up[j] == b'/' {
                out.push(b' ');
                continue;
            }
        }
        out.push(up[k]);
    }
    let mut res = trim_end(&out, &WS);
    if trailing && res.last() == Some(&b'.') {
        res = &res[..res.len() - 1];
    }
    NoteStyle { issues, fixed: String::from_utf8_lossy(res).into_owned(), rewrite }
}

fn note_style(doc: &Value, diags: &Diags) {
    for v in arr_of(doc, "views").unwrap_or(&[]) {
        let vid = str_of(v, "id");
        let keyed = matches!(v.get("notes_style").and_then(Value::str), Some("keyed" | "list"));
        let mut seen: Vec<(String, &str, Option<f64>)> = Vec::new();
        for an in arr_of(v, "annotations").unwrap_or(&[]) {
            if str_of(an, "type") != "note" {
                continue;
            }
            let Some(text) = an.get("text").and_then(Value::str) else { continue };
            let aid = str_of(an, "id");
            let path = format!("views/{vid}/annotations/{aid}/text");
            let st = check_note_text(text);
            if !st.issues.is_empty() {
                let fix = if st.rewrite && st.fixed == text {
                    "rewrite it as <SIZE/QTY> <MATERIAL> <ITEM> <W/ ATTACHMENT> <@ SPACING>, one idea, no commentary (split a long note in two)".to_string()
                } else if st.rewrite {
                    format!(
                        "rewrite it as <SIZE/QTY> <MATERIAL> <ITEM> <W/ ATTACHMENT> <@ SPACING>, one idea, no commentary; the mechanical part is \"{}\"",
                        st.fixed
                    )
                } else {
                    format!("set text to \"{}\"", st.fixed)
                };
                diags.add_fix(
                    Level::Warning,
                    "W_NOTE_STYLE",
                    Some(aid),
                    Some(&path),
                    format!("note '{aid}' in view {vid} breaks the house note style: {}. Text: \"{text}\"", st.issues),
                    fix,
                );
            }
            let norm = normalized_text(text);
            if norm.is_empty() {
                continue;
            }
            let key = an.get("key").and_then(Value::num);
            if let Some(&(_, first, first_key)) = seen.iter().find(|(t, _, _)| *t == norm) {
                if key.is_some() && key == first_key {
                    continue;
                }
                let fix = if keyed {
                    format!(
                        "give '{aid}' the \"key\" of '{first}' (one DETAIL NOTES entry, two tags) and drop its text, or remove '{aid}'"
                    )
                } else {
                    format!(
                        "remove '{aid}', or give it text that says something different (one note per element or connection)"
                    )
                };
                diags.add_fix(
                    Level::Warning,
                    "W_NOTE_STYLE",
                    Some(aid),
                    Some(&path),
                    format!("note '{aid}' in view {vid} repeats note '{first}' (same text: \"{text}\")"),
                    fix,
                );
            } else {
                seen.push((norm, aid, key));
            }
        }
    }
}

// ---- meta.requested -------------------------------------------------------------------------------------

/// `meta.requested` must be an array of non-empty strings; every item that matches nothing in the document warns.
fn requested_missing(doc: &Value, diags: &Diags) {
    let Some(meta) = doc.get("meta") else { return };
    let Some(rv) = meta.get("requested") else { return };
    let shape_fix = "e.g. \"meta\": {\"requested\": [\"cmu wall\", \"bond beam\", \"H2.5A ties\"]} (the designer's asks, one short phrase each)";
    let Some(items) = rv.arr() else {
        if !rv.is_null() {
            diags.add_fix(
                Level::Error,
                "E_PARAM",
                None,
                Some("meta/requested"),
                format!("'meta.requested' must be an array of strings, got a {}", rv.kind_name()),
                shape_fix.to_string(),
            );
        }
        return;
    };
    let mut bad = false;
    for (i, it) in items.iter().enumerate() {
        let s = it.str().unwrap_or("").trim_matches([' ', '\t', '\r', '\n']);
        if s.is_empty() {
            bad = true;
            diags.add_fix(
                Level::Error,
                "E_PARAM",
                None,
                Some(&format!("meta/requested/{i}")),
                format!("'meta.requested' entry {i} must be a non-empty string"),
                shape_fix.to_string(),
            );
        }
    }
    if bad {
        return;
    }
    for (i, item) in coverage::compute(doc).iter().enumerate() {
        if !item.found.is_empty() {
            continue;
        }
        diags.add_fix(
            Level::Warning,
            "W_REQUESTED_MISSING",
            None,
            Some(&format!("meta/requested/{i}")),
            format!("requested element \"{}\" (meta.requested) matches no component id, type, label, model or note text", item.text),
            format!("build it (name the component after it, or add a note whose text says \"{}\"), or remove it from meta.requested if the designer dropped it", item.text),
        );
    }
}

// ---- dim directions -------------------------------------------------------------------------------------

/// Resolve a dim endpoint without reporting reference errors (the view build reports those).
fn quiet_point(scene: &Scene, v: &Value) -> Option<V2> {
    let before = scene.diags.len();
    let p = scene.point(v, "", "");
    let mut list = scene.diags.take();
    list.truncate(before);
    scene.diags.set(list);
    p
}

/// Default `dir`: the dominant axis between the points (|dx| >= |dy| gives h, else v).
pub fn dominant_dir(from: V2, to: V2) -> &'static str {
    if (to.x - from.x).abs() >= (to.y - from.y).abs() { "h" } else { "v" }
}

fn dim_measure(from: V2, to: V2, dir: &str) -> f64 {
    match dir {
        "v" => (to.y - from.y).abs(),
        "aligned" => from.dist(to),
        _ => (to.x - from.x).abs(),
    }
}

fn dim_zero(scene: &Scene, doc: &Value, diags: &Diags) {
    for v in arr_of(doc, "views").unwrap_or(&[]) {
        if str_of(v, "kind") == "iso" {
            continue;
        }
        let vid = str_of(v, "id");
        for an in arr_of(v, "annotations").unwrap_or(&[]) {
            if str_of(an, "type") != "dim" {
                continue;
            }
            let (Some(fv), Some(tv)) = (an.get("from"), an.get("to")) else { continue };
            let (Some(from), Some(to)) = (quiet_point(scene, fv), quiet_point(scene, tv)) else { continue };
            let explicit = str_of(an, "dir");
            let dir = if explicit.is_empty() { dominant_dir(from, to) } else { explicit };
            if !matches!(dir, "h" | "v" | "aligned") {
                continue;
            }
            let m = dim_measure(from, to, dir);
            if m >= 1.0 / 16.0 {
                continue;
            }
            let aid = str_of(an, "id");
            let dx = (to.x - from.x).abs();
            let dy = (to.y - from.y).abs();
            let path = format!("views/{vid}/annotations/{aid}");
            if dx < 1.0 / 16.0 && dy < 1.0 / 16.0 {
                diags.add_fix(
                    Level::Warning,
                    "W_DIM_ZERO",
                    Some(aid),
                    Some(&path),
                    format!(
                        "dim '{aid}' in view {vid} measures {}: 'from' and 'to' are the same point",
                        units::fmt_ft_in(m)
                    ),
                    "point `from` and `to` at two different anchors, e.g. two corners of the member".to_string(),
                );
            } else {
                let other = if dir == "h" { "v" } else { "h" };
                diags.add_fix(
                    Level::Warning,
                    "W_DIM_ZERO",
                    Some(aid),
                    Some(&path),
                    format!(
                        "dim '{aid}' in view {vid} measures {} along dir \"{dir}\" (the points are {} apart horizontally and {} vertically)",
                        units::fmt_ft_in(m),
                        units::fmt_ft_in(dx),
                        units::fmt_ft_in(dy)
                    ),
                    format!("set \"dir\": \"{other}\" (or \"aligned\" for the true distance)"),
                );
            }
        }
    }
}

fn undirected(an: &Value) -> bool {
    str_of(an, "type") == "dim" && an.get("dir").is_none_or(Value::is_null)
}

/// True when some dim has no `dir` (the document needs `with_dim_dirs` before it is drawn).
pub fn needs_dim_dirs(doc: &Value) -> bool {
    matches!(doc, Value::Object(_))
        && arr_of(doc, "views")
            .unwrap_or(&[])
            .iter()
            .any(|v| arr_of(v, "annotations").unwrap_or(&[]).iter().any(undirected))
}

fn set_member(obj: &mut Value, key: &str, val: Value) {
    if let Value::Object(ms) = obj {
        match ms.iter_mut().find(|m| m.key == key) {
            Some(m) => m.value = val,
            None => ms.push(Member { key: key.to_string(), value: val }),
        }
    }
}

/// The document with `dir` filled in on every section-view dim that lacks one (dominant axis). The stored document is
/// never changed: renderers get this copy. Unresolvable endpoints are left alone (the view build reports them).
pub fn with_dim_dirs<'d>(scene: &Scene, doc: &'d Value) -> Cow<'d, Value> {
    if !needs_dim_dirs(doc) {
        return Cow::Borrowed(doc);
    }
    let mut out = doc.clone();
    let Some(Value::Array(vs)) = out.get_mut("views") else { return Cow::Owned(out) };
    for v in vs {
        if !matches!(v, Value::Object(_)) {
            continue;
        }
        let Some(Value::Array(anns)) = v.get_mut("annotations") else { continue };
        for an in anns {
            if !matches!(an, Value::Object(_)) || str_of(an, "type") != "dim" {
                continue;
            }
            if an.get("dir").is_some_and(|d| !d.is_null()) {
                continue;
            }
            let (Some(fv), Some(tv)) = (an.get("from"), an.get("to")) else { continue };
            let (Some(from), Some(to)) = (quiet_point(scene, fv), quiet_point(scene, tv)) else { continue };
            set_member(an, "dir", Value::String(dominant_dir(from, to).to_string()));
        }
    }
    Cow::Owned(out)
}

/// Compile-and-patch for callers that hold only the document (api export/drawing): one extra compile, only when a dim lacks `dir`.
pub fn with_dim_dirs_from_doc<'d>(doc: &'d Value, st: &Style) -> Cow<'d, Value> {
    if !needs_dim_dirs(doc) {
        return Cow::Borrowed(doc);
    }
    let scratch = Diags::new();
    let scene = compile::compile(doc, st, &scratch);
    Cow::Owned(with_dim_dirs(&scene, doc).into_owned())
}

// ---- acknowledge ----------------------------------------------------------------------------------------

/// Warnings are suppressed; info codes (I_*) are accepted as a no-op (an agent acknowledging I_SOLID_USED is not an error).
fn is_ackable(code: &str) -> bool {
    code.starts_with("W_") || code.starts_with("I_")
}

/// Shape errors of `acknowledge` (array of {code, reason}; codes must be warning codes).
fn ack_shape(doc: &Value, diags: &Diags) {
    for c in arr_of(doc, "components").unwrap_or(&[]) {
        let Some(ak) = c.get("acknowledge") else { continue };
        if ak.is_null() {
            continue;
        }
        let cid = str_of(c, "id");
        let path = format!("components/{cid}/acknowledge");
        let Some(items) = ak.arr() else {
            diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(cid),
                Some(&path),
                "'acknowledge' must be an array of {\"code\": \"W_...\", \"reason\": \"...\"}".to_string(),
                "e.g. \"acknowledge\": [{\"code\": \"W_UNTREATED_CONTACT\", \"reason\": \"truss seat moisture barrier by mfr.\"}]".to_string(),
            );
            continue;
        };
        for (i, it) in items.iter().enumerate() {
            let code = str_of(it, "code");
            let reason = str_of(it, "reason").trim_matches(' ');
            let ipath = format!("{path}/{i}");
            if !matches!(it, Value::Object(_)) || code.is_empty() || reason.is_empty() {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    Some(cid),
                    Some(&ipath),
                    format!("acknowledge entry {i} of '{cid}' needs a string \"code\" and a non-empty \"reason\""),
                    "e.g. {\"code\": \"W_UNTREATED_CONTACT\", \"reason\": \"why this is fine\"}".to_string(),
                );
            } else if !is_ackable(code) {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    Some(cid),
                    Some(&ipath),
                    format!("'{code}' cannot be acknowledged: only warnings (W_*) can (I_* is accepted and ignored); errors must be fixed"),
                    "fix the cause, or acknowledge a W_ code".to_string(),
                );
            }
        }
    }
}

fn mentions(d: &Diag, id: &str) -> bool {
    if d.id.as_deref() == Some(id) {
        return true;
    }
    if matches!(d.code, "W_OVERLAP" | "W_NEAR_MISS" | "W_UNTREATED_CONTACT") {
        return quoted(&d.message, id);
    }
    false
}

/// `'id'` occurs in `msg` (any id length).
fn quoted(msg: &str, id: &str) -> bool {
    let m = msg.as_bytes();
    let mut from = 0;
    while let Some(off) = msg.get(from..).and_then(|s| s.find(id)) {
        let i = from + off;
        if i > 0 && m[i - 1] == b'\'' && i + id.len() < m.len() && m[i + id.len()] == b'\'' {
            return true;
        }
        from = i + 1;
        while !msg.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// Replace every warning that a component acknowledges with one `I_ACK` info line (code, component, reason).
/// Call after all other diagnostics are collected.
pub fn apply_acknowledge(doc: &Value, diags: &Diags) {
    let Some(cs) = arr_of(doc, "components") else { return };
    if !cs.iter().any(|c| c.get("acknowledge").is_some()) {
        return;
    }
    let mut kept: Vec<Diag> = Vec::new();
    let mut acks: Vec<Diag> = Vec::new();
    for d in diags.take() {
        let mut matched: Option<(&str, &str)> = None;
        if d.level == Level::Warning {
            'outer: for c in cs {
                let cid = str_of(c, "id");
                let Some(items) = c.get("acknowledge").and_then(Value::arr) else { continue };
                for it in items {
                    if !matches!(it, Value::Object(_)) || str_of(it, "code") != d.code {
                        continue;
                    }
                    if str_of(it, "reason").trim_matches(' ').is_empty() {
                        continue;
                    }
                    if mentions(&d, cid) {
                        matched = Some((cid, str_of(it, "reason")));
                        break 'outer;
                    }
                }
            }
        }
        match matched {
            Some((cid, reason)) => acks.push(Diag {
                level: Level::Info,
                code: "I_ACK",
                id: Some(cid.to_string()),
                path: d.path.clone(),
                message: format!("{} on '{cid}' acknowledged: {reason} (was: {})", d.code, d.message),
                fix: None,
            }),
            None => kept.push(d),
        }
    }
    kept.extend(acks);
    diags.set(kept);
}
