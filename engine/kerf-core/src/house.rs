//! House text rules shared by the note lint, the renderer's case transform and the `meta.requested` coverage (SPEC 25.3):
//! the abbreviation table, the tokens whose case is significant, the dashed-fraction test, and the normalized word list
//! coverage matches on.

use crate::style::{AbbrevRules, Style};
use alloc::string::String;
use alloc::vec::Vec;

/// A house abbreviation and the word(s) it stands for (the first entry is the canonical spelling).
pub struct Abbr {
    pub abbr: &'static str,
    pub words: &'static [&'static str],
    /// The bare letters are also an ordinary word: coverage reads them as the abbreviation only when written with the period.
    pub dot: bool,
    /// How often the reference sets write this form (0 = not counted), for spec/styles/ABBREVIATIONS.md.
    pub refs: u16,
}

const fn ab(abbr: &'static str, words: &'static [&'static str]) -> Abbr {
    Abbr { abbr, words, dot: false, refs: 0 }
}

const fn ab_dot(abbr: &'static str, words: &'static [&'static str]) -> Abbr {
    Abbr { abbr, words, dot: true, refs: 0 }
}

impl Abbr {
    const fn refs(self, refs: u16) -> Abbr {
        Abbr { refs, ..self }
    }
}

/// The approved forms of the built-in list `us-structural`: US structural practice as written in the MASS reference sets
/// (US Southwest firms, 2014-2024), periods on shortened words, dotted initials for phrases, none on material/product
/// acronyms (SPEC 25.3, 25.6). A note may end on any of these with its period; coverage reads each as its words.
pub static ABBREVIATIONS: [Abbr; 141] = [
    ab("A.B.", &["ANCHOR BOLT"]).refs(25),
    ab("ABV.", &["ABOVE"]).refs(27),
    ab("ADD'L", &["ADDITIONAL"]),
    ab("ADJ.", &["ADJACENT"]).refs(3),
    ab("A.F.F.", &["ABOVE FINISH FLOOR"]).refs(1),
    ab_dot("ALT.", &["ALTERNATE", "ALTERNATIVE"]).refs(14),
    ab("APPROX.", &["APPROXIMATE", "APPROXIMATELY"]).refs(6),
    ab_dot("ARCH.", &["ARCHITECT", "ARCHITECTURAL"]).refs(44),
    ab("BD.", &["BOARD"]).refs(16),
    ab("BLDG.", &["BUILDING"]).refs(9),
    ab("BLK.", &["BLOCK"]).refs(9),
    ab("BLKG.", &["BLOCKING"]).refs(34),
    ab("BM.", &["BEAM"]).refs(21),
    ab("B.N.", &["BOUNDARY NAILING", "BOUNDARY NAIL"]).refs(28),
    ab("B.O.", &["BOTTOM OF"]).refs(3),
    ab("B.O.F.", &["BOTTOM OF FOOTING"]),
    ab("BOT.", &["BOTTOM"]).refs(11),
    ab("BRG.", &["BEARING"]).refs(1),
    ab("BTWN.", &["BETWEEN"]).refs(2),
    ab("CALC.", &["CALCULATION", "CALCULATIONS"]),
    ab("C.I.P.", &["CAST IN PLACE"]).refs(1),
    ab("C.J.", &["CONTROL JOINT"]).refs(7),
    ab("CLG.", &["CEILING"]).refs(29),
    ab("CLR.", &["CLEAR"]).refs(60),
    ab("CMU", &["CONCRETE MASONRY UNIT"]).refs(98),
    ab_dot("COL.", &["COLUMN"]).refs(26),
    ab("CONC.", &["CONCRETE"]).refs(61),
    ab("CONN.", &["CONNECTION"]).refs(26),
    ab("CONST.", &["CONSTRUCTION"]),
    ab("CONT.", &["CONTINUOUS"]).refs(114),
    ab("CTR.", &["CENTER"]).refs(1),
    ab("DBL.", &["DOUBLE"]).refs(39),
    ab("DET.", &["DETAIL"]).refs(22),
    ab("D.F.", &["DOUGLAS FIR"]).refs(13),
    ab("DF-L", &["DOUGLAS FIR-LARCH"]),
    ab("DIA.", &["DIAMETER"]).refs(117),
    ab("DIAG.", &["DIAGONAL"]).refs(6),
    ab_dot("DIM.", &["DIMENSION"]).refs(9),
    ab("DN.", &["DOWN"]).refs(39),
    ab("DWG.", &["DRAWING"]).refs(2),
    ab("DWGS.", &["DRAWINGS"]).refs(20),
    ab("EA.", &["EACH"]).refs(122),
    ab("E.F.", &["EACH FACE"]),
    ab("E.J.", &["EXPANSION JOINT"]).refs(1),
    ab("EL.", &["ELEVATION"]).refs(2),
    ab("ELEC.", &["ELECTRICAL"]).refs(4),
    ab_dot("EMBED.", &["EMBEDMENT"]).refs(31),
    ab("E.N.", &["EDGE NAILING", "EDGE NAIL"]).refs(56),
    ab("ENGR.", &["ENGINEER"]).refs(1),
    ab("E.O.R.", &["ENGINEER OF RECORD"]).refs(4),
    ab("EQ.", &["EQUAL"]).refs(49),
    ab("E.W.", &["EACH WAY"]).refs(11),
    ab_dot("EXIST.", &["EXISTING"]).refs(1),
    ab_dot("EXP.", &["EXPANSION"]).refs(7),
    ab("EXT.", &["EXTERIOR"]).refs(35),
    ab("FDN.", &["FOUNDATION"]).refs(12),
    ab("F.F.", &["FINISH FLOOR"]).refs(38),
    ab_dot("FIN.", &["FINISH", "FINISHED"]).refs(9),
    ab("FLR.", &["FLOOR"]).refs(4),
    ab("F.N.", &["FIELD NAILING", "FIELD NAIL"]).refs(7),
    ab("FTG.", &["FOOTING"]).refs(17),
    ab("GA.", &["GAUGE", "GAGE"]).refs(37),
    ab("GALV.", &["GALVANIZED"]).refs(9),
    ab("GLB", &["GLUED LAMINATED BEAM", "GLULAM BEAM"]).refs(16),
    ab("GR.", &["GRADE"]).refs(1),
    ab("GYP.", &["GYPSUM"]).refs(87),
    ab("HD", &["HOLDOWN", "HOLD-DOWN"]).refs(12),
    ab("HDR.", &["HEADER"]).refs(15),
    ab("HGR.", &["HANGER"]),
    ab("HORIZ.", &["HORIZONTAL"]).refs(14),
    ab("HSS", &["HOLLOW STRUCTURAL SECTION"]).refs(19),
    ab("HT.", &["HEIGHT"]).refs(10),
    ab("ICF", &["INSULATED CONCRETE FORM"]).refs(1),
    ab_dot("I.D.", &["INSIDE DIAMETER"]).refs(1),
    ab("INSUL.", &["INSULATION"]).refs(6),
    ab_dot("INT.", &["INTERIOR"]).refs(17),
    ab("JST.", &["JOIST"]).refs(1),
    ab("JT.", &["JOINT"]).refs(1),
    ab("LG.", &["LONG"]),
    ab("L.L.H.", &["LONG LEG HORIZONTAL"]).refs(1),
    ab("L.L.V.", &["LONG LEG VERTICAL"]).refs(1),
    ab("LSL", &["LAMINATED STRAND LUMBER"]).refs(4),
    ab("LVL", &["LAMINATED VENEER LUMBER"]).refs(36),
    ab("MAS.", &["MASONRY"]).refs(4),
    ab("MAX.", &["MAXIMUM"]).refs(107),
    ab("M.B.", &["MACHINE BOLT"]),
    ab("MECH.", &["MECHANICAL"]).refs(16),
    ab("MFR.", &["MANUFACTURER"]).refs(29),
    ab("MIN.", &["MINIMUM"]).refs(456),
    ab("MISC.", &["MISCELLANEOUS"]).refs(5),
    ab("MTL.", &["METAL"]),
    ab("NOM.", &["NOMINAL"]),
    ab("N.S.", &["NON-SHRINK"]).refs(5),
    ab("N.T.S.", &["NOT TO SCALE"]).refs(25),
    ab("O.C.", &["ON CENTER"]).refs(537),
    ab("O.D.", &["OUTSIDE DIAMETER"]).refs(1),
    ab("OPNG.", &["OPENING"]).refs(1),
    ab("OPP.", &["OPPOSITE"]).refs(4),
    ab("OSB", &["ORIENTED STRAND BOARD"]).refs(28),
    ab("P.A.F.", &["POWER-ACTUATED FASTENER"]).refs(1),
    ab("PERP.", &["PERPENDICULAR"]).refs(10),
    ab("PL", &["PLATE"]).refs(13),
    ab("PLYWD.", &["PLYWOOD"]).refs(2),
    ab("PSL", &["PARALLEL STRAND LUMBER"]).refs(8),
    ab("P.T.", &["PRESSURE TREATED"]).refs(17),
    ab("REF.", &["REFERENCE"]).refs(14),
    ab("REINF.", &["REINFORCING", "REINFORCEMENT", "REINFORCED"]).refs(38),
    ab("REQ'D", &["REQUIRED"]).refs(23),
    ab_dot("S.A.D.", &["SEE ARCHITECTURAL DRAWINGS"]),
    ab("SCHED.", &["SCHEDULE"]).refs(37),
    ab_dot("SECT.", &["SECTION"]).refs(1),
    ab("SHT.", &["SHEET"]).refs(1),
    ab("SHTG.", &["SHEATHING"]).refs(56),
    ab_dot("SIM.", &["SIMILAR"]).refs(23),
    ab("S.O.G.", &["SLAB ON GRADE"]),
    ab_dot("SPA.", &["SPACES", "SPACING", "SPACED"]).refs(4),
    ab("SPEC.", &["SPECIFICATION"]).refs(4),
    ab("SPECS.", &["SPECIFICATIONS"]).refs(19),
    ab("SQ.", &["SQUARE"]).refs(54),
    ab("S.S.", &["STAINLESS STEEL"]).refs(1),
    ab_dot("STAG.", &["STAGGERED", "STAGGER"]).refs(2),
    ab("STD.", &["STANDARD"]).refs(12),
    ab_dot("STIFF.", &["STIFFENER"]),
    ab("STL.", &["STEEL"]).refs(3),
    ab("STRUCT.", &["STRUCTURAL"]).refs(8),
    ab("T&B", &["TOP AND BOTTOM"]).refs(5),
    ab("T&G", &["TONGUE AND GROOVE"]).refs(7),
    ab("THK.", &["THICK", "THICKNESS"]).refs(30),
    ab("THRU", &["THROUGH"]).refs(21),
    ab_dot("T.O.", &["TOP OF"]).refs(21),
    ab("T.O.C.", &["TOP OF CONCRETE"]).refs(1),
    ab("T.O.F.", &["TOP OF FOOTING"]),
    ab_dot("T.O.M.", &["TOP OF MASONRY"]).refs(1),
    ab("T.O.S.", &["TOP OF SLAB"]).refs(2),
    ab_dot("T.O.W.", &["TOP OF WALL"]).refs(3),
    ab("TYP.", &["TYPICAL"]).refs(234),
    ab("U.N.O.", &["UNLESS NOTED OTHERWISE"]).refs(92),
    ab("VERT.", &["VERTICAL"]).refs(26),
    ab("V.I.F.", &["VERIFY IN FIELD"]).refs(1),
    ab("WD.", &["WOOD"]).refs(4),
    ab("WWR", &["WELDED WIRE REINFORCEMENT"]),
];

/// A spelling in use that is not the approved form: W_ABBREV (rule `variants`) asks for `to`; coverage reads it as the words
/// of `to`.
pub struct Variant {
    pub from: &'static str,
    pub to: &'static str,
    /// The bare letters are an ordinary word (`PLY`): only the spelling with its period is the variant.
    pub dot: bool,
    /// Added to the W_ABBREV message ("" = nothing to add).
    pub why: &'static str,
    pub refs: u16,
}

const fn var(from: &'static str, to: &'static str) -> Variant {
    Variant { from, to, dot: false, why: "", refs: 0 }
}

impl Variant {
    const fn refs(self, refs: u16) -> Variant {
        Variant { refs, ..self }
    }
}

/// The variants of `us-structural`: each maps to the form the reference sets use most, or the one the MASS principal
/// engineer chose (`BOT.`, `P.T.`, `N.T.S.`, `PL`). Any spelling that differs from an approved form only in periods
/// (`TYP`, `OC`, `C.M.U.`) breaks a period rule without an entry; the listed ones of that kind (`UNO`, `NTS`, `EOR`,
/// `PL.`) are here for their counts and also break the period rule, not `variants`.
pub static VARIANTS: [Variant; 17] = [
    var("ADDL.", "ADD'L"),
    var("BLK'G", "BLKG.").refs(28),
    var("BOTT.", "BOT."),
    var("BTM.", "BOT.").refs(17),
    var("ELEV.", "EL.").refs(1),
    var("EOR", "E.O.R."),
    var("EXIST'G", "EXIST."),
    var("GRD.", "GR."),
    var("MFR'S", "MFR."),
    var("NTS", "N.T.S.").refs(43),
    var("PL.", "PL").refs(57),
    Variant { from: "PLY.", to: "PLYWD.", dot: true, why: "", refs: 4 },
    Variant {
        from: "PT",
        to: "P.T.",
        dot: false,
        why: "; PT also reads as post-tensioned, so pressure treated is P.T. (for post-tensioned, acknowledge it)",
        refs: 2,
    },
    var("REQD", "REQ'D"),
    var("REQD.", "REQ'D").refs(1),
    var("STRUC.", "STRUCT.").refs(1),
    var("UNO", "U.N.O.").refs(68),
];

/// Abbreviations that end a note with their period although they stand for no word coverage should expand.
pub const PERIOD_ONLY: [&str; 2] = ["NO", "NOS"];

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

// ---- the style's abbreviation table and W_ABBREV (SPEC 25.6) ----------------------------------------------------

/// How an approved abbreviation is written, which decides the rule a deviation from it breaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// A shortened word with its period: `TYP.`, `CONC.`.
    Shortened,
    /// Dotted initials of a phrase: `O.C.`, `U.N.O.`.
    Initials,
    /// No periods: material and product acronyms and symbols (`CMU`, `LVL`, `T&B`, `THRU`).
    Bare,
    /// An apostrophe contraction: `REQ'D`, `ADD'L`.
    Contraction,
}

pub fn form_of(abbr: &str) -> Form {
    if abbr.contains('\'') {
        Form::Contraction
    } else if abbr.trim_end_matches('.').contains('.') {
        Form::Initials
    } else if abbr.ends_with('.') {
        Form::Shortened
    } else {
        Form::Bare
    }
}

/// The W_ABBREV rules: the four form rules a style can switch off (`notes.abbreviations.rules`) and the style's
/// `always` list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    PeriodOnShortened,
    DottedInitials,
    BareAcronyms,
    Variants,
    Always,
}

impl Rule {
    pub fn name(self) -> &'static str {
        match self {
            Rule::PeriodOnShortened => "period_on_shortened",
            Rule::DottedInitials => "dotted_initials",
            Rule::BareAcronyms => "bare_acronyms",
            Rule::Variants => "variants",
            Rule::Always => "always",
        }
    }

    pub fn what(self) -> &'static str {
        match self {
            Rule::PeriodOnShortened => "a shortened word takes a period",
            Rule::DottedInitials => "a phrase takes dotted initials",
            Rule::BareAcronyms => "a material or product acronym takes no periods",
            Rule::Variants => "a variant spelling takes the approved form",
            Rule::Always => "the company style always abbreviates these words",
        }
    }
}

/// One spelling of the table: an approved form (`to` empty) or a variant of one.
pub struct Entry<'s> {
    pub abbr: &'s str,
    pub words: Vec<&'s str>,
    /// The bare letters are an ordinary word (`SIM`, `TO`): they count as this entry only when written with its periods.
    pub dot: bool,
    /// A variant: the approved form it is written as.
    pub to: &'s str,
    pub why: &'s str,
    pub refs: u16,
}

/// The abbreviations of a style: the built-in list (`extends`), less `remove`, plus `approved` and `variants`.
pub struct Table<'s> {
    pub entries: Vec<Entry<'s>>,
    pub always: Vec<&'s str>,
    pub rules: AbbrevRules,
}

/// One abbreviation of a text not written as the style wants (W_ABBREV): the byte span, the rule, and the approved form.
pub struct Hit<'s> {
    pub start: usize,
    pub end: usize,
    pub rule: Rule,
    pub approved: &'s str,
    pub meaning: &'s str,
    pub why: &'s str,
}

/// The rule a spelling of entry `e` that is not its approved form breaks: a period rule when it differs from the approved
/// form only in periods (`TYP`, `UNO`, `C.M.U.`, `PL.`), else `variants` (`BOTT.`, `REQD`).
pub fn rule_for(e: &Entry) -> Rule {
    let target = if e.to.is_empty() { e.abbr } else { e.to };
    if !e.to.is_empty() && !same_key(e.abbr, target) {
        return Rule::Variants;
    }
    match form_of(target) {
        Form::Shortened => Rule::PeriodOnShortened,
        Form::Initials => Rule::DottedInitials,
        Form::Bare => Rule::BareAcronyms,
        Form::Contraction => Rule::Variants,
    }
}

/// `a` and `b` spell the same letters, ignoring periods and case.
fn same_key(a: &str, b: &str) -> bool {
    let mut x = a.bytes().filter(|&c| c != b'.');
    let mut y = b.bytes().filter(|&c| c != b'.');
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if p.eq_ignore_ascii_case(&q) => {}
            _ => return false,
        }
    }
}

impl<'s> Table<'s> {
    pub fn new(st: &'s Style) -> Table<'s> {
        let cfg = &st.abbrev;
        let mut entries: Vec<Entry<'s>> = Vec::new();
        if cfg.builtin {
            entries.extend(ABBREVIATIONS.iter().map(|a| Entry {
                abbr: a.abbr,
                words: a.words.to_vec(),
                dot: a.dot,
                to: "",
                why: "",
                refs: a.refs,
            }));
            entries.extend(VARIANTS.iter().map(|v| Entry {
                abbr: v.from,
                words: Vec::new(),
                dot: v.dot,
                to: v.to,
                why: v.why,
                refs: v.refs,
            }));
        }
        entries.retain(|e| !cfg.remove.iter().any(|r| r.eq_ignore_ascii_case(e.abbr)));
        for (a, ws) in &cfg.approved {
            let dot = entries.iter().any(|e| e.dot && e.abbr.eq_ignore_ascii_case(a));
            entries.retain(|e| !e.abbr.eq_ignore_ascii_case(a));
            entries.push(Entry {
                abbr: a,
                words: ws.iter().map(String::as_str).collect(),
                dot,
                to: "",
                why: "",
                refs: 0,
            });
        }
        for (f, t) in &cfg.variants {
            entries.retain(|e| !e.abbr.eq_ignore_ascii_case(f));
            entries.push(Entry { abbr: f, words: Vec::new(), dot: false, to: t, why: "", refs: 0 });
        }
        for i in 0..entries.len() {
            if entries[i].to.is_empty() {
                continue;
            }
            if let Some(j) = entries.iter().position(|e| e.to.is_empty() && e.abbr.eq_ignore_ascii_case(entries[i].to))
            {
                entries[i].words = entries[j].words.clone();
                entries[i].to = entries[j].abbr;
            }
        }
        Table { entries, always: cfg.always.iter().map(String::as_str).collect(), rules: cfg.rules }
    }

    /// The approved entry spelled `a` (case-insensitive).
    pub fn approved(&self, a: &str) -> Option<&Entry<'s>> {
        self.entries.iter().find(|e| e.to.is_empty() && e.abbr.eq_ignore_ascii_case(a))
    }

    /// Style mistakes the table reveals: a variant of no approved form, an `always` entry that is not approved.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in self.entries.iter().filter(|e| !e.to.is_empty() && e.words.is_empty()) {
            out.push(alloc::format!(
                "style key 'notes.abbreviations': the variant \"{}\" maps to \"{}\", which is not an approved abbreviation: add \"{}\" to notes.abbreviations.approved, or add \"{}\" to notes.abbreviations.remove",
                e.abbr, e.to, e.to, e.abbr
            ));
        }
        for a in self.always.iter().filter(|a| self.approved(a).is_none()) {
            let near = self
                .entries
                .iter()
                .find(|e| e.to.is_empty() && same_key(e.abbr, a))
                .map(|e| alloc::format!("; did you mean \"{}\"?", e.abbr))
                .unwrap_or_default();
            out.push(alloc::format!(
                "style key 'notes.abbreviations.always': \"{a}\" is not an approved abbreviation (built in, or in notes.abbreviations.approved){near}"
            ));
        }
        out
    }

    /// `base` (a note's last word without its period) is an abbreviation of the table, also one written in the wrong form
    /// (`OC.`: W_ABBREV reports that), `NO` or `NOS`; the bare letters of an ordinary-word entry (`TO.`) are not.
    pub fn is_period_abbr(&self, base: &[u8]) -> bool {
        let same = |a: &str| a.trim_end_matches('.').as_bytes().eq_ignore_ascii_case(base);
        let letters = String::from_utf8_lossy(base);
        self.entries.iter().any(|e| same(e.abbr) || (!e.dot && same_key(e.abbr, &letters)))
            || PERIOD_ONLY.iter().any(|a| same(a))
    }

    fn known(&self, tok: &str) -> bool {
        self.entries.iter().any(|e| same_key(e.abbr, tok))
    }

    /// The rule `tok` (one token as written, `’` read as `'`) breaks, with the approved entry it should be written as.
    fn check(&self, tok: &str) -> Option<(Rule, &Entry<'s>)> {
        if let Some(e) = self.entries.iter().find(|e| e.abbr.eq_ignore_ascii_case(tok)) {
            return if e.to.is_empty() { None } else { Some((rule_for(e), e)) };
        }
        let has_dot = tok.contains('.');
        let inner = tok.trim_end_matches('.').contains('.');
        let mut best: Option<&Entry<'s>> = None;
        for e in self.entries.iter().filter(|e| same_key(e.abbr, tok)) {
            let written_as_abbr = !e.dot || if form_of(e.abbr) == Form::Initials { inner } else { has_dot };
            if written_as_abbr && best.is_none_or(|b| !b.to.is_empty() && e.to.is_empty()) {
                best = Some(e);
            }
        }
        let e = best?;
        if !e.to.is_empty() {
            self.approved(e.to)?;
        }
        Some((rule_for(e), e))
    }

    fn hit(&self, start: usize, end: usize, rule: Rule, e: &Entry<'s>) -> Option<Hit<'s>> {
        let a = if e.to.is_empty() { e } else { self.approved(e.to)? };
        Some(Hit { start, end, rule, approved: a.abbr, meaning: a.words.first().copied().unwrap_or(""), why: e.why })
    }
}

const CURLY_APOS: [u8; 3] = [0xE2, 0x80, 0x99];
const CURLY_OPEN1: [u8; 3] = [0xE2, 0x80, 0x98];

/// Length of the token character at `i` (ASCII letter, digit, `.`, `'`, `&`, `-`, or a curly apostrophe), if any.
fn abbr_char(b: &[u8], i: usize) -> Option<usize> {
    let c = b[i];
    if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'\'' | b'&' | b'-') {
        return Some(1);
    }
    let t = b.get(i..i + 3)?;
    (t == CURLY_APOS || t == CURLY_OPEN1).then_some(3)
}

/// Bytes inside double quotes (`"..."`, `“...”`); a `"` or `”` right after a digit is an inch mark, not a quote.
fn quoted_bytes(b: &[u8]) -> Vec<bool> {
    let mut m = alloc::vec![false; b.len()];
    let mut inq = false;
    let mut i = 0;
    while i < b.len() {
        let (len, kind) = if b[i] == b'"' {
            (1, 0)
        } else if b[i] == 0xE2 && b.get(i + 1) == Some(&0x80) && matches!(b.get(i + 2), Some(0x9C | 0x9D)) {
            (3, if b[i + 2] == 0x9C { 1 } else { 2 })
        } else {
            m[i] = inq;
            i += 1;
            continue;
        };
        let inch = kind != 1 && i > 0 && b[i - 1].is_ascii_digit();
        if !inch {
            inq = match kind {
                1 => true,
                2 => false,
                _ => !inq,
            };
        }
        i += len;
    }
    m
}

/// `phrase` at `i` of `b` as whole words, case-insensitive.
fn word_at(b: &[u8], i: usize, phrase: &str) -> bool {
    let p = phrase.as_bytes();
    i + p.len() <= b.len()
        && b[i..i + p.len()].eq_ignore_ascii_case(p)
        && !(i > 0 && b[i - 1].is_ascii_alphanumeric())
        && !(i + p.len() < b.len() && b[i + p.len()].is_ascii_alphanumeric())
}

/// The abbreviations of `text` the style wants written otherwise (W_ABBREV, SPEC 25.6), in text order. Tokens with a
/// digit (`HU212-2`, `16d`, `H2.5A`), case-significant tokens (`f'c`) and text inside double quotes are never read.
pub fn abbrev_hits<'s>(text: &str, st: &Style, t: &Table<'s>) -> Vec<Hit<'s>> {
    let b = text.as_bytes();
    let quoted = quoted_bytes(b);
    let on = |r: Rule| t.rules.get(r.name());
    let mut out: Vec<Hit<'s>> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if abbr_char(b, i).is_none() {
            i += 1;
            continue;
        }
        let mut s = i;
        while i < b.len()
            && let Some(l) = abbr_char(b, i)
        {
            i += l;
        }
        let mut e = i;
        loop {
            if s < e && matches!(b[s], b'.' | b'\'' | b'-' | b'&') {
                s += 1;
            } else if b[s..e].starts_with(&CURLY_APOS) || b[s..e].starts_with(&CURLY_OPEN1) {
                s += 3;
            } else {
                break;
            }
        }
        loop {
            if e > s && matches!(b[e - 1], b'\'' | b'-' | b'&') {
                e -= 1;
            } else if b[s..e].ends_with(&CURLY_APOS) {
                e -= 3;
            } else {
                break;
            }
        }
        let span = &b[s..e];
        if span.is_empty()
            || quoted[s..e].iter().any(|&q| q)
            || span.iter().any(u8::is_ascii_digit)
            || !span.iter().any(u8::is_ascii_alphabetic)
            || keep_case_token(span, st)
        {
            continue;
        }
        let tok = tok_core(span);
        let parts: Vec<(usize, usize)> = if tok.contains('-') && !t.known(&tok) {
            let mut v = Vec::new();
            let mut a = s;
            for (k, &c) in span.iter().enumerate() {
                if c == b'-' {
                    v.push((a, s + k));
                    a = s + k + 1;
                }
            }
            v.push((a, e));
            v
        } else {
            alloc::vec![(s, e)]
        };
        for (ps, pe) in parts {
            if ps >= pe {
                continue;
            }
            let pt = tok_core(&b[ps..pe]);
            if let Some((rule, ent)) = t.check(&pt)
                && on(rule)
                && let Some(h) = t.hit(ps, pe, rule, ent)
            {
                out.push(h);
            }
        }
    }
    for a in &t.always {
        let Some(ent) = t.approved(a) else { continue };
        let mut words: Vec<&str> = ent.words.clone();
        crate::sort::stable(&mut words, |x, y| x.len() > y.len());
        for w in words {
            let mut k = 0;
            while k < b.len() {
                if !word_at(b, k, w) || quoted[k] || out.iter().any(|h| k < h.end && k + w.len() > h.start) {
                    k += 1;
                    continue;
                }
                let mut end = k + w.len();
                if ent.abbr.ends_with('.') && b.get(end) == Some(&b'.') {
                    end += 1;
                }
                if let Some(h) = t.hit(k, end, Rule::Always, ent) {
                    out.push(h);
                }
                k = end;
            }
        }
    }
    crate::sort::stable(&mut out, |x, y| x.start < y.start);
    out
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn count(n: u16) -> String {
    if n == 0 { String::new() } else { alloc::format!("{n}") }
}

/// The table of `st` as Markdown (`kerf schema abbreviations`, spec/styles/ABBREVIATIONS.md): the rules in force, then one
/// row per approved form with its meaning, its form and the other spellings W_ABBREV flags (the listed variants, with
/// their counts in the reference sets, and the period-only deviation).
pub fn abbreviations_markdown(st: &Style) -> String {
    use alloc::format;
    let t = Table::new(st);
    let cfg = &st.abbrev;
    let r = &t.rules;
    let mut out = format!(
        "List: {}{}{}. Rules (notes.abbreviations.rules): period_on_shortened {}, dotted_initials {}, bare_acronyms {}, variants {}. Always abbreviated (notes.abbreviations.always): {}.\n\n",
        if cfg.builtin { "us-structural (built in)" } else { "none (extends: \"none\")" },
        if cfg.remove.is_empty() { String::new() } else { format!(", less {}", cfg.remove.join(" ")) },
        if cfg.approved.is_empty() && cfg.variants.is_empty() {
            String::new()
        } else {
            format!(", plus {} approved and {} variants from the style", cfg.approved.len(), cfg.variants.len())
        },
        on_off(r.period_on_shortened),
        on_off(r.dotted_initials),
        on_off(r.bare_acronyms),
        on_off(r.variants),
        if t.always.is_empty() { String::from("none") } else { t.always.join(" ") },
    );
    out.push_str("Refs: how often the 32 reference sets write that spelling (blank: not counted).\n\n| Approved | Refs | Meaning | Form | Other spellings W_ABBREV flags (refs, rule) |\n|---|--:|---|---|---|\n");
    let mut rows: Vec<&Entry> = t.entries.iter().filter(|e| e.to.is_empty()).collect();
    crate::sort::stable(&mut rows, |a, b| {
        let k = |e: &Entry| -> String { e.abbr.chars().filter(|&c| c != '.').collect::<String>().to_ascii_uppercase() };
        k(a) < k(b)
    });
    for e in rows {
        let bare: String = e.abbr.chars().filter(|&c| c != '.').collect();
        let (form, generic) = match form_of(e.abbr) {
            Form::Shortened => ("shortened word", format!("`{bare}` (period_on_shortened)")),
            Form::Initials => ("dotted initials", format!("`{bare}` (dotted_initials)")),
            Form::Bare if e.abbr.contains(['&', '-']) => ("symbol", String::new()),
            Form::Bare => {
                let dotted: String = e.abbr.chars().flat_map(|c| [c, '.']).collect();
                ("acronym", format!("`{dotted}` (bare_acronyms)"))
            }
            Form::Contraction => ("contraction", String::new()),
        };
        let mut flagged: Vec<String> = Vec::new();
        for v in t.entries.iter().filter(|v| !v.to.is_empty() && v.to == e.abbr) {
            let n = count(v.refs);
            flagged.push(format!("`{}` ({}{}{})", v.abbr, n, if n.is_empty() { "" } else { ", " }, rule_for(v).name()));
        }
        if e.dot {
            flagged.push(format!("`{bare}` is an ordinary word: never flagged"));
        } else if !generic.is_empty()
            && !t.entries.iter().any(|v| !v.to.is_empty() && v.to == e.abbr && same_key(v.abbr, &bare))
        {
            flagged.push(generic);
        }
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            e.abbr,
            count(e.refs),
            e.words.join(" / "),
            form,
            flagged.join(", ")
        ));
    }
    out
}

/// `text` with every hit replaced by its approved form and nothing else changed.
pub fn apply_hits(text: &str, hits: &[Hit]) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut at = 0;
    for h in hits {
        if h.start < at || !text.is_char_boundary(h.start) || !text.is_char_boundary(h.end) {
            continue;
        }
        out.push_str(&text[at..h.start]);
        out.push_str(h.approved);
        at = h.end;
    }
    out.push_str(&text[at..]);
    out
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
            if let Some(e3) = unit_word(e2, &["inches", "inch", "in"]) {
                return Some((e3, sixteenths(feet * 12.0 + inch), true));
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
struct Rewrite {
    from: Vec<String>,
    to: Vec<String>,
    dot: bool,
}

/// The abbreviation table (built in, plus the style's `notes.abbreviations`) as word rewrites.
pub struct Words {
    rules: Vec<Rewrite>,
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
            rules.push(Rewrite { from: alloc::vec![key_of(abbr)], to: to.clone(), dot });
            for w in words {
                let from = plain_words(w);
                if !from.is_empty() && from != to {
                    rules.push(Rewrite { from, to: to.clone(), dot: false });
                }
            }
        };
        let table = Table::new(st);
        for e in &table.entries {
            if e.to.is_empty() {
                add(e.abbr, &e.words, e.dot);
            } else if let Some(first) = e.words.first() {
                add(e.abbr, &[first], e.dot);
            }
            if !e.dot && form_of(e.abbr) == Form::Initials {
                let bare: String = e.abbr.chars().filter(|&c| c != '.').collect();
                if let Some(first) = e.words.first() {
                    add(&bare, &[first], false);
                }
            }
        }
        Words { rules }
    }

    fn rewrite(&self, toks: Vec<Tok>) -> Vec<Tok> {
        let mut out = Vec::with_capacity(toks.len());
        let mut i = 0;
        while i < toks.len() {
            let mut best: Option<&Rewrite> = None;
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

    /// The normalized words a `meta.requested` item asks for, each with its word forms (`forms`): the word is found
    /// when a candidate offers any of them.
    pub fn wanted(&self, text: &str) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        for t in self.rewrite(raw_tokens(text)) {
            if t.req && !out.iter().any(|w| w[0] == t.s) {
                let mut w = alloc::vec![t.s.clone()];
                w.extend(forms(&t.s));
                out.push(w);
            }
        }
        out
    }

    /// The normalized words a candidate text offers with their word forms, sorted (binary search).
    pub fn offered(&self, text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self.rewrite(raw_tokens(text)) {
            out.extend(forms(&t.s));
            out.push(t.s);
        }
        crate::sort::stable(&mut out, |a, b| a < b);
        out.dedup();
        out
    }
}

/// `-ing` and `-ed` words whose base word names something else on a structural drawing (`BLOCKING` is not `BLOCK`,
/// `TOPPING` is not `TOP`), so they keep only their own form.
const UNSTEMMED: [&str; 22] = [
    "bedding", "blocking", "building", "casing", "ceiling", "coping", "covering", "during", "engineered", "fitting",
    "footing", "headed", "lagging", "landing", "netting", "opening", "planed", "setting", "siding", "spacing", "spring",
    "topping",
];

/// The base forms of an `-ing` / `-ed` word, used besides the word itself: `nailed`, `nailing` -> `nail`; `bracing` ->
/// `brac`, `brace`; `lapped` -> `lap`; `specified` -> `specify`. The stem keeps at least 4 letters and a vowel, a doubled
/// final consonant (not l, s, z) is undoubled, `-eed` words and `UNSTEMMED` stay as written.
fn forms(w: &str) -> Vec<String> {
    let b = w.as_bytes();
    if !b.iter().all(u8::is_ascii_lowercase) || UNSTEMMED.contains(&w) {
        return Vec::new();
    }
    if w.len() >= 6 && w.ends_with("ied") {
        return alloc::vec![alloc::format!("{}y", &w[..w.len() - 3])];
    }
    let stem = if let Some(s) = w.strip_suffix("ing") {
        s
    } else if let Some(s) = w.strip_suffix("ed").filter(|_| !w.ends_with("eed")) {
        s
    } else {
        return Vec::new();
    };
    let sb = stem.as_bytes();
    if sb.len() < 4 || !sb.iter().any(|c| b"aeiouy".contains(c)) {
        return Vec::new();
    }
    let last = sb[sb.len() - 1];
    if last == sb[sb.len() - 2] && !b"aeiouylsz".contains(&last) {
        return alloc::vec![String::from(&stem[..stem.len() - 1])];
    }
    if last == b'e' {
        return alloc::vec![String::from(stem)];
    }
    alloc::vec![String::from(stem), alloc::format!("{stem}e")]
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
