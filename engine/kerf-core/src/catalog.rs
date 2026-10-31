//! Component catalog (SPEC 5): the single source of truth for param names (validation of unknown
//! keys), defaults, parts and anchors, rendered as JSON and markdown for the LLM system prompt.
//! The per-type parameter rows come from `builders::params_rows(Type)` (generated from each builder's `Params`).

use crate::builders;
use crate::json::{Member, Value};
use crate::params::Row;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// What the engine asks of a component type besides building it: code that needs "the type with a pitch" or "hardware" reads a flag
/// here instead of comparing type names, so a new type opts in by setting a flag.
#[derive(Clone, Copy, Debug)]
pub struct Traits {
    /// Anchor a placement uses when `at` does not name one.
    pub default_anchor: &'static str,
    /// Has a roof pitch that `slope: "@id"` follows (truss).
    pub has_pitch: bool,
    /// Schematic steel hardware with a hardware table (connector): exempt from overlap checks, lists its models in the catalog.
    pub hardware: bool,
    /// Reinforcement checked for cover against its host (rebar).
    pub reinforcement: bool,
    /// Escape hatch reported as I_SOLID_USED (solid).
    pub escape_hatch: bool,
    /// Line-like member (membrane, flashing, connector, rebar): notes land on its own line, not inside a region.
    pub line_like: bool,
    /// Lumber cut along its length is drawn with wood grain.
    pub lengthwise_grain: bool,
    /// Takes part in near-miss checks (framing lumber).
    pub near_miss: bool,
    /// A sheet (panel, membrane) that, when sloped and resting on a sloped member, must reach that member's end (W_SHORT_SLOPE).
    pub sheet: bool,
    /// A rigid sheet (panel): thin ones land notes on their midline, and a panel does not rest on another panel.
    pub rigid_sheet: bool,
    /// Can carry a sloped sheet (lumber, panel, truss).
    pub slope_host: Option<SlopeHost>,
    /// Sizes come from the steel shape table, which the catalog lists (steel).
    pub shape_table: bool,
    /// A host that `voids` and sleeves cut (concrete, cmu_wall; SPEC 26.4).
    pub takes_voids: bool,
    /// Subtracted from the hosts its `through` names (sleeve).
    pub passes_through: bool,
    /// Minimum bearing of a member seen lengthwise on what is under it: [at an end, at an intermediate support] (ijoist;
    /// zero: not checked, W_BEARING).
    pub min_bearing: [f64; 2],
}

impl Traits {
    pub const NONE: Traits = Traits {
        default_anchor: "bottom_left",
        has_pitch: false,
        hardware: false,
        reinforcement: false,
        escape_hatch: false,
        line_like: false,
        lengthwise_grain: false,
        near_miss: false,
        sheet: false,
        rigid_sheet: false,
        slope_host: None,
        shape_table: false,
        takes_voids: false,
        passes_through: false,
        min_bearing: [0.0, 0.0],
    };
}

#[derive(Clone, Copy, Debug)]
pub struct SlopeHost {
    /// The part a sheet rests on ("" = any part).
    pub part: &'static str,
    /// The anchor at the member's far end that `until` should name ("" = top_right / top_left by direction).
    pub end_anchor: &'static str,
}

/// The component types, in catalog order. The variant's `name()` is the type's name in documents (`"type": "lumber"`).
/// Adding a type: add the variant, an entry to `ENTRIES` (same position), `name`, and a `builders::build` arm (exhaustive matches).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Type {
    Lumber,
    Panel,
    CmuWall,
    Concrete,
    Rebar,
    AnchorBolt,
    Connector,
    Truss,
    Ijoist,
    Membrane,
    Fill,
    Insulation,
    Flashing,
    Joint,
    Sleeve,
    Steel,
    Bolt,
    Nailing,
    Solid,
}

impl Type {
    pub const ALL: [Type; 19] = [
        Type::Lumber,
        Type::Panel,
        Type::CmuWall,
        Type::Concrete,
        Type::Rebar,
        Type::AnchorBolt,
        Type::Connector,
        Type::Truss,
        Type::Ijoist,
        Type::Membrane,
        Type::Fill,
        Type::Insulation,
        Type::Flashing,
        Type::Joint,
        Type::Sleeve,
        Type::Steel,
        Type::Bolt,
        Type::Nailing,
        Type::Solid,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Type::Lumber => "lumber",
            Type::Panel => "panel",
            Type::CmuWall => "cmu_wall",
            Type::Concrete => "concrete",
            Type::Rebar => "rebar",
            Type::AnchorBolt => "anchor_bolt",
            Type::Connector => "connector",
            Type::Truss => "truss",
            Type::Ijoist => "ijoist",
            Type::Membrane => "membrane",
            Type::Fill => "fill",
            Type::Insulation => "insulation",
            Type::Flashing => "flashing",
            Type::Joint => "joint",
            Type::Sleeve => "sleeve",
            Type::Steel => "steel",
            Type::Bolt => "bolt",
            Type::Nailing => "nailing",
            Type::Solid => "solid",
        }
    }

    pub fn from_name(s: &str) -> Option<Type> {
        Type::ALL.into_iter().find(|t| t.name() == s)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub ty: Type,
    pub summary: &'static str,
    pub parts: &'static str,
    pub anchors: &'static str,
    pub draws: &'static str,
    /// One complete component object (valid on its own); `kerf schema <type>` prints it.
    pub example: &'static str,
    /// Behaviour the compiler, validators and renderers ask of the type (instead of naming it).
    pub traits: Traits,
}

impl Entry {
    pub fn name(&self) -> &'static str {
        self.ty.name()
    }

    /// The parameter rows, generated from the builder's `Params`.
    pub fn params(&self) -> Vec<Row> {
        builders::params_rows(self.ty)
    }
}

pub struct Hardware {
    pub model: &'static str,
    pub width: f64,
    pub gauge: u32,
    pub length: f64,
    pub kind: &'static str,
}

/// Schematic hardware table: `connector.model` fills width and gauge from here.
pub const HARDWARE: [Hardware; 60] = [
    Hardware { model: "H2.5A", width: 1.375, gauge: 18, length: 5.5, kind: "hurricane tie" },
    Hardware { model: "H1", width: 1.375, gauge: 18, length: 5.5, kind: "hurricane tie" },
    Hardware { model: "H10A", width: 1.5, gauge: 18, length: 8.0, kind: "hurricane tie" },
    Hardware { model: "MSTA9", width: 1.25, gauge: 12, length: 9.0, kind: "strap" },
    Hardware { model: "MSTA12", width: 1.25, gauge: 12, length: 12.0, kind: "strap" },
    Hardware { model: "MSTA15", width: 1.25, gauge: 12, length: 15.0, kind: "strap" },
    Hardware { model: "MSTA18", width: 1.25, gauge: 12, length: 18.0, kind: "strap" },
    Hardware { model: "MSTA21", width: 1.25, gauge: 12, length: 21.0, kind: "strap" },
    Hardware { model: "MSTA24", width: 1.25, gauge: 12, length: 24.0, kind: "strap" },
    Hardware { model: "MSTA30", width: 1.25, gauge: 12, length: 30.0, kind: "strap" },
    Hardware { model: "MSTA36", width: 1.25, gauge: 12, length: 36.0, kind: "strap" },
    Hardware { model: "MSTA49", width: 1.25, gauge: 12, length: 49.0, kind: "strap" },
    Hardware { model: "LSTA9", width: 1.25, gauge: 20, length: 9.0, kind: "strap" },
    Hardware { model: "LSTA12", width: 1.25, gauge: 20, length: 12.0, kind: "strap" },
    Hardware { model: "LSTA15", width: 1.25, gauge: 20, length: 15.0, kind: "strap" },
    Hardware { model: "LSTA18", width: 1.25, gauge: 20, length: 18.0, kind: "strap" },
    Hardware { model: "LSTA21", width: 1.25, gauge: 20, length: 21.0, kind: "strap" },
    Hardware { model: "LSTA24", width: 1.25, gauge: 20, length: 24.0, kind: "strap" },
    Hardware { model: "LSTA36", width: 1.25, gauge: 20, length: 36.0, kind: "strap" },
    Hardware { model: "CS14", width: 1.25, gauge: 14, length: 0.0, kind: "coil strap" },
    Hardware { model: "CS16", width: 1.25, gauge: 16, length: 0.0, kind: "coil strap" },
    Hardware { model: "CS18", width: 1.25, gauge: 18, length: 0.0, kind: "coil strap" },
    Hardware { model: "CS20", width: 1.25, gauge: 20, length: 0.0, kind: "coil strap" },
    Hardware { model: "CMST14", width: 3.0, gauge: 14, length: 0.0, kind: "coil strap" },
    Hardware { model: "CMST12", width: 3.0, gauge: 12, length: 0.0, kind: "coil strap" },
    Hardware { model: "CMSTC16", width: 3.0, gauge: 16, length: 0.0, kind: "coil strap" },
    Hardware { model: "MST37", width: 3.0, gauge: 12, length: 37.5, kind: "strap" },
    Hardware { model: "MST48", width: 3.0, gauge: 12, length: 48.0, kind: "strap" },
    Hardware { model: "META12", width: 1.25, gauge: 18, length: 12.0, kind: "embedded truss anchor" },
    Hardware { model: "META16", width: 1.25, gauge: 18, length: 16.0, kind: "embedded truss anchor" },
    Hardware { model: "META20", width: 1.25, gauge: 18, length: 20.0, kind: "embedded truss anchor" },
    Hardware { model: "META24", width: 1.25, gauge: 18, length: 24.0, kind: "embedded truss anchor" },
    Hardware { model: "HETA12", width: 1.25, gauge: 16, length: 12.0, kind: "embedded truss anchor" },
    Hardware { model: "HETA16", width: 1.25, gauge: 16, length: 16.0, kind: "embedded truss anchor" },
    Hardware { model: "HETA20", width: 1.25, gauge: 16, length: 20.0, kind: "embedded truss anchor" },
    Hardware { model: "HETA24", width: 1.25, gauge: 16, length: 24.0, kind: "embedded truss anchor" },
    Hardware { model: "HETA40", width: 1.25, gauge: 16, length: 40.0, kind: "embedded truss anchor" },
    Hardware { model: "HHETA16", width: 1.25, gauge: 14, length: 16.0, kind: "embedded truss anchor" },
    Hardware { model: "HHETA20", width: 1.25, gauge: 14, length: 20.0, kind: "embedded truss anchor" },
    Hardware { model: "HETAL20", width: 1.25, gauge: 16, length: 20.0, kind: "embedded truss anchor" },
    Hardware { model: "DETAL20", width: 2.5, gauge: 16, length: 20.0, kind: "embedded truss anchor" },
    Hardware { model: "RSP4", width: 2.125, gauge: 20, length: 4.5, kind: "rafter/stud plate tie" },
    Hardware { model: "A34", width: 2.5, gauge: 18, length: 2.875, kind: "framing angle, 1-7/16 legs" },
    Hardware { model: "A35", width: 4.5, gauge: 18, length: 2.875, kind: "framing angle, 1-7/16 legs" },
    Hardware { model: "LTP4", width: 3.0, gauge: 20, length: 4.25, kind: "lateral tie plate" },
    Hardware { model: "LSTHD8", width: 3.0, gauge: 14, length: 18.625, kind: "strap-tie holdown (embedded)" },
    Hardware { model: "STHD14", width: 3.0, gauge: 12, length: 26.125, kind: "strap-tie holdown (embedded)" },
    Hardware {
        model: "LUS26",
        width: 1.5625,
        gauge: 18,
        length: 4.75,
        kind: "face-mount joist hanger, 2x6 (length = height)",
    },
    Hardware {
        model: "MUS26",
        width: 1.5625,
        gauge: 18,
        length: 5.1875,
        kind: "face-mount joist hanger, 2x6 (length = height)",
    },
    Hardware {
        model: "HUS26",
        width: 1.625,
        gauge: 16,
        length: 5.375,
        kind: "heavy face-mount hanger, 2x6 (length = height)",
    },
    Hardware {
        model: "HHUS26-2",
        width: 3.3125,
        gauge: 14,
        length: 5.375,
        kind: "heavy face-mount hanger, double 2x6 (length = height)",
    },
    Hardware { model: "ST2215", width: 2.0625, gauge: 20, length: 16.3125, kind: "strap tie" },
    Hardware { model: "ST6224", width: 2.0625, gauge: 16, length: 23.3125, kind: "strap tie" },
    Hardware { model: "FHA18", width: 1.4375, gauge: 12, length: 17.75, kind: "strap tie" },
    Hardware { model: "LTS12", width: 1.25, gauge: 18, length: 12.0, kind: "twist strap" },
    Hardware { model: "H3", width: 1.25, gauge: 18, length: 4.625, kind: "hurricane tie" },
    Hardware { model: "ST22", width: 1.625, gauge: 20, length: 21.625, kind: "strap tie" },
    Hardware { model: "ST6236", width: 2.0625, gauge: 16, length: 33.8125, kind: "strap tie" },
    Hardware { model: "MST27", width: 2.0625, gauge: 12, length: 27.0, kind: "strap" },
    Hardware { model: "MST60", width: 2.0625, gauge: 10, length: 60.0, kind: "strap" },
];

/// A family of hardware recognised by its model prefix (SPEC 27.2): the connector kind it draws as, the hanger mount, a
/// typical gauge and (straps and ties) width. Generic: sizes come from the members, the note carries the exact model.
pub struct Series {
    pub prefix: &'static str,
    pub kind: &'static str,
    pub mount: &'static str,
    pub gauge: u32,
    pub width: f64,
    pub desc: &'static str,
}

const fn ser(
    prefix: &'static str,
    kind: &'static str,
    mount: &'static str,
    gauge: u32,
    width: f64,
    desc: &'static str,
) -> Series {
    Series { prefix, kind, mount, gauge, width, desc }
}

pub const SERIES: [Series; 68] = [
    ser("LUS", "hanger", "face", 18, 0.0, "face-mount joist hanger"),
    ser("MUS", "hanger", "face", 18, 0.0, "face-mount joist hanger"),
    ser("HUS", "hanger", "face", 16, 0.0, "heavy face-mount hanger"),
    ser("HHUS", "hanger", "face", 14, 0.0, "heavy face-mount hanger"),
    ser("HU", "hanger", "face", 14, 0.0, "face-mount hanger"),
    ser("HUC", "hanger", "face", 14, 0.0, "concealed-flange face-mount hanger"),
    ser("HGUS", "hanger", "face", 12, 0.0, "heavy girder hanger"),
    ser("IUS", "hanger", "face", 18, 0.0, "face-mount I-joist hanger"),
    ser("LU", "hanger", "face", 20, 0.0, "face-mount joist hanger"),
    ser("U", "hanger", "face", 16, 0.0, "face-mount joist hanger"),
    ser("SUR", "hanger", "face", 16, 0.0, "skewed face-mount hanger"),
    ser("SUL", "hanger", "face", 16, 0.0, "skewed face-mount hanger"),
    ser("LSSU", "hanger", "face", 18, 0.0, "sloped/skewed face-mount hanger"),
    ser("ITS", "hanger", "top_flange", 18, 0.0, "top-flange I-joist hanger"),
    ser("MIT", "hanger", "top_flange", 16, 0.0, "top-flange I-joist hanger"),
    ser("HIT", "hanger", "top_flange", 14, 0.0, "top-flange I-joist hanger"),
    ser("THA", "hanger", "top_flange", 18, 0.0, "top-flange truss hanger"),
    ser("LBV", "hanger", "top_flange", 14, 0.0, "top-flange hanger"),
    ser("JB", "hanger", "top_flange", 16, 0.0, "top-flange joist hanger"),
    ser("HB", "hanger", "top_flange", 12, 0.0, "heavy top-flange hanger"),
    ser("WPU", "hanger", "top_flange", 10, 0.0, "welded top-flange hanger"),
    ser("GLTV", "hanger", "top_flange", 10, 0.0, "glulam top-flange hanger"),
    ser("CC", "post_cap", "", 10, 0.0, "column cap"),
    ser("CCQ", "post_cap", "", 10, 0.0, "column cap (screwed)"),
    ser("ECC", "post_cap", "", 10, 0.0, "end column cap"),
    ser("ECCQ", "post_cap", "", 10, 0.0, "end column cap (screwed)"),
    ser("BC", "post_cap", "", 18, 0.0, "post cap / base"),
    ser("AC", "post_cap", "", 18, 0.0, "adjustable post cap"),
    ser("LCE", "post_cap", "", 20, 0.0, "post cap"),
    ser("PC", "post_cap", "", 12, 0.0, "post cap"),
    ser("ABU", "post_base", "", 12, 0.0, "adjustable standoff post base"),
    ser("ABA", "post_base", "", 16, 0.0, "adjustable post base"),
    ser("ABW", "post_base", "", 14, 0.0, "adjustable post base"),
    ser("CBSQ", "post_base", "", 12, 0.0, "standoff column base"),
    ser("CB", "post_base", "", 10, 0.0, "column base"),
    ser("PB", "post_base", "", 12, 0.0, "post base"),
    ser("HDU", "holdown", "", 10, 0.0, "holdown (bucket, screwed)"),
    ser("HDQ", "holdown", "", 10, 0.0, "holdown (bucket, screwed)"),
    ser("HHDQ", "holdown", "", 10, 0.0, "heavy holdown"),
    ser("HD", "holdown", "", 10, 0.0, "holdown (bolted)"),
    ser("HDC", "holdown", "", 10, 0.0, "holdown"),
    ser("PHD", "holdown", "", 10, 0.0, "holdown"),
    ser("HTT", "holdown", "", 11, 0.0, "tension tie holdown"),
    ser("DTT", "holdown", "", 14, 0.0, "deck tension tie"),
    ser("LTT", "holdown", "", 14, 0.0, "light tension tie"),
    ser("H", "strap", "", 18, 1.375, "hurricane / seismic tie"),
    ser("STHD", "strap", "", 12, 3.0, "strap-tie holdown (embedded)"),
    ser("LSTHD", "strap", "", 14, 3.0, "strap-tie holdown (embedded)"),
    ser("CS", "strap", "", 16, 1.25, "coil strap"),
    ser("CMST", "strap", "", 14, 3.0, "coil strap"),
    ser("MSTA", "strap", "", 12, 1.25, "strap"),
    ser("LSTA", "strap", "", 20, 1.25, "strap"),
    ser("META", "strap", "", 18, 1.25, "embedded truss anchor"),
    ser("HETA", "strap", "", 16, 1.25, "embedded truss anchor"),
    ser("SP", "strap", "", 20, 1.625, "stud plate tie"),
    ser("ST", "strap", "", 16, 2.0625, "strap tie"),
    ser("MST", "strap", "", 12, 2.0625, "strap"),
    ser("HRS", "strap", "", 12, 1.375, "strap"),
    ser("TC", "strap", "", 16, 1.5, "truss connector"),
    ser("PA", "strap", "", 12, 2.0, "purlin anchor (embedded)"),
    ser("HPA", "strap", "", 12, 2.0, "purlin anchor (embedded)"),
    ser("LTS", "strap", "", 18, 1.25, "twist strap"),
    ser("MTS", "strap", "", 16, 1.25, "twist strap"),
    ser("HTS", "strap", "", 16, 1.25, "twist strap"),
    ser("TBE", "strap", "", 18, 2.5, "truss bearing enhancer"),
    ser("GA", "strap", "", 18, 2.5, "gusset angle"),
    ser("A", "strap", "", 18, 1.5, "framing angle"),
    ser("L", "strap", "", 16, 1.25, "reinforcing angle"),
];

/// How a connector `model` resolves (SPEC 27.2).
pub enum ModelMatch {
    /// In the hardware table: width, gauge, length.
    Exact(&'static Hardware),
    /// A known family by prefix: kind, mount, gauge, width.
    Series(&'static Series),
    Unknown,
}

/// The table entry for `model` (case-insensitive), else the family whose prefix is the longest one `model` starts with
/// and continues with a digit, a separator or nothing (`HDU5`, `HU212-2`, `ITS2.06/11.88`, `CCQ44`).
pub fn resolve_model(model: &str) -> ModelMatch {
    if let Some(h) = HARDWARE.iter().find(|h| h.model.eq_ignore_ascii_case(model)) {
        return ModelMatch::Exact(h);
    }
    let up = model.to_ascii_uppercase();
    let mut best: Option<&'static Series> = None;
    for s in &SERIES {
        let Some(rest) = up.strip_prefix(s.prefix) else { continue };
        let ok = rest.chars().next().is_none_or(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | '/' | ' '));
        if ok && best.is_none_or(|b| s.prefix.len() > b.prefix.len()) {
            best = Some(s);
        }
    }
    best.map_or(ModelMatch::Unknown, ModelMatch::Series)
}

/// Every model name and family prefix, for "did you mean" (table models first).
pub fn model_names() -> Vec<&'static str> {
    HARDWARE.iter().map(|h| h.model).chain(SERIES.iter().map(|s| s.prefix)).collect()
}

pub fn series_line(s: &Series) -> String {
    let mount = if s.mount.is_empty() { String::new() } else { format!(" ({})", s.mount.replace('_', "-")) };
    let width = if s.width > 0.0 { format!(", {}\" wide", crate::model::fmt_num(s.width)) } else { String::new() };
    format!("{}*: {}{mount}, kind {}, {} ga{width}", s.prefix, s.desc, s.kind, s.gauge)
}

pub fn hardware_line(h: &Hardware) -> String {
    let (w, g) = (crate::model::fmt_num(h.width), h.gauge);
    if h.length == 0.0 {
        return format!("{}: {}, {w}\" wide, {g} ga, cut to length", h.model, h.kind);
    }
    format!("{}: {}, {w}\" wide, {g} ga, {}\" long", h.model, h.kind, crate::model::fmt_num(h.length))
}

const COMMON: [(&[&str], &str, &str); 14] = [
    (&["id"], "required", "unique slug [a-z][a-z0-9_]*; annotations and refs name it"),
    (&["type"], "required", "one of the catalog types"),
    (&["label"], "null", "short human label for summaries"),
    (
        &["at"],
        "origin",
        "{anchor (default bottom_left), to: Ref | {ref, offset} | [x,y], offset: [dx,dy]}: translates the part so its anchor lands on `to`+offset. Point-list types (connector, membrane, fill, rebar path, concrete polygon, solid points): literal [x,y] entries are relative to `to`+offset, Refs are absolute; a written `anchor` (box anchor or vertex_k) puts that point of the shape on `to` instead (no Ref points then)",
    ),
    (&["rotate"], "0", "degrees CCW about the placement point"),
    (
        &["slope"],
        "null",
        "\"4:12\" rise:run, degrees, or \"@<component>\" (e.g. \"@truss\": inherit that member's slope, so pitch lives in ONE place; a truss with exterior right slopes the other way). Rotation about the placement point; adds to rotate",
    ),
    (
        &["mirror"],
        "false",
        "mirror the profile about its vertical centerline before placement (not for point-list shapes: write mirrored points)",
    ),
    (
        &["z"],
        "null",
        "[z0,z1] absolute, or a number = centered there with the member's natural z thickness. Default: spans doc `run` (members along Z); members with a natural z thickness (lumber run x/y, truss, connector, path rebar, anchor_bolt) are centered on the first section view's `cut_z` (when it sets one, so they are cut, not hidden), else on the middle of `run`",
    ),
    (
        &["array"],
        "null",
        "{axis: x|y|z, count, spacing}: instance k offset by k*spacing from instance 0 (a numeric `z` places instance 0, it does not center the group: for n instances centered on c use z = c - (n-1)*spacing/2); instances are id#0..id#n-1, refs to `id` mean instance 0, `id#k@anchor` addresses instance k. Instances that touch nothing while others do get W_FLOATING",
    ),
    (&["embedded"], "type default", "drawn over cut solids and never occluded (rebar, anchor bolts)"),
    (&["visible"], "true", "false hides the component from views and mesh"),
    (
        &["views"],
        "null (every view)",
        "[view id]: the component is drawn and checked only in these views (one scenario per view: shared members get no `views`, a scenario's members `views: [\"B\"]`, all at one origin). Excluded components are not drawn, not even as beyond outlines, and skip the geometry checks and W_CROP_STALE of that view. Unknown view ids are E_REF_UNKNOWN",
    ),
    (
        &["shown"],
        "solid",
        "solid | hidden | where_occurs. hidden = a member behind or below the others (e.g. a base plate under a closure pour in plan, a stiffener behind a web): all edges in the hidden (dashed) pen, no hatch or cut mark, never hides anything, exempt from W_FLOATING / W_NEAR_MISS / W_OVERLAP. where_occurs = the same graphics, and notes targeting it get \" (WHERE OCCURS)\" appended. The old \"dashed\" means where_occurs and warns W_SHOWN_DASHED. Section, plan and elevation views (iso omits it)",
    ),
    (
        &["acknowledge"],
        "null",
        "[{code, reason}]: suppress that warning (e.g. W_UNTREATED_CONTACT) for this component; it is counted on the summary's DOC line and listed with the reason in its ACKNOWLEDGED block (`kerf check --strict` shows it as a warning), and logged. Errors cannot be acknowledged; an entry that suppresses nothing is W_ACK_UNUSED (`kerf schema acknowledge`)",
    ),
];

pub fn common() -> Vec<Row> {
    COMMON.iter().map(|&(names, def, desc)| Row { names: names.to_vec(), def: def.to_string(), desc }).collect()
}

pub const BOX_ANCHORS: &str = "top_left top_center top_right middle_left center middle_right bottom_left bottom_center bottom_right (of the profile box; rotate with the member)";

pub static ENTRIES: [Entry; 19] = [
    Entry {
        ty: Type::Lumber,
        summary: "Sawn or engineered wood member (stud, plate, joist, beam, blocking, post). Standard view for a beam in a wall (flush beam, header): an ELEVATION along the wall, i.e. the beam seen lengthwise (run x, face wide) with the top and bottom plates interrupted where they butt it and king/jack studs (run y) at its ends. Draw the end-on section (run z, beam cut) only when the designer asks for it; either way, say in your report which reading you drew.",
        parts: "barrier (when set)",
        anchors: "the 9 box anchors; with cuts or from/to also start_top, start_bottom, end_top, end_bottom (cut corners) and seat_outer, seat_inner, heel_bottom (birdmouth)",
        draws: "Section: run z cut => outline + wood X mark per ply (blocking: one diagonal); run x/y cut lengthwise => outline only; beyond => outline. Actual sizes: 1x 3/4 thick, 2x 1 1/2, 3x 2 1/2, 4x 3 1/2, 6x 5 1/2, 8x 7 1/2; depths x3 2 1/2, x4 3 1/2, x6 5 1/2, x8 7 1/4, x10 9 1/4, x12 11 1/4, x14 13 1/4 (6x and 8x: x6 5 1/2, x8 7 1/2, x10 9 1/2, x12 11 1/2). Natural z thickness for run x/y; run z with `length` is that long along z.",
        example: "{\"id\":\"stud\",\"type\":\"lumber\",\"size\":\"2x4\",\"run\":\"y\",\"face\":\"narrow\",\"length\":92.625,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,1.5]}}",
        traits: Traits {
            lengthwise_grain: true,
            near_miss: true,
            slope_host: Some(SlopeHost { part: "", end_anchor: "" }),
            ..Traits::NONE
        },
    },
    Entry {
        ty: Type::Panel,
        summary: "Sheathing, boards, gypsum, soffit, fascia/trim boards as a thin rectangle; `lay: face` for a sheet seen face-on.",
        parts: "none",
        anchors: "the 9 box anchors",
        draws: "Edge-on (default): cut rectangle with the material's hatch / cut mark (wood_board: one diagonal), spanning the document run along Z. Use `slope` for roof sheathing (rotates about the placement anchor). `lay: face`: the sheet in the XY plane (`length` x `height`, `thickness` along Z, `on: \"stud.face_front\"` sets z): its outline in elevations and iso, a thin strip in plans; shear walls, portal frames and wall sheathing take edge and field `nailing` on it.",
        example: "{\"id\":\"roof_sheathing\",\"type\":\"panel\",\"material\":\"osb\",\"thickness\":0.4375,\"length\":66,\"run\":\"x\",\"slope\":\"4:12\",\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
        traits: Traits {
            sheet: true,
            rigid_sheet: true,
            slope_host: Some(SlopeHost { part: "", end_anchor: "" }),
            ..Traits::NONE
        },
    },
    Entry {
        ty: Type::CmuWall,
        summary: "CMU wall: section (shells, grout, bond beams any course) or face-on (run x).",
        parts: "course_1..course_n (1 = bottom), bond_beam (the highest), bond_beam_1.. (each run of bond-beam courses from the bottom), grout",
        anchors: "9 box anchors + bond_beam_center, bond_beam_<k>_center, top_center, cell_center_top; cell_left, cell_center, cell_right (inside faces of the face shells and the cell middle at the wall bottom) and cell_left_top, cell_right_top: vertical path bars in the cell follow the wall width with {\"ref\": \"wall@cell_left\", \"offset\": [1.5, -12]}; run x: control_joint_<k>, control_joint_<k>_top",
        draws: "Box height = courses*8 - 0.375 (+0.375 with top_joint); the lowest course sits on the box bottom. Per course: two face shells (cut, cmu hatch), grouted cell (grout hatch) or an empty cell with the cross web as a beyond line, mortar joints as cut lines. A bond beam at a slab or ledger is one wall: {\"courses\": 7, \"bond_beams\": [4]}; solid grout below grade: {\"grout\": \"none\", \"grout_courses\": [[1, 3]]}. run x (fence and retaining wall elevations): {\"run\": \"x\", \"length\": 480, \"courses\": 9, \"bond_beam_courses\": 1, \"control_joints\": [240]} draws the face with bed and head joints (running bond) and 3/8\" control joints in an elevation view. 3D: 15.625\" units with 0.375\" head joints, running bond.",
        example: "{\"id\":\"cmu\",\"type\":\"cmu_wall\",\"width\":8,\"courses\":3,\"bond_beam_courses\":1,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
        traits: Traits { takes_voids: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Concrete,
        summary: "Cast-in-place concrete: rect/footing, polygon, slab_edge, plan footprint (L/T, 45 deg); `voids`, `pour`.",
        parts: "footing (turndown zone), slab (slab zone), base (when base is set) for slab_edge; footing for shape footing",
        anchors: "9 box anchors; polygon and footprint add vertex_1..vertex_n at the points (in `points` order; a footprint's at its top, plan views at their z) and edge_k_mid (middle of the edge from point k to the next; the arc middle for a bulge); slab_edge adds top_exterior (datum (0,0) at the exterior face), slab_top, footing_bottom_exterior, footing_bottom_interior, slab_bottom_interior, haunch_top, recess_bottom_exterior, recess_bottom_interior, recess_top_interior, base_bottom_interior and base_bottom_footing (with base)",
        draws: "slab_edge local origin: exterior face at x=0, top of slab at y=0, footing bottom at y=-footing_depth. Cut region with the material hatch; parts are zones (rebar place.in, cover), not separate outlines. `voids` (also on cmu_wall) subtracts members cast into it (an HSS column, base plate and grout in a closure pour: `[\"col\", \"plate\", \"grout\"]`) or shapes (block-outs) in section, iso and 3D, so the embedded steel shows with its own fill and the hole follows the member; a void that misses is W_VOID_MISS. footprint: a plan outline [x, z] extruded in y, for L/T footings, notched slabs, block-outs with 45-degree ends and angled walls, drawn exact in plan, section, elevation and iso: {\"shape\": \"footprint\", \"points\": [[0, 0], [48, 0], [48, -12], [12, -12], [12, -48], [0, -48]], \"height\": 12, \"top\": 0}. Pieces of one pour (slab + thickened edge + block-out fill) share \"pour\": \"slab\" and draw without joint lines.",
        example: "{\"id\":\"slab\",\"type\":\"concrete\",\"shape\":\"slab_edge\",\"slab_thickness\":4,\"footing_width\":12,\"footing_depth\":18,\"at\":{\"anchor\":\"top_exterior\",\"to\":[0,0]}}",
        traits: Traits { takes_voids: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Rebar,
        summary: "Reinforcing bar: a dot in section (along_z) or a line in the XY plane (path).",
        parts: "none",
        anchors: "9 box anchors of the bar (center = bar center for along_z); path bars add start, end, bend_k (on the bar at the middle of the bend at interior point k+1) and seg_k_mid (middle of the straight part of leg k), all on the centreline: aim notes and dims at these, not the box anchors of a bent bar",
        draws: "Default embedded:true. Cut dots draw solid; path bars draw as two parallel lines (bar outline) in pen rebar, filled solid when cut. 3D: swept circle. Natural z thickness = bar diameter. W_COVER is checked against the host concrete/cmu (in XY only: the cover of an xz bar along z is not checked). Plan views: bars along z and path bars below the cut draw as lines along their run, vertical legs and bars crossing the cut as dots, xz bars as their path. An xz corner bar: {\"id\":\"corner\",\"type\":\"rebar\",\"size\":\"#4\",\"plane\":\"xz\",\"y\":24,\"points\":[[4.5,-16],[4.5,-43.5],[32,-43.5]]}",
        example: "{\"id\":\"top_bar\",\"type\":\"rebar\",\"size\":\"#4\",\"at\":{\"anchor\":\"center\",\"to\":[6,-2.5]}}",
        traits: Traits { reinforcement: true, line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::AnchorBolt,
        summary: "Anchor bolt in the XY plane at a z (hook, nut, washer; leveling nut, adhesive).",
        parts: "shank, nut, washer (wedge adds clip; screw has threads, washer, head instead of nut; leveling_nut adds leveling_nut, leveling_washer; adhesive adds adhesive, the filled hole)",
        anchors: "9 box anchors + top_of_concrete (where the bolt meets the host top surface, local (0,0))",
        draws: "Embedded steel, natural z thickness = diameter. The placement anchor DEFAULTS to top_of_concrete, so `\"at\": {\"to\": \"cmu@top_center\"}` seats the bolt in the top surface, projection up (you do not set `anchor`). z defaults to the first section view's cut_z, so the bolt is cut, not hidden; set `z` only to move it. `array` {axis z, count, spacing} draws bolts @ spacing in iso/3D (one is cut in section). Through a base plate: `\"plate\": \"<id>\"` seats the washer and nut on the plate at the rod and fits `projection`; `leveling_nut: true` adds a nut and washer under the plate; `plate_washer` \"3x3x1/4\" replaces the round washer; `hook: \"adhesive\"` draws the drilled hole filled (grout hatch).",
        example: "{\"id\":\"anchor_bolt\",\"type\":\"anchor_bolt\",\"diameter\":0.5,\"embed\":7,\"projection\":2.75,\"hook\":\"J\",\"at\":{\"to\":[3,0]}}",
        traits: Traits { default_anchor: "top_of_concrete", ..Traits::NONE },
    },
    Entry {
        ty: Type::Connector,
        summary: "Steel hardware: straps (points or `on` a face), ties and angles from two `members`, hangers, post caps/bases, holdowns.",
        parts: "strap: none; tie, angle (from members): tie or angle; hanger: seat, side, flange (face mount) or back, top_flange; post_cap: seat, beam_side, post_side; post_base: base, seat, side; holdown: back, seat, side, screw",
        anchors: "9 box anchors of the resolved profile + (strap) vertex_1..vertex_n at the points (in `points` order), edge_k_mid (middle of the edge from point k to the next; the arc middle for a bulge); hanger and post_cap `seat`, member tie/angle `joint` (middle of the joint line), post_base `base_bottom`, holdown `seat_top` (the rod axis at the top of the seat, where the nut bears)",
        draws: "Pen steel; filled solid when cut, outline when beyond. Schematic: the note carries the model, the geometry shows location and path. `array` {axis z, count, spacing} draws the strap/tie @ spacing along the wall in iso/3D (section shows the cut one). Use named Refs (e.g. truss@heel_outer) in `points`, not literal offsets. Hangers, caps, bases, holdowns, member ties and `on` straps are plates at their true place in 3D (no at, z or points): section, plan, elevation, iso and the mesh agree; in a section that cuts one of their members they draw solid in front of what they overlap (`front`), elsewhere a nearer member hides them dashed and a note still lands on them; plates thinner than 0.022\" on paper are drawn that thick; plates seen face-on show nail dots. A sloped carried member needs its end cut at the face (`cuts` trim). Checks: W_UNKNOWN_MODEL, W_HANGER_SIZE, W_HOLDOWN_ROD, W_POST_BASE.",
        example: "{\"id\":\"hanger\",\"type\":\"connector\",\"model\":\"LUS210\",\"carried\":\"joist\",\"carrying\":\"header\"}",
        traits: Traits { hardware: true, line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Truss,
        summary: "Prefab wood truss: roof heel (pitched, scissor, mono), parallel_chord floor/flat truss, or end_on. Profiles and bearings: `kerf schema truss`.",
        parts: "top_chord, bottom_chord, heel_web (raised heel), end_vertical (mono, bearing face, parapet, parallel_chord), web (the web pattern: pitched/scissor/mono webs, parallel_chord diagonals, end_on web strips), plate (heel profiles), tail",
        anchors: "9 box anchors + bearing_outer (local origin: outer edge of bearing at bottom of bottom chord; top_chord bearing: at the underside of the top chord), bearing_inner, tail_bottom, tail_top, top_chord_at_bearing, top_chord_bottom_at_bearing (lower edge of the top chord at the bearing plane), heel_outer (middle of the heel's outer vertical face, between the bottom chord's top and the top chord's top at the bearing: where ties and straps land), heel_block_top (top of the top chord above the bearing's outer edge: `until` target for full-height eave blocking between trusses), top_chord_end, bottom_chord_top_inner; heel_joint (standard heel: where the top chord's underside meets the bottom chord's top), peak (top of the ridge, when drawn), bearing_far (far bearing's outer edge, whole truss drawn); end_vertical, end_top, end_bottom (mono, face, parapet, parallel_chord: the end vertical's outer face middle, top, bottom); parapet_top, parapet_top_inner (integral parapet); parallel_chord also bottom_chord_end. End-on: top_chord, top_chord_bottom, bottom_chord, bottom_chord_top, web_center (box middle across the plies)",
        draws: "Side: members in-plane, natural z thickness = chord thickness (1 1/2; 3 1/2 for 4x2 chords): set `z` and `array` for spacing. Standard heel: top chord lower edge passes through bearing_outer (the bearing's outer top corner) at the pitch and extends to the tail at x=-overhang; the bottom chord's end is cut along it. Raised: a heel vertical heel_height tall at the bearing. Scissor: bottom chord flat over bearing_width, then rising at bottom_pitch. Mono: end vertical heel_height tall at the bearing, top chord falling inward (`slope: \"@truss\"` follows it down). Webs (pitched, scissor, mono): the `webs` pattern at the panel points of `span` (schematic when span is unset), 3 1/2 wide, cut at span_shown; span_shown >= span draws the whole truss (ridge, far heel and tail). parapet: the end vertical runs up past the top chord. Parallel_chord: level chords `depth` apart, an end vertical at the bearing (top_chord bearing: just inside the support, the top chord over it), diagonals alternating down and up every web_spacing, break at span_shown. End_on: chord plies cut with the wood X (top chord taller by 1/cos(pitch)), a web strip between them behind the cut every web_spacing along z.",
        example: "{\"id\":\"truss\",\"type\":\"truss\",\"pitch\":\"4:12\",\"top_chord\":\"2x4\",\"bottom_chord\":\"2x4\",\"bearing_width\":7.25,\"overhang\":18,\"at\":{\"anchor\":\"bearing_outer\",\"to\":[0,0]}}",
        traits: Traits {
            has_pitch: true,
            slope_host: Some(SlopeHost { part: "top_chord", end_anchor: "top_chord_end" }),
            ..Traits::NONE
        },
    },
    Entry {
        ty: Type::Ijoist,
        summary: "Wood I-joist (depth x flange series): side, end-on, stiffeners. Floor/roof joists and I-joist blocking.",
        parts: "top_flange, web, bottom_flange, stiffener (with stiffeners), squash_block (with squash_blocks)",
        anchors: "9 box anchors (the joist only: (0,0)-(length or flange width, depth)). Run x: levels top_flange, top_flange_bottom, web_center, bottom_flange_top, bottom_flange at mid-length, each also <name>_start and <name>_end at the ends; bearing_left, bearing_right (underside at the ends). Run z: top_flange, bottom_flange (face centres), web_center, web_left, web_right (web faces), top_flange_tip_left/right (underside of the top flange at its edges), bottom_flange_tip_left/right (top of the bottom flange at its edges)",
        draws: "Run x (side): flange lines and the web outline, never hatched or grained, even where the section plane passes through the joist; stiffeners and squash blocks as rectangles at their ends. Run z (end-on): flanges cut with the wood X (blocking: one diagonal), the OSB web as a thin open strip, stiffeners/squash blocks cut beside it. True 3D slabs (iso, plan, mesh). W_BEARING: a run x joist bearing on a member under its end needs 1 3/4 in (3 1/2 in at an intermediate support).",
        example: "{\"id\":\"joist\",\"type\":\"ijoist\",\"size\":\"11-7/8 x 2-5/16\",\"length\":48,\"stiffeners\":\"left\",\"at\":{\"anchor\":\"bottom_left\",\"to\":\"plate@top_left\"}}",
        traits: Traits {
            near_miss: true,
            slope_host: Some(SlopeHost { part: "top_flange", end_anchor: "" }),
            min_bearing: [1.75, 3.5],
            ..Traits::NONE
        },
    },
    Entry {
        ty: Type::Membrane,
        summary: "Thin layers: underlayment, vapor retarder, WRB, roofing, flashing.",
        parts: "none",
        anchors: "9 box anchors of the resolved profile + vertex_1..vertex_n at the points (in `points` order), edge_k_mid (middle of the edge from point k to the next; the arc middle for a bulge)",
        draws: "Drawn as a line per the material pen (vapor retarder: dashed heavy; shingles: heavy line with tick marks). Never occludes or hatches.",
        example: "{\"id\":\"vapor\",\"type\":\"membrane\",\"material\":\"vapor_retarder\",\"points\":[[0,0],[48,0]]}",
        traits: Traits { sheet: true, line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Fill,
        summary: "Earth, gravel, sand, compacted fill: drawn under solids, never notched.",
        parts: "none",
        anchors: "9 box anchors of the polygon + vertex_1..vertex_n at the points (in `points` order) and edge_k_mid (middle of the edge from point k to the next; the arc middle for a bulge)",
        draws: "Hatched cut region, drawn under every solid the view cuts with it: its hatch and grade line stop at them (SPEC 26.4; `except` lists solids that do not clip it), and a fill over a solid is not W_OVERLAP. So earth around a footing and stem is one rectangle. The hatch stops at the crop and fills never get break lines.",
        example: "{\"id\":\"gravel\",\"type\":\"fill\",\"material\":\"gravel\",\"points\":[[0,0],[48,0],[48,-4],[0,-4]]}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Insulation,
        summary: "Rigid (hatched) or batt (loop symbol) insulation.",
        parts: "none",
        anchors: "9 box anchors; with points also vertex_1..vertex_n and edge_k_mid",
        draws: "Rigid: hatched region. Batt: sinusoidal loop line fitted to the rectangle.",
        example: "{\"id\":\"foam\",\"type\":\"insulation\",\"form\":\"rigid\",\"width\":2,\"height\":24}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Flashing,
        summary: "Sheet-metal flashing in section: Z, L, drip edge, weep screed or free polyline.",
        parts: "none",
        anchors: "9 box anchors + corner (first bend, local (0,0) for presets), start, end",
        draws: "Presets: corner at (0,0), wall surface x=0, exterior -x. Spans the document run along Z. Thin metal: solid fill + steel outline (SPEC 16). Embedded: drawn over hatch, exempt from W_OVERLAP. Place with at.anchor \"corner\".",
        example: "{\"id\":\"pan\",\"type\":\"flashing\",\"profile\":\"z\",\"at\":{\"anchor\":\"corner\",\"to\":[0,0]}}",
        traits: Traits { line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Joint,
        summary: "Concrete joints: expansion filler, saw-cut control notch, tooled edge, sealant on backer rod.",
        parts: "expansion: filler (+ sealant with cap); control: notch; tooled_edge: radius; sealant: bead, rod",
        anchors: "9 box anchors + joint_top (local (0,0): top surface at the joint centerline); tooled_edge adds corner",
        draws: "Local origin: top surface, joint centerline (tooled_edge: the sharp corner). Expansion: filler strip width x depth with the joint_filler hatch (place it in the gap between two concrete pieces). Control and tooled_edge are void shapes (embedded: they cut a hole in the host hatch; not in 3D/iso). Sealant: bead (filled) over a backer rod circle. Spans the document run along Z.",
        example: "{\"id\":\"ej\",\"type\":\"joint\",\"kind\":\"expansion\",\"width\":0.5,\"depth\":4,\"at\":{\"anchor\":\"joint_top\",\"to\":[0,0]}}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Sleeve,
        summary: "Pipe sleeve, packing and pipe; `through` cuts its hole in concrete or CMU.",
        parts: "sleeve, fill (with fill), pipe (with pipe)",
        anchors: "the 9 box anchors (center = the pipe axis for run z; the default placement anchor)",
        draws: "Run z: rings seen end-on, centered on the placement point; run x/y: the sleeve cut along its axis (walls as strips over `length`, natural z thickness = od). Cut sleeve and pipe walls take their material's fill (steel: solid), the packing its hatch, the pipe bore stays open. `through` subtracts the sleeve's outside from those hosts in section, iso and 3D (the same as naming the sleeve in the host's `voids`); a sleeve inside concrete without it is W_OVERLAP. A block-out with no sleeve is a host `voids` shape.",
        example: "{\"id\":\"sleeve\",\"type\":\"sleeve\",\"pipe\":4,\"fill\":\"joint_filler\",\"through\":\"stem\",\"at\":{\"to\":[36,26]}}",
        traits: Traits { default_anchor: "center", passes_through: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Steel,
        summary: "AISC steel: W, HSS (rect, square, round), pipe, angle, channel (C, MC), plate. Drawn true to the tabulated dimensions in section (end-on), elevation (side) or plan (top).",
        parts: "section: one piece (HSS and pipe: two `wall` halves); elevation/plan: top_flange, web, bottom_flange (W, channel), wall / tube (HSS, pipe), vertical_leg, horizontal_leg (angle), plate; hidden (dashed hidden lines, with hidden_lines)",
        anchors: "9 box anchors + per shape. Section: W top_flange, bottom_flange (face centers), web_center, web_left, web_right (web faces), top_flange_tip_left/right (underside of the top flange at its tips), bottom_flange_tip_left/right (top of the bottom flange at its tips); HSS/pipe inner_top, inner_bottom, inner_left, inner_right (inside wall faces); angle heel (outside corner), heel_inside, toe_vertical, toe_horizontal, vertical_leg_face, horizontal_leg_top; channel web_back, web_face, top_flange, bottom_flange, top_flange_tip, bottom_flange_tip; plate edge_left, edge_right, face_top, face_bottom. Elevation/plan: the levels across the member (W top_flange, top_flange_bottom, web_center, bottom_flange_top, bottom_flange; HSS/pipe inner_top, center_line, inner_bottom; plan W flange_tip_left, web_left, web_center, web_right, flange_tip_right ...) at mid-length, each also as <name>_start and <name>_end at the member ends",
        draws: "Local box: section (0,0)-(width, depth) of the cross-section, bottom-left at the flange tip / heel / outside corner. Section: cut => solid steel fill, beyond => outline (pen steel); W with k-fillets, HSS with corner radii. Elevation/plan: outlines only, never filled: the silhouette in the visible steel pen (a W in elevation also shows its flanges as lines), hidden faces omitted unless `hidden_lines`, a break line where the crop cuts the member off; the member is a stack of slabs at their true z, so iso/3D and occlusion are right. Natural z thickness in elevation/plan = the cross-section dimension along z; section spans the doc run unless `length`. Pipe and round HSS seen lengthwise are boxes in 3D (2.5D: use orient section with `z` for a round member in 3D).",
        example: "{\"id\":\"beam\",\"type\":\"steel\",\"shape\":\"W8x18\",\"orient\":\"elevation\",\"length\":96,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
        traits: Traits { shape_table: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Bolt,
        summary: "Bolt group (heavy hex, nut, washer) through steel plies, side or end-on; studs.",
        parts: "shank, head, washer, nut (one set per bolt); studs: weld, shank, washer and nut (weld_stud) or head (headed_stud)",
        anchors: "9 box anchors + head_bearing (local origin: underside of the head on bolt 1's axis; the default placement anchor), nut_bearing (side: the grip's far face), bolt_1..bolt_n (each bolt's axis at the head bearing); studs: head_bearing = weld_face (the welded end, local origin), nut_bearing (top of the grip)",
        draws: "Embedded steel (drawn over cut plates, exempt from W_OVERLAP). Side: shank d wide through the grip, head 1.5d+1/8 wide x 0.625d on top, washer 2d x 5/32 and nut 1.5d+1/8 x d under the grip, 0.5d stick-out; natural z thickness = d, centered on the first section view's cut_z. End: hex head toward the viewer, shank circle, washer and nut behind; natural z thickness = grip. Place with `\"at\": {\"to\": \"plate@face_top\"}` (head on the top ply) and set grip to the ply stack. Checks: W_BOLT_EDGE (edge distance to every steel ply below AISC 360 Table J3.4), W_BOLT_SPACING (spacing < 2 2/3 d).",
        example: "{\"id\":\"cap_bolts\",\"type\":\"bolt\",\"diameter\":0.75,\"grip\":0.875,\"count\":2,\"spacing\":3.5,\"at\":{\"to\":[0,0]}}",
        traits: Traits { default_anchor: "head_bearing", ..Traits::NONE },
    },
    Entry {
        ty: Type::Nailing,
        summary: "Nails/screws from their `members`: face, toenail, panel edge, field.",
        parts: "none",
        anchors: "the 9 box anchors of the fasteners' extent",
        draws: "Section, plan, elevation: a nail lying in the drawing plane is its shank with a head bar (toenails angled), one seen end-on is a dot (screws: a circle with a cross); in iso a dot on the face the heads are on. Placement comes from `members` (the first holds the heads): face finds the joint face of the first two and lays `rows` x `count` (or @ `spacing`) on it; edge rows run at `edge_distance` inside each panel edge and keep only nails with framing behind; field rows follow the intermediate framing. Own members never hide the nails, cut or not (end nails and toenails inside a cut block draw); a section draws the nails at or behind its cut that no other cut solid covers, and a row seen end-on where it crosses the cut. Several sheets side by side: list them all first in `members`. A note targeting it without `text` prints its text form. Checks: W_NAIL_MEMBERS (members do not touch), W_NAIL_EDGE (edge distance, edges without framing), W_NAIL_SPACING, W_NAIL_LENGTH (points out the far face, short penetration), W_NAIL_HIDDEN (a view draws none of them; in iso it names the face turned away or the piece covering the heads).",
        example: "{\"id\":\"splice_nails\",\"type\":\"nailing\",\"members\":[\"upper_plate\",\"lower_plate\"],\"size\":\"16d\",\"count\":8,\"rows\":2,\"through\":true}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Solid,
        summary: "Escape hatch: any extruded profile with an explicit material (flagged I_SOLID_USED).",
        parts: "none",
        anchors: "9 box anchors; profile.points also gives vertex_1..vertex_n and edge_k_mid",
        draws: "Use only when no typed component fits.",
        example: "{\"id\":\"angle\",\"type\":\"solid\",\"material\":\"steel\",\"profile\":{\"rect\":[3,0.25]}}",
        traits: Traits { escape_hatch: true, ..Traits::NONE },
    },
];

pub fn entry(t: Type) -> &'static Entry {
    &ENTRIES[t as usize]
}

pub fn find(name: &str) -> Option<&'static Entry> {
    Type::from_name(name).map(entry)
}

pub fn type_names() -> Vec<&'static str> {
    ENTRIES.iter().map(Entry::name).collect()
}

/// Is `key` an accepted top-level key of a component of type `ty`?
pub fn allowed_key(ty: &Entry, key: &str) -> bool {
    COMMON.iter().any(|c| c.0.contains(&key)) || builders::params_has_key(ty.ty, key)
}

/// Every accepted top-level key of a component of type `ty` (type params, then common fields).
pub fn allowed_key_names(ty: &Entry) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = ty.params().into_iter().flat_map(|p| p.names).collect();
    out.extend(COMMON.iter().flat_map(|c| c.0.iter().copied()));
    out
}

pub fn allowed_keys_text(ty: &Entry) -> String {
    let mut out = ty.params().iter().flat_map(|p| p.names.iter().copied()).collect::<Vec<_>>().join(", ");
    out.push_str(", + common: ");
    out.push_str(&COMMON.iter().map(|c| c.0[0]).collect::<Vec<_>>().join(", "));
    out
}

// ---- rendering -----------------------------------------------------------------------------------------

fn obj(items: Vec<(&str, Value)>) -> Value {
    Value::Object(items.into_iter().map(|(k, v)| Member { key: k.to_string(), value: v }).collect())
}

fn s(x: &str) -> Value {
    Value::String(x.to_string())
}

fn param_json(p: &Row) -> Value {
    obj(alloc::vec![("name", Value::String(p.name_text())), ("default", s(&p.def)), ("desc", s(p.desc))])
}

pub fn entry_json(e: &Entry) -> Value {
    let ps = Value::Array(e.params().iter().map(param_json).collect());
    let mut m = alloc::vec![("type", s(e.name())), ("summary", s(e.summary)), ("params", ps)];
    if e.traits.hardware {
        m.push(("models", Value::Array(HARDWARE.iter().map(|h| Value::String(hardware_line(h))).collect())));
        m.push(("families", Value::Array(SERIES.iter().map(|f| Value::String(series_line(f))).collect())));
    }
    if e.traits.shape_table {
        m.push(("shapes", s(&builders::steel_shapes::catalog_text())));
    }
    m.push(("parts", s(e.parts)));
    m.push(("anchors", s(e.anchors)));
    m.push(("draws", s(e.draws)));
    obj(m)
}

pub fn catalog_json() -> Value {
    obj(alloc::vec![
        ("kerf_catalog", s("0.1")),
        ("box_anchors", s(BOX_ANCHORS)),
        ("common", Value::Array(common().iter().map(param_json).collect())),
        ("types", Value::Array(ENTRIES.iter().map(entry_json).collect())),
    ])
}

pub fn append_entry_markdown(out: &mut String, e: &Entry) {
    out.push_str(&format!("### `{}`: {}\n\n", e.name(), e.summary));
    out.push_str("| param | default | notes |\n|---|---|---|\n");
    for p in e.params() {
        out.push_str(&format!("| `{}` | {} | {} |\n", p.name_text(), p.def, p.desc));
    }
    if e.traits.hardware {
        out.push_str("\nHardware models (auto-fill width and gauge):\n");
        for h in &HARDWARE {
            out.push_str(&format!("- {}\n", hardware_line(h)));
        }
        out.push_str("\nHardware families (a model starting with the prefix: kind, mount and gauge; sizes come from the members):\n");
        for f in &SERIES {
            out.push_str(&format!("- {}\n", series_line(f)));
        }
    }
    if e.traits.shape_table {
        out.push_str(&format!("\nShape table: {}\n", builders::steel_shapes::catalog_text()));
    }
    out.push_str(&format!("\n- Parts: {}\n- Anchors: {}\n- Draws: {}\n\n", e.parts, e.anchors, e.draws));
}

pub fn catalog_markdown() -> String {
    let mut out = String::from("# Kerf component catalog\n\n");
    out.push_str("Coordinates are inches, X right, Y up, Z toward the viewer; sections look along -Z. Lengths accept numbers or strings (\"7 5/8\", \"3'-4\\\"\").\n\n");
    out.push_str("## Common fields\n\n| field | default | notes |\n|---|---|---|\n");
    for p in common() {
        out.push_str(&format!("| `{}` | {} | {} |\n", p.name_text(), p.def, p.desc));
    }
    out.push_str(&format!("\nBox anchors: {BOX_ANCHORS}.\n\n## Types\n\n"));
    for e in &ENTRIES {
        append_entry_markdown(&mut out, e);
    }
    out
}
