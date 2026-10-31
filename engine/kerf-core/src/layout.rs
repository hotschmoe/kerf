//! Sheet layouts (SPEC 26.2): several views of one document on one sheet, each with its own bubble, title and scale,
//! arranged in a row, a column or a grid, with one shared DETAIL NOTES list numbered without gaps over the layout, a
//! common scale picked for the whole sheet, and the document-level default `scale`.
//!
//! A layout builds each view with `drawview::build_with` (any view kind), then maps every view's items into the
//! layout's coordinates: paper inches times the scale of the layout's first view (so views at that scale keep model
//! inches 1:1, a view at another scale is scaled by the ratio of the two, text heights included).

use crate::annot::notes::{self, ListEntry, ListLook};
use crate::compile;
use crate::drawing::{Cell, Drawing, HatchItem, Item, KeyedEntry};
use crate::drawview::{self, ViewOpts};
use crate::font::{self, Font};
use crate::geom::{Box, Pt};
use crate::json::{self, Value};
use crate::model::{self, Diag, Diags, Level};
use crate::scene::{self, Scene};
use crate::style::Style;
use crate::units;
use crate::view::{self, Kind, NotesSide, NotesStyle, ViewSpec};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Paper inches between neighbouring cells unless the layout sets `gap`.
const DEFAULT_GAP_IN: f64 = 0.5;
const MAX_GAP_IN: f64 = 4.0;
const MIN_SHEET_IN: f64 = 5.0;
const MAX_SHEET_IN: f64 = 60.0;
/// The standard sheets, smallest first (ANSI A, ANSI B, ANSI C, ARCH C, ARCH D landscape): `sheet: "auto"` picks one
/// and a `W_VIEW_FIT` fix may suggest a larger one.
const SHEETS: [(f64, f64, &str); 5] =
    [(11.0, 8.5, "11x8.5"), (17.0, 11.0, "17x11"), (22.0, 17.0, "22x17"), (24.0, 18.0, "24x18"), (36.0, 24.0, "36x24")];
/// `sheet: "auto"` with a common scale takes the smallest sheet at which the views reach this scale (1" = 1'-0"), or
/// the largest scale any sheet reaches when none reaches it.
const AUTO_TARGET_SCALE: f64 = 12.0;
const MIN_ISO_RATIO: f64 = 0.1;
const MAX_ISO_RATIO: f64 = 4.0;

/// `"17x11"` or `[17, 11]`: width and height in paper inches.
fn sheet_size(v: &Value) -> Option<(f64, f64)> {
    let (w, h) = match v {
        Value::String(s) => {
            let (a, b) = s.split_once(['x', 'X'])?;
            (units::parse_finite(a.trim())?, units::parse_finite(b.trim())?)
        }
        Value::Array(a) if a.len() == 2 => (a[0].num()?, a[1].num()?),
        _ => return None,
    };
    let ok = |x: f64| (MIN_SHEET_IN..=MAX_SHEET_IN).contains(&x);
    (ok(w) && ok(h)).then_some((w, h))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrange {
    Row,
    Column,
    Grid(usize),
}

impl Arrange {
    fn text(self) -> String {
        match self {
            Arrange::Row => "row".to_string(),
            Arrange::Column => "column".to_string(),
            Arrange::Grid(c) => format!("grid, {c} columns"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListSide {
    Right,
    Left,
    Below,
    Above,
}

impl ListSide {
    fn name(self) -> &'static str {
        match self {
            ListSide::Right => "right",
            ListSide::Left => "left",
            ListSide::Below => "below",
            ListSide::Above => "above",
        }
    }
}

/// An entry of `views`: a view id, or a nested list of view ids stacked across the parent direction (a column inside a
/// row or a grid, a row inside a column).
#[derive(Clone, Debug)]
pub enum Entry<'a> {
    View(&'a str),
    Stack(Vec<&'a str>),
}

#[derive(Clone, Debug)]
pub struct Layout<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub number: &'a str,
    pub entries: Vec<Entry<'a>>,
    pub arrange: Arrange,
    pub shared: bool,
    pub side: ListSide,
    /// None = `common`; else the scale every view without its own `scale` is drawn at.
    pub scale: Option<(f64, &'a str)>,
    pub gap: f64,
    /// Sheet size (paper inches, width x height); None = the style's (or, with `auto_sheet`, not picked yet).
    pub sheet: Option<(f64, f64)>,
    /// `sheet: "auto"`: the smallest standard sheet that holds the views (SPEC 28.2).
    pub auto_sheet: bool,
    /// An iso view without its own scale is drawn at the common scale over this ratio (1 = the sections' size).
    pub iso_ratio: f64,
}

/// A view keeps its own scale in a layout: an explicit (or document) `scale`.
fn fixed_scale(s: &ViewSpec) -> bool {
    s.has_scale
}

impl Layout<'_> {
    /// The paper area an NTS view fits: the sheet area divided among the entries along each axis of the arrangement.
    fn share(&self, st: &Style) -> (f64, f64) {
        let (aw, ah) = self.area(st);
        let n = self.entries.len().max(1);
        let (cols, rows) = match self.arrange {
            Arrange::Row => (n, 1),
            Arrange::Column => (1, n),
            Arrange::Grid(c) => (c.clamp(1, n), n.div_ceil(c.clamp(1, n))),
        };
        let part = |a: f64, k: usize| ((a - self.gap * (k - 1) as f64) / k as f64).max(1.0);
        (part(aw, cols), part(ah, rows))
    }

    /// The area the arranged views and the list must fit (paper inches), on the layout's sheet.
    fn area(&self, st: &Style) -> (f64, f64) {
        let (w, h) = self.sheet.unwrap_or((st.sheet_w_in, st.sheet_h_in));
        drawview::fit_area_of(st, w, h)
    }

    /// The view ids in layout order (row-major, nested lists in place).
    pub fn views(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for e in &self.entries {
            match e {
                Entry::View(v) => out.push(*v),
                Entry::Stack(vs) => out.extend(vs.iter().copied()),
            }
        }
        out
    }
}

pub fn layout_ids(doc: &Value) -> Vec<&str> {
    let Some(ls) = doc.get("layouts").and_then(Value::arr) else { return Vec::new() };
    ls.iter().filter_map(|l| l.get("id").and_then(Value::str)).collect()
}

/// Some layout of the document names view `vid` (it is then fit-checked as part of that layout, SPEC 28.2).
pub fn names_view(doc: &Value, vid: &str) -> bool {
    let Some(ls) = doc.get("layouts").and_then(Value::arr) else { return false };
    ls.iter().filter_map(|l| l.get("views").and_then(Value::arr)).flatten().any(|v| match v.arr() {
        Some(inner) => inner.iter().any(|x| x.str() == Some(vid)),
        None => v.str() == Some(vid),
    })
}

fn find_layout<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    doc.get("layouts")?.arr()?.iter().find(|l| l.get("id").and_then(Value::str) == Some(id))
}

/// The document-level default `scale` (a numeric scale; NTS is not a default).
pub fn doc_scale(doc: &Value) -> Option<(f64, &str)> {
    let t = doc.get("scale").and_then(Value::str)?;
    units::parse_scale(t).filter(|s| s.factor > 0.0).map(|s| (s.factor, t))
}

/// A view spec with the document's default `scale` applied: every view without its own `scale`, except iso views
/// (NTS by default), is drawn at it.
pub fn view_spec<'a>(doc: &'a Value, node: &'a Value, index: usize, diags: &Diags) -> Option<ViewSpec<'a>> {
    let mut spec = view::parse(node, index, diags)?;
    if !spec.has_scale
        && spec.kind != Kind::Iso
        && let Some((f, t)) = doc_scale(doc)
    {
        spec.scale = f;
        spec.scale_text = t;
        spec.has_scale = true;
    }
    Some(spec)
}

fn err(diags: &Diags, id: Option<&str>, path: &str, msg: String, fix: &str) {
    diags.add_fix(Level::Error, "E_PARAM", id, Some(path), msg, fix.to_string());
}

/// Parses layout `node` (index `i` in `layouts`); every problem is diagnosed, None when the layout cannot be built.
pub fn parse<'a>(doc: &'a Value, node: &'a Value, i: usize, diags: &Diags) -> Option<Layout<'a>> {
    let Some(id) = node.get("id").and_then(Value::str).filter(|s| !s.is_empty()) else {
        err(
            diags,
            None,
            &format!("layouts/{i}"),
            format!("layout {i} needs a string \"id\" (e.g. \"S1\"; `kerf export <doc> --layout S1` names it)"),
            "add \"id\": \"S1\"",
        );
        return None;
    };
    let base = format!("layouts/{id}");
    let mut ok = true;
    let doc_views = view::view_ids(doc);
    let mut entries: Vec<Entry<'a>> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut check_view = |v: &'a Value, ok: &mut bool| -> Option<&'a str> {
        let Some(vid) = v.str() else {
            err(
                diags,
                Some(id),
                &format!("{base}/views"),
                format!(
                    "layout {id}: \"views\" entries are view ids or lists of view ids (got {})",
                    model::kind_or_text(v)
                ),
                "e.g. \"views\": [\"A\", \"B\", [\"C\", \"D\"]]",
            );
            *ok = false;
            return None;
        };
        if !doc_views.contains(&vid) {
            let hint = model::nearest(vid, &doc_views).map(|n| format!(" Did you mean '{n}'?")).unwrap_or_default();
            diags.add_fix(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(id),
                Some(&format!("{base}/views")),
                format!(
                    "layout {id} names view '{vid}', which is not in views[].{hint} Views: {}",
                    scene::join_ids(&doc_views)
                ),
                format!("use one of the view ids, or add view '{vid}' first"),
            );
            *ok = false;
            return None;
        }
        if seen.contains(&vid) {
            err(
                diags,
                Some(id),
                &format!("{base}/views"),
                format!("layout {id} names view '{vid}' twice; a view is drawn once per layout"),
                "remove the second entry",
            );
            *ok = false;
            return None;
        }
        seen.push(vid);
        Some(vid)
    };
    match node.get("views").and_then(Value::arr) {
        Some(vs) if !vs.is_empty() => {
            for v in vs {
                if let Some(inner) = v.arr() {
                    let mut stack = Vec::new();
                    for x in inner {
                        if let Some(vid) = check_view(x, &mut ok) {
                            stack.push(vid);
                        }
                    }
                    if inner.is_empty() {
                        err(
                            diags,
                            Some(id),
                            &format!("{base}/views"),
                            format!("layout {id}: a nested list in \"views\" is empty"),
                            "remove it, or list the view ids to stack there",
                        );
                        ok = false;
                    }
                    entries.push(Entry::Stack(stack));
                } else if let Some(vid) = check_view(v, &mut ok) {
                    entries.push(Entry::View(vid));
                }
            }
        }
        _ => {
            err(
                diags,
                Some(id),
                &format!("{base}/views"),
                format!(
                    "layout {id} needs \"views\": a non-empty list of view ids in reading order (a nested list stacks views across the arrangement)"
                ),
                "e.g. \"views\": [\"A\", \"B\", \"C\", \"D\"]",
            );
            ok = false;
        }
    }
    let str_field = |k: &str| node.get(k).filter(|v| !v.is_null());
    let mut columns: Option<usize> = None;
    if let Some(cv) = str_field("columns") {
        match cv
            .num()
            .and_then(crate::num::to_int::<u32>)
            .filter(|&n| (1..=12).contains(&n) && cv.num() == Some(f64::from(n)))
        {
            Some(n) => columns = Some(n as usize),
            None => {
                err(
                    diags,
                    Some(id),
                    &format!("{base}/columns"),
                    format!(
                        "layout {id}: \"columns\" must be a whole number from 1 to 12 (got {})",
                        model::kind_or_text(cv)
                    ),
                    "e.g. \"columns\": 2",
                );
                ok = false;
            }
        }
    }
    let arrange = match str_field("arrange").map(|v| v.str()) {
        None => match columns {
            Some(c) => Arrange::Grid(c),
            None => Arrange::Row,
        },
        Some(Some("row")) => Arrange::Row,
        Some(Some("column")) => Arrange::Column,
        Some(Some("grid")) => Arrange::Grid(columns.unwrap_or(2)),
        Some(_) => {
            err(
                diags,
                Some(id),
                &format!("{base}/arrange"),
                format!(
                    "layout {id}: \"arrange\" must be \"row\", \"column\" or \"grid\" (got {})",
                    model::kind_or_text(node.get("arrange").unwrap_or(&Value::Null))
                ),
                "row: side by side; column: one above the other; grid: rows of \"columns\" views",
            );
            ok = false;
            Arrange::Row
        }
    };
    if columns.is_some() && matches!(arrange, Arrange::Row | Arrange::Column) {
        diags.add_fix(
            Level::Warning,
            "W_PARAM",
            Some(id),
            Some(&format!("{base}/columns")),
            format!("layout {id}: \"columns\" only applies to \"arrange\": \"grid\"; ignored"),
            "set \"arrange\": \"grid\", or remove \"columns\"".to_string(),
        );
    }
    let shared = match str_field("notes").map(|v| v.str()) {
        None | Some(Some("shared")) => true,
        Some(Some("per_view")) => false,
        Some(_) => {
            err(
                diags,
                Some(id),
                &format!("{base}/notes"),
                format!("layout {id}: \"notes\" must be \"shared\" or \"per_view\""),
                "shared: one DETAIL NOTES list for the layout's keyed notes; per_view: each keyed view keeps its own",
            );
            ok = false;
            true
        }
    };
    let side = match str_field("notes_side").map(|v| v.str()) {
        None | Some(Some("right")) => ListSide::Right,
        Some(Some("left")) => ListSide::Left,
        Some(Some("below")) => ListSide::Below,
        Some(Some("above")) => ListSide::Above,
        Some(_) => {
            err(
                diags,
                Some(id),
                &format!("{base}/notes_side"),
                format!("layout {id}: \"notes_side\" must be \"right\", \"left\", \"below\" or \"above\""),
                "where the shared DETAIL NOTES list goes relative to the arranged views",
            );
            ok = false;
            ListSide::Right
        }
    };
    let scale = match str_field("scale") {
        None => None,
        Some(sv) => match sv.str() {
            Some("common") => None,
            Some(t) => match units::parse_scale(t).filter(|s| s.factor > 0.0) {
                Some(s) => Some((s.factor, t)),
                None => {
                    err(
                        diags,
                        Some(id),
                        &format!("{base}/scale"),
                        format!(
                            "layout {id}: \"scale\" must be \"common\" or a numeric scale such as \"3/4\\\"=1'-0\\\"\" (got \"{t}\")"
                        ),
                        "\"common\" picks the largest standard scale at which every view fits",
                    );
                    ok = false;
                    None
                }
            },
            None => {
                err(
                    diags,
                    Some(id),
                    &format!("{base}/scale"),
                    format!("layout {id}: \"scale\" must be a string (got {})", model::kind_or_text(sv)),
                    "use \"common\" or a scale such as \"3/4\\\"=1'-0\\\"\"",
                );
                ok = false;
                None
            }
        },
    };
    let gap = match str_field("gap") {
        None => DEFAULT_GAP_IN,
        Some(gv) => match gv.num().filter(|g| (0.0..=MAX_GAP_IN).contains(g)) {
            Some(g) => g,
            None => {
                err(
                    diags,
                    Some(id),
                    &format!("{base}/gap"),
                    format!(
                        "layout {id}: \"gap\" is the paper space between views in inches, 0 to {} (got {})",
                        model::fmt_num(MAX_GAP_IN),
                        model::kind_or_text(gv)
                    ),
                    "e.g. \"gap\": 0.5",
                );
                ok = false;
                DEFAULT_GAP_IN
            }
        },
    };
    let auto_sheet = str_field("sheet").and_then(Value::str) == Some("auto");
    let sheet = match str_field("sheet") {
        None => None,
        Some(_) if auto_sheet => None,
        Some(sv) => match sheet_size(sv) {
            Some(s) => Some(s),
            None => {
                err(
                    diags,
                    Some(id),
                    &format!("{base}/sheet"),
                    format!(
                        "layout {id}: \"sheet\" is \"auto\" or the paper size \"<width>x<height>\" in inches, each {}-{} (got {})",
                        model::fmt_num(MIN_SHEET_IN),
                        model::fmt_num(MAX_SHEET_IN),
                        model::kind_or_text(sv)
                    ),
                    "e.g. \"sheet\": \"auto\" (the smallest standard sheet that holds the views), \"17x11\" (ANSI B landscape) or \"36x24\"",
                );
                ok = false;
                None
            }
        },
    };
    let iso_ratio = match str_field("iso_ratio") {
        None => 1.0,
        Some(rv) => match rv.num().filter(|r| (MIN_ISO_RATIO..=MAX_ISO_RATIO).contains(r)) {
            Some(r) => r,
            None => {
                err(
                    diags,
                    Some(id),
                    &format!("{base}/iso_ratio"),
                    format!(
                        "layout {id}: \"iso_ratio\" is the size of the iso views against the common scale, {} to {} (got {})",
                        model::fmt_num(MIN_ISO_RATIO),
                        model::fmt_num(MAX_ISO_RATIO),
                        model::kind_or_text(rv)
                    ),
                    "e.g. \"iso_ratio\": 0.5 draws the iso views at half the sections' scale",
                );
                ok = false;
                1.0
            }
        },
    };
    let text = |k: &str| node.get(k).and_then(Value::str).unwrap_or("");
    if !ok {
        return None;
    }
    Some(Layout {
        id,
        title: text("title"),
        number: text("number"),
        entries,
        arrange,
        shared,
        side,
        scale,
        gap,
        sheet,
        auto_sheet,
        iso_ratio,
    })
}

/// Document checks of `layouts` and `scale` (run with the other document checks).
pub fn check(doc: &Value, diags: &Diags) {
    if let Some(sv) = doc.get("scale").filter(|v| !v.is_null()) {
        let good = sv.str().and_then(units::parse_scale).is_some_and(|s| s.factor > 0.0);
        if !good {
            diags.add_fix(
                Level::Error,
                "E_PARAM",
                None,
                Some("scale"),
                format!(
                    "document \"scale\" (the default for views without their own) must be a numeric scale such as \"3/4\\\"=1'-0\\\"\" or \"1:16\" (got {})",
                    model::kind_or_text(sv)
                ),
                "set it to the scale most views share, or remove it".to_string(),
            );
        }
    }
    let Some(lv) = doc.get("layouts").filter(|v| !v.is_null()) else { return };
    let Some(ls) = lv.arr() else {
        diags.add_fix(
            Level::Error,
            "E_PARAM",
            None,
            Some("layouts"),
            format!("\"layouts\" must be an array of layout objects (got {})", model::kind_or_text(lv)),
            "e.g. \"layouts\": [{\"id\":\"S1\",\"views\":[\"A\",\"B\"]}] (`kerf schema layout`)".to_string(),
        );
        return;
    };
    let mut seen: Vec<&str> = Vec::new();
    for (i, l) in ls.iter().enumerate() {
        if let Some(lid) = l.get("id").and_then(Value::str) {
            if seen.contains(&lid) {
                diags.add(
                    Level::Error,
                    "E_DUP_ID",
                    Some(lid),
                    Some(&format!("layouts/{i}")),
                    format!("duplicate layout id '{lid}'"),
                );
                continue;
            }
            seen.push(lid);
        }
        parse(doc, l, i, diags);
    }
}

/// A view's note `key` as the view would read it (1-999), else None.
fn key_of(an: &Value) -> Option<u32> {
    let k = an.get("key")?.num()?;
    crate::num::to_int::<u32>(k).filter(|&n| f64::from(n) == k && (1..=999).contains(&n))
}

struct Group<'a> {
    key: Option<u32>,
    text: Option<&'a str>,
    cite: Option<&'a Value>,
    num: u32,
}

/// The numbers of the layout's keyed notes (SPEC 26.2): notes with the same `key` share an entry across views, an
/// unkeyed note shares the entry of an earlier note with the same text and cite, the rest are numbered in layout order
/// into the lowest free numbers; the entries are then numbered 1..N in that order. Returns per view the note numbers.
fn shared_keys<'a>(lay: &Layout, specs: &[ViewSpec<'a>], st: &Style, diags: &Diags) -> Vec<Vec<(String, u32)>> {
    let mut groups: Vec<Group> = Vec::new();
    let mut member: Vec<Vec<(String, usize)>> = Vec::new();
    for spec in specs {
        let mut mine = Vec::new();
        if spec.kind == Kind::Schedule || spec.notes_style.unwrap_or(st.notes_mode) != NotesStyle::Leader {
            for an in spec.annotations {
                if an.get("type").and_then(Value::str) != Some("note") {
                    continue;
                }
                let Some(nid) = an.get("id").and_then(Value::str) else { continue };
                let key = key_of(an);
                let text = an.get("text").and_then(Value::str).map(str::trim);
                let cite = an.get("cite").filter(|c| !c.is_null());
                let gi = match key {
                    Some(k) => groups.iter().position(|g| g.key == Some(k)),
                    None => text.and_then(|t| {
                        groups.iter().position(|g| g.text == Some(t) && cite_text(g.cite) == cite_text(cite))
                    }),
                };
                let gi = gi.unwrap_or_else(|| {
                    groups.push(Group { key, text: None, cite: None, num: 0 });
                    groups.len() - 1
                });
                if groups[gi].text.is_none() && text.is_some() {
                    groups[gi].text = text;
                    groups[gi].cite = cite;
                }
                mine.push((nid.to_string(), gi));
            }
        }
        member.push(mine);
    }
    let mut used: Vec<u32> = groups.iter().filter_map(|g| g.key).collect();
    let mut next = 1u32;
    for g in &mut groups {
        g.num = g.key.unwrap_or_else(|| {
            while used.contains(&next) {
                next += 1;
            }
            used.push(next);
            next
        });
    }
    let mut order: Vec<usize> = (0..groups.len()).collect();
    crate::sort::stable(&mut order, |&a, &b| groups[a].num < groups[b].num);
    let mut final_num = alloc::vec![0u32; groups.len()];
    for (rank, &gi) in order.iter().enumerate() {
        final_num[gi] = rank as u32 + 1;
    }
    let moved: Vec<String> = order
        .iter()
        .filter(|&&gi| groups[gi].key.is_some_and(|k| k != final_num[gi]))
        .map(|&gi| format!("key {} prints as {}", groups[gi].num, final_num[gi]))
        .collect();
    if !moved.is_empty() {
        let lid = lay.id;
        diags.add_fix(
            Level::Info,
            "I_KEY_RENUMBERED",
            Some(lid),
            Some(&format!("layouts/{lid}")),
            format!(
                "layout {lid} numbers its shared DETAIL NOTES 1..{} without gaps: {}",
                groups.len(),
                moved.join(", ")
            ),
            "nothing to do; to print the keys as written, give the notes consecutive keys starting at 1".to_string(),
        );
    }
    member.into_iter().map(|m| m.into_iter().map(|(id, gi)| (id, final_num[gi])).collect()).collect()
}

fn cite_text(c: Option<&Value>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(v) = c {
        json::write_compact(&mut out, v);
    }
    out
}

/// The shape of the arrangement: leaves index the layout's views in order.
enum Shape {
    Leaf(usize),
    Row(Vec<Shape>),
    Col(Vec<Shape>),
    Grid(Vec<Shape>, usize),
}

fn shape_of(lay: &Layout, arrange: Arrange) -> Shape {
    let mut k = 0usize;
    let mut kids = Vec::new();
    let across_col = arrange == Arrange::Column;
    for e in &lay.entries {
        match e {
            Entry::View(_) => {
                kids.push(Shape::Leaf(k));
                k += 1;
            }
            Entry::Stack(vs) => {
                let leaves: Vec<Shape> = (k..k + vs.len()).map(Shape::Leaf).collect();
                k += vs.len();
                kids.push(if across_col { Shape::Row(leaves) } else { Shape::Col(leaves) });
            }
        }
    }
    match arrange {
        Arrange::Row => Shape::Row(kids),
        Arrange::Column => Shape::Col(kids),
        Arrange::Grid(c) => Shape::Grid(kids, c),
    }
}

fn grid_dims(kids: &[Shape], cols: usize, sizes: &[(f64, f64)], gap: f64) -> (Vec<f64>, Vec<f64>) {
    let cols = cols.max(1).min(kids.len().max(1));
    let rows = kids.len().div_ceil(cols);
    let mut cw = alloc::vec![0.0f64; cols];
    let mut rh = alloc::vec![0.0f64; rows];
    for (i, k) in kids.iter().enumerate() {
        let (w, h) = measure(k, sizes, gap);
        cw[i % cols] = cw[i % cols].max(w);
        rh[i / cols] = rh[i / cols].max(h);
    }
    (cw, rh)
}

/// Width and height (paper inches) of a shape.
fn measure(s: &Shape, sizes: &[(f64, f64)], gap: f64) -> (f64, f64) {
    match s {
        Shape::Leaf(i) => sizes[*i],
        Shape::Row(k) => {
            let ms: Vec<(f64, f64)> = k.iter().map(|c| measure(c, sizes, gap)).collect();
            let w = ms.iter().map(|m| m.0).sum::<f64>() + gap * (ms.len().max(1) - 1) as f64;
            (w, ms.iter().map(|m| m.1).fold(0.0, f64::max))
        }
        Shape::Col(k) => {
            let ms: Vec<(f64, f64)> = k.iter().map(|c| measure(c, sizes, gap)).collect();
            let h = ms.iter().map(|m| m.1).sum::<f64>() + gap * (ms.len().max(1) - 1) as f64;
            (ms.iter().map(|m| m.0).fold(0.0, f64::max), h)
        }
        Shape::Grid(k, c) => {
            let (cw, rh) = grid_dims(k, *c, sizes, gap);
            (
                cw.iter().sum::<f64>() + gap * (cw.len().max(1) - 1) as f64,
                rh.iter().sum::<f64>() + gap * (rh.len().max(1) - 1) as f64,
            )
        }
    }
}

/// Lower-left corner (paper inches) of every leaf: a shape fills the box `(x, y, w, h)` it is given; a leaf sits
/// centred across and on the bottom of its box (the titles of one row share a line), rows centre in their width,
/// columns stack from the top.
fn place(s: &Shape, sizes: &[(f64, f64)], gap: f64, bx: [f64; 4], out: &mut [(f64, f64)]) {
    let [x, y, w, h] = bx;
    match s {
        Shape::Leaf(i) => out[*i] = (x + (w - sizes[*i].0) * 0.5, y),
        Shape::Row(k) => {
            let (tw, _) = measure(s, sizes, gap);
            let mut cx = x + (w - tw) * 0.5;
            for c in k {
                let (cw, _) = measure(c, sizes, gap);
                place(c, sizes, gap, [cx, y, cw, h], out);
                cx += cw + gap;
            }
        }
        Shape::Col(k) => {
            let mut top = y + h;
            for c in k {
                let (_, ch) = measure(c, sizes, gap);
                place(c, sizes, gap, [x, top - ch, w, ch], out);
                top -= ch + gap;
            }
        }
        Shape::Grid(k, c) => {
            let (cw, rh) = grid_dims(k, *c, sizes, gap);
            let (tw, _) = measure(s, sizes, gap);
            let x0 = x + (w - tw) * 0.5;
            let mut top = y + h;
            for (r, &rhh) in rh.iter().enumerate() {
                let mut cx = x0;
                for (ci, &cww) in cw.iter().enumerate() {
                    if let Some(kid) = k.get(r * cw.len() + ci) {
                        place(kid, sizes, gap, [cx, top - rhh, cww, rhh], out);
                    }
                    cx += cww + gap;
                }
                top -= rhh + gap;
            }
        }
    }
}

/// One view of the layout as built: its drawing and the paper size of its extents.
#[derive(Clone)]
struct Built<'s> {
    d: Drawing<'s>,
    diags: Vec<Diag>,
}

impl Built<'_> {
    fn paper(&self) -> (f64, f64) {
        let b = self.d.bounds;
        ((b[2] - b[0]) / self.d.scale, (b[3] - b[1]) / self.d.scale)
    }
}

struct Ctx<'c, 'a> {
    doc: &'c Value,
    st: &'a Style,
    scene: &'c Scene<'c>,
    font: &'c Font,
    /// The paper area an NTS view of the layout fits: its share of the sheet.
    share: (f64, f64),
}

fn build_view<'a>(
    c: &Ctx<'_, 'a>,
    spec: &ViewSpec,
    keys: Option<&[(String, u32)]>,
    iso_factor: Option<f64>,
) -> Option<Built<'a>> {
    let vd = Diags::new();
    let opts = ViewOpts { keys, in_layout: true, fit: Some(c.share), iso_factor };
    let d = drawview::build_with(c.doc, c.st, c.scene, spec, &vd, opts)?;
    Some(Built { d, diags: vd.into_vec() })
}

fn with_scale<'v>(spec: &ViewSpec<'v>, sc: f64, label: &'v str) -> ViewSpec<'v> {
    let mut s = spec.clone();
    s.scale = sc;
    s.scale_text = label;
    s.has_scale = true;
    s
}

/// The shared DETAIL NOTES entries of the built views, merged by number (the first view's text wins; a different text
/// under one number warns).
fn merge_entries(lay: &Layout, built: &[Built], diags: &Diags) -> Vec<KeyedEntry> {
    let mut out: Vec<(KeyedEntry, &str)> = Vec::new();
    for b in built {
        for e in &b.d.keyed {
            match out.iter().find(|(o, _)| o.num == e.num) {
                Some((o, v)) if o.text != e.text => {
                    let (field, mine, theirs) = if o.body == e.body {
                        ("cite", cite_part(e), cite_part(o))
                    } else if cite_part(o) == cite_part(e) {
                        ("text", format!("\"{}\"", e.body), format!("\"{}\"", o.body))
                    } else {
                        ("text and cite", format!("\"{}\"", e.text), format!("\"{}\"", o.text))
                    };
                    let path_field = if field == "cite" { "cite" } else { "text" };
                    diags.add_fix(
                        Level::Warning,
                        "W_PARAM",
                        Some(&e.first),
                        Some(&format!("views/{}/annotations/{}/{path_field}", b.d.view, e.first)),
                        format!(
                            "layout {}: note '{}' in view {} prints as shared note {} but its {field} differs from note '{}' in view {v} ({mine} vs {theirs}); the DETAIL NOTES list shows the {field} of '{}'",
                            lay.id, e.first, b.d.view, e.num, o.first, o.first
                        ),
                        format!(
                            "write the text and cite once: remove \"text\" and \"cite\" from note '{}' in view {} and keep its \"key\" (it then prints the entry of note '{}' in view {v}); or give '{}' its own \"key\"",
                            e.first, b.d.view, o.first, e.first
                        ),
                    );
                }
                Some(_) => {}
                None => out.push((e.clone(), &b.d.view)),
            }
        }
    }
    let mut v: Vec<KeyedEntry> = out.into_iter().map(|(e, _)| e).collect();
    crate::sort::stable(&mut v, |a, b| a.num < b.num);
    v
}

/// The printed citation of a list entry (its text after the note body), quoted, or `no cite`.
fn cite_part(e: &KeyedEntry) -> String {
    let c = e.text.strip_prefix(e.body.as_str()).unwrap_or(&e.text).trim();
    if c.is_empty() { "no cite".to_string() } else { format!("\"{c}\"") }
}

fn map_pts(pts: &[Pt], m: &impl Fn(f64, f64) -> (f64, f64)) -> Vec<Pt> {
    pts.iter()
        .map(|p| {
            let (x, y) = m(p.x, p.y);
            Pt { x, y, b: p.b }
        })
        .collect()
}

/// A view's items mapped into the layout: `p' = (p - origin) * f + to`.
fn map_items(items: &[Item], origin: (f64, f64), f: f64, to: (f64, f64)) -> Vec<Item> {
    let m = |x: f64, y: f64| ((x - origin.0) * f + to.0, (y - origin.1) * f + to.1);
    items
        .iter()
        .map(|it| match it {
            Item::Path(p) => {
                let mut q = p.clone();
                q.pts = map_pts(&p.pts, &m);
                Item::Path(q)
            }
            Item::Fill(p) => {
                let mut q = p.clone();
                q.loops = p.loops.iter().map(|l| map_pts(l, &m)).collect();
                Item::Fill(q)
            }
            Item::Region(p) => {
                let mut q = p.clone();
                q.loops = p.loops.iter().map(|l| map_pts(l, &m)).collect();
                Item::Region(q)
            }
            Item::Hatch(h) => {
                let lines = h
                    .lines
                    .iter()
                    .map(|l| {
                        let (a, b) = m(l[0], l[1]);
                        let (c, d) = m(l[2], l[3]);
                        [a, b, c, d]
                    })
                    .collect();
                Item::Hatch(HatchItem { loops: h.loops.iter().map(|l| map_pts(l, &m)).collect(), lines, ..h.clone() })
            }
            Item::Text(t) => {
                let mut q = t.clone();
                (q.x, q.y) = m(t.x, t.y);
                q.h = t.h * f;
                Item::Text(q)
            }
        })
        .collect()
}

/// The arranged views and the list (paper inches): leaf positions, the arrangement box and the list block.
struct Arranged {
    pos: Vec<(f64, f64)>,
    arr: (f64, f64),
    total: (f64, f64),
}

/// Paper size of the DETAIL NOTES block for `entries` at the layout's look (one column, or two past 8 lines).
fn list_size(st: &Style, font: &Font, entries: &[KeyedEntry], side: ListSide, arr_h: f64) -> (f64, f64) {
    if entries.is_empty() {
        return (0.0, 0.0);
    }
    let le: Vec<ListEntry> =
        entries.iter().map(|e| ListEntry { num: e.num, text: &e.text, body: &e.body, first: &e.first }).collect();
    let look = ListLook { st, font, sc: 1.0, side: list_notes_side(side), title: None };
    let ext = Box { x0: 0.0, y0: 0.0, x1: 1.0, y1: arr_h.max(1.0) };
    let mut unk = false;
    let b = crate::annot::items_box(font, &notes::list_block(&look, &le, ext, &mut unk));
    if b.is_empty() { (0.0, 0.0) } else { (b.width(), b.height()) }
}

fn list_notes_side(side: ListSide) -> NotesSide {
    match side {
        ListSide::Right => NotesSide::Right,
        ListSide::Left => NotesSide::Left,
        ListSide::Below | ListSide::Above => NotesSide::Above,
    }
}

fn arrange(
    lay: &Layout,
    arrange: Arrange,
    side: ListSide,
    sizes: &[(f64, f64)],
    list: (f64, f64),
    gutter: f64,
) -> Arranged {
    let shape = shape_of(lay, arrange);
    let arr = measure(&shape, sizes, lay.gap);
    let mut pos = alloc::vec![(0.0, 0.0); sizes.len()];
    place(&shape, sizes, lay.gap, [0.0, 0.0, arr.0, arr.1], &mut pos);
    let total = if list.0 <= 0.0 {
        arr
    } else {
        match side {
            ListSide::Right | ListSide::Left => (arr.0 + gutter + list.0, arr.1.max(list.1)),
            ListSide::Below | ListSide::Above => (arr.0.max(list.0), arr.1 + gutter + list.1),
        }
    };
    Arranged { pos, arr, total }
}

/// Builds layout `id` into one Drawing in the layout's coordinates (SPEC 26.2). None (with diagnostics) when the layout
/// does not exist, is invalid, or a view of it cannot be built.
pub fn build<'a>(doc: &Value, st: &'a Style, id: &str, diags: &Diags) -> Option<Drawing<'a>> {
    if find_layout(doc, id).is_none() {
        let ids = layout_ids(doc);
        let msg = if ids.is_empty() {
            format!("no layout with id '{id}': the document has no \"layouts\" (`kerf schema layout`)")
        } else {
            format!("no layout with id '{id}'. Layouts: {}", scene::join_ids(&ids))
        };
        diags.add(Level::Error, "E_REF_UNKNOWN", Some(id), Some("layouts"), msg);
        return None;
    }
    let scene = compile::compile(doc, st, diags);
    build_from_scene(doc, st, &scene, id, diags)
}

/// `build` with an already compiled scene.
pub fn build_from_scene<'a>(doc: &Value, st: &'a Style, scene: &Scene, id: &str, diags: &Diags) -> Option<Drawing<'a>> {
    let node = find_layout(doc, id)?;
    let li = doc.get("layouts").and_then(Value::arr).and_then(|ls| ls.iter().position(|l| core::ptr::eq(l, node)));
    let lay = parse(doc, node, li.unwrap_or(0), diags)?;
    let font = font::embedded();
    let views = lay.views();
    let all = doc.get("views").and_then(Value::arr).unwrap_or(&[]);
    let mut specs: Vec<ViewSpec> = Vec::new();
    for vid in &views {
        let i = all.iter().position(|v| v.get("id").and_then(Value::str) == Some(*vid))?;
        specs.push(view_spec(doc, &all[i], i, diags)?);
    }
    let keys = if lay.shared { shared_keys(&lay, &specs, st, diags) } else { alloc::vec![Vec::new(); specs.len()] };
    let key_of = |k: usize| if lay.shared { Some(keys[k].as_slice()) } else { None };
    let sheets: Vec<Option<(f64, f64)>> =
        if lay.auto_sheet { SHEETS.iter().map(|&(w, h, _)| Some((w, h))).collect() } else { alloc::vec![lay.sheet] };
    let first = Layout { sheet: sheets[0], ..lay.clone() };
    let c0 = Ctx { doc, st, scene, font, share: first.share(st) };
    let fixed: Vec<Option<Built>> = specs
        .iter()
        .enumerate()
        .map(|(k, s)| if fixed_scale(s) { build_view(&c0, s, key_of(k), None) } else { None })
        .collect();
    if specs.iter().zip(&fixed).any(|(s, f)| fixed_scale(s) && f.is_none()) {
        return None;
    }
    let flat_free = specs.iter().any(|s| !fixed_scale(s) && s.kind.is_flat());
    let candidates: Vec<f64> = match lay.scale {
        Some((f, _)) => alloc::vec![f],
        None if !flat_free => alloc::vec![0.0],
        None => drawview::STANDARD_SCALES.to_vec(),
    };
    let reached = |sc: f64| lay.scale.is_some() || !flat_free || sc <= AUTO_TARGET_SCALE + 1e-9;
    let mut trials: Vec<Trial> = Vec::new();
    let mut picks: Vec<(Layout, usize, bool)> = Vec::new();
    for &sheet in &sheets {
        let l = Layout { sheet, ..lay.clone() };
        let c = Ctx { share: l.share(st), ..c0 };
        let (aw, ah) = l.area(st);
        let mut pick = None;
        for (n, &sc) in candidates.iter().enumerate() {
            let t = match trials.iter().position(|t| t.sc == sc && (sc > 0.0 || t.share == c.share)) {
                Some(t) => t,
                None => {
                    trials.push(trial(&c, &l, &specs, &fixed, &key_of, sc)?);
                    trials.len() - 1
                }
            };
            let total = trials[t].total;
            let fits = total.0 <= aw + 1e-6 && total.1 <= ah + 1e-6;
            if fits || n + 1 == candidates.len() {
                pick = Some((t, fits));
                break;
            }
        }
        let (t, fits) = pick?;
        let done = fits && reached(trials[t].sc);
        picks.push((l, t, fits));
        if done {
            break;
        }
    }
    let best = picks.iter().filter(|p| p.2).map(|p| trials[p.1].sc).fold(f64::INFINITY, f64::min);
    let k = picks
        .iter()
        .position(|p| p.2 && (reached(trials[p.1].sc) || trials[p.1].sc <= best + 1e-9))
        .unwrap_or(picks.len() - 1);
    let (l, t, fits) = picks.swap_remove(k);
    let common = trials[t].sc;
    let built = trials.swap_remove(t).built;
    let c = Ctx { share: l.share(st), ..c0 };
    if lay.auto_sheet
        && let Some((w, h)) = l.sheet
    {
        let name = SHEETS.iter().find(|s| s.0 == w && s.1 == h).map_or("", |s| s.2);
        let at =
            if common > 0.0 { format!(" with the views at {}", units::scale_label(common)) } else { String::new() };
        let why = if !fits {
            "no standard sheet holds it, so the largest is used"
        } else if reached(common) {
            "the smallest standard sheet that holds it"
        } else {
            "no standard sheet holds it at 1\" = 1'-0\"; the smallest that reaches the largest scale"
        };
        diags.add_fix(
            Level::Info,
            "I_LAYOUT_SHEET",
            Some(lay.id),
            Some(&format!("layouts/{}", lay.id)),
            format!("layout {}: \"sheet\": \"auto\" picked {name}{at} ({why})", lay.id),
            "nothing to do; set \"sheet\" (e.g. \"22x17\") or a numeric layout \"scale\" to choose yourself"
                .to_string(),
        );
    }
    compose(&c, &l, common, built, diags)
}

/// The views of a layout built at one common scale, with the paper size the arrangement and the list need.
struct Trial<'a> {
    sc: f64,
    share: (f64, f64),
    built: Vec<Built<'a>>,
    total: (f64, f64),
}

/// Builds the views without their own scale at common scale `sc` (0: none to pick; an iso view then fits its share of
/// the sheet), an iso view at `sc` over the layout's `iso_ratio`.
fn trial<'a, 'k>(
    c: &Ctx<'_, 'a>,
    lay: &Layout,
    specs: &[ViewSpec],
    fixed: &[Option<Built<'a>>],
    key_of: &impl Fn(usize) -> Option<&'k [(String, u32)]>,
    sc: f64,
) -> Option<Trial<'a>> {
    let label = units::scale_label(sc);
    let mut built: Vec<Built> = Vec::new();
    for (k, s) in specs.iter().enumerate() {
        let b = match &fixed[k] {
            Some(f) => f.clone(),
            None if s.kind == Kind::Iso => build_view(c, s, key_of(k), (sc > 0.0).then(|| sc / lay.iso_ratio))?,
            None if s.kind.is_flat() && sc > 0.0 => build_view(c, &with_scale(s, sc, &label), key_of(k), None)?,
            None => build_view(c, s, key_of(k), None)?,
        };
        built.push(b);
    }
    let gutter = c.st.gutter_in;
    let sizes: Vec<(f64, f64)> = built.iter().map(Built::paper).collect();
    let entries = if lay.shared { merge_entries(lay, &built, &Diags::new()) } else { Vec::new() };
    let probe = arrange(lay, lay.arrange, lay.side, &sizes, (0.0, 0.0), gutter);
    let list = list_size(c.st, c.font, &entries, lay.side, probe.arr.1);
    let total = arrange(lay, lay.arrange, lay.side, &sizes, list, gutter).total;
    Some(Trial { sc, share: c.share, built, total })
}

fn compose<'a>(
    c: &Ctx<'_, 'a>,
    lay: &Layout,
    common: f64,
    built: Vec<Built<'a>>,
    diags: &Diags,
) -> Option<Drawing<'a>> {
    let st = c.st;
    let font = c.font;
    let (aw, ah) = lay.area(st);
    let gutter = st.gutter_in;
    let ld = Diags::new();
    let entries = if lay.shared { merge_entries(lay, &built, &ld) } else { Vec::new() };
    let sizes: Vec<(f64, f64)> = built.iter().map(Built::paper).collect();
    let probe = arrange(lay, lay.arrange, lay.side, &sizes, (0.0, 0.0), gutter);
    let list = list_size(st, font, &entries, lay.side, probe.arr.1);
    let ar = arrange(lay, lay.arrange, lay.side, &sizes, list, gutter);
    let sref = built.first()?.d.scale;
    let lx0 = if lay.side == ListSide::Left && list.0 > 0.0 { list.0 + gutter } else { 0.0 };
    let ly0 = if lay.side == ListSide::Below && list.0 > 0.0 { list.1 + gutter } else { 0.0 };
    let mut items: Vec<Item> = Vec::new();
    let mut cells: Vec<Cell> = Vec::new();
    let mut unverified = false;
    let mut out_diags: Vec<Diag> = Vec::new();
    for (b, &(px, py)) in built.iter().zip(&ar.pos) {
        let d = &b.d;
        let f = sref / d.scale;
        let to = ((lx0 + px) * sref, (ly0 + py) * sref);
        items.extend(map_items(&d.items, (d.bounds[0], d.bounds[1]), f, to));
        let (w, h) = b.paper();
        cells.push(Cell {
            view: d.view.clone(),
            scale: d.scale,
            scale_label: d.scale_label.clone(),
            bounds: [to.0, to.1, to.0 + w * sref, to.1 + h * sref],
        });
        unverified |= d.has_unverified;
        for dg in &b.diags {
            if !out_diags.contains(dg) {
                out_diags.push(dg.clone());
            }
        }
    }
    let arr_box = Box { x0: lx0 * sref, y0: ly0 * sref, x1: (lx0 + ar.arr.0) * sref, y1: (ly0 + ar.arr.1) * sref };
    if !entries.is_empty() {
        let le: Vec<ListEntry> =
            entries.iter().map(|e| ListEntry { num: e.num, text: &e.text, body: &e.body, first: &e.first }).collect();
        let look = ListLook { st, font, sc: sref, side: list_notes_side(lay.side), title: None };
        let mut unk = false;
        let mut block = notes::list_block(&look, &le, arr_box, &mut unk);
        if lay.side == ListSide::Below {
            let bb = crate::annot::items_box(font, &block);
            let dy = (arr_box.y0 - gutter * sref) - bb.y1;
            block = map_items(&block, (0.0, 0.0), 1.0, (0.0, dy));
        }
        if unk {
            ld.add(
                Level::Info,
                "I_GLYPH",
                Some(lay.id),
                None,
                "some characters of the shared DETAIL NOTES are not in the plotter font and were replaced by '?'"
                    .to_string(),
            );
        }
        items.extend(block);
    }
    let mut bounds = crate::annot::items_box(font, &items);
    bounds.add_box(arr_box);
    let (pw, ph) = (bounds.width() / sref, bounds.height() / sref);
    if pw > aw + 1e-6 || ph > ah + 1e-6 {
        layout_fit(c, lay, common, &built, &sizes, list, (pw, ph), &ld);
    }
    let lay_diags = ld.into_vec();
    diags.extend(lay_diags.iter().cloned());
    out_diags.extend(lay_diags);
    let first = &built[0].d;
    let mut d = Drawing::new(
        first.doc.clone(),
        lay.id.to_string(),
        first.kind,
        sref,
        [bounds.x0, bounds.y0, bounds.x1, bounds.y1],
        st,
    );
    d.layers = drawview::collect_layers(&items, st);
    d.items = items;
    d.diagnostics = out_diags;
    d.number = lay.number.to_string();
    d.title = if lay.title.is_empty() { first.doc_title.clone() } else { lay.title.to_string() };
    let one = cells.iter().all(|cl| cl.scale_label == cells[0].scale_label);
    d.scale_label = if one { cells[0].scale_label.clone() } else { "AS NOTED".to_string() };
    d.sheet_no = first.sheet_no.clone();
    d.date = first.date.clone();
    d.code_basis = first.code_basis.clone();
    d.crop = arr_box;
    d.detail_bounds = bounds;
    d.has_unverified = unverified;
    d.author = first.author.clone();
    d.project = first.project.clone();
    d.doc_title = first.doc_title.clone();
    d.cells = cells;
    d.sheet_size = lay.sheet;
    Some(d)
}

/// `W_VIEW_FIT` for a layout: the size it needs against the sheet area, each cell's share, and the fixes that would fit
/// (a smaller common scale, another arrangement, the list on another side).
#[allow(clippy::too_many_arguments)]
fn layout_fit(
    c: &Ctx,
    lay: &Layout,
    common: f64,
    built: &[Built],
    sizes: &[(f64, f64)],
    list: (f64, f64),
    need: (f64, f64),
    diags: &Diags,
) {
    let st = c.st;
    let (aw, ah) = lay.area(st);
    let gutter = st.gutter_in;
    let lid = lay.id;
    let fits = |t: (f64, f64)| t.0 <= aw + 1e-6 && t.1 <= ah + 1e-6;
    let scale_text = if common > 0.0 { units::scale_label(common) } else { "their own scales".to_string() };
    let mut msg = format!(
        "layout {lid} ({}) needs {} x {} paper inches but the sheet area is {} x {} (inside the frame, above the title block, 1/8\" clear of both) with the views at {scale_text}.",
        lay.arrange.text(),
        fixed2(need.0),
        fixed2(need.1),
        fixed2(aw),
        fixed2(ah)
    );
    let mut cells: Vec<String> = Vec::new();
    for (b, s) in built.iter().zip(sizes) {
        cells.push(format!("{} {} x {} at {}", b.d.view, fixed2(s.0), fixed2(s.1), b.d.scale_label));
    }
    msg.push_str(&format!(" Views (paper inches with their notes and title): {}.", cells.join(", ")));
    if list.0 > 0.0 {
        msg.push_str(&format!(
            " The shared DETAIL NOTES list ({}) is {} x {}.",
            lay.side.name(),
            fixed2(list.0),
            fixed2(list.1)
        ));
    }
    let big = (0..sizes.len()).fold(0, |m, k| if sizes[k].0 * sizes[k].1 > sizes[m].0 * sizes[m].1 { k } else { m });
    if let Some(b) = built.get(big) {
        msg.push_str(&format!(" The largest is view {}.", b.d.view));
    }
    let mut fixes: Vec<String> = Vec::new();
    let (sw, sh) = lay.sheet.unwrap_or((st.sheet_w_in, st.sheet_h_in));
    let now = arrange(lay, lay.arrange, lay.side, sizes, list, gutter).total;
    for (w, h, name) in SHEETS {
        let (bw, bh) = drawview::fit_area_of(st, w, h);
        if w * h > sw * sh && now.0 <= bw + 1e-6 && now.1 <= bh + 1e-6 {
            let grow = if lay.scale.is_none() && common > 0.0 {
                " (the common scale then grows to the largest that fits)"
            } else {
                ""
            };
            fixes.push(format!(
                "use a larger sheet: {{\"op\":\"update\",\"path\":\"layouts/{lid}\",\"value\":{{\"sheet\":\"{name}\"}}}}{grow}"
            ));
            break;
        }
    }
    if let Some((f, _)) = lay.scale {
        let sized_at = |sc: f64| -> Vec<(f64, f64)> {
            sizes
                .iter()
                .zip(built)
                .map(|(s, b)| {
                    if b.d.scale != f {
                        return *s;
                    }
                    let (gw, gh) = (b.d.crop.width() / f, b.d.crop.height() / f);
                    (s.0 - gw + gw * f / sc, s.1 - gh + gh * f / sc)
                })
                .collect()
        };
        let pick = drawview::STANDARD_SCALES
            .iter()
            .copied()
            .find(|&s2| s2 > f && fits(arrange(lay, lay.arrange, lay.side, &sized_at(s2), list, gutter).total));
        let to = pick.map_or_else(
            || "\"common\"".to_string(),
            |s2| format!("\"{}\"", units::scale_label(s2).replace(' ', "").replace('"', "\\\"")),
        );
        fixes.push(format!(
            "draw the views smaller: {{\"op\":\"update\",\"path\":\"layouts/{lid}\",\"value\":{{\"scale\":{to}}}}}"
        ));
    }
    if common == 0.0 && lay.scale.is_none() && built.len() > 1 {
        let mut ops: Vec<String> = built
            .iter()
            .filter(|b| b.d.kind != Kind::Iso)
            .filter(|b| view::find_view(c.doc, &b.d.view).is_some_and(|n| n.get("scale").is_some_and(|s| !s.is_null())))
            .map(|b| format!("{{\"op\":\"update\",\"path\":\"views/{}\",\"value\":{{\"scale\":null}}}}", b.d.view))
            .collect();
        if doc_scale(c.doc).is_some() {
            ops.push("{\"op\":\"remove\",\"path\":\"scale\"}".to_string());
        }
        if !ops.is_empty() {
            fixes.push(format!(
                "the views have their own scales; remove them so the layout picks the largest common scale that fits: {}",
                ops.join(", ")
            ));
        }
    }
    for b in built {
        if common > 0.0 && b.d.kind.is_flat() && b.d.scale < common - 1e-9 {
            fixes.push(format!(
                "view {} has its own larger scale {} (the others are at {scale_text}): remove its \"scale\" or set a smaller one",
                b.d.view, b.d.scale_label
            ));
        }
    }
    let n = lay.views().len();
    let mut tries: Vec<(Arrange, &str)> =
        alloc::vec![(Arrange::Row, "{\"arrange\":\"row\"}"), (Arrange::Column, "{\"arrange\":\"column\"}")];
    let grid_ops: Vec<String> = (2..n).map(|k| format!("{{\"arrange\":\"grid\",\"columns\":{k}}}")).collect();
    for (k, g) in (2..n).zip(&grid_ops) {
        tries.push((Arrange::Grid(k), g.as_str()));
    }
    let mut found = false;
    for (a, op) in &tries {
        if *a == lay.arrange {
            continue;
        }
        for side in [lay.side, ListSide::Right, ListSide::Below] {
            if list.0 <= 0.0 && side != lay.side {
                continue;
            }
            if fits(arrange(lay, *a, side, sizes, list, gutter).total) {
                let value = if side == lay.side {
                    (*op).to_string()
                } else {
                    format!("{},\"notes_side\":\"{}\"}}", &op[..op.len() - 1], side.name())
                };
                fixes.push(format!(
                    "arrange the views differently: {{\"op\":\"update\",\"path\":\"layouts/{lid}\",\"value\":{value}}}"
                ));
                found = true;
                break;
            }
        }
        if found {
            break;
        }
    }
    if !found && list.0 > 0.0 {
        for side in [ListSide::Right, ListSide::Below, ListSide::Left, ListSide::Above] {
            if side != lay.side && fits(arrange(lay, lay.arrange, side, sizes, list, gutter).total) {
                fixes.push(format!(
                    "move the list: {{\"op\":\"update\",\"path\":\"layouts/{lid}\",\"value\":{{\"notes_side\":\"{}\"}}}}",
                    side.name()
                ));
                break;
            }
        }
    }
    fixes.push(format!(
        "shrink the crop of view {} or split the layout in two",
        built.get(big).map_or("", |b| b.d.view.as_str())
    ));
    let fix = fixes.iter().enumerate().map(|(i, f)| format!("{}) {f}", i + 1)).collect::<Vec<_>>().join("; ");
    diags.add_fix(Level::Warning, "W_VIEW_FIT", Some(lid), Some(&format!("layouts/{lid}")), msg, fix);
}

fn fixed2(x: f64) -> String {
    let mut buf = [0u8; 40];
    json::fmt_fixed(&mut buf, x, 2).to_string()
}
