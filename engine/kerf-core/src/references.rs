//! `meta.references`: the drawings a detail was redrawn from, as image files inside the workspace folder
//! (`[{"path": "refs/0123_exterior_wall_at_stem.png", "caption": "..."}]`).
//!
//! The engine cannot see files. It checks the shape and the path text (relative, inside the folder, a type the UI can
//! show); the host (`kerf check`, `kerf serve`) looks each path up and passes the ones it could not resolve as the
//! `reference_files` input of `check`.

use crate::json::Value;
use crate::model::{Diags, Level};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "gif", "svg", "pdf"];
const KEYS: [&str; 2] = ["path", "caption"];
const CODE: &str = "W_REFERENCE_FILE";
const SHAPE_FIX: &str = "e.g. \"meta\": {\"references\": [{\"path\": \"refs/0123_exterior_wall_at_stem.png\", \"caption\": \"EXTERIOR WALL AT STEM\"}]} (paths relative to the document's folder)";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathProblem {
    Invalid,
    NotRelative,
    Escapes,
    Backslash,
    Extension,
}

impl PathProblem {
    pub fn describe(self) -> &'static str {
        match self {
            PathProblem::Invalid => "is empty or contains a control character",
            PathProblem::NotRelative => {
                "is not relative to the document's folder (an absolute path, a drive letter or a URL)"
            }
            PathProblem::Escapes => "climbs out of the document's folder (a \"..\" segment)",
            PathProblem::Backslash => "uses backslashes",
            PathProblem::Extension => {
                "is not a file type the reference panel shows (png, jpg, jpeg, webp, gif, svg, pdf)"
            }
        }
    }
}

/// What is wrong with a reference path as text, before any file is looked up; `None` for a usable relative path.
pub fn path_problem(p: &str) -> Option<PathProblem> {
    if p.is_empty() || p.chars().any(char::is_control) {
        return Some(PathProblem::Invalid);
    }
    if p.starts_with(['/', '\\']) || p.contains(':') {
        return Some(PathProblem::NotRelative);
    }
    if p.split(['/', '\\']).any(|s| s == "..") {
        return Some(PathProblem::Escapes);
    }
    if p.contains('\\') {
        return Some(PathProblem::Backslash);
    }
    let name = p.rsplit('/').next().unwrap_or("");
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    if !EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(ext)) {
        return Some(PathProblem::Extension);
    }
    None
}

fn entries(doc: &Value) -> &[Value] {
    doc.get("meta").and_then(|m| m.get("references")).and_then(Value::arr).unwrap_or(&[])
}

fn path_of(entry: &Value) -> Option<&str> {
    entry.get("path").and_then(Value::str).filter(|p| !p.is_empty())
}

/// The `path` of every entry that has one, in document order (index into `meta.references`, path).
pub fn paths(doc: &Value) -> Vec<(usize, &str)> {
    entries(doc).iter().enumerate().filter_map(|(i, e)| path_of(e).map(|p| (i, p))).collect()
}

/// The paths a host should look up: those without a `path_problem`, each once, in document order.
pub fn lookups(doc: &Value) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for (_, p) in paths(doc) {
        if path_problem(p).is_none() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

fn base_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("snip.png")
}

fn problem_fix(p: &str, problem: PathProblem) -> String {
    match problem {
        PathProblem::Invalid => {
            "write the image's path relative to the document's folder, e.g. \"refs/0123_exterior_wall_at_stem.png\""
                .to_string()
        }
        PathProblem::NotRelative | PathProblem::Escapes => format!(
            "copy the image into the workspace folder and write its path relative to the document, e.g. \"refs/{}\"",
            base_name(p)
        ),
        PathProblem::Backslash => format!("write it with forward slashes: \"{}\"", p.replace('\\', "/")),
        PathProblem::Extension => {
            "save the snip as PNG (or JPG, WEBP, GIF, SVG, PDF) and point path at that file".to_string()
        }
    }
}

fn shape_error(diags: &Diags, path: &str, message: String) {
    diags.add_fix(Level::Error, "E_PARAM", None, Some(path), message, SHAPE_FIX.to_string());
}

/// Shape of `meta.references` (E_PARAM), unknown entry keys (W_UNKNOWN_KEY) and path text (W_REFERENCE_FILE).
pub(crate) fn lint(doc: &Value, diags: &Diags) {
    let Some(rv) = doc.get("meta").and_then(|m| m.get("references")) else { return };
    let Some(items) = rv.arr() else {
        if !rv.is_null() {
            shape_error(
                diags,
                "meta/references",
                format!("'meta.references' must be an array of {{path, caption}} objects, got a {}", rv.kind_name()),
            );
        }
        return;
    };
    for (i, it) in items.iter().enumerate() {
        let base = format!("meta/references/{i}");
        let Value::Object(members) = it else {
            shape_error(
                diags,
                &base,
                format!("'meta.references' entry {i} must be an object {{path, caption}}, got a {}", it.kind_name()),
            );
            continue;
        };
        for m in members.iter().filter(|m| !KEYS.contains(&m.key.as_str())) {
            diags.add_fix(
                Level::Warning,
                "W_UNKNOWN_KEY",
                None,
                Some(&format!("{base}/{}", m.key)),
                format!(
                    "unknown key \"{}\" in {base} (reference): it is ignored (kept in the file). Valid keys: path, caption",
                    m.key
                ),
                format!("remove \"{}\" (`kerf schema doc`)", m.key),
            );
        }
        if it.get("caption").is_some_and(|c| c.str().is_none()) {
            shape_error(
                diags,
                &format!("{base}/caption"),
                format!("'meta.references' entry {i}: caption must be a string"),
            );
        }
        let Some(p) = path_of(it) else {
            shape_error(
                diags,
                &format!("{base}/path"),
                format!(
                    "'meta.references' entry {i} needs \"path\": a non-empty string (the image file, relative to the document's folder)"
                ),
            );
            continue;
        };
        if let Some(problem) = path_problem(p) {
            diags.add_fix(
                Level::Warning,
                CODE,
                None,
                Some(&format!("{base}/path")),
                format!("meta.references[{i}].path \"{p}\" {}", problem.describe()),
                problem_fix(p, problem),
            );
        }
    }
}

/// W_REFERENCE_FILE for the paths the host could not resolve. `files` is the `reference_files` input:
/// `{"<path as written>": "missing" | "outside", ...}`.
pub(crate) fn host_report(doc: &Value, files: &Value, diags: &Diags) -> Result<(), &'static str> {
    const BAD: &str = "\"reference_files\" must be an object {\"<meta.references path>\": \"missing\" | \"outside\"}";
    let Value::Object(members) = files else { return Err(BAD) };
    if members.iter().any(|m| !matches!(m.value.str(), Some("missing" | "outside"))) {
        return Err(BAD);
    }
    for (i, p) in paths(doc) {
        if path_problem(p).is_some() {
            continue;
        }
        let Some(status) = members.iter().find(|m| m.key == p).and_then(|m| m.value.str()) else { continue };
        let (what, fix) = if status == "outside" {
            (
                "resolves outside the document's folder through a symbolic link",
                "copy the image into the folder instead of linking to it (kerf serve does not follow links out of its folder)"
                    .to_string(),
            )
        } else {
            (
                "does not exist relative to the document's folder (or is not a file)",
                format!("copy the snip to {p} (relative to the folder that holds the .kerf.json) or correct the path"),
            )
        };
        diags.add_fix(
            Level::Warning,
            CODE,
            None,
            Some(&format!("meta/references/{i}/path")),
            format!("meta.references[{i}].path \"{p}\" {what}"),
            fix,
        );
    }
    Ok(())
}
