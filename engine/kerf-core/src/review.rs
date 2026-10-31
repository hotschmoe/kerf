//! Review overlay of an export (SPEC 25.4, `export` `review: true`): every component with an acknowledged warning
//! gets a dashed box and an `ACK n` tag, and the sheet (or the space above the detail) gets a stamp
//! `N ACKNOWLEDGED WARNINGS` with one legend line per tag. The default export never carries it.

use crate::annot;
use crate::drawing::{Drawing, Item, PathItem, TextItem};
use crate::drawview;
use crate::font::{self, Font};
use crate::geom::{self, Box, Pt, V2};
use crate::lint::Acked;
use crate::pen::Pen;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const SRC: &str = "review";
const PAD_IN: f64 = 0.08;
const LEGEND_LINES: usize = 10;
const LEGEND_CHARS: usize = 110;

fn is_of(src: &str, id: &str) -> bool {
    src == id || src.strip_prefix(id).is_some_and(|r| r.starts_with('#'))
}

/// The model-space extent of what the drawing draws for component `id`.
fn extent(d: &Drawing<'_>, id: &str) -> Box {
    let mut b = Box::default();
    let mut add = |pts: &[Pt]| b.add_box(geom::points_box(pts));
    for it in &d.items {
        match it {
            Item::Path(p) if is_of(&p.src, id) => add(&p.pts),
            Item::Fill(f) if is_of(&f.src, id) => f.loops.iter().for_each(|l| add(l)),
            Item::Hatch(h) if is_of(&h.src, id) => h.loops.iter().for_each(|l| add(l)),
            Item::Region(r) if is_of(&r.src, id) => r.loops.iter().for_each(|l| add(l)),
            _ => {}
        }
    }
    b
}

fn upper(s: &str) -> String {
    String::from_utf8_lossy(&font::upper_ascii(annot::ascii_fold(s).as_bytes())).into_owned()
}

fn clip_chars(mut s: String, n: usize) -> String {
    if s.len() > n {
        let mut cut = n.saturating_sub(3);
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("...");
    }
    s
}

/// One legend line per acknowledged component: its tag, id and the codes with counts and reasons.
fn legend_line(tag: usize, comp: &str, acked: &[&Acked]) -> String {
    let mut groups: Vec<(&str, &str, usize)> = Vec::new();
    for a in acked {
        match groups.iter_mut().find(|g| g.0 == a.warning.code && g.1 == a.reason) {
            Some(g) => g.2 += 1,
            None => groups.push((a.warning.code, &a.reason, 1)),
        }
    }
    let mut s = format!("ACK {tag}  {comp}: ");
    for (i, (code, reason, n)) in groups.iter().enumerate() {
        if i > 0 {
            s.push_str("; ");
        }
        s.push_str(&format!("{code} X{n} {reason}"));
    }
    clip_chars(upper(&s), LEGEND_CHARS)
}

fn comps_of(acked: &[Acked]) -> Vec<&str> {
    let mut comps: Vec<&str> = Vec::new();
    for a in acked {
        if !comps.contains(&a.comp.as_str()) {
            comps.push(&a.comp);
        }
    }
    comps
}

/// The stamp and the legend lines (at most LEGEND_LINES).
fn legend(acked: &[Acked]) -> (String, Vec<String>) {
    let n = acked.len();
    let stamp = format!("{n} ACKNOWLEDGED WARNING{}", if n == 1 { "" } else { "S" });
    let comps = comps_of(acked);
    let mut lines: Vec<String> = comps
        .iter()
        .enumerate()
        .map(|(k, comp)| {
            let mine: Vec<&Acked> = acked.iter().filter(|a| a.comp == *comp).collect();
            legend_line(k + 1, comp, &mine)
        })
        .collect();
    if lines.len() > LEGEND_LINES {
        let more = lines.len() - (LEGEND_LINES - 1);
        lines.truncate(LEGEND_LINES - 1);
        lines.push(format!("+{more} MORE COMPONENTS: KERF CHECK LISTS EVERY ACKNOWLEDGED WARNING"));
    }
    (stamp, lines)
}

struct Pens {
    layer: String,
}

impl Pens {
    fn text(&self, s: String, x: f64, y: f64, h: f64, pen: Pen) -> Item {
        Item::Text(TextItem::new(self.layer.clone(), pen, String::from(SRC), s, x, y, h))
    }
}

/// A dashed box around every acknowledged component and an `ACK n` tag on its top edge (tags never overprint).
fn marks(d: &Drawing<'_>, font: &Font, pens: &Pens) -> Vec<Item> {
    let s = d.scale;
    let th = d.style.text_height_in * s;
    let acked = d.acknowledged.as_deref().unwrap_or(&[]);
    let mut items: Vec<Item> = Vec::new();
    let mut tags: Vec<Box> = Vec::new();
    for (k, comp) in comps_of(acked).iter().enumerate() {
        let b = extent(d, comp);
        if b.is_empty() {
            continue;
        }
        let p = PAD_IN * s;
        let (x0, y0, x1, y1) = (b.x0 - p, b.y0 - p, b.x1 + p, b.y1 + p);
        let pts = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].iter().map(|q| Pt::at(V2::init(q[0], q[1]), 0.0)).collect();
        items.push(Item::Path(PathItem {
            layer: pens.layer.clone(),
            pen: Pen::Hidden,
            src: String::from(SRC),
            closed: true,
            pts,
        }));
        let label = format!("ACK {}", k + 1);
        let w = font.width(&label, th);
        let ty = y1 + 0.3 * th;
        let mut placed = Box { x0, y0: ty, x1: x0 + w, y1: ty + th };
        while let Some(hit) = tags.iter().find(|t| t.overlaps(placed, 0.0)) {
            let tx = hit.x1 + 0.5 * th;
            placed = Box { x0: tx, y0: ty, x1: tx + w, y1: ty + th };
        }
        tags.push(placed);
        items.push(pens.text(label, placed.x0, ty, th, Pen::Anno));
    }
    items
}

/// The stamp and legend as a block whose top-left corner is (x, top), text height `th`.
fn block(pens: &Pens, stamp: &str, lines: &[String], x: f64, top: f64, th: f64) -> Vec<Item> {
    let sh = 1.6 * th;
    let mut out = alloc::vec![pens.text(String::from(stamp), x, top - sh, sh, Pen::Title)];
    let mut y = top - sh - 0.4 * th;
    for line in lines {
        y -= 1.5 * th;
        out.push(pens.text(line.clone(), x, y + 0.5 * th, th, Pen::Anno));
    }
    out
}

fn block_size(font: &Font, stamp: &str, lines: &[String], th: f64) -> (f64, f64) {
    let w = lines.iter().map(|l| font.width(l, th)).fold(font.width(stamp, 1.6 * th), f64::max);
    (w, 1.6 * th + 0.4 * th + 1.5 * th * lines.len() as f64)
}

/// Review overlay of a drawing without a sheet: the marks, and the stamp and legend above everything.
pub fn overlay<'a>(mut d: Drawing<'a>, font: &Font) -> Drawing<'a> {
    let st = d.style;
    let th = st.text_height_in * d.scale;
    let pens = Pens { layer: String::from(st.layer_for_pen(Pen::Anno)) };
    let mut items = marks(&d, font, &pens);
    let (stamp, lines) = legend(d.acknowledged.as_deref().unwrap_or(&[]));
    let all = annot::items_box(font, &d.items);
    let left = if all.is_empty() { d.bounds[0] } else { all.x0.min(d.bounds[0]) };
    let below = 0.25 * d.scale + annot::items_box(font, &items).y1.max(all.y1).max(d.bounds[3]);
    let (_, h) = block_size(font, &stamp, &lines, th);
    items.extend(block(&pens, &stamp, &lines, left, below + h, th));
    let ib = annot::items_box(font, &items);
    d.bounds = [d.bounds[0].min(ib.x0), d.bounds[1].min(ib.y0), d.bounds[2].max(ib.x1), d.bounds[3].max(ib.y1)];
    d.items.extend(items);
    d.layers = drawview::collect_layers(&d.items, st);
    d
}

/// Review overlay of a sheet (`sheet::with_sheet` output): the marks, and the stamp and legend at the first free spot
/// of the frame (scanning from the top left), with smaller text or fewer legend lines when nothing fits.
pub fn overlay_sheet<'a>(mut d: Drawing<'a>, font: &Font) -> Drawing<'a> {
    let st = d.style;
    let s = d.scale;
    let pens = Pens { layer: String::from(st.layer_for_pen(Pen::Anno)) };
    let mut items = marks(&d, font, &pens);
    let taken: Vec<Box> = d
        .items
        .iter()
        .filter(|it| it.src() != "sheet")
        .chain(items.iter())
        .map(|it| annot::items_box(font, core::slice::from_ref(it)))
        .filter(|b| !b.is_empty())
        .collect();
    let g = 0.1 * s;
    let (m, tbh) = (st.margin_in * s, st.title_block_h_in * s);
    let f = Box {
        x0: d.bounds[0] + m + g,
        y0: d.bounds[1] + m + tbh + g,
        x1: d.bounds[2] - m - g,
        y1: d.bounds[3] - m - g,
    };
    let free = |w: f64, h: f64| -> Option<(f64, f64)> {
        let mut top = f.y1;
        while top - h >= f.y0 - 1e-9 {
            let mut x = f.x0;
            while x + w <= f.x1 + 1e-9 {
                let b = Box { x0: x - 1.5 * g, y0: top - h - 1.5 * g, x1: x + w + 1.5 * g, y1: top + 1.5 * g };
                match taken.iter().find(|t| t.overlaps(b, 0.0)) {
                    None => return Some((x, top)),
                    Some(t) => x = (t.x1 + 1.5 * g).max(x + g),
                }
            }
            top -= g;
        }
        None
    };
    let (stamp, mut lines) = legend(d.acknowledged.as_deref().unwrap_or(&[]));
    let th0 = st.text_height_in * s;
    let mut placed = None;
    'fit: loop {
        for k in [1.0, 0.8, 0.65] {
            let (w, h) = block_size(font, &stamp, &lines, th0 * k);
            if let Some((x, top)) = free(w, h) {
                placed = Some((x, top, th0 * k));
                break 'fit;
            }
        }
        if lines.pop().is_none() {
            break;
        }
    }
    let (x, top, th) = placed.unwrap_or((f.x0, f.y1, th0 * 0.65));
    items.extend(block(&pens, &stamp, &lines, x, top, th));
    d.items.extend(items);
    d.layers = drawview::collect_layers(&d.items, st);
    d
}
