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
    Membrane,
    Fill,
    Insulation,
    Flashing,
    Joint,
    Steel,
    Bolt,
    Solid,
}

impl Type {
    pub const ALL: [Type; 16] = [
        Type::Lumber,
        Type::Panel,
        Type::CmuWall,
        Type::Concrete,
        Type::Rebar,
        Type::AnchorBolt,
        Type::Connector,
        Type::Truss,
        Type::Membrane,
        Type::Fill,
        Type::Insulation,
        Type::Flashing,
        Type::Joint,
        Type::Steel,
        Type::Bolt,
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
            Type::Membrane => "membrane",
            Type::Fill => "fill",
            Type::Insulation => "insulation",
            Type::Flashing => "flashing",
            Type::Joint => "joint",
            Type::Steel => "steel",
            Type::Bolt => "bolt",
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
pub const HARDWARE: [Hardware; 55] = [
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
];

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
        "{anchor (default bottom_left), to: Ref | {ref, offset} | [x,y], offset: [dx,dy]}: translates the part so its anchor lands on `to`+offset. Point-list types (connector, membrane, fill, rebar path, concrete polygon, solid points) ignore `anchor`: literal [x,y] entries are relative to `to`+offset, Refs are absolute",
    ),
    (&["rotate"], "0", "degrees CCW about the placement point"),
    (
        &["slope"],
        "null",
        "\"4:12\" rise:run, degrees, or \"@<component>\" (e.g. \"@truss\": inherit that member's slope, so pitch lives in ONE place; a truss with exterior right slopes the other way). Rotation about the placement point; adds to rotate",
    ),
    (&["mirror"], "false", "mirror the profile about its vertical centerline before placement"),
    (
        &["z"],
        "null",
        "[z0,z1] absolute, or a number = centered there with the member's natural z thickness. Default: spans doc `run` (members along Z); members with a natural z thickness (lumber run x/y, truss, connector, path rebar, anchor_bolt) are centered on the first section view's `cut_z` (when it sets one, so they are cut, not hidden), else on the middle of `run`",
    ),
    (
        &["array"],
        "null",
        "{axis: x|y|z, count, spacing}: instance k offset by k*spacing; instances are id#0..id#n-1, refs to `id` mean instance 0, `id#k@anchor` addresses instance k",
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
        "dashed = \"where occurs\" graphics: all edges in the hidden (dashed) pen, no hatch or cut mark, never hides anything, exempt from W_FLOATING / W_NEAR_MISS / W_OVERLAP; notes targeting it get \" (WHERE OCCURS)\" appended. Section views only (iso omits it)",
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

pub static ENTRIES: [Entry; 16] = [
    Entry {
        ty: Type::Lumber,
        summary: "Sawn or engineered wood member (stud, plate, joist, beam, blocking, post). Standard view for a beam in a wall (flush beam, header): an ELEVATION along the wall, i.e. the beam seen lengthwise (run x, face wide) with the top and bottom plates interrupted where they butt it and king/jack studs (run y) at its ends. Draw the end-on section (run z, beam cut) only when the designer asks for it; either way, say in your report which reading you drew.",
        parts: "barrier (when set)",
        anchors: "the 9 box anchors",
        draws: "Section: run z cut => outline + wood X mark per ply (blocking: one diagonal); run x/y cut lengthwise => outline only; beyond => outline. Actual sizes: 2x 1.5 thick; 4x 3.5; 6x 5.5; depths x4 3.5, x6 5.5, x8 7.25, x10 9.25, x12 11.25 (6x8 7.5, 6x10 9.5, 6x12 11.5). Natural z thickness for run x/y.",
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
        summary: "Sheathing, boards, gypsum, soffit, fascia/trim boards as a thin rectangle.",
        parts: "none",
        anchors: "the 9 box anchors",
        draws: "Cut rectangle with the material's hatch / cut mark (wood_board: one diagonal). Spans the document run along Z. Use `slope` for roof sheathing (rotates about the placement anchor).",
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
        summary: "Concrete masonry wall in section: face shells, grouted cells, mortar joints, bond beam.",
        parts: "course_1..course_n (1 = bottom), bond_beam, grout",
        anchors: "9 box anchors + bond_beam_center, top_center, cell_center_top",
        draws: "Box height = courses*8 - 0.375 (+0.375 with top_joint); the lowest course sits on the box bottom. Per course: two face shells (cut, cmu hatch), grouted cell (grout hatch) or an empty cell with the cross web as a beyond line, mortar joints as cut lines. 3D: 15.625\" units with 0.375\" head joints, running bond.",
        example: "{\"id\":\"cmu\",\"type\":\"cmu_wall\",\"width\":8,\"courses\":3,\"bond_beam_courses\":1,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Concrete,
        summary: "Cast-in-place concrete: rect/footing, free polygon, or a monolithic slab with turned-down edge.",
        parts: "footing (turndown zone), slab (slab zone), base (when base is set) for slab_edge; footing for shape footing",
        anchors: "9 box anchors; slab_edge adds top_exterior (datum (0,0) at the exterior face), slab_top, footing_bottom_exterior, footing_bottom_interior, slab_bottom_interior, haunch_top, recess_bottom_exterior, recess_bottom_interior, recess_top_interior, base_bottom_interior and base_bottom_footing (with base)",
        draws: "slab_edge local origin: exterior face at x=0, top of slab at y=0, footing bottom at y=-footing_depth. Cut region with the material hatch; parts are zones (rebar place.in, cover), not separate outlines.",
        example: "{\"id\":\"slab\",\"type\":\"concrete\",\"shape\":\"slab_edge\",\"slab_thickness\":4,\"footing_width\":12,\"footing_depth\":18,\"at\":{\"anchor\":\"top_exterior\",\"to\":[0,0]}}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Rebar,
        summary: "Reinforcing bar: a dot in section (along_z) or a line in the XY plane (path).",
        parts: "none",
        anchors: "9 box anchors of the bar (center = bar center for along_z)",
        draws: "Default embedded:true. Cut dots draw solid; path bars draw as two parallel lines (bar outline) in pen rebar, filled solid when cut. 3D: swept circle. Natural z thickness = bar diameter. W_COVER is checked against the host concrete/cmu.",
        example: "{\"id\":\"top_bar\",\"type\":\"rebar\",\"size\":\"#4\",\"at\":{\"anchor\":\"center\",\"to\":[6,-2.5]}}",
        traits: Traits { reinforcement: true, line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::AnchorBolt,
        summary: "Anchor bolt in the XY plane at a given z (shank, hook, nut and washer).",
        parts: "shank, nut, washer (wedge adds clip; screw has threads, washer, head instead of nut)",
        anchors: "9 box anchors + top_of_concrete (where the bolt meets the host top surface, local (0,0))",
        draws: "Embedded steel, natural z thickness = diameter. The placement anchor DEFAULTS to top_of_concrete, so `\"at\": {\"to\": \"cmu@top_center\"}` seats the bolt in the top surface, projection up (you do not set `anchor`). z defaults to the first section view's cut_z, so the bolt is cut, not hidden; set `z` only to move it. `array` {axis z, count, spacing} draws bolts @ spacing in iso/3D (one is cut in section).",
        example: "{\"id\":\"anchor_bolt\",\"type\":\"anchor_bolt\",\"diameter\":0.5,\"embed\":7,\"projection\":2.75,\"hook\":\"J\",\"at\":{\"to\":[3,0]}}",
        traits: Traits { default_anchor: "top_of_concrete", ..Traits::NONE },
    },
    Entry {
        ty: Type::Connector,
        summary: "Schematic steel hardware: straps, ties, embedded anchors, drawn as a thickened polyline.",
        parts: "none",
        anchors: "9 box anchors of the resolved profile",
        draws: "Pen steel; filled solid when cut, outline when beyond. Schematic: the note carries the model, the geometry shows location and path. `array` {axis z, count, spacing} draws the strap/tie @ spacing along the wall in iso/3D (section shows the cut one). Use named Refs (e.g. truss@heel_outer) in `points`, not literal offsets.",
        example: "{\"id\":\"tie\",\"type\":\"connector\",\"model\":\"H2.5A\",\"lay\":\"face\",\"points\":[[0,0],[0,6]]}",
        traits: Traits { hardware: true, line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Truss,
        summary: "Prefab wood truss heel and tail in side view.",
        parts: "top_chord, bottom_chord, heel_web (raised only), plate, tail",
        anchors: "9 box anchors + bearing_outer (local origin: outer edge of bearing at bottom of bottom chord), bearing_inner, tail_bottom, tail_top, top_chord_at_bearing, top_chord_bottom_at_bearing (lower edge of the top chord at the bearing plane), heel_outer (middle of the heel's outer vertical face, between the bottom chord's top and the top chord's top at the bearing: where ties and straps land), top_chord_end, bottom_chord_top_inner",
        draws: "Standard heel: bottom chord from the bearing outer edge inward; top chord lower edge passes through (bearing_outer.x, top of bottom chord) at the pitch and extends to the plumb tail at x=-overhang. Members are in-plane, z thickness 1.5: set `z` and `array` for spacing.",
        example: "{\"id\":\"truss\",\"type\":\"truss\",\"pitch\":\"4:12\",\"top_chord\":\"2x4\",\"bottom_chord\":\"2x4\",\"bearing_width\":7.25,\"overhang\":18,\"at\":{\"anchor\":\"bearing_outer\",\"to\":[0,0]}}",
        traits: Traits {
            has_pitch: true,
            slope_host: Some(SlopeHost { part: "top_chord", end_anchor: "top_chord_end" }),
            ..Traits::NONE
        },
    },
    Entry {
        ty: Type::Membrane,
        summary: "Thin layers: underlayment, vapor retarder, WRB, roofing, flashing.",
        parts: "none",
        anchors: "9 box anchors of the resolved profile",
        draws: "Drawn as a line per the material pen (vapor retarder: dashed heavy; shingles: heavy line with tick marks). Never occludes or hatches.",
        example: "{\"id\":\"vapor\",\"type\":\"membrane\",\"material\":\"vapor_retarder\",\"points\":[[0,0],[48,0]]}",
        traits: Traits { sheet: true, line_like: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Fill,
        summary: "Earth, gravel, sand, compacted fill as a hatched polygon.",
        parts: "none",
        anchors: "9 box anchors of the polygon",
        draws: "Hatched cut region; the hatch stops at the crop and fills never get break lines.",
        example: "{\"id\":\"gravel\",\"type\":\"fill\",\"material\":\"gravel\",\"points\":[[0,0],[48,0],[48,-4],[0,-4]]}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Insulation,
        summary: "Rigid (hatched) or batt (loop symbol) insulation.",
        parts: "none",
        anchors: "9 box anchors",
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
        summary: "Concrete joints: expansion filler strip, control (saw-cut) notch, tooled edge radius, sealant bead on backer rod.",
        parts: "expansion: filler (+ sealant with cap); control: notch; tooled_edge: radius; sealant: bead, rod",
        anchors: "9 box anchors + joint_top (local (0,0): top surface at the joint centerline); tooled_edge adds corner",
        draws: "Local origin: top surface, joint centerline (tooled_edge: the sharp corner). Expansion: filler strip width x depth with the joint_filler hatch (place it in the gap between two concrete pieces). Control and tooled_edge are void shapes (embedded: they cut a hole in the host hatch; not in 3D/iso). Sealant: bead (filled) over a backer rod circle. Spans the document run along Z.",
        example: "{\"id\":\"ej\",\"type\":\"joint\",\"kind\":\"expansion\",\"width\":0.5,\"depth\":4,\"at\":{\"anchor\":\"joint_top\",\"to\":[0,0]}}",
        traits: Traits::NONE,
    },
    Entry {
        ty: Type::Steel,
        summary: "Structural steel shape from the AISC table: W, HSS (rect, square, round), pipe, angle, channel (C, MC), plate. Drawn true to the tabulated dimensions in section (end-on), elevation (side) or plan (top).",
        parts: "section: one piece (HSS and pipe: two `wall` halves); elevation/plan: top_flange, web, bottom_flange (W, channel), wall / tube (HSS, pipe), vertical_leg, horizontal_leg (angle), plate; hidden (dashed hidden lines, with hidden_lines)",
        anchors: "9 box anchors + per shape. Section: W top_flange, bottom_flange (face centers), web_center, web_left, web_right (web faces), top_flange_tip_left/right (underside of the top flange at its tips), bottom_flange_tip_left/right (top of the bottom flange at its tips); HSS/pipe inner_top, inner_bottom, inner_left, inner_right (inside wall faces); angle heel (outside corner), heel_inside, toe_vertical, toe_horizontal, vertical_leg_face, horizontal_leg_top; channel web_back, web_face, top_flange, bottom_flange, top_flange_tip, bottom_flange_tip; plate edge_left, edge_right, face_top, face_bottom. Elevation/plan: the levels across the member (W top_flange, top_flange_bottom, web_center, bottom_flange_top, bottom_flange; HSS/pipe inner_top, center_line, inner_bottom; plan W flange_tip_left, web_left, web_center, web_right, flange_tip_right ...) at mid-length, each also as <name>_start and <name>_end at the member ends",
        draws: "Local box: section (0,0)-(width, depth) of the cross-section, bottom-left at the flange tip / heel / outside corner. Section: cut => solid steel fill, beyond => outline (pen steel); W with k-fillets, HSS with corner radii. Elevation/plan: outlines only, never filled: the silhouette in the visible steel pen (a W in elevation also shows its flanges as lines), hidden faces omitted unless `hidden_lines`, a break line where the crop cuts the member off; the member is a stack of slabs at their true z, so iso/3D and occlusion are right. Natural z thickness in elevation/plan = the cross-section dimension along z; section spans the doc run unless `length`. Pipe and round HSS seen lengthwise are boxes in 3D (2.5D: use orient section with `z` for a round member in 3D).",
        example: "{\"id\":\"beam\",\"type\":\"steel\",\"shape\":\"W8x18\",\"orient\":\"elevation\",\"length\":96,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
        traits: Traits { shape_table: true, ..Traits::NONE },
    },
    Entry {
        ty: Type::Bolt,
        summary: "Structural bolt group: heavy hex bolts with nut and washer through steel plies, seen from the side (axis in-plane) or end-on (hex head).",
        parts: "shank, head, washer, nut (one set per bolt)",
        anchors: "9 box anchors + head_bearing (local origin: underside of the head on bolt 1's axis; the default placement anchor), nut_bearing (side: the grip's far face), bolt_1..bolt_n (each bolt's axis at the head bearing)",
        draws: "Embedded steel (drawn over cut plates, exempt from W_OVERLAP). Side: shank d wide through the grip, head 1.5d+1/8 wide x 0.625d on top, washer 2d x 5/32 and nut 1.5d+1/8 x d under the grip, 0.5d stick-out; natural z thickness = d, centered on the first section view's cut_z. End: hex head toward the viewer, shank circle, washer and nut behind; natural z thickness = grip. Place with `\"at\": {\"to\": \"plate@face_top\"}` (head on the top ply) and set grip to the ply stack. Checks: W_BOLT_EDGE (edge distance to every steel ply below AISC 360 Table J3.4), W_BOLT_SPACING (spacing < 2 2/3 d).",
        example: "{\"id\":\"cap_bolts\",\"type\":\"bolt\",\"diameter\":0.75,\"grip\":0.875,\"count\":2,\"spacing\":3.5,\"at\":{\"to\":[0,0]}}",
        traits: Traits { default_anchor: "head_bearing", ..Traits::NONE },
    },
    Entry {
        ty: Type::Solid,
        summary: "Escape hatch: any extruded profile with an explicit material (flagged I_SOLID_USED).",
        parts: "none",
        anchors: "9 box anchors",
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
