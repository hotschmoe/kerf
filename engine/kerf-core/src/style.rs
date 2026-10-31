//! Kerf Style (`*.kerfstyle.json`, SPEC 7): pens, materials, hatch patterns, text/notes/dims
//! settings, layers, sheet. The default style is embedded; a user style is merged over it
//! (RFC 7396) so partial styles work.

use crate::json::{self, Value};
use crate::num;
pub use crate::pen::{LayerKey, Pen};
use crate::view::NotesStyle;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const DEFAULT_SRC: &[u8] = include_bytes!("../../../spec/styles/kerf-standard.kerfstyle.json");

/// JSON whitespace between tokens dropped, strings and escapes kept (`out: None` only counts).
const fn minify_into(src: &[u8], mut out: Option<&mut [u8]>) -> usize {
    let (mut i, mut n, mut in_str, mut esc) = (0, 0, false, false);
    while i < src.len() {
        let c = src[i];
        i += 1;
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
            continue;
        } else if c == b'"' {
            in_str = true;
        }
        if let Some(o) = &mut out {
            o[n] = c;
        }
        n += 1;
    }
    n
}

const DEFAULT_LEN: usize = minify_into(DEFAULT_SRC, None);
const DEFAULT_MIN: [u8; DEFAULT_LEN] = {
    let mut buf = [0u8; DEFAULT_LEN];
    minify_into(DEFAULT_SRC, Some(&mut buf));
    buf
};

/// The embedded default style (spec/styles/kerf-standard.kerfstyle.json), minified at compile time (12.1 -> 6.6 KB).
pub const DEFAULT_JSON: &[u8] = &DEFAULT_MIN;

#[derive(Clone, Debug)]
pub struct PenDef {
    pub name: String,
    pub width_mm: f64,
    pub dash_mm: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct HatchSpec {
    pub pattern: String,
    pub scale: f64,
    pub angle: f64,
}

/// Wood grain on lumber cut lengthwise (SPEC 19): a straight-line pattern (also the DXF fallback) made wavy by the engine.
#[derive(Clone, Debug)]
pub struct GrainSpec {
    pub pattern: String,
    pub scale: f64,
    /// Wave amplitude and wavelength, paper inches.
    pub amplitude: f64,
    pub wavelength: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CutMark {
    None,
    X,
    Diagonal,
}

/// The tag drawn at the leader end of a keyed note.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyTag {
    Plain,
    Circle,
    Hex,
}

/// What a material is, for the rules the engine applies by kind. A style sets it with `"role": "steel"` on a
/// material; without the key the role follows from the material name (`default_role`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Generic,
    /// Loose fill (earth, gravel, sand, compacted fill): not part of the view's auto-fit, never break-lined, drawn only in cutaway iso.
    Soil,
    /// Wood that W_UNTREATED_CONTACT checks against masonry.
    Wood,
    /// Concrete, CMU and mortar.
    Masonry,
    /// Grout: masonry that is not split into units in 3D.
    Grout,
    /// Structural steel and hardware: steel pen, thin sections are thickened.
    Steel,
    /// Reinforcing bar: drawn along its centerline when it is a path bar.
    Rebar,
    /// Thin sheet metal (aluminum, flashing membrane): thin sections render as a solid fill plus outline.
    SheetMetal,
    /// Vapor retarder: its dashed line keeps a minimum gap from the host edge it follows.
    VaporRetarder,
    /// Negative space (joint notches): not drawn as a body, not in 3D.
    Void,
}

impl Role {
    pub const ALL: [Role; 10] = [
        Role::Generic,
        Role::Soil,
        Role::Wood,
        Role::Masonry,
        Role::Grout,
        Role::Steel,
        Role::Rebar,
        Role::SheetMetal,
        Role::VaporRetarder,
        Role::Void,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::Generic => "generic",
            Role::Soil => "soil",
            Role::Wood => "wood",
            Role::Masonry => "masonry",
            Role::Grout => "grout",
            Role::Steel => "steel",
            Role::Rebar => "rebar",
            Role::SheetMetal => "sheet_metal",
            Role::VaporRetarder => "vapor_retarder",
            Role::Void => "void",
        }
    }

    pub fn from_name(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == s)
    }

    pub fn is_metal(self) -> bool {
        matches!(self, Role::Steel | Role::SheetMetal)
    }

    pub fn is_masonry(self) -> bool {
        matches!(self, Role::Masonry | Role::Grout)
    }
}

/// The role a material name has when the style does not say (the embedded style's materials).
pub fn default_role(name: &str) -> Role {
    match name {
        "earth" | "gravel" | "sand" | "compacted_fill" => Role::Soil,
        "wood" | "wood_engineered" | "wood_board" => Role::Wood,
        "concrete" | "cmu" | "mortar" => Role::Masonry,
        "grout" => Role::Grout,
        "steel" => Role::Steel,
        "rebar" => Role::Rebar,
        "aluminum" | "flashing_membrane" => Role::SheetMetal,
        "vapor_retarder" => Role::VaporRetarder,
        "void" => Role::Void,
        _ => Role::Generic,
    }
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub hatch: Vec<HatchSpec>,
    pub grain: Option<GrainSpec>,
    pub cut_mark: CutMark,
    pub fill: bool,
    pub batt: bool,
    pub pen: Option<Pen>,
    pub role: Role,
    pub color3d: String,
    pub layer: Option<String>,
}

impl Material {
    fn new(name: &str) -> Material {
        Material {
            name: name.to_string(),
            hatch: Vec::new(),
            grain: None,
            cut_mark: CutMark::None,
            fill: false,
            batt: false,
            pen: None,
            role: default_role(name),
            color3d: "#A0A0A0".to_string(),
            layer: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Family {
    pub angle: f64,
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub dashes: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub name: String,
    pub families: Vec<Family>,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub key: String,
    pub name: String,
    pub lineweight_mm: f64,
    pub linetype: String,
}

#[derive(Clone, Debug)]
pub struct Style {
    pub pens: Vec<PenDef>,
    pub materials: Vec<Material>,
    pub patterns: Vec<Pattern>,
    pub layers: Vec<Layer>,

    pub text_height_in: f64,
    pub title_height_in: f64,
    pub label_height_in: f64,
    pub text_case_upper: bool,
    /// `text.keep_case`: tokens printed as written under the upper-case transform, on top of the house list (SPEC 25.3).
    pub keep_case: Vec<String>,
    pub line_spacing: f64,
    pub dxf_style: String,
    pub dxf_font: String,
    /// `text.plan_title` / `text.elevation_title`: the title of a plan / elevation view, `{title}` standing for the view's
    /// own title (SPEC 26.3). Not applied when the title already names the kind (PLAN, ELEVATION).
    pub plan_title: String,
    pub elevation_title: String,

    /// `notes.mode`: the notes style of views without `notes_style` ("keynote" is the old name of keyed).
    pub notes_mode: NotesStyle,
    pub wrap_chars: f64,
    pub gutter_in: f64,
    pub shoulder_in: f64,
    pub note_gap_in: f64,
    pub arrow: String,
    pub arrow_len_in: f64,
    pub arrow_width_in: f64,
    /// Keyed-note tag at the leader end: plain (the bare number), circle or hex.
    pub key_tag: KeyTag,
    /// Heading of the keyed notes list.
    pub list_title: String,
    /// `notes.abbreviations`: house abbreviations the style adds (abbreviation, the words it stands for), SPEC 25.3.
    pub abbreviations: Vec<(String, Vec<String>)>,

    pub dim_terminator: String,
    pub tick_len_in: f64,
    pub ext_gap_in: f64,
    pub ext_over_in: f64,
    pub dim_text_gap_in: f64,
    pub dim_precision: f64,

    pub cite_format: String,
    pub cite_flag_unverified: bool,
    pub cite_flag: String,
    pub cite_footnote: String,

    pub sheet_w_in: f64,
    pub sheet_h_in: f64,
    pub margin_in: f64,
    pub title_block: String,
    pub title_block_h_in: f64,
    pub firm: String,
    pub project: String,

    pub break_zig_in: f64,
    pub break_period_in: f64,
    pub break_overshoot_in: f64,

    pub color_cut_cap: String,
    pub color_edge: String,
    pub color_background: String,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            pens: Vec::new(),
            materials: Vec::new(),
            patterns: Vec::new(),
            layers: Vec::new(),
            text_height_in: 0.09375,
            title_height_in: 0.15625,
            label_height_in: 0.078125,
            text_case_upper: true,
            keep_case: Vec::new(),
            line_spacing: 1.6,
            dxf_style: "KERF".to_string(),
            dxf_font: "romans.shx".to_string(),
            plan_title: "PLAN - {title}".to_string(),
            elevation_title: "{title}".to_string(),
            notes_mode: NotesStyle::Leader,
            wrap_chars: 28.0,
            gutter_in: 0.375,
            shoulder_in: 0.125,
            note_gap_in: 0.0625,
            arrow: "closed_filled".to_string(),
            arrow_len_in: 0.09375,
            arrow_width_in: 0.03125,
            key_tag: KeyTag::Plain,
            list_title: "DETAIL NOTES".to_string(),
            abbreviations: Vec::new(),
            dim_terminator: "tick".to_string(),
            tick_len_in: 0.0625,
            ext_gap_in: 0.0625,
            ext_over_in: 0.0625,
            dim_text_gap_in: 0.046875,
            dim_precision: 16.0,
            cite_format: " ({code} {section})".to_string(),
            cite_flag_unverified: true,
            cite_flag: "*".to_string(),
            cite_footnote: "* CODE REFERENCE NOT VERIFIED BY DESIGNER".to_string(),
            sheet_w_in: 11.0,
            sheet_h_in: 8.5,
            margin_in: 0.375,
            title_block: "kerf-strip".to_string(),
            title_block_h_in: 0.75,
            firm: String::new(),
            project: String::new(),
            break_zig_in: 0.125,
            break_period_in: 0.5,
            break_overshoot_in: 0.0625,
            color_cut_cap: "#E9D9A6".to_string(),
            color_edge: "#1A1A1A".to_string(),
            color_background: "#F2EFE6".to_string(),
        }
    }
}

impl Style {
    /// Characters per note line (validated to 4..200 at load; clamped again so a hand-built Style cannot reach a bad cast).
    pub fn wrap_cols(&self) -> usize {
        num::to_int_clamped::<usize>(self.wrap_chars, 4, 200)
    }

    pub fn pen(&self, which: Pen) -> Option<&PenDef> {
        self.pens.iter().find(|p| p.name == which.name())
    }

    pub fn pen_width_mm(&self, which: Pen) -> f64 {
        self.pen(which).map_or(0.25, |p| p.width_mm)
    }

    /// The role of a material by name: the style's `role` when it defines the material, else the name's default.
    pub fn role_of(&self, name: &str) -> Role {
        self.material(name).map_or_else(|| default_role(name), |m| m.role)
    }

    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name == name)
    }

    pub fn pattern(&self, name: &str) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.name == name)
    }

    pub fn layer_by_key(&self, key: LayerKey) -> Option<&Layer> {
        self.layers.iter().find(|l| l.key == key.key())
    }

    /// Name of a layer in this style ("0" when the style does not define it).
    pub fn layer_name(&self, key: LayerKey) -> &str {
        self.layer_by_key(key).map_or("0", |l| &l.name)
    }

    /// Layer name for a pen (`Pen::layer`).
    pub fn layer_for_pen(&self, which: Pen) -> &str {
        let key = which.layer();
        if let Some(l) = self.layer_by_key(key) {
            return &l.name;
        }
        if key == LayerKey::Rebar
            && let Some(l) = self.layer_by_key(LayerKey::Steel)
        {
            return &l.name;
        }
        "0"
    }
}

fn num_or(v: Option<&Value>, d: f64) -> f64 {
    v.and_then(Value::num).unwrap_or(d)
}

fn str_or(v: Option<&Value>, d: &str) -> String {
    v.and_then(Value::str).unwrap_or(d).to_string()
}

#[derive(Debug)]
pub enum StyleError {
    /// A message naming every offending key and the accepted range (empty when the style is structurally unusable).
    BadStyle(String),
}

/// The embedded default style as a JSON tree, parsed on first use and kept for the life of the process: it is a
/// constant, so sharing it changes no output (every call used to parse the 12 KB text again).
fn default_tree() -> Option<&'static Value> {
    use core::sync::atomic::{AtomicPtr, Ordering};
    static TREE: AtomicPtr<Value> = AtomicPtr::new(core::ptr::null_mut());
    let p = TREE.load(Ordering::Acquire);
    if !p.is_null() {
        // SAFETY: a non-null TREE was published below from `Box::into_raw` and is never freed or written again.
        return Some(unsafe { &*p });
    }
    let fresh = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(json::parse(DEFAULT_JSON).ok()?));
    match TREE.compare_exchange(core::ptr::null_mut(), fresh, Ordering::AcqRel, Ordering::Acquire) {
        // SAFETY: `fresh` is now published and, like any published tree, lives until the process ends.
        Ok(_) => Some(unsafe { &*fresh }),
        Err(won) => {
            // SAFETY: `fresh` lost the race and was never shared; `won` was published by the winning thread.
            drop(unsafe { alloc::boxed::Box::from_raw(fresh) });
            Some(unsafe { &*won })
        }
    }
}

/// Build a Style from the embedded default merged with `user` (may be `None`).
pub fn load(user: Option<&Value>) -> Result<Style, StyleError> {
    let base = default_tree().ok_or_else(|| StyleError::BadStyle(String::new()))?;
    let merged;
    let root = match user {
        Some(u @ Value::Object(m)) if !m.is_empty() => {
            merged = json::merge_patch(base, u);
            &merged
        }
        _ => base,
    };
    if let Some(msg) = validate(root) {
        return Err(StyleError::BadStyle(msg));
    }
    from_value(root)
}

// ---- validation: every number that later becomes a loop step, a character count or an array size is range-checked here once.

#[derive(Default)]
struct Check {
    msg: String,
    count: usize,
}

fn nt(x: f64) -> String {
    let mut b = [0u8; 40];
    String::from(json::fmt_number(&mut b, x))
}

impl Check {
    fn bad(&mut self, text: String) {
        self.count += 1;
        if self.count > 12 {
            return;
        }
        if !self.msg.is_empty() {
            self.msg.push_str("; ");
        }
        self.msg.push_str(&text);
    }

    fn got_text(v: &Value) -> String {
        match v {
            Value::Number(n) => nt(*n),
            Value::String(s) => format!("\"{}\"", clip(s, 24)),
            other => other.kind_name().to_string(),
        }
    }

    /// `v` (if present and not null) must be a number in [lo, hi].
    fn range(&mut self, path: &str, v: Option<&Value>, lo: f64, hi: f64, def: f64) {
        let Some(x) = v else { return };
        if x.is_null() {
            return;
        }
        if let Value::Number(n) = x
            && *n >= lo
            && *n <= hi
        {
            return;
        }
        self.bad(format!(
            "style key '{path}' must be a number from {} to {} (got {}; default {})",
            nt(lo),
            nt(hi),
            Self::got_text(x),
            nt(def)
        ));
    }

    /// `v` (if present and not null) must be one of `names`.
    fn one_of(&mut self, path: &str, v: Option<&Value>, names: &[&str], def: &str) {
        let Some(x) = v.filter(|x| !x.is_null()) else { return };
        if x.str().is_some_and(|s| names.contains(&s)) {
            return;
        }
        self.bad(format!(
            "style key '{path}' must be one of {} (got {}; default \"{def}\")",
            names.join(", "),
            Self::got_text(x)
        ));
    }

    fn section<'a>(&mut self, root: &'a Value, key: &str) -> Option<&'a Value> {
        let v = root.get(key)?;
        match v {
            Value::Null => None,
            Value::Object(_) => Some(v),
            other => {
                self.bad(format!("style key '{key}' must be an object (got {})", other.kind_name()));
                None
            }
        }
    }
}

/// The first `n` bytes of `s`, cut back to a character boundary.
fn clip(s: &str, n: usize) -> &str {
    let mut end = n.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

struct RangeRow {
    sec: &'static str,
    key: &'static str,
    def: fn(&Style) -> f64,
    lo: f64,
    hi: f64,
}

const RANGE_ROWS: [RangeRow; 20] = [
    RangeRow { sec: "text", key: "height_in", def: |s| s.text_height_in, lo: 1.0 / 64.0, hi: 2.0 },
    RangeRow { sec: "text", key: "title_height_in", def: |s| s.title_height_in, lo: 1.0 / 64.0, hi: 4.0 },
    RangeRow { sec: "text", key: "label_height_in", def: |s| s.label_height_in, lo: 1.0 / 64.0, hi: 2.0 },
    RangeRow { sec: "text", key: "line_spacing", def: |s| s.line_spacing, lo: 0.5, hi: 4.0 },
    RangeRow { sec: "notes", key: "wrap_chars", def: |s| s.wrap_chars, lo: 4.0, hi: 200.0 },
    RangeRow { sec: "notes", key: "gutter_in", def: |s| s.gutter_in, lo: 0.0, hi: 10.0 },
    RangeRow { sec: "notes", key: "shoulder_in", def: |s| s.shoulder_in, lo: 0.0, hi: 10.0 },
    RangeRow { sec: "notes", key: "note_gap_in", def: |s| s.note_gap_in, lo: 0.0, hi: 10.0 },
    RangeRow { sec: "notes", key: "arrow_len_in", def: |s| s.arrow_len_in, lo: 0.0, hi: 2.0 },
    RangeRow { sec: "notes", key: "arrow_width_in", def: |s| s.arrow_width_in, lo: 0.0, hi: 2.0 },
    RangeRow { sec: "dims", key: "tick_len_in", def: |s| s.tick_len_in, lo: 0.0, hi: 2.0 },
    RangeRow { sec: "dims", key: "ext_gap_in", def: |s| s.ext_gap_in, lo: 0.0, hi: 2.0 },
    RangeRow { sec: "dims", key: "ext_over_in", def: |s| s.ext_over_in, lo: 0.0, hi: 2.0 },
    RangeRow { sec: "dims", key: "text_gap_in", def: |s| s.dim_text_gap_in, lo: 0.0, hi: 2.0 },
    RangeRow { sec: "dims", key: "precision", def: |s| s.dim_precision, lo: 1.0, hi: 64.0 },
    RangeRow { sec: "sheet", key: "margin_in", def: |s| s.margin_in, lo: 0.0, hi: 10.0 },
    RangeRow { sec: "sheet", key: "title_block_height_in", def: |s| s.title_block_h_in, lo: 0.0, hi: 20.0 },
    RangeRow { sec: "break_line", key: "zig_in", def: |s| s.break_zig_in, lo: 0.001, hi: 5.0 },
    RangeRow { sec: "break_line", key: "period_in", def: |s| s.break_period_in, lo: 0.01, hi: 20.0 },
    RangeRow { sec: "break_line", key: "overshoot_in", def: |s| s.break_overshoot_in, lo: 0.0, hi: 5.0 },
];

/// `None` when `root` is acceptable, else a message listing the problems.
fn validate(root: &Value) -> Option<String> {
    let mut c = Check::default();
    let d = Style::default();
    for r in &RANGE_ROWS {
        if let Some(sec) = c.section(root, r.sec) {
            let path = format!("{}.{}", r.sec, r.key);
            c.range(&path, sec.get(r.key), r.lo, r.hi, (r.def)(&d));
        }
    }
    if let Some(sec) = c.section(root, "sheet")
        && let Some(sz) = sec.get("size_in")
        && !sz.is_null()
    {
        match sz.arr() {
            Some([w, h]) => {
                c.range("sheet.size_in[0]", Some(w), 2.0, 200.0, d.sheet_w_in);
                c.range("sheet.size_in[1]", Some(h), 2.0, 200.0, d.sheet_h_in);
            }
            _ => c.bad(format!(
                "style key 'sheet.size_in' must be [width, height] in inches (got {})",
                Check::got_text(sz)
            )),
        }
    }
    if let Some(sec) = c.section(root, "notes") {
        c.one_of("notes.mode", sec.get("mode"), &["leader", "keyed", "list", "keynote"], "leader");
        c.one_of("notes.key_tag", sec.get("key_tag"), &["plain", "circle", "hex"], "plain");
        validate_abbreviations(&mut c, sec.get("abbreviations"));
    }
    if let Some(sec) = c.section(root, "text") {
        validate_keep_case(&mut c, sec.get("keep_case"));
        for (key, def) in [("plan_title", "PLAN - {title}"), ("elevation_title", "{title}")] {
            match sec.get(key) {
                None | Some(Value::Null) => {}
                Some(Value::String(s)) if s.contains("{title}") && s.len() <= 80 => {}
                Some(v) => c.bad(format!(
                    "style key 'text.{key}' must be a string of at most 80 characters that contains {{title}} (the view's own title), default \"{def}\" (got {})",
                    Check::got_text(v)
                )),
            }
        }
    }
    validate_pens(&mut c, root);
    validate_materials(&mut c, root);
    validate_patterns(&mut c, root);
    if let Some(Value::Object(layers)) = root.get("layers") {
        for m in layers {
            if matches!(m.value, Value::Object(_)) {
                let path = format!("layers.{}.lineweight_mm", m.key);
                c.range(&path, m.value.get("lineweight_mm"), 0.0, 10.0, 0.25);
            }
        }
    }
    if c.count == 0 {
        let sh = root.get("sheet");
        let (mut w, mut h) = (d.sheet_w_in, d.sheet_h_in);
        if let Some([a, b]) = sh.and_then(|s| s.get("size_in")).and_then(Value::arr) {
            w = a.num().unwrap_or(w);
            h = b.num().unwrap_or(h);
        }
        let m = num_or(sh.and_then(|s| s.get("margin_in")), d.margin_in);
        let tb = num_or(sh.and_then(|s| s.get("title_block_height_in")), d.title_block_h_in);
        if w - 2.0 * m < 1.0 || h - 2.0 * m - tb < 1.0 {
            c.bad(format!(
                "style key 'sheet': margin_in {} and title_block_height_in {} leave no drawing area on a {} x {} in sheet (need at least 1 in each way)",
                nt(m), nt(tb), nt(w), nt(h)
            ));
        }
    }
    if c.count == 0 {
        return None;
    }
    if c.count > 12 {
        c.msg.push_str(&format!("; and {} more", c.count - 12));
    }
    Some(c.msg)
}

const MAX_HOUSE_ENTRIES: usize = 500;
const MAX_HOUSE_CHARS: usize = 60;

fn house_text_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= MAX_HOUSE_CHARS
}

fn validate_keep_case(c: &mut Check, v: Option<&Value>) {
    let Some(v) = v.filter(|v| !v.is_null()) else { return };
    let ok = v.arr().is_some_and(|a| {
        a.len() <= MAX_HOUSE_ENTRIES && a.iter().all(|x| x.str().is_some_and(|s| house_text_ok(s) && !s.contains(' ')))
    });
    if !ok {
        c.bad(format!(
            "style key 'text.keep_case' must be an array of up to {MAX_HOUSE_ENTRIES} single tokens (1-{MAX_HOUSE_CHARS} characters, no spaces) printed as written, e.g. [\"kN\", \"mm\"] (got {})",
            Check::got_text(v)
        ));
    }
}

fn validate_abbreviations(c: &mut Check, v: Option<&Value>) {
    let Some(v) = v.filter(|v| !v.is_null()) else { return };
    let ok = match v {
        Value::Object(m) => {
            m.len() <= MAX_HOUSE_ENTRIES
                && m.iter().all(|e| {
                    house_text_ok(&e.key)
                        && match &e.value {
                            Value::String(w) => house_text_ok(w),
                            Value::Array(ws) => {
                                !ws.is_empty() && ws.len() <= 8 && ws.iter().all(|w| w.str().is_some_and(house_text_ok))
                            }
                            _ => false,
                        }
                })
        }
        _ => false,
    };
    if !ok {
        c.bad(format!(
            "style key 'notes.abbreviations' must be an object of up to {MAX_HOUSE_ENTRIES} abbreviations, each mapped to the word(s) it stands for (a string or an array of up to 8 strings, 1-{MAX_HOUSE_CHARS} characters), e.g. {{\"BRG.\": \"BEARING\", \"ARCH.\": [\"ARCHITECT\", \"ARCHITECTURAL\"]}} (got {})",
            Check::got_text(v)
        ));
    }
}

fn validate_pens(c: &mut Check, root: &Value) {
    let Some(Value::Object(pens)) = root.get("pens") else { return };
    for m in pens {
        if !matches!(m.value, Value::Object(_)) {
            continue;
        }
        c.range(&format!("pens.{}.width_mm", m.key), m.value.get("width_mm"), 0.0, 10.0, 0.25);
        if let Some(Value::Array(dv)) = m.value.get("dash_mm") {
            for (i, x) in dv.iter().enumerate() {
                c.range(&format!("pens.{}.dash_mm[{i}]", m.key), Some(x), 0.01, 100.0, 1.0);
            }
        }
    }
}

fn name_list(names: impl Iterator<Item = &'static str>) -> String {
    let mut out = String::new();
    for (i, n) in names.enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(n);
    }
    out
}

fn validate_materials(c: &mut Check, root: &Value) {
    let Some(Value::Object(mats)) = root.get("materials") else { return };
    let pen_names = name_list(Pen::ALL.into_iter().map(Pen::name));
    let role_names = name_list(Role::ALL.into_iter().map(Role::name));
    for m in mats {
        if !matches!(m.value, Value::Object(_)) {
            continue;
        }
        let k = &m.key;
        if let Some(pv) = m.value.get("pen").filter(|v| !v.is_null()) {
            match pv.str() {
                Some(t) => {
                    if Pen::from_name(t).is_none() {
                        c.bad(format!(
                            "style key 'materials.{k}.pen' must be one of the engine's pens ({pen_names}) (got \"{}\")",
                            clip(t, 24)
                        ));
                    }
                }
                None => {
                    c.bad(format!("style key 'materials.{k}.pen' must be a pen name string (got {})", pv.kind_name()));
                }
            }
        }
        if let Some(rv) = m.value.get("role").filter(|v| !v.is_null()) {
            match rv.str() {
                Some(t) => {
                    if Role::from_name(t).is_none() {
                        c.bad(format!(
                            "style key 'materials.{k}.role' must be one of {role_names} (got \"{}\")",
                            clip(t, 24)
                        ));
                    }
                }
                None => {
                    c.bad(format!(
                        "style key 'materials.{k}.role' must be a role name string (got {})",
                        rv.kind_name()
                    ));
                }
            }
        }
        if let Some(Value::Array(h)) = m.value.get("hatch") {
            for (i, x) in h.iter().enumerate() {
                let base = format!("materials.{k}.hatch[{i}]");
                c.range(&format!("{base}.scale"), x.get("scale"), 0.01, 100.0, 1.0);
                c.range(&format!("{base}.angle"), x.get("angle"), -3600.0, 3600.0, 0.0);
            }
        }
        if let Some(g @ Value::Object(_)) = m.value.get("grain") {
            let base = format!("materials.{k}.grain");
            c.range(&format!("{base}.scale"), g.get("scale"), 0.01, 100.0, 1.0);
            c.range(&format!("{base}.amplitude"), g.get("amplitude"), 0.0, 1.0, 0.010);
            c.range(&format!("{base}.wavelength"), g.get("wavelength"), 0.05, 100.0, 1.1);
        }
    }
}

fn validate_patterns(c: &mut Check, root: &Value) {
    let Some(Value::Object(pats)) = root.get("patterns") else { return };
    for m in pats {
        if m.key.starts_with('_') {
            continue;
        }
        let Some(fams) = m.value.arr() else { continue };
        for (i, f) in fams.iter().enumerate() {
            let Some(arr) = f.arr() else { continue };
            if arr.len() < 5 {
                continue;
            }
            let base = format!("patterns.{}[{i}]", m.key);
            c.range(&format!("{base} angle"), Some(&arr[0]), -3600.0, 3600.0, 0.0);
            c.range(&format!("{base} x0"), Some(&arr[1]), -1e4, 1e4, 0.0);
            c.range(&format!("{base} y0"), Some(&arr[2]), -1e4, 1e4, 0.0);
            c.range(&format!("{base} dx"), Some(&arr[3]), -1e4, 1e4, 0.0);
            // |dy| >= 1e-4 keeps a hatch run finite
            match &arr[4] {
                Value::Number(n) if n.abs() < 1e-4 || n.abs() > 1e4 => c.bad(format!(
                    "style key '{base} dy' must be a number with 0.0001 <= |dy| <= 10000 (got {})",
                    nt(*n)
                )),
                Value::Number(_) => {}
                other => c.bad(format!("style key '{base} dy' must be a number (got {})", Check::got_text(other))),
            }
            for (k, dd) in arr[5..].iter().enumerate() {
                c.range(&format!("{base} dash[{k}]"), Some(dd), -1e4, 1e4, 0.0);
            }
        }
    }
}

pub fn from_value(root: &Value) -> Result<Style, StyleError> {
    let bad = || StyleError::BadStyle(String::new());
    let Some(Value::Object(pens)) = root.get("pens") else { return Err(bad()) };
    let Some(Value::Object(mats)) = root.get("materials") else { return Err(bad()) };
    let mut s = Style::default();

    for m in pens {
        let dash = match m.value.get("dash_mm") {
            Some(Value::Array(d)) if !d.is_empty() => Some(d.iter().map(|x| x.num().unwrap_or(1.0)).collect()),
            _ => None,
        };
        s.pens.push(PenDef { name: m.key.clone(), width_mm: num_or(m.value.get("width_mm"), 0.25), dash_mm: dash });
    }

    for m in mats {
        let mut mat = Material::new(&m.key);
        if let Some(Value::Array(h)) = m.value.get("hatch") {
            mat.hatch = h
                .iter()
                .map(|x| HatchSpec {
                    pattern: str_or(x.get("pattern"), "ANSI31"),
                    scale: num_or(x.get("scale"), 1.0),
                    angle: num_or(x.get("angle"), 0.0),
                })
                .collect();
        }
        if let Some(g @ Value::Object(_)) = m.value.get("grain") {
            mat.grain = Some(GrainSpec {
                pattern: str_or(g.get("pattern"), "KERF-GRAIN"),
                scale: num_or(g.get("scale"), 1.0),
                amplitude: num_or(g.get("amplitude"), 0.010),
                wavelength: num_or(g.get("wavelength"), 1.1),
            });
        }
        match m.value.get("cut_mark").and_then(Value::str) {
            Some("x") => mat.cut_mark = CutMark::X,
            Some("diagonal") => mat.cut_mark = CutMark::Diagonal,
            _ => {}
        }
        if let Some(Value::Bool(f)) = m.value.get("fill") {
            mat.fill = *f;
        }
        if let Some(Value::Bool(f)) = m.value.get("batt") {
            mat.batt = *f;
        }
        if let Some(t) = m.value.get("pen").and_then(Value::str) {
            mat.pen = Pen::from_name(t);
        }
        if let Some(role) = m.value.get("role").and_then(Value::str).and_then(Role::from_name) {
            mat.role = role;
        }
        if let Some(c) = m.value.get("color3d").and_then(Value::str) {
            mat.color3d = c.to_string();
        }
        mat.layer = m.value.get("layer").and_then(Value::str).map(str::to_string);
        s.materials.push(mat);
    }

    if let Some(Value::Object(pats)) = root.get("patterns") {
        for m in pats {
            if m.key.starts_with('_') {
                continue;
            }
            let Some(fams) = m.value.arr() else { continue };
            let mut families = Vec::new();
            for f in fams {
                let Some(arr) = f.arr() else { continue };
                if arr.len() < 5 {
                    continue;
                }
                families.push(Family {
                    angle: num_or(arr.first(), 0.0),
                    x0: num_or(arr.get(1), 0.0),
                    y0: num_or(arr.get(2), 0.0),
                    dx: num_or(arr.get(3), 0.0),
                    dy: num_or(arr.get(4), 0.125),
                    dashes: arr[5..].iter().map(|d| d.num().unwrap_or(0.0)).collect(),
                });
            }
            s.patterns.push(Pattern { name: m.key.clone(), families });
        }
    }

    if let Some(Value::Object(layers)) = root.get("layers") {
        for m in layers {
            match &m.value {
                Value::String(nm) => s.layers.push(Layer {
                    key: m.key.clone(),
                    name: nm.clone(),
                    lineweight_mm: 0.25,
                    linetype: "CONTINUOUS".to_string(),
                }),
                Value::Object(_) => s.layers.push(Layer {
                    key: m.key.clone(),
                    name: str_or(m.value.get("name"), &m.key),
                    lineweight_mm: num_or(m.value.get("lineweight_mm"), 0.25),
                    linetype: str_or(m.value.get("linetype"), "CONTINUOUS"),
                }),
                _ => {}
            }
        }
    }

    if let Some(t) = root.get("text") {
        s.text_height_in = num_or(t.get("height_in"), s.text_height_in);
        s.title_height_in = num_or(t.get("title_height_in"), s.title_height_in);
        s.label_height_in = num_or(t.get("label_height_in"), s.label_height_in);
        s.text_case_upper = t.get("case").and_then(Value::str).unwrap_or("upper") != "as_is";
        s.line_spacing = num_or(t.get("line_spacing"), s.line_spacing);
        s.keep_case = t
            .get("keep_case")
            .and_then(Value::arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(Value::str)
            .map(String::from)
            .collect();
        s.dxf_style = str_or(t.get("dxf_style"), &s.dxf_style);
        s.dxf_font = str_or(t.get("dxf_font"), &s.dxf_font);
        s.plan_title = str_or(t.get("plan_title"), &s.plan_title);
        s.elevation_title = str_or(t.get("elevation_title"), &s.elevation_title);
    }
    if let Some(t) = root.get("notes") {
        s.notes_mode = match t.get("mode").and_then(Value::str) {
            Some("keyed" | "keynote") => NotesStyle::Keyed,
            Some("list") => NotesStyle::List,
            _ => NotesStyle::Leader,
        };
        s.wrap_chars = num_or(t.get("wrap_chars"), s.wrap_chars);
        s.gutter_in = num_or(t.get("gutter_in"), s.gutter_in);
        s.shoulder_in = num_or(t.get("shoulder_in"), s.shoulder_in);
        s.note_gap_in = num_or(t.get("note_gap_in"), s.note_gap_in);
        s.arrow = str_or(t.get("arrow"), &s.arrow);
        s.arrow_len_in = num_or(t.get("arrow_len_in"), s.arrow_len_in);
        s.arrow_width_in = num_or(t.get("arrow_width_in"), s.arrow_width_in);
        s.key_tag = match t.get("key_tag").and_then(Value::str) {
            Some("circle") => KeyTag::Circle,
            Some("hex") => KeyTag::Hex,
            _ => KeyTag::Plain,
        };
        s.list_title = str_or(t.get("list_title"), &s.list_title);
        if let Some(Value::Object(ab)) = t.get("abbreviations") {
            for m in ab {
                let words: Vec<String> = match &m.value {
                    Value::String(w) => alloc::vec![w.clone()],
                    Value::Array(ws) => ws.iter().filter_map(Value::str).map(String::from).collect(),
                    _ => Vec::new(),
                };
                if !words.is_empty() {
                    s.abbreviations.push((m.key.clone(), words));
                }
            }
        }
    }
    if let Some(t) = root.get("dims") {
        s.dim_terminator = str_or(t.get("terminator"), &s.dim_terminator);
        s.tick_len_in = num_or(t.get("tick_len_in"), s.tick_len_in);
        s.ext_gap_in = num_or(t.get("ext_gap_in"), s.ext_gap_in);
        s.ext_over_in = num_or(t.get("ext_over_in"), s.ext_over_in);
        s.dim_text_gap_in = num_or(t.get("text_gap_in"), s.dim_text_gap_in);
        s.dim_precision = num_or(t.get("precision"), s.dim_precision);
    }
    if let Some(t) = root.get("citations") {
        s.cite_format = str_or(t.get("format"), &s.cite_format);
        s.cite_flag_unverified = t.get("unverified").and_then(Value::str).unwrap_or("flag") == "flag";
        s.cite_flag = str_or(t.get("flag"), &s.cite_flag);
        s.cite_footnote = str_or(t.get("footnote"), &s.cite_footnote);
    }
    if let Some(t) = root.get("sheet") {
        if let Some([w, h, ..]) = t.get("size_in").and_then(Value::arr) {
            s.sheet_w_in = num_or(Some(w), s.sheet_w_in);
            s.sheet_h_in = num_or(Some(h), s.sheet_h_in);
        }
        s.margin_in = num_or(t.get("margin_in"), s.margin_in);
        s.title_block = str_or(t.get("title_block"), &s.title_block);
        s.title_block_h_in = num_or(t.get("title_block_height_in"), s.title_block_h_in);
        s.firm = str_or(t.get("firm"), &s.firm);
        s.project = str_or(t.get("project"), &s.project);
    }
    if let Some(t) = root.get("break_line") {
        s.break_zig_in = num_or(t.get("zig_in"), s.break_zig_in);
        s.break_period_in = num_or(t.get("period_in"), s.break_period_in);
        s.break_overshoot_in = num_or(t.get("overshoot_in"), s.break_overshoot_in);
    }
    if let Some(t) = root.get("colors3d") {
        s.color_cut_cap = str_or(t.get("cut_cap"), &s.color_cut_cap);
        s.color_edge = str_or(t.get("edge"), &s.color_edge);
        s.color_background = str_or(t.get("background"), &s.color_background);
    }
    Ok(s)
}
