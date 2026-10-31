//! Tables (SPEC 26.1): a schedule drawn as a ruled grid of cells, sized in paper inches so it does not depend on the view
//! scale. `read` validates the document form (columns, header rows, body rows, spans), `render` lays it out (column
//! widths, wrapping, row heights) and draws it with its top-left border corner at the origin; the caller places it.
//! `texts` lists every text of a table for the house-style lint and coverage.

use super::text::{ascii_fold, text_item, upper_if, wrap_width};
use super::{Env, non_null, str_member};
use crate::drawing::{Align, Item, PathItem, VAlign};
use crate::geom::{Pt, V2};
use crate::json::{self, Value};
use crate::model::{self, Level};
use crate::num;
use crate::pen::{LayerKey, Pen};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const MAX_COLUMNS: usize = 40;
pub const MAX_ROWS: usize = 300;
const MIN_WIDTH_IN: f64 = 0.1;
const MAX_WIDTH_IN: f64 = 30.0;

/// Which rules a table draws (SPEC 26.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rules {
    /// Border, header rule, every row and column rule.
    All,
    /// Border, header rule, the rules inside the header and every column rule; no rules between body rows.
    Header,
    /// Border and header rule.
    Outer,
    None,
}

struct Cell {
    text: String,
    /// Path of the cell below the table's path, e.g. `rows/2/1` or `header/0/3`.
    path: String,
    row: usize,
    col: usize,
    rs: usize,
    cs: usize,
    align: Align,
}

pub struct Table<'a> {
    pub k: usize,
    pub id: &'a str,
    /// Drawing position of the border's top-left corner, when the author gave `at`.
    pub at: Option<V2>,
    apath: String,
    title: String,
    notes: Vec<String>,
    rules: Rules,
    /// `columns[k].width` in paper inches.
    widths: Vec<Option<f64>>,
    cells: Vec<Cell>,
    /// Cell index per grid slot, row-major.
    owner: Vec<usize>,
    nhead: usize,
    nrows: usize,
    ncols: usize,
}

fn fixed2(x: f64) -> String {
    let mut b = [0u8; 40];
    json::fmt_fixed(&mut b, x, 2).to_string()
}

fn param_error(env: &Env, id: &str, path: &str, msg: String, fix: &str) {
    env.diags.add_fix(Level::Error, "E_PARAM", Some(id), Some(path), msg, fix.to_string());
}

fn read_align(v: Option<&Value>) -> Result<Option<Align>, String> {
    match v.filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => match v.str() {
            Some("left") => Ok(Some(Align::Left)),
            Some("center") => Ok(Some(Align::Center)),
            Some("right") => Ok(Some(Align::Right)),
            _ => Err(model::kind_or_text(v)),
        },
    }
}

/// The text of a cell, a title or a column header: a string, a number (printed as written in JSON) or null (empty).
fn plain_text(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(json::number_string(*n)),
        Some(_) => None,
    }
}

struct RawCell {
    text: String,
    cs: usize,
    rs: usize,
    align: Option<Align>,
}

const CELL_FORMS: &str = "a cell is a string, a number, null (empty) or {\"text\": ..., \"colspan\": n, \"rowspan\": n, \"align\": \"left|center|right\"}";

fn read_cell(
    env: &Env,
    tid: &str,
    (path, full): (&str, &str),
    v: &Value,
    max_cs: usize,
    max_rs: usize,
) -> Option<RawCell> {
    let obj = matches!(v, Value::Object(_));
    let tv = if obj { v.get("text") } else { Some(v) };
    let Some(text) = plain_text(tv) else {
        param_error(
            env,
            tid,
            full,
            format!("table '{tid}': {path} is {}; {CELL_FORMS}", model::kind_or_text(v)),
            "write the cell as a string, e.g. \"24\", or \"\" for an empty cell",
        );
        return None;
    };
    if text.len() > crate::limits::MAX_TEXT_CHARS {
        param_error(
            env,
            tid,
            full,
            format!("table '{tid}': {path} has {} characters (at most {})", text.len(), crate::limits::MAX_TEXT_CHARS),
            "shorten the cell text; long remarks belong in the table's \"notes\"",
        );
        return None;
    }
    if !obj {
        return Some(RawCell { text, cs: 1, rs: 1, align: None });
    }
    let span = |key: &str, max: usize| -> Option<usize> {
        let Some(sv) = non_null(v, key) else { return Some(1) };
        let n = sv.num().and_then(|x| num::to_int::<usize>(x).filter(|&n| n as f64 == x));
        match n.filter(|n| (1..=max).contains(n)) {
            Some(n) => Some(n),
            None => {
                param_error(
                    env,
                    tid,
                    &format!("{full}/{key}"),
                    format!(
                        "table '{tid}': {path} \"{key}\" must be a whole number from 1 to {max} (got {})",
                        model::kind_or_text(sv)
                    ),
                    &if key == "colspan" {
                        format!(
                            "set \"colspan\" to at most {max}: it counts the columns the cell covers, from its own column to the right"
                        )
                    } else {
                        format!(
                            "set \"rowspan\" to at most {max}: it counts the rows the cell covers from its own row down, within its section (header or rows)"
                        )
                    },
                );
                None
            }
        }
    };
    let cs = span("colspan", max_cs)?;
    let rs = span("rowspan", max_rs)?;
    let align = match read_align(v.get("align")) {
        Ok(a) => a,
        Err(got) => {
            param_error(
                env,
                tid,
                &format!("{full}/align"),
                format!("table '{tid}': {path} \"align\" must be \"left\", \"center\" or \"right\" (got {got})"),
                "use \"left\", \"center\" or \"right\", or remove it to take the column's align",
            );
            return None;
        }
    };
    Some(RawCell { text, cs, rs, align })
}

/// Places the rows of one section (`header` or `rows`) on the grid, HTML style: each cell takes the next free slot of
/// its row (slots covered by a rowspan from above are skipped) and must fit the columns and the section.
fn place_section(
    env: &mut Env,
    t: &mut Table,
    key: &str,
    rows: &[Value],
    start: usize,
    col_align: &[Align],
    header: bool,
) -> Option<()> {
    let tid = t.id;
    let ncols = t.ncols;
    let end = start + rows.len();
    let none = usize::MAX;
    for (ri, row) in rows.iter().enumerate() {
        let r = start + ri;
        let rpath = format!("{key}/{ri}");
        let Some(cells) = row.arr() else {
            param_error(
                env,
                tid,
                &format!("{}/{rpath}", t.apath),
                format!("table '{tid}': {rpath} must be an array of cells (got {})", model::kind_or_text(row)),
                "write each row as an array, e.g. [\"#3\", \"24\", \"31\"]",
            );
            return None;
        };
        let from_above: Vec<String> =
            (0..ncols).filter(|&cc| t.owner[r * ncols + cc] != none).map(|cc| (cc + 1).to_string()).collect();
        let above = if from_above.is_empty() {
            String::new()
        } else {
            format!(
                " (column{} {} of this row {} covered by rowspans from rows above)",
                if from_above.len() == 1 { "" } else { "s" },
                from_above.join(", "),
                if from_above.len() == 1 { "is" } else { "are" }
            )
        };
        let mut c = 0usize;
        for (ci, cv) in cells.iter().enumerate() {
            while c < ncols && t.owner[r * ncols + c] != none {
                c += 1;
            }
            let path = format!("{rpath}/{ci}");
            let full = format!("{}/{path}", t.apath);
            if c >= ncols {
                param_error(
                    env,
                    tid,
                    &full,
                    format!(
                        "table '{tid}': {rpath} has more cells than the table has columns: cell {ci} would start after column {ncols}{above}"
                    ),
                    &format!(
                        "remove cell {ci} of {rpath}, reduce a colspan in that row, or add a column to \"columns\""
                    ),
                );
                return None;
            }
            let raw = read_cell(env, tid, (&path, &full), cv, ncols, end - r)?;
            if c + raw.cs > ncols {
                param_error(
                    env,
                    tid,
                    &format!("{full}/colspan"),
                    format!(
                        "table '{tid}': {path} starts in column {} and spans {} columns, past the last column ({ncols}){above}",
                        c + 1,
                        raw.cs
                    ),
                    &format!("set its colspan to {} or less", ncols - c),
                );
                return None;
            }
            if let Some(cc) = (c..c + raw.cs).find(|&cc| t.owner[r * ncols + cc] != none) {
                let other = &t.cells[t.owner[r * ncols + cc]].path;
                param_error(
                    env,
                    tid,
                    &format!("{full}/colspan"),
                    format!(
                        "table '{tid}': {path} spans column {} of {rpath}, which the rowspan of {other} already covers",
                        cc + 1
                    ),
                    &format!("reduce the colspan of {path} to {}", cc - c),
                );
                return None;
            }
            let idx = t.cells.len();
            for rr in r..r + raw.rs {
                for cc in c..c + raw.cs {
                    t.owner[rr * ncols + cc] = idx;
                }
            }
            let align = raw.align.unwrap_or(if header { Align::Center } else { col_align[c] });
            let text = upper_if(env, &raw.text).into_owned();
            t.cells.push(Cell { text, path, row: r, col: c, rs: raw.rs, cs: raw.cs, align });
            c += raw.cs;
        }
        let empty: Vec<String> =
            (0..ncols).filter(|&cc| t.owner[r * ncols + cc] == none).map(|cc| (cc + 1).to_string()).collect();
        if !empty.is_empty() {
            param_error(
                env,
                tid,
                &format!("{}/{rpath}", t.apath),
                format!(
                    "table '{tid}': {rpath} leaves column{} {} empty (the table has {ncols} columns; every row must cover all of them, with colspans){above}",
                    if empty.len() == 1 { "" } else { "s" },
                    empty.join(", ")
                ),
                "add cells to the row (\"\" for an empty cell) or widen a colspan",
            );
            return None;
        }
    }
    Some(())
}

struct Column {
    header: String,
    unit: String,
    width: Option<f64>,
    align: Align,
}

fn read_columns(env: &Env, tid: &str, apath: &str, an: &Value) -> Option<Vec<Column>> {
    let cpath = format!("{apath}/columns");
    let cols = match an.get("columns").and_then(Value::arr) {
        Some(c) if !c.is_empty() && c.len() <= MAX_COLUMNS => c,
        other => {
            let got = match (other, an.get("columns")) {
                (Some(c), _) => format!("{} columns", c.len()),
                (None, Some(v)) => model::kind_or_text(v),
                (None, None) => "nothing".to_string(),
            };
            param_error(
                env,
                tid,
                &cpath,
                format!("table '{tid}' needs \"columns\": an array of 1 to {MAX_COLUMNS} columns (got {got})"),
                "e.g. \"columns\": [\"BAR SIZE\", {\"header\": \"LAP\", \"unit\": \"IN.\", \"width\": 0.6, \"align\": \"center\"}]",
            );
            return None;
        }
    };
    let mut out = Vec::new();
    for (k, cv) in cols.iter().enumerate() {
        let path = format!("{cpath}/{k}");
        let obj = matches!(cv, Value::Object(_));
        let Some(header) = plain_text(if obj { cv.get("header") } else { Some(cv) }) else {
            param_error(
                env,
                tid,
                &path,
                format!(
                    "table '{tid}': columns/{k} is {}; a column is a header string or {{\"header\", \"unit\", \"width\", \"align\"}}",
                    model::kind_or_text(cv)
                ),
                "write e.g. \"BAR SIZE\" or {\"header\": \"LAP\", \"unit\": \"IN.\", \"width\": 0.6}",
            );
            return None;
        };
        let Some(unit) = plain_text(if obj { cv.get("unit") } else { None }) else {
            param_error(
                env,
                tid,
                &format!("{path}/unit"),
                format!("table '{tid}': columns/{k} \"unit\" must be a string, e.g. \"IN.\""),
                "write the unit as a string; it prints as (IN.) under the header",
            );
            return None;
        };
        let mut width = None;
        if let Some(wv) = obj.then(|| non_null(cv, "width")).flatten() {
            match crate::units::parse_length(wv).filter(|w| (MIN_WIDTH_IN..=MAX_WIDTH_IN).contains(w)) {
                Some(w) => width = Some(w),
                None => {
                    param_error(
                        env,
                        tid,
                        &format!("{path}/width"),
                        format!(
                            "table '{tid}': columns/{k} \"width\" must be a paper width from {MIN_WIDTH_IN} to {MAX_WIDTH_IN} inches (got {})",
                            model::kind_or_text(wv)
                        ),
                        "give the printed column width in paper inches, e.g. 0.75, or remove it to fit the widest cell",
                    );
                    return None;
                }
            }
        }
        let align = match read_align(if obj { cv.get("align") } else { None }) {
            Ok(a) => a.unwrap_or(Align::Center),
            Err(got) => {
                param_error(
                    env,
                    tid,
                    &format!("{path}/align"),
                    format!(
                        "table '{tid}': columns/{k} \"align\" must be \"left\", \"center\" or \"right\" (got {got})"
                    ),
                    "use \"left\", \"center\" (the default) or \"right\"",
                );
                return None;
            }
        };
        out.push(Column { header, unit, width, align });
    }
    Some(out)
}

fn unit_text(unit: &str) -> String {
    if unit.starts_with('(') { unit.to_string() } else { format!("({unit})") }
}

/// A table annotation, or `None` (with diagnostics) when it cannot be drawn.
pub fn read<'a>(env: &mut Env<'a, '_>, an: &'a Value, k: usize, id: &'a str, apath: &str) -> Option<Table<'a>> {
    let cols = read_columns(env, id, apath, an)?;
    let ncols = cols.len();
    let rows_of = |key: &str| -> Result<&'a [Value], String> {
        match an.get(key) {
            None | Some(Value::Null) => Ok(&[]),
            Some(v) => v.arr().ok_or_else(|| model::kind_or_text(v)),
        }
    };
    let (header, rows) = match (rows_of("header"), rows_of("rows")) {
        (Ok(h), Ok(r)) => (h, r),
        (Err(got), _) | (_, Err(got)) => {
            let key = if rows_of("header").is_err() { "header" } else { "rows" };
            param_error(
                env,
                id,
                &format!("{apath}/{key}"),
                format!("table '{id}': \"{key}\" must be an array of rows, each an array of cells (got {got})"),
                "e.g. \"rows\": [[\"#3\", \"24\", \"31\"], [\"#4\", \"32\", \"41\"]]",
            );
            return None;
        }
    };
    let col_heads = cols.iter().any(|c| !c.header.is_empty());
    if !header.is_empty() && col_heads {
        param_error(
            env,
            id,
            &format!("{apath}/header"),
            format!(
                "table '{id}' has both \"header\" rows and column headers in \"columns\"; use one: \"header\" for grouped (colspan/rowspan) headers, column headers for one plain header row"
            ),
            "move the column headers into the last \"header\" row, or remove \"header\"",
        );
        return None;
    }
    let nhead = if header.is_empty() {
        usize::from(col_heads || cols.iter().any(|c| !c.unit.is_empty()))
    } else {
        header.len()
    };
    let nrows = nhead + rows.len();
    if rows.is_empty() && nhead == 0 {
        param_error(
            env,
            id,
            &format!("{apath}/rows"),
            format!("table '{id}' has no rows: give \"rows\" (and column headers or \"header\")"),
            "e.g. \"rows\": [[\"#3\", \"24\", \"31\"]]",
        );
        return None;
    }
    if nrows > MAX_ROWS {
        param_error(
            env,
            id,
            &format!("{apath}/rows"),
            format!("table '{id}' has {nrows} rows (header and body); at most {MAX_ROWS}"),
            "split the schedule into two tables",
        );
        return None;
    }
    let rules = match str_member(an, "rules").or(non_null(an, "rules").map(|_| "")) {
        None | Some("all") => Rules::All,
        Some("header") => Rules::Header,
        Some("outer") => Rules::Outer,
        Some("none") => Rules::None,
        Some(_) => {
            param_error(
                env,
                id,
                &format!("{apath}/rules"),
                format!(
                    "table '{id}': \"rules\" must be \"all\", \"header\", \"outer\" or \"none\" (got {})",
                    model::kind_or_text(an.get("rules").unwrap_or(&Value::Null))
                ),
                "use \"all\" (every cell ruled), \"header\" (no rules between body rows), \"outer\" (border and header rule) or \"none\" (text only)",
            );
            return None;
        }
    };
    let Some(title) = plain_text(an.get("title")) else {
        param_error(
            env,
            id,
            &format!("{apath}/title"),
            format!("table '{id}': \"title\" must be a string"),
            "e.g. \"title\": \"CLASS B TENSION LAP SPLICE (IN.)\"",
        );
        return None;
    };
    let mut notes = Vec::new();
    if let Some(nv) = non_null(an, "notes") {
        let list = nv.arr().filter(|a| a.iter().all(|x| x.str().is_some()));
        let Some(list) = list else {
            param_error(
                env,
                id,
                &format!("{apath}/notes"),
                format!("table '{id}': \"notes\" must be an array of strings (got {})", model::kind_or_text(nv)),
                "e.g. \"notes\": [\"VALUES IN INCHES\", \"* CLASS A WHERE NOTED ON PLANS\"]",
            );
            return None;
        };
        for x in list {
            let s = x.str().unwrap_or("");
            if super::notes::text_too_long(env, "table note of", id, apath, s) {
                return None;
            }
            notes.push(upper_if(env, s).into_owned());
        }
    }
    let at = match non_null(an, "at") {
        Some(atv) => {
            let p = env.scene.point(atv, id, &format!("{apath}/at"))?;
            Some(super::at_drawn(env, atv, "", p))
        }
        None => None,
    };
    let mut t = Table {
        k,
        id,
        at,
        apath: apath.to_string(),
        title: upper_if(env, &title).into_owned(),
        notes,
        rules,
        widths: cols.iter().map(|c| c.width).collect(),
        cells: Vec::new(),
        owner: alloc::vec![usize::MAX; nrows * ncols],
        nhead,
        nrows,
        ncols,
    };
    let col_align: Vec<Align> = cols.iter().map(|c| c.align).collect();
    if header.is_empty() && nhead == 1 {
        let synth: Vec<Value> = cols
            .iter()
            .map(|c| {
                let mut s = c.header.clone();
                if !c.unit.is_empty() {
                    if !s.is_empty() {
                        s.push('\n');
                    }
                    s.push_str(&unit_text(&c.unit));
                }
                Value::String(s)
            })
            .collect();
        place_section(env, &mut t, "columns", &[Value::Array(synth)], 0, &col_align, true)?;
        for (k, cell) in t.cells.iter_mut().enumerate() {
            cell.path = format!("columns/{k}");
        }
    } else {
        place_section(env, &mut t, "header", header, 0, &col_align, true)?;
        for (k, c) in cols.iter().enumerate().filter(|(_, c)| !c.unit.is_empty()) {
            let bottom = t.owner[(nhead - 1) * ncols + k];
            match t.cells.get_mut(bottom).filter(|cell| cell.cs == 1) {
                Some(cell) => {
                    let u = upper_if(env, &unit_text(&c.unit)).into_owned();
                    if !cell.text.is_empty() {
                        cell.text.push('\n');
                    }
                    cell.text.push_str(&u);
                }
                None => env.diags.add_fix(
                    Level::Warning,
                    "W_PARAM",
                    Some(id),
                    Some(&format!("{apath}/columns/{k}/unit")),
                    format!(
                        "table '{id}': the unit of columns/{k} is not printed: the lowest header cell over column {} spans several columns",
                        k + 1
                    ),
                    "write the unit into that header cell's text, e.g. \"LAP (IN.)\", and remove \"unit\"".to_string(),
                ),
            }
        }
    }
    place_section(env, &mut t, "rows", rows, nhead, &col_align, false)?;
    Some(t)
}

/// Paper sizes (model units at the view scale) of the table layout.
struct Metrics {
    h: f64,
    pitch: f64,
    padx: f64,
    pady: f64,
}

fn metrics(env: &Env) -> Metrics {
    let h = env.style.text_height_in * env.s;
    Metrics { h, pitch: h * env.style.line_spacing, padx: 0.6 * h, pady: 0.7 * h }
}

fn text_width(env: &Env, s: &str, h: f64) -> f64 {
    env.font.width(ascii_fold(s), h)
}

/// The width a cell's text wants: its longest line (each `\n` paragraph unwrapped) up to `cap`, but at least its widest
/// word, plus the padding.
fn wanted_width(env: &Env, text: &str, m: &Metrics, cap: f64) -> f64 {
    let mut natural: f64 = 0.0;
    let mut word: f64 = 0.0;
    for para in text.split('\n') {
        natural = natural.max(text_width(env, para, m.h));
        for w in para.split(' ') {
            word = word.max(text_width(env, w, m.h));
        }
    }
    natural.min(cap).max(word) + 2.0 * m.padx
}

/// Column boundaries (x from 0 to the right) and row boundaries (distance down from the top), model units.
struct Grid {
    xs: Vec<f64>,
    ys: Vec<f64>,
    lines: Vec<Vec<String>>,
}

fn layout(env: &Env, t: &Table, m: &Metrics) -> Grid {
    let s = env.s;
    let cap = env.style.wrap_chars * 0.75 * m.h;
    let min_w = 3.0 * m.h + 2.0 * m.padx;
    let mut w: Vec<f64> = t.widths.iter().map(|x| x.map_or(min_w, |x| x * s)).collect();
    for c in t.cells.iter().filter(|c| c.cs == 1 && t.widths[c.col].is_none()) {
        w[c.col] = w[c.col].max(wanted_width(env, &c.text, m, cap));
    }
    let mut spans: Vec<&Cell> = t.cells.iter().filter(|c| c.cs > 1).collect();
    crate::sort::stable(&mut spans, |a, b| a.cs < b.cs);
    for c in spans {
        let need = wanted_width(env, &c.text, m, cap * c.cs as f64);
        let have: f64 = w[c.col..c.col + c.cs].iter().sum();
        let auto: Vec<usize> = (c.col..c.col + c.cs).filter(|&k| t.widths[k].is_none()).collect();
        if need > have && !auto.is_empty() {
            let add = (need - have) / auto.len() as f64;
            for k in auto {
                w[k] += add;
            }
        }
    }
    let mut xs = alloc::vec![0.0];
    for x in &w {
        xs.push(xs[xs.len() - 1] + x);
    }
    let lines: Vec<Vec<String>> = t
        .cells
        .iter()
        .map(|c| {
            if c.text.is_empty() {
                return Vec::new();
            }
            wrap_width(env.font, &c.text, m.h, xs[c.col + c.cs] - xs[c.col] - 2.0 * m.padx)
        })
        .collect();
    let block = |n: usize| if n == 0 { m.h } else { m.h + (n - 1) as f64 * m.pitch };
    let mut rh = alloc::vec![m.h + 2.0 * m.pady; t.nrows];
    for (c, l) in t.cells.iter().zip(&lines).filter(|(c, _)| c.rs == 1) {
        rh[c.row] = rh[c.row].max(block(l.len()) + 2.0 * m.pady);
    }
    let mut tall: Vec<(&Cell, &Vec<String>)> = t.cells.iter().zip(&lines).filter(|(c, _)| c.rs > 1).collect();
    crate::sort::stable(&mut tall, |a, b| a.0.rs < b.0.rs);
    for (c, l) in tall {
        let need = block(l.len()) + 2.0 * m.pady;
        let have: f64 = rh[c.row..c.row + c.rs].iter().sum();
        if need > have {
            rh[c.row + c.rs - 1] += need - have;
        }
    }
    let mut ys = alloc::vec![0.0];
    for y in &rh {
        ys.push(ys[ys.len() - 1] + y);
    }
    Grid { xs, ys, lines }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    Header,
    Light,
}

fn rule_item(env: &Env, kind: RuleKind, src: &str, a: V2, b: V2) -> Item {
    let pen = match kind {
        RuleKind::Header => Pen::Profile,
        RuleKind::Light => Pen::Anno,
    };
    Item::Path(PathItem {
        layer: String::from(env.style.layer_name(LayerKey::Notes)),
        pen,
        src: String::from(src),
        closed: false,
        pts: alloc::vec![Pt::at(a, 0.0), Pt::at(b, 0.0)],
    })
}

/// The runs of consecutive true slots: (first, one past the last).
fn runs(on: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (k, &v) in on.iter().chain(core::iter::once(&false)).enumerate() {
        match (v, start) {
            (true, None) => start = Some(k),
            (false, Some(s0)) => {
                out.push((s0, k));
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn rules_items(env: &Env, t: &Table, g: &Grid, out: &mut Vec<Item>) {
    if t.rules == Rules::None {
        return;
    }
    let (w, h) = (g.xs[t.ncols], g.ys[t.nrows]);
    let own = |r: usize, c: usize| t.owner[r * t.ncols + c];
    for r in 1..t.nrows {
        let kind = if r == t.nhead { RuleKind::Header } else { RuleKind::Light };
        let drawn = match t.rules {
            Rules::All => true,
            Rules::Header => r <= t.nhead,
            Rules::Outer => r == t.nhead,
            Rules::None => false,
        };
        if !drawn {
            continue;
        }
        let on: Vec<bool> = (0..t.ncols).map(|c| own(r - 1, c) != own(r, c)).collect();
        for (c0, c1) in runs(&on) {
            out.push(rule_item(env, kind, t.id, V2::init(g.xs[c0], -g.ys[r]), V2::init(g.xs[c1], -g.ys[r])));
        }
    }
    if matches!(t.rules, Rules::All | Rules::Header) {
        for c in 1..t.ncols {
            let on: Vec<bool> = (0..t.nrows).map(|r| own(r, c - 1) != own(r, c)).collect();
            for (r0, r1) in runs(&on) {
                out.push(rule_item(
                    env,
                    RuleKind::Light,
                    t.id,
                    V2::init(g.xs[c], -g.ys[r0]),
                    V2::init(g.xs[c], -g.ys[r1]),
                ));
            }
        }
    }
    let corners = [V2::init(0.0, 0.0), V2::init(w, 0.0), V2::init(w, -h), V2::init(0.0, -h)];
    out.push(Item::Path(PathItem {
        layer: String::from(env.style.layer_name(LayerKey::Notes)),
        pen: Pen::Title,
        src: String::from(t.id),
        closed: true,
        pts: corners.iter().map(|&p| Pt::at(p, 0.0)).collect(),
    }));
}

fn text_at(env: &mut Env, src: &str, s: &str, x: f64, y: f64, h: f64, align: Align) -> Item {
    text_item(env, LayerKey::Notes, Pen::Anno, src, s, x, y, h, 0.0, align, VAlign::Baseline)
}

/// `W_TABLE_CELL`: a cell's text is wider than its fixed-width column(s) and runs over the rule.
fn report_overflow(env: &Env, t: &Table, g: &Grid, m: &Metrics) {
    let vid = env.spec.id;
    for (c, lines) in t.cells.iter().zip(&g.lines) {
        let avail = g.xs[c.col + c.cs] - g.xs[c.col] - 2.0 * m.padx;
        let widest = lines.iter().map(|l| text_width(env, l, m.h)).fold(0.0, f64::max);
        if widest <= avail + 1e-9 {
            continue;
        }
        let s = env.s;
        let need = (widest + 2.0 * m.padx) / s;
        let cols =
            if c.cs == 1 { format!("column {}", c.col + 1) } else { format!("columns {}-{}", c.col + 1, c.col + c.cs) };
        let fixed: Vec<String> =
            (c.col..c.col + c.cs).filter(|&k| t.widths[k].is_some()).map(|k| format!("columns/{k}/width")).collect();
        let widen = if c.cs == 1 {
            format!("set columns/{}/width to {} or more", c.col, fixed2(math_ceil2(need)))
        } else {
            format!("widen {} so the {cols} add up to {}\" or more", fixed.join(", "), fixed2(math_ceil2(need)))
        };
        env.diags.add_fix(
            Level::Warning,
            "W_TABLE_CELL",
            Some(t.id),
            Some(&format!("{}/{}", t.apath, c.path)),
            format!(
                "table '{}' in view {vid}: {} \"{}\" needs {}\" (paper) but {cols} {} {}\" wide, so its text runs over the rule",
                t.id,
                c.path,
                c.text.replace('\n', " "),
                fixed2(need),
                if c.cs == 1 { "is" } else { "are" },
                fixed2((avail + 2.0 * m.padx) / s)
            ),
            format!("{widen}, remove the width to fit the text, or split the long word (a space or \"\\n\" lets it wrap)"),
        );
    }
}

fn math_ceil2(x: f64) -> f64 {
    crate::math::ceil(x * 20.0 - 1e-9) / 20.0
}

/// The table drawn with the top-left corner of its border at the origin: title above (centred, underlined), cells, rules,
/// notes below. Reports cells whose text overflows a fixed-width column.
pub fn render(env: &mut Env, t: &Table) -> Vec<Item> {
    let m = metrics(env);
    let g = layout(env, t, &m);
    report_overflow(env, t, &g, &m);
    let mut out = Vec::new();
    for (c, lines) in t.cells.iter().zip(&g.lines) {
        let (x0, x1) = (g.xs[c.col], g.xs[c.col + c.cs]);
        let (top, bot) = (-g.ys[c.row], -g.ys[c.row + c.rs]);
        let n = lines.len();
        if n == 0 {
            continue;
        }
        let block = m.h + (n - 1) as f64 * m.pitch;
        let first = (top + bot) * 0.5 + block * 0.5 - m.h;
        let x = match c.align {
            Align::Left => x0 + m.padx,
            Align::Center => (x0 + x1) * 0.5,
            Align::Right => x1 - m.padx,
        };
        for (i, l) in lines.iter().enumerate() {
            let it = text_at(env, t.id, l, x, first - i as f64 * m.pitch, m.h, c.align);
            out.push(it);
        }
    }
    rules_items(env, t, &g, &mut out);
    let w = g.xs[t.ncols];
    if !t.title.is_empty() {
        let lines = wrap_width(env.font, &t.title, m.h, w);
        let n = lines.len();
        let widest = lines.iter().map(|l| text_width(env, l, m.h)).fold(0.0, f64::max);
        let rule_y = 0.6 * m.h;
        let last = rule_y + 0.35 * m.h;
        for (i, l) in lines.iter().enumerate() {
            let it = text_at(env, t.id, l, w * 0.5, last + (n - 1 - i) as f64 * m.pitch, m.h, Align::Center);
            out.push(it);
        }
        out.push(rule_item(
            env,
            RuleKind::Light,
            t.id,
            V2::init(w * 0.5 - widest * 0.5, rule_y),
            V2::init(w * 0.5 + widest * 0.5, rule_y),
        ));
    }
    if !t.notes.is_empty() {
        let numbered = t.notes.iter().filter(|n| !n.starts_with('*')).count();
        let mut y = -g.ys[t.nrows] - 2.0 * m.h;
        if numbered > 0 {
            let head = upper_if(env, if numbered == 1 { "NOTE:" } else { "NOTES:" }).into_owned();
            out.push(text_at(env, t.id, &head, 0.0, y, m.h, Align::Left));
            y -= m.pitch;
        }
        let wrap_w = w.max(env.style.wrap_chars * 1.25 * 0.75 * m.h);
        let indent = text_width(env, &format!("{numbered}.  "), m.h);
        let mut k = 0usize;
        for note in &t.notes {
            let (x, lines) = if note.starts_with('*') {
                (0.0, wrap_width(env.font, note, m.h, wrap_w))
            } else {
                k += 1;
                out.push(text_at(env, t.id, &format!("{k}."), 0.0, y, m.h, Align::Left));
                (indent, wrap_width(env.font, note, m.h, wrap_w - indent))
            };
            for l in lines {
                out.push(text_at(env, t.id, &l, x, y, m.h, Align::Left));
                y -= m.pitch;
            }
        }
    }
    out
}

/// Moves drawn table items by `d`.
pub fn translate(items: &mut [Item], d: V2) {
    for it in items {
        match it {
            Item::Path(p) => {
                for q in &mut p.pts {
                    q.x += d.x;
                    q.y += d.y;
                }
            }
            Item::Text(tx) => {
                tx.x += d.x;
                tx.y += d.y;
            }
            _ => {}
        }
    }
}

/// Every text of a table annotation in the document with its path below the annotation: title, column headers and
/// units, header and body cells, notes. Numbers are left out (nothing to lint, and coverage reads them from the rows).
pub fn texts(an: &Value) -> Vec<(String, &str)> {
    let mut out: Vec<(String, &str)> = Vec::new();
    if let Some(t) = an.get("title").and_then(Value::str) {
        out.push(("title".to_string(), t));
    }
    for (k, c) in an.get("columns").and_then(Value::arr).unwrap_or(&[]).iter().enumerate() {
        if let Some(s) = c.str() {
            out.push((format!("columns/{k}"), s));
        }
        for key in ["header", "unit"] {
            if let Some(s) = c.get(key).and_then(Value::str) {
                out.push((format!("columns/{k}/{key}"), s));
            }
        }
    }
    for key in ["header", "rows"] {
        for (r, row) in an.get(key).and_then(Value::arr).unwrap_or(&[]).iter().enumerate() {
            for (c, cell) in row.arr().unwrap_or(&[]).iter().enumerate() {
                let s = cell.str().or_else(|| cell.get("text").and_then(Value::str));
                if let Some(s) = s {
                    out.push((format!("{key}/{r}/{c}"), s));
                }
            }
        }
    }
    for (k, n) in an.get("notes").and_then(Value::arr).unwrap_or(&[]).iter().enumerate() {
        if let Some(s) = n.str() {
            out.push((format!("notes/{k}"), s));
        }
    }
    out
}

/// The texts of one body row (strings and numbers) for coverage.
pub fn row_texts(an: &Value) -> Vec<String> {
    let cell = |v: &Value| match v {
        Value::Object(_) => plain_text(v.get("text")).unwrap_or_default(),
        _ => plain_text(Some(v)).unwrap_or_default(),
    };
    an.get("rows")
        .and_then(Value::arr)
        .unwrap_or(&[])
        .iter()
        .map(|row| row.arr().unwrap_or(&[]).iter().map(cell).collect::<Vec<_>>().join(" "))
        .collect()
}
