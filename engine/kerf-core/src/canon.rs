//! Canonical document JSON (SPEC 16): 2-space indent, schema key order, unknown keys alphabetical.

use crate::catalog;
use crate::json::{KeyCtx, Pretty, Value};
use crate::schema_fields::{self as schema, Object};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const META_KEYS: [&str; 10] = [
    "author",
    "discipline",
    "classification",
    "jurisdiction",
    "requested",
    "references",
    "tags",
    "forked_from",
    "sheet",
    "date",
];
const REFERENCE_KEYS: [&str; 2] = ["path", "caption"];
const CLASS_KEYS: [&str; 2] = ["uniformat", "masterformat"];
const JURIS_KEYS: [&str; 2] = ["code", "edition"];
const COMMON_KEYS: [&str; 15] = [
    "id",
    "type",
    "label",
    "material",
    "at",
    "rotate",
    "slope",
    "mirror",
    "z",
    "array",
    "embedded",
    "visible",
    "views",
    "shown",
    "acknowledge",
];
const REF_KEYS: [&str; 2] = ["ref", "offset"];
const RECESS_KEYS: [&str; 3] = ["width", "depth", "from_edge"];
const COVER_KEYS: [&str; 4] = ["bottom", "sides", "top", "parts"];
const PLACE_KEYS: [&str; 7] = ["in", "face", "cover", "count", "side_cover", "axis", "station"];
const PROFILE_KEYS: [&str; 3] = ["rect", "circle", "points"];
const CROP_KEYS: [&str; 2] = ["x", "y"];

fn keys_of(o: &Object) -> Vec<String> {
    o.fields.iter().map(|f| f.name.to_string()).collect()
}

fn strs(keys: &[&str]) -> Vec<String> {
    keys.iter().map(alloc::string::ToString::to_string).collect()
}

fn type_of(ctx: &KeyCtx) -> String {
    ctx.obj.iter().find(|m| m.key == "type").and_then(|m| m.value.str()).unwrap_or("").to_string()
}

/// Canonical key order of a component: the common keys, then the type's own parameters.
fn component_keys(ctx: &KeyCtx) -> Vec<String> {
    let mut keys = strs(&COMMON_KEYS);
    if let Some(e) = catalog::find(&type_of(ctx)) {
        for p in e.params() {
            keys.extend(p.names.iter().map(alloc::string::ToString::to_string));
        }
    }
    keys
}

pub fn order(ctx: &KeyCtx) -> Vec<String> {
    let p = ctx.path;
    let Some(&last) = p.last() else { return keys_of(&schema::DOC) };
    if p.len() == 1 {
        return if last == "meta" { strs(&META_KEYS) } else { Vec::new() };
    }
    if p.len() == 2 && p[0] == "meta" {
        if last == "classification" {
            return strs(&CLASS_KEYS);
        }
        if last == "jurisdiction" {
            return strs(&JURIS_KEYS);
        }
    }
    if p.len() == 3 && p[0] == "meta" && p[1] == "references" && last == "[]" {
        return strs(&REFERENCE_KEYS);
    }
    if p.len() == 2 && p[0] == "components" && last == "[]" {
        return component_keys(ctx);
    }
    if p.len() == 2 && p[0] == "views" && last == "[]" {
        return keys_of(&schema::VIEW);
    }
    if p.len() == 2 && p[0] == "layouts" && last == "[]" {
        return keys_of(&schema::LAYOUT);
    }
    if p.len() == 3 && p[0] == "components" {
        match last {
            "at" => return keys_of(&schema::AT),
            "array" => return keys_of(&schema::ARRAY),
            "recess" => return strs(&RECESS_KEYS),
            "cover" => return strs(&COVER_KEYS),
            "place" => return strs(&PLACE_KEYS),
            "profile" => return strs(&PROFILE_KEYS),
            _ => {}
        }
    }
    if p.len() == 4 && p[0] == "components" && p[2] == "acknowledge" && last == "[]" {
        return keys_of(&schema::ACK);
    }
    if p[0] == "components" && p.len() >= 3 {
        if ctx.obj.iter().any(|m| m.key == "ref") {
            return strs(&REF_KEYS);
        }
        if p.len() == 4 && p[2] == "at" && last == "to" {
            return strs(&REF_KEYS);
        }
    }
    if p[0] == "views" {
        if p.len() == 3 && last == "crop" {
            return strs(&CROP_KEYS);
        }
        if p.len() == 4 && p[2] == "annotations" && last == "[]" {
            return match type_of(ctx).as_str() {
                "dim" => keys_of(&schema::DIM),
                "label" => keys_of(&schema::LABEL),
                "weld" => keys_of(&schema::WELD),
                "table" => keys_of(&schema::TABLE),
                "ref" => keys_of(&schema::REF),
                _ => keys_of(&schema::NOTE),
            };
        }
        if p.len() == 5 && p[2] == "annotations" && last == "cite" {
            return Vec::new();
        }
        if p.len() == 6 && p[4] == "cite" && last == "[]" {
            return keys_of(&schema::CITE);
        }
        if p.len() >= 5 && p[2] == "annotations" && ctx.obj.iter().any(|m| m.key == "ref") {
            return strs(&REF_KEYS);
        }
    }
    Vec::new()
}

/// Canonical text of a document (with trailing newline).
pub fn write(doc: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pw = Pretty::new(&mut out);
    pw.order = Some(order);
    pw.write(doc, 0);
    out.push(b'\n');
    out
}
