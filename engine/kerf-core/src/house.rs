//! House text rules shared by the note lint, the renderer's case transform and the `meta.requested` coverage (SPEC 25.3):
//! the abbreviation table, the tokens whose case is significant, the dashed-fraction test, and the normalized word list
//! coverage matches on.

use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;

/// A house abbreviation and the word(s) it stands for (the first entry is the canonical spelling).
pub struct Abbr {
    pub abbr: &'static str,
    pub words: &'static [&'static str],
    /// The bare letters are also an ordinary word: coverage reads them as the abbreviation only when written with the period.
    pub dot: bool,
}

const fn ab(abbr: &'static str, words: &'static [&'static str]) -> Abbr {
    Abbr { abbr, words, dot: false }
}

const fn ab_dot(abbr: &'static str, words: &'static [&'static str]) -> Abbr {
    Abbr { abbr, words, dot: true }
}

/// The house abbreviation list. A note may end on any of these with its period; coverage reads each as its words.
pub static ABBREVIATIONS: [Abbr; 90] = [
    ab("EA.", &["EACH"]),
    ab("CONT.", &["CONTINUOUS"]),
    ab("TYP.", &["TYPICAL"]),
    ab("MIN.", &["MINIMUM"]),
    ab("MAX.", &["MAXIMUM"]),
    ab("CLR.", &["CLEAR"]),
    ab("DIA.", &["DIAMETER"]),
    ab("BOTT.", &["BOTTOM"]),
    ab("BOT.", &["BOTTOM"]),
    ab("CONC.", &["CONCRETE"]),
    ab("REINF.", &["REINFORCING", "REINFORCEMENT", "REINFORCED"]),
    ab("FA.", &["FACE"]),
    ab("GA.", &["GAUGE", "GAGE"]),
    ab_dot("SIM.", &["SIMILAR"]),
    ab("EMBED.", &["EMBEDMENT"]),
    ab("MFR.", &["MANUFACTURER"]),
    ab("MFR'S", &["MANUFACTURER"]),
    ab("FTG.", &["FOOTING"]),
    ab("FDN.", &["FOUNDATION"]),
    ab("GRD.", &["GRADE"]),
    ab("HDR.", &["HEADER"]),
    ab("DBL.", &["DOUBLE"]),
    ab("STL.", &["STEEL"]),
    ab("GALV.", &["GALVANIZED"]),
    ab("VERT.", &["VERTICAL"]),
    ab("HORIZ.", &["HORIZONTAL"]),
    ab("GYP.", &["GYPSUM"]),
    ab("BD.", &["BOARD"]),
    ab("SHTG.", &["SHEATHING"]),
    ab("PLY.", &["PLYWOOD"]),
    ab("STD.", &["STANDARD"]),
    ab("SQ.", &["SQUARE"]),
    ab("EQ.", &["EQUAL"]),
    ab("NOM.", &["NOMINAL"]),
    ab("APPROX.", &["APPROXIMATE", "APPROXIMATELY"]),
    ab("REQD.", &["REQUIRED"]),
    ab("REQ'D", &["REQUIRED"]),
    ab("SPEC.", &["SPECIFICATION"]),
    ab("SPECS.", &["SPECIFICATIONS"]),
    ab("DWG.", &["DRAWING"]),
    ab("DWGS.", &["DRAWINGS"]),
    ab("EXIST.", &["EXISTING"]),
    ab("EXIST'G", &["EXISTING"]),
    ab("THK.", &["THICK", "THICKNESS"]),
    ab_dot("ALT.", &["ALTERNATE"]),
    ab_dot("ARCH.", &["ARCHITECT", "ARCHITECTURAL"]),
    ab("EXT.", &["EXTERIOR"]),
    ab_dot("INT.", &["INTERIOR"]),
    ab_dot("EXP.", &["EXPANSION"]),
    ab_dot("FIN.", &["FINISH", "FINISHED"]),
    ab("FLR.", &["FLOOR"]),
    ab("ELEV.", &["ELEVATION"]),
    ab("EL.", &["ELEVATION"]),
    ab("STRUCT.", &["STRUCTURAL"]),
    ab("ENGR.", &["ENGINEER"]),
    ab("MECH.", &["MECHANICAL"]),
    ab("ELEC.", &["ELECTRICAL"]),
    ab("CTR.", &["CENTER"]),
    ab("JT.", &["JOINT"]),
    ab("JST.", &["JOIST"]),
    ab("BM.", &["BEAM"]),
    ab_dot("COL.", &["COLUMN"]),
    ab("CONN.", &["CONNECTION"]),
    ab("OPNG.", &["OPENING"]),
    ab("PERP.", &["PERPENDICULAR"]),
    ab("LG.", &["LONG"]),
    ab("ADDL.", &["ADDITIONAL"]),
    ab("BLKG.", &["BLOCKING"]),
    ab("SCHED.", &["SCHEDULE"]),
    ab("SECT.", &["SECTION"]),
    ab("DET.", &["DETAIL"]),
    ab_dot("DIM.", &["DIMENSION"]),
    ab("OPP.", &["OPPOSITE"]),
    ab("INSUL.", &["INSULATION"]),
    ab("THRU", &["THROUGH"]),
    ab("BTWN.", &["BETWEEN"]),
    ab("BRG.", &["BEARING"]),
    ab("HT.", &["HEIGHT"]),
    ab("PL.", &["PLATE"]),
    ab("EOR", &["ENGINEER OF RECORD"]),
    ab("O.C.", &["ON CENTER"]),
    ab("E.F.", &["EACH FACE"]),
    ab("E.W.", &["EACH WAY"]),
    ab("T&B", &["TOP AND BOTTOM"]),
    ab("T.O.", &["TOP OF"]),
    ab("B.O.", &["BOTTOM OF"]),
    ab("U.N.O.", &["UNLESS NOTED OTHERWISE"]),
    ab("UNO", &["UNLESS NOTED OTHERWISE"]),
    ab("PT", &["PRESSURE TREATED"]),
    ab("CMU", &["CONCRETE MASONRY UNIT"]),
];

/// Abbreviations that end a note with their period although they stand for no word coverage should expand.
pub const PERIOD_ONLY: [&str; 3] = ["NO", "NOS", "DWGS"];

/// ACI / AISC / NDS symbols and units whose lower case is significant (`48 db`, `ld`, `f'c`, `Fy`, `3,000 psi`). Matched
/// exactly, optionally after a number (`8db`).
pub const KEEP_CASE: [&str; 33] = [
    "db", "ld", "ldh", "ldc", "ldt", "lc", "lst", "f'c", "f'ci", "f'm", "f'g", "fy", "fyt", "fu", "fc", "fr", "fs",
    "Fy", "Fu", "Fb", "Fv", "Fc", "Fp", "Fnt", "Fnv", "ksi", "psi", "psf", "plf", "pcf", "kip", "kips", "ksf",
];

fn is_tok_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'\'' || c >= 0x80
}

/// The case-significant core of a token: `’` read as `'`.
fn tok_core(t: &[u8]) -> String {
    String::from_utf8_lossy(t).replace('\u{2019}', "'")
}

/// A penny nail size (`16d`, `8d`): digits then a lower-case d.
fn is_penny(t: &[u8]) -> bool {
    t.len() >= 2 && t[t.len() - 1] == b'd' && t[..t.len() - 1].iter().all(u8::is_ascii_digit)
}

/// `t` is a token (a run of letters, digits and apostrophes) whose case the house style keeps as written.
pub fn keep_case_token(t: &[u8], st: &Style) -> bool {
    if is_penny(t) {
        return true;
    }
    let digits = t.iter().take_while(|c| c.is_ascii_digit()).count();
    let rest = tok_core(&t[digits..]);
    if rest.is_empty() || !rest.bytes().any(|c| c.is_ascii_lowercase()) {
        return false;
    }
    KEEP_CASE.contains(&rest.as_str()) || st.keep_case.iter().any(|k| *k == rest || k.as_bytes() == t)
}

/// Token spans `(start, end)` of `text` for the case rules.
pub fn case_tokens(text: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if !is_tok_byte(text[i]) {
            i += 1;
            continue;
        }
        let s = i;
        while i < text.len() && is_tok_byte(text[i]) {
            i += 1;
        }
        let mut e = i;
        while e > s && text[e - 1] == b'\'' {
            e -= 1;
        }
        out.push((s, e));
    }
    out
}

/// Upper-case `text` except the tokens whose case is significant (penny sizes, ACI symbols, the style's `text.keep_case`).
pub fn upper_bytes(text: &[u8], st: &Style) -> Vec<u8> {
    let mut out = text.to_ascii_uppercase();
    for (s, e) in case_tokens(text) {
        if keep_case_token(&text[s..e], st) {
            out[s..e].copy_from_slice(&text[s..e]);
        }
    }
    out
}

pub fn upper(text: &str, st: &Style) -> String {
    String::from_utf8_lossy(&upper_bytes(text.as_bytes(), st)).into_owned()
}

/// Lower-case letters outside the tokens whose case is significant.
pub fn has_folding_lowercase(text: &[u8], st: &Style) -> bool {
    case_tokens(text).into_iter().any(|(s, e)| {
        let t = &text[s..e];
        t.iter().any(u8::is_ascii_lowercase) && !keep_case_token(t, st)
    })
}

/// A dashed fraction (`1-1/2"`) whose dash is at `k`: a whole number that starts a token, the dash, then a proper fraction
/// with a 2..64 power-of-two denominator that ends the token. `HU212-2/HUC212-2` is a model number, not a fraction.
pub fn dash_fraction_at(t: &[u8], k: usize) -> bool {
    if t.get(k) != Some(&b'-') || k == 0 || !t[k - 1].is_ascii_digit() {
        return false;
    }
    let mut s = k;
    while s > 0 && t[s - 1].is_ascii_digit() {
        s -= 1;
    }
    if s > 0 && (t[s - 1].is_ascii_alphanumeric() || matches!(t[s - 1], b'.' | b'-' | b'/' | b'#')) {
        return false;
    }
    let Some((num, j)) = digits_at(t, k + 1) else { return false };
    if t.get(j) != Some(&b'/') {
        return false;
    }
    let Some((den, e)) = digits_at(t, j + 1) else { return false };
    if e < t.len() && (t[e].is_ascii_alphanumeric() || matches!(t[e], b'/' | b'.' | b'-')) {
        return false;
    }
    matches!(den, 2 | 4 | 8 | 16 | 32 | 64) && num > 0 && num < den
}

fn digits_at(t: &[u8], i: usize) -> Option<(u64, usize)> {
    let mut j = i;
    let mut n: u64 = 0;
    while j < t.len() && t[j].is_ascii_digit() && j - i < 6 {
        n = n * 10 + u64::from(t[j] - b'0');
        j += 1;
    }
    if j == i || (j < t.len() && t[j].is_ascii_digit()) { None } else { Some((n, j)) }
}

pub fn has_dash_fraction(t: &[u8]) -> bool {
    (0..t.len()).any(|k| dash_fraction_at(t, k))
}

/// `base` (a note's last word without its period) is a house abbreviation, or one the style adds.
pub fn is_period_abbr(base: &[u8], st: &Style) -> bool {
    let same = |a: &str| {
        let a = a.trim_end_matches('.');
        a.as_bytes().eq_ignore_ascii_case(base)
    };
    ABBREVIATIONS.iter().any(|a| same(a.abbr))
        || PERIOD_ONLY.iter().any(|a| same(a))
        || st.abbreviations.iter().any(|(a, _)| same(a))
}

// ---- coverage words ---------------------------------------------------------------------------------------------

const STOP_WORDS: [&str; 13] = ["w", "with", "and", "at", "of", "the", "a", "an", "to", "for", "in", "on", "per"];

/// One normalized word. `req`: it counts when it comes from a `meta.requested` item (the pieces of a length with units
/// do not, the length does); candidates count every word.
#[derive(Clone, Debug)]
struct Tok {
    s: String,
    dot: bool,
    req: bool,
}

fn word_tok(s: String, dot: bool) -> Tok {
    Tok { s, dot, req: true }
}

fn singular(mut w: Vec<u8>) -> Vec<u8> {
    if w.len() > 3 && w[w.len() - 1] == b's' && w[w.len() - 2] != b's' && w.iter().all(u8::is_ascii_alphanumeric) {
        w.pop();
    }
    w
}

fn sixteenths(inch: f64) -> i64 {
    crate::num::to_int_clamped(inch * 16.0 + 0.5, 0, 1 << 52)
}

/// A length at `i` (lower-case text): `15'-0"`, `4'`, `1 1/2"`, `5/8"`, `48in`, `15 ft`, `2.5 in.`; also a bare proper
/// fraction (`3/16`, `unit` false). Returns the end, the length in 1/16 in and whether units were written.
fn length_at(b: &[u8], i: usize) -> Option<(usize, i64, bool)> {
    if i > 0 && (b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b'.' | b'/')) {
        return None;
    }
    let num = |j: usize| -> Option<(f64, usize)> {
        let (w, mut e) = digits_at(b, j)?;
        let mut v = w as f64;
        if e + 1 < b.len() && b[e] == b'.' && b[e + 1].is_ascii_digit() {
            let (f, e2) = digits_at(b, e + 1)?;
            let mut d = 1.0;
            for _ in e + 1..e2 {
                d *= 10.0;
            }
            v += f as f64 / d;
            e = e2;
        }
        Some((v, e))
    };
    let frac = |j: usize| -> Option<(f64, usize)> {
        let (n, e) = digits_at(b, j)?;
        if b.get(e) != Some(&b'/') {
            return None;
        }
        let (d, e2) = digits_at(b, e + 1)?;
        if d == 0 || n >= d {
            return None;
        }
        Some((n as f64 / d as f64, e2))
    };
    let inches = |j: usize| -> Option<(f64, usize)> {
        if let Some(f) = frac(j) {
            return Some(f);
        }
        let (w, e) = num(j)?;
        if matches!(b.get(e), Some(b' ' | b'-'))
            && let Some((f, e2)) = frac(e + 1)
        {
            return Some((w + f, e2));
        }
        Some((w, e))
    };
    let word_end = |e: usize| e >= b.len() || !b[e].is_ascii_alphanumeric();
    let unit_word = |j: usize, words: &[&str]| -> Option<usize> {
        let mut k = j;
        if b.get(k) == Some(&b' ') {
            k += 1;
        }
        for w in words {
            let e = k + w.len();
            if e <= b.len() && &b[k..e] == w.as_bytes() && word_end(e) {
                return Some(if b.get(e) == Some(&b'.') { e + 1 } else { e });
            }
        }
        None
    };
    let (v, e) = inches(i)?;
    if b.get(e) == Some(&b'\'') {
        let feet = v;
        let mut k = e + 1;
        if b.get(k) == Some(&b'-') || (b.get(k) == Some(&b' ') && b.get(k + 1).is_some_and(u8::is_ascii_digit)) {
            k += 1;
        }
        if let Some((inch, e2)) = inches(k) {
            if b.get(e2) == Some(&b'"') {
                return Some((e2 + 1, sixteenths(feet * 12.0 + inch), true));
            }
            if b.get(e) == Some(&b'\'') && b.get(e + 1) == Some(&b'-') && word_end(e2) && b.get(e2) != Some(&b'/') {
                return Some((e2, sixteenths(feet * 12.0 + inch), true));
            }
        }
        return Some((e + 1, sixteenths(feet * 12.0), true));
    }
    if b.get(e) == Some(&b'"') {
        return Some((e + 1, sixteenths(v), true));
    }
    if let Some(e2) = unit_word(e, &["feet", "foot", "ft"]) {
        return Some((e2, sixteenths(v * 12.0), true));
    }
    if let Some(e2) = unit_word(e, &["inches", "inch", "in"]) {
        return Some((e2, sixteenths(v), true));
    }
    if let Some((f, e2)) = frac(i)
        && e2 == e
        && word_end(e2)
        && b.get(e2) != Some(&b'/')
    {
        return Some((e2, sixteenths(f), false));
    }
    None
}

/// Raw words of `text`: lower case, `’` as `'`, letters joined across `.` in dotted abbreviations (`o.c.`), across `&`
/// (`t&b`) and `'` (`req'd`, `f'c`); a plural `s` dropped; lengths added as `len:<sixteenths>`; stop words dropped.
fn raw_tokens(text: &str) -> Vec<Tok> {
    let low = text.replace(['\u{2019}', '\u{2032}'], "'").replace(['\u{201D}', '\u{2033}'], "\"").to_ascii_lowercase();
    let b = low.as_bytes();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit()
            && let Some((e, n, unit)) = length_at(b, i)
        {
            let mut k = i;
            while k < e {
                if b[k].is_ascii_alphanumeric() {
                    let s = k;
                    while k < e && b[k].is_ascii_alphanumeric() {
                        k += 1;
                    }
                    let w = String::from_utf8_lossy(&b[s..k]).into_owned();
                    if !STOP_WORDS.contains(&w.as_str()) {
                        out.push(Tok { s: w, dot: false, req: !unit });
                    }
                } else {
                    k += 1;
                }
            }
            out.push(Tok { s: alloc::format!("len:{n}"), dot: false, req: unit });
            i = e;
            continue;
        }
        if !c.is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let s = i;
        while i < b.len() && b[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let mut joined = false;
        if i - s == 1 && c.is_ascii_alphabetic() {
            let mut k = i;
            while k + 1 < b.len()
                && matches!(b[k], b'.' | b'&')
                && b[k + 1].is_ascii_alphabetic()
                && (k + 2 >= b.len() || !b[k + 2].is_ascii_alphanumeric())
            {
                k += 2;
                joined = true;
            }
            i = k;
        }
        while !joined
            && i + 1 < b.len()
            && b[i] == b'\''
            && b[i + 1].is_ascii_alphabetic()
            && b[s..i].iter().all(u8::is_ascii_alphabetic)
        {
            i += 1;
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            joined = true;
        }
        let dot = b.get(i) == Some(&b'.') && (i + 1 >= b.len() || !b[i + 1].is_ascii_alphanumeric());
        let w = if joined { b[s..i].to_vec() } else { singular(b[s..i].to_vec()) };
        let w = String::from_utf8_lossy(&w).into_owned();
        if dot {
            i += 1;
        }
        if joined || !STOP_WORDS.contains(&w.as_str()) {
            out.push(word_tok(w, dot || joined));
        }
    }
    out
}

/// One rewrite of the coverage normalization: these words (an abbreviation, or one spelling of its words) read as `to`.
/// A `dot` rule (the abbreviation is also a plain word) applies only to the abbreviation written with its period, and a
/// candidate then offers the written word too, so `alt` asks for `ALT.` and `alt.` asks for `ALTERNATE` or `ALT.`.
struct Rule {
    from: Vec<String>,
    to: Vec<String>,
    dot: bool,
}

/// The abbreviation table (built in, plus the style's `notes.abbreviations`) as word rewrites.
pub struct Words {
    rules: Vec<Rule>,
}

fn plain_words(s: &str) -> Vec<String> {
    raw_tokens(s).into_iter().map(|t| t.s).collect()
}

fn key_of(abbr: &str) -> String {
    abbr.trim_end_matches('.').to_ascii_lowercase()
}

impl Words {
    pub fn new(st: &Style) -> Words {
        let mut rules = Vec::new();
        let mut add = |abbr: &str, words: &[&str], dot: bool| {
            let Some(first) = words.first() else { return };
            let to = plain_words(first);
            if to.is_empty() {
                return;
            }
            rules.push(Rule { from: alloc::vec![key_of(abbr)], to: to.clone(), dot });
            for w in words {
                let from = plain_words(w);
                if !from.is_empty() && from != to {
                    rules.push(Rule { from, to: to.clone(), dot: false });
                }
            }
        };
        for a in &ABBREVIATIONS {
            add(a.abbr, a.words, a.dot);
        }
        for (a, ws) in &st.abbreviations {
            let ws: Vec<&str> = ws.iter().map(String::as_str).collect();
            add(a, &ws, false);
        }
        Words { rules }
    }

    fn rewrite(&self, toks: Vec<Tok>) -> Vec<Tok> {
        let mut out = Vec::with_capacity(toks.len());
        let mut i = 0;
        while i < toks.len() {
            let mut best: Option<&Rule> = None;
            for r in &self.rules {
                let n = r.from.len();
                if i + n > toks.len() || best.is_some_and(|b| b.from.len() >= n) {
                    continue;
                }
                let hit = toks[i..i + n].iter().zip(&r.from).all(|(t, f)| t.req && t.s == *f);
                if hit && (!r.dot || toks[i + n - 1].dot) {
                    best = Some(r);
                }
            }
            match best {
                Some(r) => {
                    out.extend(r.to.iter().map(|w| word_tok(w.clone(), false)));
                    if r.dot {
                        out.extend(toks[i..i + r.from.len()].iter().map(|t| Tok { req: false, ..t.clone() }));
                    }
                    i += r.from.len();
                }
                None => {
                    out.push(toks[i].clone());
                    i += 1;
                }
            }
        }
        out
    }

    /// The normalized words a `meta.requested` item asks for.
    pub fn wanted(&self, text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self.rewrite(raw_tokens(text)) {
            if t.req && !out.contains(&t.s) {
                out.push(t.s);
            }
        }
        out
    }

    /// The normalized words a candidate text offers, sorted (binary search).
    pub fn offered(&self, text: &str) -> Vec<String> {
        let mut out: Vec<String> = self.rewrite(raw_tokens(text)).into_iter().map(|t| t.s).collect();
        crate::sort::stable(&mut out, |a, b| a < b);
        out.dedup();
        out
    }
}

/// A length in 1/16 in as the coverage word a measured dimension offers.
pub fn length_word(inches: f64) -> String {
    alloc::format!("len:{}", sixteenths(inches.abs()))
}

/// Shown in a diagnostic: a normalized word the way a person reads it (`len:768` is `4'-0"`).
pub fn display_word(w: &str) -> String {
    match w.strip_prefix("len:").and_then(|n| n.parse::<i64>().ok()) {
        Some(n) => crate::units::fmt_ft_in(n as f64 / 16.0),
        None => String::from(w),
    }
}
