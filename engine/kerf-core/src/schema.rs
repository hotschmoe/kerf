//! Field schemas of the non-component objects (SPEC 19): the single source of truth for `kerf schema <topic>`, the schema section of
//! `kerf guide`, and the minimal example document.

use crate::canon;
use crate::catalog;
use crate::json::{self, Member, Value};
use crate::schema_fields::{self as fields, Field, Object};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const EXAMPLE_DOC: &str = concat!(
    r#"{"kerf":"0.1","id":"wall-at-sill","title":"WALL AT P.T. SILL","meta":{"jurisdiction":{"code":"IRC","edition":2021}},"run":[-24,24],"#,
    "\n",
    r#""components":["#,
    "\n",
    r#"{"id":"sill","type":"lumber","size":"2x6","orient":"flat","treated":true,"at":{"anchor":"bottom_left","to":[0,0]}},"#,
    "\n",
    r#"{"id":"stud","type":"lumber","size":"2x6","run":"y","length":24,"at":{"anchor":"bottom_left","to":"sill@top_left"}}],"#,
    "\n",
    r#""views":[{"id":"A","kind":"section","number":"1","title":"WALL AT P.T. SILL","scale":"1\"=1'-0\"","cut_z":0,"crop":{"x":[-12,18],"y":[-8,30]},"notes_side":"both","#,
    "\n",
    r#""annotations":["#,
    "\n",
    r#"{"id":"n_sill","type":"note","text":"2X6 P.T. SILL PLATE","target":"sill","cite":[{"code":"IRC","edition":2021,"section":"R317.1","title":"Location required","status":"suggested"}]},"#,
    "\n",
    r#"{"id":"n_stud","type":"note","text":"2X6 STUDS @ 16\" O.C.","target":"stud"},"#,
    "\n",
    r#"{"id":"d_sill","type":"dim","from":"sill@bottom_left","to":"sill@bottom_right","dir":"h","offset":-3},"#,
    "\n",
    r#"{"id":"l_ext","type":"label","text":"EXTERIOR","at":"sill@top_left","offset":[-6,6]}]}]}"#,
);

fn example_value() -> Option<Value> {
    json::parse(EXAMPLE_DOC.as_bytes()).ok()
}

/// The example document with its id (and title when non-empty) replaced, in canonical form.
pub fn template_text(id: &str, title: &str) -> Option<Vec<u8>> {
    let Value::Object(members) = example_value()? else { return None };
    let members = members
        .into_iter()
        .map(|m| match m.key.as_str() {
            "id" => Member { key: m.key, value: Value::String(id.into()) },
            "title" if !title.is_empty() => Member { key: m.key, value: Value::String(title.into()) },
            _ => m,
        })
        .collect();
    Some(canon::write(&Value::Object(members)))
}

pub fn example_doc_text() -> String {
    match example_value() {
        Some(v) => String::from_utf8_lossy(&canon::write(&v)).into_owned(),
        None => EXAMPLE_DOC.into(),
    }
}

fn field_line(out: &mut String, f: &Field) {
    out.push_str(&format!("- {}: {}, ", f.name, f.ty));
    if f.required {
        out.push_str("required");
    } else if !f.def.is_empty() {
        out.push_str(&format!("default {}", f.def));
    } else {
        out.push_str("optional");
    }
    out.push_str(&format!(". {}\n", f.desc));
}

fn append_example(out: &mut String, text: &str) {
    if !text.is_empty() {
        out.push_str(&format!("\nExample:\n{text}\n"));
    }
}

fn render_object(o: &Object) -> String {
    let mut out = format!("{}: {}\n\n", o.name, o.summary);
    for f in o.fields {
        field_line(&mut out, f);
    }
    if !o.notes.is_empty() {
        out.push_str(&format!("\n{}\n", o.notes));
    }
    if o.name == "doc" {
        out.push_str("\nMinimal complete document (`kerf new x.kerf.json --template section` writes it):\n");
        out.push_str(&example_doc_text());
    } else {
        append_example(&mut out, o.example);
    }
    out
}

fn render_common() -> String {
    let mut out =
        String::from("common: fields every component accepts (plus the type's own params: `kerf schema <type>`).\n\n");
    for p in catalog::common() {
        out.push_str(&format!("- {}: ", p.name_text()));
        if p.def == "required" {
            out.push_str("required");
        } else {
            out.push_str(&format!("default {}", p.def));
        }
        out.push_str(&format!(". {}\n", p.desc));
    }
    out.push_str("\nTypes: ");
    for (i, e) in catalog::ENTRIES.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(e.name());
    }
    out.push('\n');
    out
}

fn render_component(e: &catalog::Entry) -> String {
    let mut out = format!("{}: {}\n\n", e.name(), e.summary);
    for p in e.params() {
        let req = p.def.starts_with("required") || p.def.ends_with(": required");
        out.push_str(&format!("- {}: ", p.name_text()));
        if req {
            out.push_str(&p.def);
        } else {
            out.push_str(&format!("default {}", p.def));
        }
        out.push_str(&format!(". {}\n", p.desc));
    }
    out.push_str("\nCommon fields (every type): ");
    for (i, p) in catalog::common().iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(p.names[0]);
    }
    out.push_str(" (`kerf schema common`).\n");
    out.push_str(&format!("Parts: {}\nAnchors: {}\nDraws: {}\n", e.parts, e.anchors, e.draws));
    if e.traits.hardware {
        out.push_str("Hardware models: ");
        for (i, h) in catalog::HARDWARE.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(h.model);
        }
        out.push_str(" (`kerf catalog` lists widths and gauges)\n");
    }
    if e.traits.shape_table {
        out.push_str(&format!("Shape table: {}\n", crate::builders::steel_shapes::catalog_text()));
    }
    append_example(&mut out, e.example);
    out
}

const REFS_TEXT: &str = concat!(
    "refs: how components name points (anywhere a point is accepted: `at.to`, `until`, note `at`, dim `from`/`to`, label `at`, point lists).\n",
    "\n",
    "- \"comp@anchor\" e.g. \"sill@top_left\"; \"comp.part@anchor\" e.g. \"truss.top_chord@top_right\"; \"@origin\" is (0, 0).\n",
    "- \"comp#k@anchor\" addresses instance k of an array (`kerf schema array`).\n",
    "- {\"ref\": \"footing@bottom_left\", \"offset\": [3, 3]} adds an offset; a literal point is [x, y] in inches (lengths also accept \"3'-4 1/2\\\"\").\n",
    "- Anchors: the 9 box anchors top_left top_center top_right middle_left center middle_right bottom_left bottom_center bottom_right,\n",
    "  plus named anchors per type (`kerf schema <type>`; `kerf call inspect` with {\"q\":\"anchors\",\"id\":\"<comp>\"} lists them with coordinates).\n",
    "- Coordinates: inches, X right, Y up, Z toward the viewer; sections look along -Z.\n",
);

pub const TOPICS_HINT: &str = "doc view layout note dim label table ref cite ops at array cuts acknowledge abbreviations common refs <component type>";

/// Heading of `kerf schema abbreviations` (the table itself is `house::abbreviations_markdown` of the active style).
pub const ABBREV_HEAD: &str = "# Abbreviations (W_ABBREV, SPEC 25.6)\n\nSpell a word out or abbreviate it: both are fine. An abbreviation must be written in its approved form: shortened words take a period (`TYP.`), phrases take dotted initials (`O.C.`), material and product acronyms take none (`CMU`), and a listed variant takes the form it maps to (`BOTT.` -> `BOT.`). A bare form that is also an ordinary word (`SIM`, `TO`, `ID`) counts only with its periods. Tokens with a digit (`HU212-2`, `16d`), case-significant tokens (`f'c`) and quoted text are never read. Keep a deviation: `acknowledge` W_ABBREV on the note, label, dim, weld or view, with a reason. The company style (`kerf.kerfstyle.json`, `notes.abbreviations`) adds (`approved`), remaps (`variants`), removes (`remove`) entries, switches rules off (`rules`) and lists words always abbreviated (`always`).\n\n";

pub fn render(topic: &str) -> Option<String> {
    if let Some(o) = fields::find_object(topic) {
        return Some(render_object(o));
    }
    if let Some(e) = catalog::find(topic) {
        return Some(render_component(e));
    }
    match topic {
        "common" | "component" | "components" => Some(render_common()),
        "refs" | "ref" | "anchors" | "anchor" => Some(String::from(REFS_TEXT)),
        "annotation" | "annotations" => {
            let mut out = String::new();
            for o in [&fields::NOTE, &fields::DIM, &fields::LABEL, &fields::WELD, &fields::TABLE] {
                out.push_str(&render_object(o));
                out.push('\n');
            }
            Some(out)
        }
        _ => None,
    }
}

fn first_sentence(s: &str) -> &str {
    match s.find(". ") {
        Some(i) => &s[..=i],
        None => s,
    }
}

pub fn index() -> String {
    let mut out = String::from(
        "kerf schema <topic>: field reference (required/optional, types, defaults, one example). Topics:\n\n",
    );
    for o in fields::OBJECTS {
        out.push_str(&format!("  {:<12} {}\n", o.name, o.brief));
    }
    out.push_str(&format!("  {:<12} {}\n", "common", "fields every component accepts"));
    out.push_str(&format!("  {:<12} {}\n", "refs", "how to name points and anchors"));
    out.push_str(&format!(
        "  {:<12} {}\n",
        "abbreviations", "approved abbreviation forms of the style in force (W_ABBREV)"
    ));
    for e in &catalog::ENTRIES {
        out.push_str(&format!("  {:<12} {}\n", e.name(), first_sentence(e.summary)));
    }
    out
}

pub fn guide_section() -> String {
    let mut out = String::from("# Document schema (generated: `kerf schema <topic>`)\n\n");
    out.push_str("Topics: ");
    out.push_str(TOPICS_HINT);
    out.push_str(". Unknown keys are kept but warn (W_UNKNOWN_KEY, with a suggestion): the field names below are the only ones.\n\n");
    for o in [&fields::VIEW, &fields::NOTE, &fields::DIM, &fields::LABEL, &fields::TABLE, &fields::CITE, &fields::OPS] {
        out.push_str("```text\n");
        out.push_str(&render_object(o));
        out.push_str("```\n\n");
    }
    out.push_str("Complete minimal document (two components, a section view, two notes with one citation, a dimension, a label). `kerf new x.kerf.json --template section` writes it:\n\n```json\n");
    out.push_str(&example_doc_text());
    out.push_str("```\n\n");
    out
}

fn compact_line(out: &mut String, o: &Object) {
    out.push_str(&format!("- {}:", o.name));
    for f in o.fields {
        out.push_str(&format!(" {}", f.name));
        if f.required {
            out.push('*');
        }
        if f.ty.len() <= 24 && f.ty != "string" {
            out.push_str(&format!(":{}", f.ty));
        }
        if !f.required && !f.def.is_empty() && f.def.len() <= 12 {
            out.push_str(&format!("={}", f.def));
        }
        out.push(',');
    }
    out.pop();
    out.push('\n');
}

pub fn compact_section() -> String {
    let mut out = String::from(
        "## Document schema (field names are exact; `*` required, `:type`, `=default`; descriptions: `kerf schema <topic>`)\n",
    );
    for o in [&fields::DOC, &fields::VIEW, &fields::NOTE, &fields::DIM, &fields::LABEL, &fields::CITE] {
        compact_line(&mut out, o);
    }
    out.push_str("- annotations go in a view's `annotations`; note `target` is a component id (or `comp.part`), `at`/dim `from`/`to` are Refs or [x,y]\n\n");
    out.push_str("## Complete example (`kerf new x.kerf.json --template section` writes it)\n```json\n");
    out.push_str(EXAMPLE_DOC);
    out.push_str("\n```\n\n## Topics: `kerf schema <topic> [<topic> ...]`\n");
    out.push_str(TOPICS_HINT);
    out.push_str("\nComponent types (`kerf schema <type>` = params, anchors, example):\n");
    for e in &catalog::ENTRIES {
        out.push_str(&format!("- {}: {}\n", e.name(), first_sentence(e.summary)));
    }
    out
}
