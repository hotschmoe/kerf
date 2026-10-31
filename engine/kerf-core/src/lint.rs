//! Document lints (SPEC 19): W_UNKNOWN_KEY, W_NOTE_STYLE, W_DIM_ZERO, `acknowledge` handling (I_ACK),
//! and the dim `dir` default. They look at the document and the compiled scene, not at geometry output;
//! field names come from schema_fields / catalog (the same tables the parsers and `kerf schema` use).

use crate::compile;
use crate::coverage;
use crate::geom::V2;
use crate::house;
use crate::json::{self, Member, Value};
use crate::model::{self, Diag, Diags, Level};
use crate::references;
use crate::scene::{self, Scene};
use crate::schema_fields::{self as schema, Object};
use crate::style::Style;
use crate::units;
use crate::view;
use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn run(scene: &Scene, doc: &Value, cov: &[coverage::Item], diags: &Diags) {
    if !matches!(doc, Value::Object(_)) {
        return;
    }
    unknown_keys(doc, diags);
    layout_keys(doc, diags);
    note_style(doc, scene.style, diags);
    dim_zero(scene, doc, diags);
    ack_shape(doc, diags);
    members(doc, diags);
    requested_missing(doc, cov, diags);
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
                "weld" => &schema::WELD,
                "ref" => &schema::REF,
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

fn layout_keys(doc: &Value, diags: &Diags) {
    for l in arr_of(doc, "layouts").unwrap_or(&[]) {
        let lid = str_of(l, "id");
        if matches!(l, Value::Object(_)) && !lid.is_empty() {
            lint_object(diags, l, &schema::LAYOUT, &format!("layouts/{lid}"), Some(lid));
        }
    }
}

// ---- view membership (SPEC 25.4) -----------------------------------------------------------------------

fn did_you_mean(x: &str, names: &[&str]) -> String {
    model::nearest(x, names).map(|n| format!(" Did you mean '{n}'?")).unwrap_or_default()
}

/// Component `views` must name existing views; view `omit` / `only` are lists of ids or `*` globs. An id or glob that
/// matches no component warns (it may name a component a later edit adds), so ops can arrive in any order.
fn members(doc: &Value, diags: &Diags) {
    let comps = arr_of(doc, "components").unwrap_or(&[]);
    let cids: Vec<&str> = comps.iter().map(|c| str_of(c, "id")).filter(|s| !s.is_empty()).collect();
    let views = arr_of(doc, "views").unwrap_or(&[]);
    let vids: Vec<&str> = views.iter().map(|v| str_of(v, "id")).filter(|s| !s.is_empty()).collect();
    let view_list =
        if vids.is_empty() { "(none: the document has no views)".to_string() } else { scene::join_ids(&vids) };
    for c in comps {
        let Some(vv) = c.get("views").filter(|v| !v.is_null()) else { continue };
        let cid = str_of(c, "id");
        let path = format!("components/{cid}/views");
        let Some(list) = vv.arr().filter(|a| !a.is_empty() && a.iter().all(|x| x.str().is_some())) else {
            diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(cid),
                Some(&path),
                format!("'views' of '{cid}' must be a non-empty array of view ids (got {}). Views: {view_list}", model::kind_or_text(vv)),
                format!("e.g. \"views\": [\"{}\"]; remove \"views\" to draw '{cid}' in every view, or use \"visible\": false to hide it everywhere", vids.first().copied().unwrap_or("A")),
            );
            continue;
        };
        for v in list.iter().filter_map(Value::str) {
            if !vids.contains(&v) {
                diags.add_fix(
                    Level::Error,
                    "E_REF_UNKNOWN",
                    Some(cid),
                    Some(&path),
                    format!("'{cid}' lists view '{v}' in \"views\", but there is no view '{v}'. Views: {view_list}"),
                    format!(
                        "list existing view ids only, or add view '{v}' (op add, path \"views\") in the same batch"
                    ),
                );
            }
        }
    }
    for v in views {
        let vid = str_of(v, "id");
        if vid.is_empty() {
            continue;
        }
        for key in ["omit", "only"] {
            let Some(pv) = v.get(key).filter(|x| !x.is_null()) else { continue };
            let path = format!("views/{vid}/{key}");
            let Some(pats) = pv.arr().filter(|a| a.iter().all(|x| x.str().is_some())) else {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    Some(vid),
                    Some(&path),
                    format!(
                        "view {vid}: \"{key}\" must be an array of component ids or `*` globs (got {})",
                        model::kind_or_text(pv)
                    ),
                    format!("e.g. \"{key}\": [\"b_*\", \"roof_sheathing\"]"),
                );
                continue;
            };
            if key == "only" && pats.is_empty() {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    Some(vid),
                    Some(&path),
                    format!("view {vid}: \"only\": [] draws no component"),
                    "list the ids or globs the view draws (e.g. [\"shared_*\", \"a_*\"]), or remove \"only\" to draw every component".to_string(),
                );
                continue;
            }
            for p in pats.iter().filter_map(Value::str) {
                if cids.iter().any(|id| view::glob_match(p, id)) {
                    continue;
                }
                let verb = if key == "omit" { "omits" } else { "lists in \"only\"" };
                let message = if view::is_glob(p) {
                    format!("view {vid} {verb} '{p}', a pattern that matches no component id, so it has no effect")
                } else {
                    format!(
                        "view {vid} {verb} '{p}', which is not a component id (yet), so it has no effect.{}",
                        did_you_mean(p, &cids)
                    )
                };
                diags.add_fix(
                    Level::Warning,
                    "W_REF_UNKNOWN",
                    Some(vid),
                    Some(&path),
                    message,
                    format!("fix the id or pattern (`*` matches any run of characters), add the component, or remove '{p}' from views/{vid}/{key}"),
                );
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

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric()
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
    /// `fixed` keeps every model number of the text (upper-cased): a fix that would change one is not offered.
    pub fix_safe: bool,
}

/// Longest note, in characters: one idea per note. SPEC 21 says 120; the reference flush-beam strap note is 121 characters and
/// the reference details must stay clean, so the limit is 130 (recorded in NOTES.md).
const MAX_NOTE_LEN: usize = 130;

/// Words that make a note read as a person talking (matched as whole words, case-insensitive).
const CHATTER_WORDS: [&str; 2] = ["WE", "PLEASE"];
const CHATTER_PHRASES: [&str; 2] = ["SHOULD BE", "NOTE:"];

/// Words before a Roman numeral `I` (`TYPE I CEMENT`, `CLASS I`): not first person.
const NUMERAL_BEFORE: [&str; 14] = [
    "TYPE", "CLASS", "GRADE", "CATEGORY", "CAT.", "ZONE", "GROUP", "PHASE", "LEVEL", "TIER", "STAGE", "OPTION", "CASE",
    "PART",
];

/// The word that ends just before `at` (upper case, trailing spaces skipped).
fn word_before(text: &[u8], at: usize) -> String {
    let mut e = at;
    while e > 0 && text[e - 1] == b' ' {
        e -= 1;
    }
    let mut s = e;
    while s > 0 && !matches!(text[s - 1], b' ' | b'(' | b',') {
        s -= 1;
    }
    String::from_utf8_lossy(&text[s..e]).to_ascii_uppercase()
}

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
        let numeral = NUMERAL_BEFORE.contains(&word_before(text, at).as_str());
        if !numeral && matches!(text.get(at + 1), Some(b' ' | b'\'')) {
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

fn add_issue(list: &mut String, what: &str) {
    if !list.is_empty() {
        list.push_str("; ");
    }
    list.push_str(what);
}

/// Tokens a mechanical fix must leave alone apart from upper-casing: catalog and model numbers (`HU212-2`, `H2.5A`, `A35`),
/// i.e. runs of letters, digits, `-`, `/` and inner `.` that hold both a letter and a digit.
fn model_tokens(text: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    for raw in text.split(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b',' | b';' | b':')) {
        let t = trim_end(raw, b".");
        let has_letter = t.iter().any(u8::is_ascii_alphabetic);
        let has_digit = t.iter().any(u8::is_ascii_digit);
        if has_letter && has_digit && t.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'/' | b'.')) {
            out.push(t);
        }
    }
    out
}

/// Replace `word` by `abbr` everywhere (whole words), without doubling a period that follows (`EACH.` -> `EA.`).
fn abbreviate(work: Vec<u8>, word: &str, abbr: &str) -> Vec<u8> {
    let mut work = work;
    let mut from = 0;
    let mut guard = 0;
    while let Some(at) = find_word(&work, word, from) {
        if guard > 50 {
            break;
        }
        guard += 1;
        let mut end = at + word.len();
        if abbr.ends_with('.') && work.get(end) == Some(&b'.') {
            end += 1;
        }
        let mut next = Vec::with_capacity(work.len());
        next.extend_from_slice(&work[..at]);
        next.extend_from_slice(abbr.as_bytes());
        next.extend_from_slice(&work[end..]);
        from = at + abbr.len();
        work = next;
    }
    work
}

/// The text (spelled-out words already abbreviated) ends on a period that is not part of an abbreviation, a dotted
/// token (`O.C.`) or a number/inch mark.
fn trailing_period(t: &[u8], st: &Style) -> bool {
    if t.len() <= 1 || t[t.len() - 1] != b'.' {
        return false;
    }
    let base = trim_end(last_token(t), b".");
    !(base.contains(&b'.') || house::is_period_abbr(base, st) || base.last().is_some_and(|&c| !c.is_ascii_alphabetic()))
}

/// Check one note text against the house style (SPEC 19, 25.3). Pure. Penny sizes (`16d`), ACI symbols (`db`, `ld`,
/// `f'c`) and the style's `text.keep_case` tokens keep their case; a period after a house abbreviation is not a trailing
/// period; a model number with a dash (`HU212-2/HUC212-2`) is not a dashed fraction.
pub fn check_note_text(text: &str, st: &Style) -> NoteStyle {
    let tb = text.as_bytes();
    let mut issues = String::new();
    let lower = house::has_folding_lowercase(tb, st);
    if lower {
        add_issue(
            &mut issues,
            "lowercase letters (notes are UPPERCASE; penny sizes like 16d and ACI symbols like db, ld, f'c may stay as written)",
        );
    }
    if tb.windows(3).any(|w| w == b" x ") {
        add_issue(&mut issues, "\" x \" as a size separator (use \" X \")");
    }
    if house::has_dash_fraction(tb) {
        add_issue(&mut issues, "dashed fraction like 1-1/2\" (house style is 1 1/2\")");
    }
    for sp in &SPELLED_OUT {
        if find_word(tb, sp.word, 0).is_some() {
            add_issue(&mut issues, &format!("\"{}\" spelled out (use \"{}\")", sp.word, sp.abbr));
        }
    }
    let mut work: Vec<u8> = tb.to_vec();
    for sp in &SPELLED_OUT {
        work = abbreviate(work, sp.word, sp.abbr);
    }
    let trimmed = trim_end(&work, &WS);
    let trailing = trailing_period(trimmed, st);
    if trailing {
        let word = String::from_utf8_lossy(trim_end(last_token(trimmed), b"."));
        add_issue(
            &mut issues,
            &format!(
                "trailing period (notes are not sentences; if \"{word}.\" is an abbreviation, add it to the style's notes.abbreviations)"
            ),
        );
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
        return NoteStyle { issues, fixed: text.to_string(), rewrite: false, fix_safe: true };
    }

    let up = house::upper_bytes(&work, st);
    let mut out: Vec<u8> = Vec::with_capacity(up.len());
    for k in 0..up.len() {
        if house::dash_fraction_at(&up, k) {
            out.push(b' ');
            continue;
        }
        out.push(up[k]);
    }
    let mut res = trim_end(&out, &WS);
    if trailing && res.last() == Some(&b'.') {
        res = &res[..res.len() - 1];
    }
    let fixed = String::from_utf8_lossy(res).into_owned();
    let fix_safe = model_tokens(tb).iter().all(|m| {
        let want = house::upper_bytes(m, st);
        fixed.as_bytes().windows(want.len()).any(|w| w == want.as_slice())
    });
    NoteStyle { issues, fixed, rewrite, fix_safe }
}

fn note_style(doc: &Value, st: &Style, diags: &Diags) {
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
            let ns = check_note_text(text, st);
            if !ns.issues.is_empty() {
                let fix = if !ns.fix_safe {
                    "edit the text by hand to fix the problems listed (no automatic fix: it would change a model number in the text)".to_string()
                } else if ns.rewrite && ns.fixed == text {
                    "rewrite it as <SIZE/QTY> <MATERIAL> <ITEM> <W/ ATTACHMENT> <@ SPACING>, one idea, no commentary (split a long note in two)".to_string()
                } else if ns.rewrite {
                    format!(
                        "rewrite it as <SIZE/QTY> <MATERIAL> <ITEM> <W/ ATTACHMENT> <@ SPACING>, one idea, no commentary; the mechanical part is \"{}\"",
                        ns.fixed
                    )
                } else {
                    format!("set text to \"{}\"", ns.fixed)
                };
                diags.add_fix(
                    Level::Warning,
                    "W_NOTE_STYLE",
                    Some(aid),
                    Some(&path),
                    format!("note '{aid}' in view {vid} breaks the house note style: {}. Text: \"{text}\"", ns.issues),
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

/// `s` cut to at most `n` characters, with `...` when cut.
fn clip_text(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => String::from(s),
    }
}

/// `meta.requested` must be an array of non-empty strings; every item that matches nothing in the document warns.
fn requested_missing(doc: &Value, cov: &[coverage::Item], diags: &Diags) {
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
    for (i, item) in cov.iter().enumerate() {
        if !item.found.is_empty() {
            continue;
        }
        let closest = match &item.closest {
            Some(c) => format!(
                "; closest: {} \"{}\" ({} of {} words; lacks {})",
                c.source,
                clip_text(&c.text, 80),
                c.have,
                c.total,
                model::join_quoted(&c.missing.iter().map(String::as_str).collect::<Vec<_>>())
            ),
            None => String::from("; no candidate shares any of its words"),
        };
        diags.add_fix(
            Level::Warning,
            "W_REQUESTED_MISSING",
            None,
            Some(&format!("meta/requested/{i}")),
            format!(
                "requested element \"{}\" (meta.requested) matches no component (id, type, label, model, size) or annotation (note, label, dim text, weld, view title){closest}",
                item.text
            ),
            format!(
                "build it (name the component after it, or add a note whose text says \"{}\"; house abbreviations such as EA. = EACH match either way), or remove it from meta.requested if the designer dropped it",
                item.text
            ),
        );
    }
}

// ---- dim directions -------------------------------------------------------------------------------------

/// Resolve a dim endpoint without reporting reference errors (the view build reports those).
pub fn quiet_point(scene: &Scene, v: &Value) -> Option<V2> {
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

pub fn dim_measure(from: V2, to: V2, dir: &str) -> f64 {
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

fn entries(c: &Value) -> &[Value] {
    arr_of(c, "acknowledge").unwrap_or(&[])
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

/// A warning that a component's `acknowledge` entry suppressed (SPEC 25.4): the component, the entry's reason and the
/// warning as it would have printed.
#[derive(Clone, Debug)]
pub struct Acked {
    pub comp: String,
    pub reason: String,
    pub warning: Diag,
}

impl Acked {
    pub fn to_json(&self) -> Value {
        let mut m = alloc::vec![
            Member { key: "code".into(), value: Value::String(self.warning.code.into()) },
            Member { key: "id".into(), value: Value::String(self.comp.clone()) },
            Member { key: "reason".into(), value: Value::String(self.reason.clone()) },
        ];
        if let Some(p) = &self.warning.path {
            m.push(Member { key: "path".into(), value: Value::String(p.clone()) });
        }
        m.push(Member { key: "message".into(), value: Value::String(self.warning.message.clone()) });
        if let Some(f) = &self.warning.fix {
            m.push(Member { key: "fix".into(), value: Value::String(f.clone()) });
        }
        Value::Object(m)
    }
}

pub fn acked_json(list: &[Acked]) -> Value {
    Value::Array(list.iter().map(Acked::to_json).collect())
}

/// The document has at least one `acknowledge` entry.
pub fn has_acknowledge(doc: &Value) -> bool {
    arr_of(doc, "components").unwrap_or(&[]).iter().any(|c| arr_of(c, "acknowledge").is_some_and(|a| !a.is_empty()))
}

/// How `apply_acknowledge` treats acknowledged warnings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AckMode {
    /// `kerf check --strict`: acknowledged warnings stay warnings (their message says who acknowledged them and why).
    pub strict: bool,
    /// Every diagnostic of the document is in the list (all views built, no errors), so an entry that matched
    /// nothing is stale: report it as W_ACK_UNUSED.
    pub complete: bool,
}

/// One W_ACK_UNUSED for the entries of component `c` that suppressed nothing, with the exact op that removes them.
fn unused_entries(c: &Value, used: &[bool]) -> Option<Diag> {
    let cid = str_of(c, "id");
    let all = entries(c);
    let valid = |it: &Value| matches!(it, Value::Object(_)) && !str_of(it, "reason").trim_matches(' ').is_empty();
    let stale: Vec<usize> = (0..all.len()).filter(|&k| !used[k] && valid(&all[k])).collect();
    if stale.is_empty() {
        return None;
    }
    let mut buckets: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for &k in &stale {
        let code = str_of(&all[k], "code");
        let b = if code.starts_with("I_") {
            1
        } else if all[..k].iter().any(|e| valid(e) && str_of(e, "code") == code) {
            2
        } else {
            0
        };
        buckets[b].push(format!("{code} (#{k})"));
    }
    let mut parts: Vec<String> = Vec::new();
    if !buckets[0].is_empty() {
        parts.push(format!("no {} names '{cid}' in any view", buckets[0].join(", ")));
    }
    if !buckets[1].is_empty() {
        parts.push(format!(
            "{} {} info, never suppressed",
            buckets[1].join(", "),
            if buckets[1].len() == 1 { "is" } else { "are" }
        ));
    }
    if !buckets[2].is_empty() {
        parts.push(format!(
            "{} repeat{} an earlier entry",
            buckets[2].join(", "),
            if buckets[2].len() == 1 { "s" } else { "" }
        ));
    }
    let list = parts.join("; ");
    let keep: Vec<Value> = (0..all.len()).filter(|k| !stale.contains(k)).map(|k| all[k].clone()).collect();
    let mut value = Vec::new();
    json::write_compact(&mut value, &if keep.is_empty() { Value::Null } else { Value::Array(keep) });
    let value = String::from_utf8_lossy(&value);
    let (n, total) = (stale.len(), all.len());
    Some(Diag {
        level: Level::Warning,
        code: "W_ACK_UNUSED",
        id: Some(cid.to_string()),
        path: Some(format!("components/{cid}/acknowledge")),
        message: format!(
            "{n} of the {total} acknowledge entr{} of '{cid}' suppress{} nothing ({list}); a stale acknowledgement would hide the next real finding",
            if total == 1 { "y" } else { "ies" },
            if n == 1 { "es" } else { "" }
        ),
        fix: Some(format!(
            "remove {}: {{\"op\":\"update\",\"path\":\"components/{cid}\",\"value\":{{\"acknowledge\":{value}}}}}",
            if n == 1 { "it" } else { "them" }
        )),
    })
}

/// Replace every warning that a component acknowledges with one `I_ACK` info line (code, component, reason) and
/// return the suppressed warnings. Call after all other diagnostics are collected.
pub fn apply_acknowledge(doc: &Value, diags: &Diags, mode: AckMode) -> Vec<Acked> {
    let Some(cs) = arr_of(doc, "components") else { return Vec::new() };
    if !cs.iter().any(|c| c.get("acknowledge").is_some()) {
        return Vec::new();
    }
    let valid = |it: &Value| matches!(it, Value::Object(_)) && !str_of(it, "reason").trim_matches(' ').is_empty();
    let mut used: Vec<Vec<bool>> = cs.iter().map(|c| alloc::vec![false; entries(c).len()]).collect();
    let mut kept: Vec<Diag> = Vec::new();
    let mut infos: Vec<Diag> = Vec::new();
    let mut acked: Vec<Acked> = Vec::new();
    for mut d in diags.take() {
        let mut matched: Option<(&str, &str)> = None;
        if d.level == Level::Warning {
            for (ci, c) in cs.iter().enumerate() {
                let cid = str_of(c, "id");
                for (k, it) in entries(c).iter().enumerate() {
                    if !valid(it) || str_of(it, "code") != d.code || !mentions(&d, cid) {
                        continue;
                    }
                    used[ci][k] = true;
                    matched.get_or_insert((cid, str_of(it, "reason")));
                    break;
                }
            }
        }
        let Some((cid, reason)) = matched else {
            kept.push(d);
            continue;
        };
        acked.push(Acked { comp: cid.to_string(), reason: reason.to_string(), warning: d.clone() });
        if mode.strict {
            d.message.push_str(&format!(" [acknowledged on '{cid}': {reason}]"));
            kept.push(d);
        } else {
            infos.push(Diag {
                level: Level::Info,
                code: "I_ACK",
                id: Some(cid.to_string()),
                path: d.path.clone(),
                message: format!("{} on '{cid}' acknowledged: {reason} (was: {})", d.code, d.message),
                fix: None,
            });
        }
    }
    if mode.complete {
        for (ci, c) in cs.iter().enumerate() {
            if let Some(d) = unused_entries(c, &used[ci]) {
                kept.push(d);
            }
        }
    }
    kept.extend(infos);
    diags.set(kept);
    acked
}
