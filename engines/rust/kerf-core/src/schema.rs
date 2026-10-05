//! Parameter schema for components, views and annotations.
//! One table drives canonicalization (`fmt`), parameter validation (`E_PARAM`) and the catalog text.

use crate::diag::{Diag, nearest};
use crate::num::{length_of, parse_scale, parse_slope, type_name};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug)]
pub enum K {
    Len,
    Num,
    Int,
    Bool,
    Str,
    Enum(&'static [&'static str]),
    Slope,
    Size,
    Points,
    Point,
    Z,
    At,
    Array,
    Place,
    Cover,
    Recess,
    Profile,
    Any,
}

pub struct P {
    pub name: &'static str,
    pub kind: K,
    pub def: &'static str,
    pub req: bool,
    pub doc: &'static str,
}

const fn p(name: &'static str, kind: K, def: &'static str, doc: &'static str) -> P {
    P { name, kind, def, req: false, doc }
}
const fn rq(name: &'static str, kind: K, doc: &'static str) -> P {
    P { name, kind, def: "", req: true, doc }
}

pub struct TypeSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub params: &'static [P],
    pub parts: &'static str,
    pub anchors: &'static str,
    pub draws: &'static str,
}

/// Common fields of every component, in canonical order after `id`, `type`.
pub const COMMON_HEAD: &[P] = &[
    p("label", K::Str, "null", "short human label used by summaries"),
    p("material", K::Str, "by type", "style material key; implied by type, override allowed"),
];
pub const COMMON_TAIL: &[P] = &[
    p("at", K::At, "origin", "placement: {anchor, to, offset}; anchor = this component's anchor (default bottom_left), to = Ref | {ref, offset} | [x, y]"),
    p("rotate", K::Num, "0", "degrees CCW about the placement anchor"),
    p("slope", K::Slope, "null", "\"4:12\" rise:run, rotates about the placement anchor (sloping up toward +X)"),
    p("mirror", K::Bool, "false", "mirror the profile about its local vertical centerline before placement"),
    p("z", K::Z, "by type", "[z0, z1] absolute, or a number = centered at that z with the member's natural thickness"),
    p("array", K::Array, "null", "{axis: z|x|y, count, spacing}: instances id#0..id#n-1"),
    p("embedded", K::Bool, "by type", "drawn over cut solids (rebar, anchor bolts, embedded straps)"),
    p("visible", K::Bool, "true", "false hides the component from views and mesh"),
];

const LUMBER_RUN: &[&str] = &["z", "x", "y"];
const PRODUCT: &[&str] = &["sawn", "lvl", "psl", "lsl", "glulam"];
const PANEL_MAT: &[&str] = &["osb", "plywood", "gypsum", "fiber_cement", "wood_board"];
const SIDE: &[&str] = &["left", "right"];
const MEMBRANE_MAT: &[&str] = &["underlayment", "vapor_retarder", "wrb", "shingles", "flashing_membrane"];
const FILL_MAT: &[&str] = &["earth", "gravel", "sand", "compacted_fill"];
const REBAR_SIZES: &[&str] = &["#3", "#4", "#5", "#6", "#7", "#8"];

pub const TYPES: &[TypeSpec] = &[
    TypeSpec {
        name: "lumber",
        summary: "sawn or engineered wood member",
        params: &[
            rq("size", K::Size, "sawn nominal \"2x4\"..\"2x12\", \"4x4\"..\"4x12\", \"6x6\"..\"6x12\"; or actual \"1.75x11.875\" (thickness x depth) when product is not sawn"),
            p("product", K::Enum(PRODUCT), "sawn", "sawn, lvl, psl, lsl, glulam"),
            p("run", K::Enum(LUMBER_RUN), "z", "axis the length runs along: z (seen in cross-section), x, y"),
            p("orient", K::Enum(&["upright", "flat"]), "upright", "run z only: upright = depth vertical, flat = depth horizontal"),
            p("face", K::Enum(&["wide", "narrow"]), "wide", "run x/y only: face seen looking -Z: wide = depth in plane, narrow = thickness in plane"),
            p("length", K::Len, "null", "required when run is x or y"),
            p("plies", K::Int, "1", "built-up member; plies stack along the thickness direction (X when upright, Y when flat; Z when run is x or y)"),
            p("treated", K::Bool, "false", "preservative treated (material wood_treated; W_UNTREATED_CONTACT otherwise)"),
            p("blocking", K::Bool, "false", "discontinuous member: cross-section mark is one diagonal instead of an X"),
            p("grade", K::Str, "null", "free text e.g. \"#2 DF-L\" for notes"),
        ],
        parts: "none (plies are not separate parts)",
        anchors: "9 box anchors of the profile",
        draws: "Cut across the grain (run z): outline + X mark per ply. Cut lengthwise or beyond: outline only. Z extent: run z = document run; run x/y = ply-stack thickness centered on the middle of run.",
    },
    TypeSpec {
        name: "panel",
        summary: "sheathing, boards, gypsum, fascia/trim boards",
        params: &[
            p("material", K::Enum(PANEL_MAT), "osb", "osb, plywood, gypsum, fiber_cement, wood_board"),
            rq("thickness", K::Len, "panel thickness, e.g. 0.4375 (7/16\"), 0.46875 (15/32\"), 0.5"),
            rq("length", K::Len, "in-plane extent"),
            p("run", K::Enum(&["x", "y"]), "x", "in-plane direction of length before rotation"),
        ],
        parts: "none",
        anchors: "9 box anchors (run x: length x thickness; run y: thickness x length). Use slope for roof sheathing.",
        draws: "Thin rectangle; spans the document run in Z.",
    },
    TypeSpec {
        name: "cmu_wall",
        summary: "concrete masonry wall in section",
        params: &[
            p("width", K::Len, "8", "nominal 6, 8, 10, 12 = actual 5.625, 7.625, 9.625, 11.625"),
            rq("courses", K::Int, "number of 8\" courses (7.625 unit + 0.375 mortar joint)"),
            p("bond_beam_courses", K::Int, "0", "top N courses are bond-beam units (grouted, horizontal bars)"),
            p("grout", K::Enum(&["solid", "reinforced", "none"]), "reinforced", "solid or reinforced: cells grouted at the cut; none: hollow"),
            p("face_shell", K::Len, "1.25", "face shell thickness drawn in section"),
            p("top_joint", K::Bool, "false", "include a mortar joint above the top course"),
            p("cover", K::Cover, "{sides 1.5, top 1.5, bottom 0.5}", "required clear cover for rebar (W_COVER); cover.parts.<part> overrides"),
        ],
        parts: "course_1..course_n (1 = bottom), bond_beam, grout",
        anchors: "9 box anchors + bond_beam_center, top_center, cell_center_top",
        draws: "Per course: two face shells (cmu hatch), cell (grout hatch when grouted), mortar joints. Box = width x (courses*8 - 0.375 [+0.375 top_joint]); bottom of course 1 at box bottom.",
    },
    TypeSpec {
        name: "concrete",
        summary: "cast-in-place concrete: rect, footing, polygon, slab_edge",
        params: &[
            rq("shape", K::Enum(&["rect", "polygon", "slab_edge", "footing"]), "shape builder"),
            p("material", K::Str, "concrete", "style material"),
            p("cover", K::Cover, "{bottom 3, sides 3, top 1.5}", "REQUIRED clear cover for bars in this host (W_COVER); cover.parts.<part> overrides per part"),
            p("width", K::Len, "null", "rect/footing: width (required)"),
            p("height", K::Len, "null", "rect/footing: height (required)"),
            p("points", K::Points, "null", "polygon: outline points (required)"),
            p("exterior", K::Enum(SIDE), "left", "slab_edge: which side is the exterior edge"),
            p("slab_thickness", K::Len, "4", "slab_edge"),
            p("slab_length", K::Len, "48", "slab_edge: drawn from exterior face inward"),
            p("footing_width", K::Len, "12", "slab_edge: bottom width of turndown"),
            p("footing_depth", K::Len, "18", "slab_edge: top of slab to bottom of footing"),
            p("haunch", K::Num, "45", "slab_edge: inner face slope from horizontal in degrees; 90 = vertical inner face"),
            p("recess", K::Recess, "null", "slab_edge: {width, depth, from_edge} depression at top exterior edge; from_edge = distance from exterior face to recess start; depth measured at the interior end"),
            p("recess_slope", K::Len, "0", "slab_edge: fall of the recess floor toward the exterior, inches over its width"),
        ],
        parts: "rect/footing: footing. slab_edge: footing (turndown zone x 0..footing_width, y -footing_depth..-slab_thickness), slab (x 0..slab_length, y -slab_thickness..0)",
        anchors: "9 box anchors. slab_edge adds: top_exterior (datum 0,0), slab_top, footing_bottom_exterior, footing_bottom_interior, slab_bottom_interior, haunch_top, recess_bottom_exterior, recess_bottom_interior, recess_top_interior. Local origin: exterior face x=0, top of slab y=0 (exterior right mirrors).",
        draws: "Concrete hatch; spans the document run in Z.",
    },
    TypeSpec {
        name: "rebar",
        summary: "reinforcing bar: dot in section (along_z) or bar in the XY plane (path)",
        params: &[
            p("size", K::Str, "#4", "#3 .375, #4 .5, #5 .625, #6 .75, #7 .875, #8 1.0"),
            p("mode", K::Enum(&["along_z", "path"]), "along_z", "along_z: continuous bar seen as a dot; path: bar in the XY plane"),
            p("place", K::Place, "null", "cover-based placement (preferred): {in: \"<comp>[.<part>]\", face: bottom|top|left|right, cover, count, side_cover}; bars at clear cover from face, spread evenly between the zone's adjacent faces at side_cover (count 1 = centered)"),
            p("points", K::Points, "null", "mode path: polyline (bends become fillets)"),
            p("bend_radius", K::Len, "3*d_b", "mode path: inside bend radius"),
            p("spacing_note", K::Str, "null", "e.g. \"#4 @ 16\\\" O.C.\" used in summaries"),
        ],
        parts: "place with count>1: bar_1..bar_n",
        anchors: "9 box anchors of the bar(s)",
        draws: "Cut dots draw solid-filled; path bars draw as two parallel lines (pen rebar). Default embedded: true. Z: along_z spans the run; path bars are one bar-diameter thick centered at z (or mid-run).",
    },
    TypeSpec {
        name: "anchor_bolt",
        summary: "anchor bolt with hook, nut and washer",
        params: &[
            p("diameter", K::Len, "0.5", "0.5 or 0.625 typical"),
            rq("embed", K::Len, "length below the placement point"),
            rq("projection", K::Len, "length above the placement point"),
            p("hook", K::Enum(&["J", "L", "headed", "none"]), "J", "hook shape; hook leg length 3\""),
            p("nut_washer", K::Bool, "true", "draw nut and washer at the top"),
        ],
        parts: "none",
        anchors: "9 box anchors + top_of_concrete (where the bolt meets the host top surface; the local origin)",
        draws: "In-plane member at a given z (centered on the run mid by default). Default embedded: true.",
    },
    TypeSpec {
        name: "connector",
        summary: "schematic steel hardware: straps, ties, embedded anchors",
        params: &[
            p("model", K::Str, "null", "e.g. \"MSTA36\", \"H2.5A\", \"HETA20\", \"CS16\": fills width, gauge, length"),
            rq("points", K::Points, "polyline: bearing face (lay edge) or centerline (lay face)"),
            p("lay", K::Enum(&["edge", "face"]), "edge", "edge: seen edge-on, thickness (gauge) in plane, grows to side; width along Z. face: seen face-on, width in plane centered on the polyline"),
            p("side", K::Enum(SIDE), "left", "lay edge: side of the polyline direction the thickness grows (left of a left-to-right line = up)"),
            p("gauge", K::Int, "18", "12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359"),
            p("width", K::Len, "1.25", "extent along Z (lay edge) or in plane (lay face)"),
            p("fasteners", K::Str, "null", "text e.g. \"(10) 10d EA. END\" for notes"),
        ],
        parts: "none",
        anchors: "9 box anchors of the resolved profile (no local frame)",
        draws: "Thickened polyline, pen steel, solid-filled when cut. Schematic: the note carries the model.",
    },
    TypeSpec {
        name: "truss",
        summary: "prefab wood truss heel in side view",
        params: &[
            p("exterior", K::Enum(SIDE), "left", "side of the heel/overhang"),
            p("pitch", K::Slope, "4:12", "top chord pitch"),
            p("top_chord", K::Size, "2x4", "sawn nominal size"),
            p("bottom_chord", K::Size, "2x4", "sawn nominal size"),
            p("heel", K::Enum(&["standard", "raised"]), "standard", "heel type"),
            p("heel_height", K::Len, "null", "raised heel: vertical height at the bearing outer edge (top of bottom chord to top of top chord)"),
            p("bearing_width", K::Len, "3.5", "width of support under the heel"),
            p("overhang", K::Len, "12", "horizontal distance from outer face of bearing to the tail end"),
            p("tail", K::Enum(&["plumb", "square"]), "plumb", "tail cut"),
            p("span_shown", K::Len, "48", "how far into the building to draw (break line at the end)"),
            p("plate", K::Bool, "true", "draw truss plate outline at the heel (dashed)"),
        ],
        parts: "top_chord, bottom_chord, heel_web (raised only), plate, tail",
        anchors: "9 box anchors + bearing_outer (local origin: outer edge of bearing, bottom of bottom chord), bearing_inner, tail_bottom, tail_top, top_chord_at_bearing, top_chord_end, bottom_chord_top_inner",
        draws: "Members in plane; thickness 1.5 along Z centered on mid-run (use z and array for spacing). Chords are lengthwise: outline only.",
    },
    TypeSpec {
        name: "membrane",
        summary: "thin layers: underlayment, vapor retarder, WRB, roofing",
        params: &[
            p("material", K::Enum(MEMBRANE_MAT), "underlayment", "underlayment, vapor_retarder, wrb, shingles, flashing_membrane"),
            rq("points", K::Points, "polyline"),
            p("thickness", K::Len, "by material", "draw thickness"),
            p("side", K::Enum(SIDE), "left", "side of the polyline direction the thickness grows"),
        ],
        parts: "none",
        anchors: "9 box anchors of the resolved profile",
        draws: "Per style pen (vapor retarder: dashed heavy line; shingles: heavy band).",
    },
    TypeSpec {
        name: "fill",
        summary: "earth, gravel, sand, compacted fill",
        params: &[
            p("material", K::Enum(FILL_MAT), "earth", "earth, gravel, sand, compacted_fill"),
            rq("points", K::Points, "polygon"),
            p("outline", K::Enum(&["top", "full", "none"]), "top", "top: only the upper edge chain is stroked (grade line)"),
            p("grade_label", K::Str, "null", "optional grade text"),
        ],
        parts: "none",
        anchors: "9 box anchors of the polygon",
        draws: "Hatched per material; never gets break lines.",
    },
    TypeSpec {
        name: "insulation",
        summary: "rigid or batt insulation",
        params: &[
            p("form", K::Enum(&["rigid", "batt"]), "rigid", "rigid is hatched; batt draws the batt symbol"),
            p("width", K::Len, "null", "rect form"),
            p("height", K::Len, "null", "rect form"),
            p("points", K::Points, "null", "polygon alternative to width/height"),
        ],
        parts: "none",
        anchors: "9 box anchors",
        draws: "Rigid: hatch. Batt: sinusoidal loop line fitted to the rectangle.",
    },
    TypeSpec {
        name: "solid",
        summary: "escape hatch: any profile extruded along Z (flagged for review)",
        params: &[
            rq("material", K::Str, "style material key (e.g. aluminum, steel, generic)"),
            rq("profile", K::Profile, "{rect: [w, h]} | {circle: d} | {points: [...]}"),
        ],
        parts: "none",
        anchors: "9 box anchors",
        draws: "Use only when no typed component fits; the summary flags it (I_SOLID_USED).",
    },
];

pub fn type_spec(name: &str) -> Option<&'static TypeSpec> {
    TYPES.iter().find(|t| t.name == name)
}

pub fn type_names() -> Vec<&'static str> {
    TYPES.iter().map(|t| t.name).collect()
}

/// Param table of a component type: common head, type params (overriding common by name), common tail.
pub fn all_params(t: &TypeSpec) -> Vec<&'static P> {
    let mut out: Vec<&'static P> = Vec::new();
    for c in COMMON_HEAD {
        if !t.params.iter().any(|x| x.name == c.name) {
            out.push(c);
        }
    }
    for x in t.params {
        out.push(x);
    }
    for c in COMMON_TAIL {
        if !t.params.iter().any(|x| x.name == c.name) {
            out.push(c);
        }
    }
    out
}

pub fn kind_name(k: &K) -> String {
    match k {
        K::Len => "length".into(),
        K::Num => "number".into(),
        K::Int => "integer".into(),
        K::Bool => "boolean".into(),
        K::Str => "string".into(),
        K::Enum(vs) => vs.join("|"),
        K::Slope => "slope".into(),
        K::Size => "size".into(),
        K::Points => "points".into(),
        K::Point => "[x, y]".into(),
        K::Z => "z".into(),
        K::At => "placement".into(),
        K::Array => "array".into(),
        K::Place => "rebar place".into(),
        K::Cover => "cover".into(),
        K::Recess => "recess".into(),
        K::Profile => "profile".into(),
        K::Any => "any".into(),
    }
}

// ---------------------------------------------------------------------------------------------
// Canonicalization + validation
// ---------------------------------------------------------------------------------------------

type Errs<'a> = &'a mut Vec<Diag>;

fn perr(errs: Errs, path: &str, msg: String, fix: Option<String>) {
    let mut d = Diag::error("E_PARAM", msg).path(path.to_string());
    if let Some(f) = fix {
        d = d.fix(f);
    }
    errs.push(d);
}

fn canon_len(v: &Value, path: &str, errs: Errs) -> Value {
    match length_of(v) {
        Ok(x) if x.abs() > 1.0e6 || !x.is_finite() => {
            perr(errs, path, format!("{}: {} is not a plausible length in inches (limit 1,000,000)", path, crate::json::compact(v)), None);
            v.clone()
        }
        Ok(x) => crate::json::num(crate::num::r4(x)),
        Err(e) => {
            perr(errs, path, format!("{}: {}", path, e), None);
            v.clone()
        }
    }
}

fn canon_point(v: &Value, path: &str, errs: Errs) -> Value {
    match v.as_array() {
        Some(a) if a.len() == 2 => Value::Array(vec![canon_len(&a[0], &format!("{}/0", path), errs), canon_len(&a[1], &format!("{}/1", path), errs)]),
        _ => {
            perr(errs, path, format!("{}: expected a point [x, y] (inches), got {}", path, type_name(v)), None);
            v.clone()
        }
    }
}

fn canon_ref_obj(v: &Value, path: &str, errs: Errs) -> Value {
    // {ref, offset}
    if let Some(m) = v.as_object() {
        let mut out = Map::new();
        if let Some(r) = m.get("ref") {
            if !r.is_string() {
                perr(errs, &format!("{}/ref", path), format!("{}/ref: expected a Ref string like \"sill_plate@top_left\"", path), None);
            }
            out.insert("ref".into(), r.clone());
        } else {
            perr(errs, path, format!("{}: object points need a \"ref\" string (and optional \"offset\": [dx, dy])", path), None);
        }
        if let Some(o) = m.get("offset") {
            out.insert("offset".into(), canon_point(o, &format!("{}/offset", path), errs));
        }
        for (k, x) in m {
            if k != "ref" && k != "offset" {
                perr(errs, &format!("{}/{}", path, k), format!("{}/{}: unknown key (allowed: ref, offset)", path, k), None);
                out.insert(k.clone(), x.clone());
            }
        }
        Value::Object(out)
    } else {
        v.clone()
    }
}

fn canon_pointlike(v: &Value, path: &str, errs: Errs) -> Value {
    match v {
        Value::String(_) => v.clone(),
        Value::Object(_) => canon_ref_obj(v, path, errs),
        Value::Array(a) => {
            if a.len() == 2 || a.len() == 3 {
                let mut out = vec![canon_len(&a[0], &format!("{}/0", path), errs), canon_len(&a[1], &format!("{}/1", path), errs)];
                if a.len() == 3 {
                    match a[2].as_f64() {
                        Some(_) => out.push(a[2].clone()),
                        None => perr(errs, &format!("{}/2", path), format!("{}/2: bulge must be a number (tan(angle/4))", path), None),
                    }
                }
                Value::Array(out)
            } else {
                perr(errs, path, format!("{}: a literal point is [x, y] or [x, y, bulge]", path), None);
                v.clone()
            }
        }
        _ => {
            perr(errs, path, format!("{}: expected [x, y], a Ref string, or {{\"ref\": ..., \"offset\": [dx, dy]}}", path), None);
            v.clone()
        }
    }
}

fn canon_points(v: &Value, path: &str, errs: Errs) -> Value {
    match v.as_array() {
        Some(a) => Value::Array(a.iter().enumerate().map(|(i, x)| canon_pointlike(x, &format!("{}/{}", path, i), errs)).collect()),
        None => {
            perr(errs, path, format!("{}: expected an array of points, got {}", path, type_name(v)), None);
            v.clone()
        }
    }
}

/// Reorder an object's keys: known keys in `order`, then unknown keys alphabetically.
pub fn order_keys(m: &Map<String, Value>, order: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for k in order {
        if let Some(v) = m.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    let mut rest: Vec<&String> = m.keys().filter(|k| !order.contains(&k.as_str())).collect();
    rest.sort();
    for k in rest {
        out.insert(k.clone(), m[k].clone());
    }
    out
}

fn check_allowed(m: &Map<String, Value>, allowed: &[&str], path: &str, errs: Errs) {
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            let hint = nearest(k, allowed.iter().copied()).map(|n| format!(" Did you mean \"{}\"?", n)).unwrap_or_default();
            perr(errs, &format!("{}/{}", path, k), format!("{}/{}: unknown field. Allowed: {}.{}", path, k, allowed.join(", "), hint), None);
        }
    }
}

fn canon_fields(v: &Value, path: &str, spec: &[(&'static str, K)], errs: Errs) -> Value {
    let Some(m) = v.as_object() else {
        perr(errs, path, format!("{}: expected an object with fields {}", path, spec.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")), None);
        return v.clone();
    };
    let names: Vec<&str> = spec.iter().map(|s| s.0).collect();
    check_allowed(m, &names, path, errs);
    let mut out = Map::new();
    for (k, kind) in spec {
        if let Some(x) = m.get(*k) {
            out.insert((*k).to_string(), canon_kind(x, kind, &format!("{}/{}", path, k), errs));
        }
    }
    for (k, x) in m {
        if !names.contains(&k.as_str()) {
            out.insert(k.clone(), x.clone());
        }
    }
    Value::Object(order_keys(&out, &names))
}

pub fn canon_kind(v: &Value, kind: &K, path: &str, errs: Errs) -> Value {
    match kind {
        K::Len => canon_len(v, path, errs),
        K::Num => {
            if v.is_number() {
                v.clone()
            } else {
                perr(errs, path, format!("{}: expected a number, got {}", path, type_name(v)), None);
                v.clone()
            }
        }
        K::Int => match v.as_f64() {
            Some(x) if x.fract() == 0.0 && x.abs() <= 100000.0 => v.clone(),
            _ => {
                perr(errs, path, format!("{}: expected an integer, got {}", path, crate::json::compact(v)), None);
                v.clone()
            }
        },
        K::Bool => {
            if v.is_boolean() {
                v.clone()
            } else {
                perr(errs, path, format!("{}: expected true or false, got {}", path, type_name(v)), None);
                v.clone()
            }
        }
        K::Str | K::Size => {
            if v.is_string() {
                v.clone()
            } else {
                perr(errs, path, format!("{}: expected a string, got {}", path, type_name(v)), None);
                v.clone()
            }
        }
        K::Enum(vals) => match v.as_str() {
            Some(s) if vals.contains(&s) => v.clone(),
            Some(s) => {
                let hint = nearest(s, vals.iter().copied()).map(|n| format!(" Did you mean \"{}\"?", n)).unwrap_or_default();
                perr(errs, path, format!("{}: \"{}\" is not allowed. Allowed values: {}.{}", path, s, vals.join(", "), hint), None);
                v.clone()
            }
            None => {
                perr(errs, path, format!("{}: expected one of {}, got {}", path, vals.join(", "), type_name(v)), None);
                v.clone()
            }
        },
        K::Slope => {
            if let Err(e) = parse_slope(v) {
                perr(errs, path, format!("{}: {}", path, e), None);
            }
            v.clone()
        }
        K::Points => canon_points(v, path, errs),
        K::Point => canon_point(v, path, errs),
        K::Z => match v {
            Value::Array(a) if a.len() == 2 => Value::Array(vec![canon_len(&a[0], &format!("{}/0", path), errs), canon_len(&a[1], &format!("{}/1", path), errs)]),
            Value::Array(_) => {
                perr(errs, path, format!("{}: z must be [z0, z1] or a single number", path), None);
                v.clone()
            }
            _ => canon_len(v, path, errs),
        },
        K::At => {
            let Some(m) = v.as_object() else {
                perr(errs, path, format!("{}: placement must be an object {{\"anchor\": ..., \"to\": Ref | [x, y], \"offset\": [dx, dy]}}", path), None);
                return v.clone();
            };
            check_allowed(m, &["anchor", "to", "offset"], path, errs);
            let mut out = Map::new();
            if let Some(a) = m.get("anchor") {
                if !a.is_string() {
                    perr(errs, &format!("{}/anchor", path), format!("{}/anchor: expected an anchor name such as bottom_left", path), None);
                }
                out.insert("anchor".into(), a.clone());
            }
            if let Some(t) = m.get("to") {
                let ct = match t {
                    Value::String(_) => t.clone(),
                    Value::Object(_) => canon_ref_obj(t, &format!("{}/to", path), errs),
                    Value::Array(_) => canon_point(t, &format!("{}/to", path), errs),
                    _ => {
                        perr(errs, &format!("{}/to", path), format!("{}/to: expected a Ref string (\"comp@anchor\"), {{\"ref\", \"offset\"}} or [x, y]", path), None);
                        t.clone()
                    }
                };
                out.insert("to".into(), ct);
            }
            if let Some(o) = m.get("offset") {
                out.insert("offset".into(), canon_point(o, &format!("{}/offset", path), errs));
            }
            for (k, x) in m {
                if !["anchor", "to", "offset"].contains(&k.as_str()) {
                    out.insert(k.clone(), x.clone());
                }
            }
            Value::Object(order_keys(&out, &["anchor", "to", "offset"]))
        }
        K::Array => canon_fields(
            v,
            path,
            &[("axis", K::Enum(&["z", "x", "y"])), ("count", K::Int), ("spacing", K::Len)],
            errs,
        ),
        K::Place => canon_fields(
            v,
            path,
            &[("in", K::Str), ("face", K::Enum(&["bottom", "top", "left", "right"])), ("cover", K::Len), ("count", K::Int), ("side_cover", K::Len)],
            errs,
        ),
        K::Recess => canon_fields(v, path, &[("width", K::Len), ("depth", K::Len), ("from_edge", K::Len)], errs),
        K::Cover => {
            let Some(m) = v.as_object() else {
                perr(errs, path, format!("{}: cover must be an object {{bottom, sides, top, parts}}", path), None);
                return v.clone();
            };
            let mut out = Map::new();
            check_allowed(m, &["bottom", "sides", "top", "parts"], path, errs);
            for k in ["bottom", "sides", "top"] {
                if let Some(x) = m.get(k) {
                    out.insert(k.into(), canon_len(x, &format!("{}/{}", path, k), errs));
                }
            }
            if let Some(parts) = m.get("parts") {
                if let Some(pm) = parts.as_object() {
                    let mut po = Map::new();
                    for (pk, pv) in pm {
                        po.insert(
                            pk.clone(),
                            canon_fields(pv, &format!("{}/parts/{}", path, pk), &[("bottom", K::Len), ("sides", K::Len), ("top", K::Len)], errs),
                        );
                    }
                    out.insert("parts".into(), Value::Object(po));
                } else {
                    perr(errs, &format!("{}/parts", path), format!("{}/parts: expected an object keyed by part name", path), None);
                }
            }
            for (k, x) in m {
                if !["bottom", "sides", "top", "parts"].contains(&k.as_str()) {
                    out.insert(k.clone(), x.clone());
                }
            }
            Value::Object(order_keys(&out, &["bottom", "sides", "top", "parts"]))
        }
        K::Profile => {
            let Some(m) = v.as_object() else {
                perr(errs, path, format!("{}: profile must be {{\"rect\": [w, h]}} | {{\"circle\": d}} | {{\"points\": [...]}}", path), None);
                return v.clone();
            };
            let mut out = Map::new();
            let mut n = 0;
            for (k, x) in m {
                match k.as_str() {
                    "rect" => {
                        n += 1;
                        out.insert(k.clone(), canon_point(x, &format!("{}/rect", path), errs));
                    }
                    "circle" => {
                        n += 1;
                        out.insert(k.clone(), canon_len(x, &format!("{}/circle", path), errs));
                    }
                    "points" => {
                        n += 1;
                        out.insert(k.clone(), canon_points(x, &format!("{}/points", path), errs));
                    }
                    _ => perr(errs, &format!("{}/{}", path, k), format!("{}/{}: unknown profile key (use rect, circle or points)", path, k), None),
                }
            }
            if n != 1 {
                perr(errs, path, format!("{}: give exactly one of rect, circle, points", path), None);
            }
            Value::Object(out)
        }
        K::Any => v.clone(),
    }
}

/// Canonicalize one component object. Returns the canonical object (keys ordered per schema).
pub fn canon_component(v: &Value, path: &str, errs: Errs) -> Value {
    let Some(m) = v.as_object() else {
        perr(errs, path, format!("{}: a component must be an object", path), None);
        return v.clone();
    };
    let tname = m.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let Some(spec) = type_spec(tname) else {
        let names = type_names();
        let hint = nearest(tname, names.iter().copied()).map(|n| format!(" Did you mean \"{}\"?", n)).unwrap_or_default();
        perr(errs, &format!("{}/type", path), format!("{}/type: unknown component type \"{}\". Allowed: {}.{}", path, tname, names.join(", "), hint), None);
        return v.clone();
    };
    let params = all_params(spec);
    let mut allowed: Vec<&str> = vec!["id", "type"];
    allowed.extend(params.iter().map(|p| p.name));
    check_allowed(m, &allowed, path, errs);
    let mut out = Map::new();
    for k in ["id", "type"] {
        if let Some(x) = m.get(k) {
            out.insert(k.to_string(), x.clone());
        }
    }
    for pr in &params {
        if let Some(x) = m.get(pr.name) {
            if x.is_null() {
                continue;
            }
            out.insert(pr.name.to_string(), canon_kind(x, &pr.kind, &format!("{}/{}", path, pr.name), errs));
        } else if pr.req {
            perr(
                errs,
                &format!("{}/{}", path, pr.name),
                format!("{}/{}: required for type {} ({}).", path, pr.name, tname, pr.doc),
                Some(format!("add \"{}\"", pr.name)),
            );
        }
    }
    for (k, x) in m {
        if !allowed.contains(&k.as_str()) {
            out.insert(k.clone(), x.clone());
        }
    }
    let mut order: Vec<&str> = vec!["id", "type"];
    order.extend(params.iter().map(|p| p.name));
    Value::Object(order_keys(&out, &order))
}

pub const VIEW_KEYS: &[&str] = &["id", "kind", "number", "title", "scale", "cut_z", "crop", "from", "cutaway", "notes_side", "omit", "annotations"];

fn canon_cite(v: &Value, path: &str, errs: Errs) -> Value {
    canon_fields(
        v,
        path,
        &[("code", K::Str), ("edition", K::Int), ("section", K::Str), ("title", K::Str), ("status", K::Enum(&["suggested", "verified"]))],
        errs,
    )
}

pub fn canon_annotation(v: &Value, path: &str, errs: Errs) -> Value {
    let Some(m) = v.as_object() else {
        perr(errs, path, format!("{}: an annotation must be an object", path), None);
        return v.clone();
    };
    let t = m.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let spec: Vec<(&'static str, K)> = match t {
        "note" => vec![
            ("id", K::Str),
            ("type", K::Str),
            ("text", K::Str),
            ("target", K::Str),
            ("at", K::Any),
            ("place", K::Point),
            ("cite", K::Any),
        ],
        "dim" => vec![
            ("id", K::Str),
            ("type", K::Str),
            ("from", K::Any),
            ("to", K::Any),
            ("dir", K::Enum(&["h", "v", "aligned"])),
            ("offset", K::Len),
            ("text", K::Str),
        ],
        "label" => vec![("id", K::Str), ("type", K::Str), ("text", K::Str), ("at", K::Any), ("offset", K::Point)],
        _ => {
            perr(
                errs,
                &format!("{}/type", path),
                format!("{}/type: unknown annotation type \"{}\". Allowed: note, dim, label.", path, t),
                None,
            );
            return v.clone();
        }
    };
    let names: Vec<&str> = spec.iter().map(|s| s.0).collect();
    check_allowed(m, &names, path, errs);
    let mut out = Map::new();
    for (k, kind) in &spec {
        if let Some(x) = m.get(*k) {
            if x.is_null() {
                continue;
            }
            let cv = match *k {
                "at" | "from" | "to" => match x {
                    Value::String(_) => x.clone(),
                    Value::Object(_) => canon_ref_obj(x, &format!("{}/{}", path, k), errs),
                    Value::Array(_) => canon_point(x, &format!("{}/{}", path, k), errs),
                    _ => {
                        perr(errs, &format!("{}/{}", path, k), format!("{}/{}: expected a Ref string, {{\"ref\", \"offset\"}} or [x, y]", path, k), None);
                        x.clone()
                    }
                },
                "cite" => match x.as_array() {
                    Some(a) => Value::Array(a.iter().enumerate().map(|(i, c)| canon_cite(c, &format!("{}/cite/{}", path, i), errs)).collect()),
                    None => {
                        perr(errs, &format!("{}/cite", path), format!("{}/cite: expected an array of citations", path), None);
                        x.clone()
                    }
                },
                _ => canon_kind(x, kind, &format!("{}/{}", path, k), errs),
            };
            out.insert((*k).to_string(), cv);
        }
    }
    for k in ["id", "type", "text"] {
        if k == "text" && t == "dim" {
            continue;
        }
        if !m.contains_key(k) {
            perr(errs, &format!("{}/{}", path, k), format!("{}/{}: required", path, k), None);
        }
    }
    match t {
        "note" if !m.contains_key("target") && !m.contains_key("at") => {
            perr(errs, path, format!("{}: a note needs a \"target\" (component id or comp.part) or an \"at\" Ref", path), None)
        }
        "dim" => {
            for k in ["from", "to"] {
                if !m.contains_key(k) {
                    perr(errs, &format!("{}/{}", path, k), format!("{}/{}: required (a Ref such as \"footing@bottom_left\")", path, k), None);
                }
            }
        }
        "label" if !m.contains_key("at") => perr(errs, &format!("{}/at", path), format!("{}/at: required (a Ref or [x, y])", path), None),
        _ => {}
    }
    for (k, x) in m {
        if !names.contains(&k.as_str()) {
            out.insert(k.clone(), x.clone());
        }
    }
    Value::Object(order_keys(&out, &names))
}

pub fn canon_view(v: &Value, path: &str, errs: Errs) -> Value {
    let Some(m) = v.as_object() else {
        perr(errs, path, format!("{}: a view must be an object", path), None);
        return v.clone();
    };
    check_allowed(m, VIEW_KEYS, path, errs);
    let mut out = Map::new();
    let kind = m.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if !["section", "iso"].contains(&kind) {
        perr(errs, &format!("{}/kind", path), format!("{}/kind: must be \"section\" or \"iso\" (got {:?})", path, kind), None);
    }
    for k in VIEW_KEYS {
        let Some(x) = m.get(*k) else { continue };
        if x.is_null() {
            continue;
        }
        let kp = format!("{}/{}", path, k);
        let cv = match *k {
            "cut_z" => canon_len(x, &kp, errs),
            "crop" => {
                let mut ok = true;
                let mut o = Map::new();
                if let Some(cm) = x.as_object() {
                    check_allowed(cm, &["x", "y"], &kp, errs);
                    for ax in ["x", "y"] {
                        match cm.get(ax).and_then(|a| a.as_array()) {
                            Some(a) if a.len() == 2 => {
                                o.insert(ax.into(), Value::Array(vec![canon_len(&a[0], &format!("{}/{}/0", kp, ax), errs), canon_len(&a[1], &format!("{}/{}/1", kp, ax), errs)]));
                            }
                            _ => ok = false,
                        }
                    }
                } else {
                    ok = false;
                }
                if !ok {
                    perr(errs, &kp, format!("{}: crop must be {{\"x\": [x0, x1], \"y\": [y0, y1]}} in inches", kp), None);
                    x.clone()
                } else {
                    Value::Object(o)
                }
            }
            "scale" => {
                match x.as_str() {
                    Some(s) => {
                        if let Err(e) = parse_scale(s) {
                            perr(errs, &kp, format!("{}: {}", kp, e), None);
                        }
                    }
                    None => perr(errs, &kp, format!("{}: scale must be a string like \"1-1/2\\\"=1'-0\\\"\" or \"NTS\"", kp), None),
                }
                x.clone()
            }
            "from" => canon_kind(x, &K::Enum(&["front_right", "front_left", "back_right", "back_left"]), &kp, errs),
            "cutaway" => canon_kind(x, &K::Bool, &kp, errs),
            "omit" => match x.as_array() {
                Some(a) if a.iter().all(|e| e.is_string()) => x.clone(),
                _ => {
                    perr(errs, &kp, format!("{}: omit must be an array of component id strings, e.g. [\"roofing\"]", kp), None);
                    x.clone()
                }
            },
            "notes_side" => canon_kind(x, &K::Enum(&["right", "left", "both"]), &kp, errs),
            "annotations" => match x.as_array() {
                Some(a) => Value::Array(a.iter().enumerate().map(|(i, an)| canon_annotation(an, &format!("{}/{}", kp, i), errs)).collect()),
                None => {
                    perr(errs, &kp, format!("{}: expected an array of annotations", kp), None);
                    x.clone()
                }
            },
            "number" | "title" | "id" => {
                if let Some(n) = x.as_f64().filter(|_| *k == "number") {
                    Value::String(crate::num::fmt_num(n))
                } else {
                    x.clone()
                }
            }
            _ => x.clone(),
        };
        out.insert((*k).to_string(), cv);
    }
    for (k, x) in m {
        if !VIEW_KEYS.contains(&k.as_str()) {
            out.insert(k.clone(), x.clone());
        }
    }
    for k in ["id", "kind", "scale"] {
        if !m.contains_key(k) {
            perr(errs, &format!("{}/{}", path, k), format!("{}/{}: required", path, k), None);
        }
    }
    if kind == "section" && !m.contains_key("cut_z") {
        // cut_z defaults to the middle of run; not an error.
    }
    Value::Object(order_keys(&out, VIEW_KEYS))
}

pub const META_KEYS: &[&str] = &["author", "discipline", "classification", "jurisdiction", "tags", "forked_from", "sheet", "date"];
pub const DOC_KEYS: &[&str] = &["kerf", "id", "title", "meta", "run", "components", "views"];

pub fn canon_meta(v: &Value, path: &str, errs: Errs) -> Value {
    let Some(m) = v.as_object() else {
        perr(errs, path, format!("{}: meta must be an object", path), None);
        return v.clone();
    };
    let mut out = Map::new();
    for (k, x) in m {
        let cx = match k.as_str() {
            "classification" => match x.as_object() {
                Some(c) => Value::Object(order_keys(c, &["uniformat", "masterformat"])),
                None => x.clone(),
            },
            "jurisdiction" => match x.as_object() {
                Some(c) => Value::Object(order_keys(c, &["code", "edition"])),
                None => x.clone(),
            },
            _ => x.clone(),
        };
        out.insert(k.clone(), cx);
    }
    Value::Object(order_keys(&out, META_KEYS))
}

/// Canonicalize a whole document value, collecting E_PARAM diagnostics. Does not check ids/refs.
pub fn canon_doc(doc: &Value, errs: Errs) -> Value {
    let Some(m) = doc.as_object() else {
        perr(errs, "doc", "doc: a Kerf document must be a JSON object with kerf, id, title, run, components, views".to_string(), None);
        return doc.clone();
    };
    check_allowed(m, DOC_KEYS, "doc", errs);
    let mut out = Map::new();
    for k in DOC_KEYS {
        let Some(x) = m.get(*k) else { continue };
        let cv = match *k {
            "run" => match x.as_array() {
                Some(a) if a.len() == 2 => Value::Array(vec![canon_len(&a[0], "run/0", errs), canon_len(&a[1], "run/1", errs)]),
                _ => {
                    perr(errs, "run", "run: expected [z0, z1] in inches".to_string(), None);
                    x.clone()
                }
            },
            "meta" => canon_meta(x, "meta", errs),
            "components" => match x.as_array() {
                Some(a) => Value::Array(a.iter().enumerate().map(|(i, c)| {
                    let id = c.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()).unwrap_or_else(|| i.to_string());
                    canon_component(c, &format!("components/{}", id), errs)
                }).collect()),
                None => {
                    perr(errs, "components", "components: expected an array".to_string(), None);
                    x.clone()
                }
            },
            "views" => match x.as_array() {
                Some(a) => Value::Array(a.iter().enumerate().map(|(i, c)| {
                    let id = c.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()).unwrap_or_else(|| i.to_string());
                    canon_view(c, &format!("views/{}", id), errs)
                }).collect()),
                None => {
                    perr(errs, "views", "views: expected an array".to_string(), None);
                    x.clone()
                }
            },
            _ => x.clone(),
        };
        out.insert((*k).to_string(), cv);
    }
    for (k, x) in m {
        if !DOC_KEYS.contains(&k.as_str()) {
            out.insert(k.clone(), x.clone());
        }
    }
    Value::Object(order_keys(&out, DOC_KEYS))
}

pub fn valid_id(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    it.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn rebar_sizes() -> &'static [&'static str] {
    REBAR_SIZES
}
