//! Sheet (SPEC 12, DESIGN 5): frame, title block and footer around a detail placed at true scale.
//! `with_sheet` returns a Drawing whose bounds are the full page, in model coordinates (the sheet
//! geometry is built in paper inches and mapped through the inverse of the paper scale), so every
//! exporter can treat sheet and detail uniformly.

use crate::annot;
use crate::drawing::{Drawing, FillItem, Item, PathItem, TextItem};
use crate::font::Font;
use crate::geom::{Pt, V2};
use crate::house;
use crate::pen::{LayerKey, Pen};
use crate::style::Style;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

struct Ctx<'a> {
    st: &'a Style,
    /// paper -> model
    s: f64,
    ox: f64,
    oy: f64,
    items: Vec<Item>,
}

impl Ctx<'_> {
    fn m(&self, x: f64, y: f64) -> V2 {
        V2::init(self.ox + x * self.s, self.oy + y * self.s)
    }

    fn layer(&self, key: LayerKey) -> String {
        String::from(self.st.layer_name(key))
    }

    fn path(&mut self, pen: Pen, pts: &[[f64; 2]], closed: bool) {
        let out = pts.iter().map(|q| Pt::at(self.m(q[0], q[1]), 0.0)).collect();
        self.items.push(Item::Path(PathItem {
            layer: String::from(self.st.layer_for_pen(pen)),
            pen,
            src: String::from("sheet"),
            closed,
            pts: out,
        }));
    }

    fn rect(&mut self, pen: Pen, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.path(pen, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], true);
    }

    fn fill_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        let cs = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        let l = cs.iter().map(|q| Pt::at(self.m(q[0], q[1]), 0.0)).collect();
        self.items.push(Item::Fill(FillItem {
            layer: self.layer(LayerKey::Title),
            src: String::from("sheet"),
            loops: alloc::vec![l],
        }));
    }

    fn text(&mut self, pen: Pen, s: &str, x: f64, y: f64, h: f64) {
        let p = self.m(x, y);
        self.items.push(Item::Text(TextItem::new(
            self.layer(LayerKey::Title),
            pen,
            String::from("sheet"),
            String::from(s),
            p.x,
            p.y,
            h * self.s,
        )));
    }
}

/// A title block value in its cell: one line at `h`, shrunk to the cell width; when that would go below 3/4 of `h`, two
/// lines split at the space that balances them (at most `two_h` high each) if they come out larger. Never wider than
/// `maxw`.
fn fit_cell(font: &Font, s: &str, h: f64, maxw: f64, two_h: f64) -> (Vec<String>, f64) {
    let w = font.width(s, h);
    if w <= maxw {
        return (alloc::vec![String::from(s)], h);
    }
    let one = h * maxw / w;
    if one >= 0.75 * h {
        return (alloc::vec![String::from(s)], one);
    }
    let mut best: Option<(usize, f64)> = None;
    for (k, _) in s.match_indices(' ') {
        let wide = font.width(&s[..k], h).max(font.width(&s[k + 1..], h));
        if best.is_none_or(|(_, bw)| wide < bw) {
            best = Some((k, wide));
        }
    }
    if let Some((k, wide)) = best {
        let two = (h * maxw / wide).min(h).min(two_h);
        if two > one {
            return (alloc::vec![String::from(&s[..k]), String::from(&s[k + 1..])], two);
        }
    }
    (alloc::vec![String::from(s)], one)
}

fn upper(s: &str, st: &Style) -> String {
    house::upper(s, st)
}

pub fn with_sheet<'a>(d: Drawing<'a>, font: &Font) -> Drawing<'a> {
    let st = d.style;
    let s = d.scale;
    let (w, h) = d.sheet_size.unwrap_or((st.sheet_w_in, st.sheet_h_in));
    let (m, tbh) = (st.margin_in, st.title_block_h_in);
    let bw = (d.bounds[2] - d.bounds[0]) / s;
    let bh = (d.bounds[3] - d.bounds[1]) / s;
    let fw = w - 2.0 * m;
    let fh = h - 2.0 * m - tbh;
    let px = m + (fw - bw) * 0.5;
    let py = m + tbh + (fh - bh) * 0.5;
    let mut c = Ctx { st, s, ox: d.bounds[0] - px * s, oy: d.bounds[1] - py * s, items: Vec::new() };
    for it in &d.items {
        if matches!(it, Item::Text(t) if t.src == "footnote") {
            continue;
        }
        c.items.push(it.clone());
    }
    let fx1 = w - m;
    let fy1 = h - m;
    c.rect(Pen::Frame, m, m, fx1, fy1);
    c.rect(Pen::Title, m, m, fx1, m + tbh);
    let total = fx1 - m;
    let ratios = [3.4, 2.0, 1.2, 1.15, 1.2, 1.3];
    let sum: f64 = ratios.iter().sum();
    let detail_no = match (d.number.is_empty(), d.sheet_no.is_empty()) {
        (_, true) => d.number.clone(),
        (true, false) => d.sheet_no.clone(),
        (false, false) => format!("{}/{}", d.number, d.sheet_no),
    };
    let author = if d.author.is_empty() { String::from("KERF") } else { upper(&d.author, st) };
    let project = if !d.project.is_empty() {
        upper(&d.project, st)
    } else if !st.project.is_empty() {
        upper(&st.project, st)
    } else {
        String::new()
    };
    let title = upper(if d.title.is_empty() { &d.doc_title } else { &d.title }, st);
    let labels = ["DETAIL", "PROJECT", "SCALE", "DRAWN", "DATE", "DETAIL NO"];
    let values = [
        annot::ascii_fold(&title),
        annot::ascii_fold(&project),
        d.scale_label.clone(),
        annot::ascii_fold(&author),
        annot::ascii_fold(&d.date),
        annot::ascii_fold(&detail_no),
    ];
    let lh = st.label_height_in;
    let mut x = m;
    for i in 0..6 {
        let cw = ratios[i] * total / sum;
        if i > 0 {
            c.path(Pen::Title, &[[x, m], [x, m + tbh]], false);
        }
        c.text(Pen::Anno, labels[i], x + 0.06, m + tbh - 0.06 - lh * 0.8, lh * 0.8);
        let two_h = (tbh - 0.22 - lh * 0.8) / 2.5;
        let (lines, vh) = fit_cell(font, &values[i], 0.125, cw - 0.14, two_h);
        if let [a, b] = lines.as_slice() {
            c.text(Pen::Title, a, x + 0.07, m + 0.1 + 1.5 * vh, vh);
            c.text(Pen::Title, b, x + 0.07, m + 0.1, vh);
        } else {
            c.text(Pen::Title, &values[i], x + 0.07, m + 0.2, vh);
        }
        x += cw;
    }
    let fy = m * 0.42;
    let fth = lh;
    c.text(Pen::Title, "KERF", m, fy, fth * 1.1);
    let kw = font.width("KERF", fth * 1.1);
    for k in 0..3 {
        let bx = m + kw + 0.06 + f64::from(k) * 0.045;
        c.fill_rect(bx, fy, bx + 0.025, fy + fth * 1.1);
    }
    let mut foot = String::new();
    if !d.code_basis.is_empty() {
        foot.push_str("CODE BASIS: ");
        foot.push_str(&upper(&d.code_basis, st));
    }
    if d.has_unverified {
        if !foot.is_empty() {
            foot.push_str("   ");
        }
        foot.push_str(&st.cite_footnote);
    }
    if !foot.is_empty() {
        c.text(Pen::Anno, &foot, m + kw + 0.3, fy, fth);
    }
    let mut out = d;
    out.bounds = [c.ox, c.oy, c.ox + w * s, c.oy + h * s];
    out.items = c.items;
    out.page_w = w;
    out.page_h = h;
    out
}
