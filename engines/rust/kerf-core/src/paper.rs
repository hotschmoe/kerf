//! Paper-space primitives: a Drawing placed at true scale on a page, with optional sheet frame and title block.

use crate::drawing::{Drawing, Item, layer_name, pen_layer_key};
use crate::font::font;
use crate::geom::*;
use crate::style::Style;

#[derive(Clone, Debug)]
pub enum Geo {
    Path { pts: Vec<V>, closed: bool },
    Fill { loops: Vec<Vec<V>> },
    Lines(Vec<[f64; 4]>),
    Strokes(Vec<Vec<Pt>>),
}

#[derive(Clone, Debug)]
pub struct Prim {
    pub layer: String,
    pub pen: String,
    pub src: String,
    pub geo: Geo,
}

pub struct Paper {
    pub w: f64,
    pub h: f64,
    pub origin: (f64, f64),
    pub prims: Vec<Prim>,
}

fn xf_loop(l: &[V], f: &impl Fn(Pt) -> Pt) -> Vec<V> {
    l.iter()
        .map(|p| {
            let q = f(p.p());
            V { x: q.x, y: q.y, b: p.b }
        })
        .collect()
}

fn prim_path(style: &Style, pen: &str, src: &str, pts: Vec<V>, closed: bool) -> Prim {
    Prim { layer: layer_name(style, pen_layer_key(pen)), pen: pen.into(), src: src.into(), geo: Geo::Path { pts, closed } }
}

fn rect_path(style: &Style, pen: &str, src: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Prim {
    prim_path(style, pen, src, vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], true)
}

fn line(style: &Style, pen: &str, src: &str, a: Pt, b: Pt) -> Prim {
    prim_path(style, pen, src, vec![v(a.x, a.y), v(b.x, b.y)], false)
}

fn text(style: &Style, pen: &str, src: &str, s: &str, x: f64, y: f64, h: f64, align: &str, valign: &str) -> Prim {
    Prim {
        layer: layer_name(style, "title"),
        pen: pen.into(),
        src: src.into(),
        geo: Geo::Strokes(font().strokes(&crate::font::ascii_fold(s), h, x, y, 0.0, align, valign)),
    }
}

/// Fit text into a width by shrinking its height (down to 60%).
fn fit_h(s: &str, h: f64, maxw: f64) -> f64 {
    let w = font().width(&crate::font::ascii_fold(s), h);
    if w <= maxw { h } else { (h * maxw / w).max(h * 0.6) }
}

pub fn paper_for(d: &Drawing, style: &Style, sheet: bool) -> Paper {
    let s = d.scale;
    let bw = d.bounds.w() / s;
    let bh = d.bounds.h() / s;
    let (w, h, px, py) = if sheet {
        let m = style.margin;
        let fx0 = m;
        let fy0 = m + style.title_block_h;
        let fw = style.sheet_w - 2.0 * m;
        let fh = style.sheet_h - 2.0 * m - style.title_block_h;
        (style.sheet_w, style.sheet_h, fx0 + (fw - bw) * 0.5, fy0 + (fh - bh) * 0.5)
    } else {
        let pad = 0.25;
        (bw + 2.0 * pad, bh + 2.0 * pad, pad, pad)
    };
    let ox = d.bounds.x0;
    let oy = d.bounds.y0;
    let f = move |p: Pt| pt((p.x - ox) / s + px, (p.y - oy) / s + py);
    let mut prims: Vec<Prim> = vec![];
    for it in &d.items {
        if sheet {
            if let Item::Text { src, .. } = it {
                if src.starts_with("title:") {
                    // keep: detail title stays under the view
                }
            }
        }
        match it {
            Item::Path { layer, pen, src, closed, pts } => prims.push(Prim { layer: layer.clone(), pen: pen.clone(), src: src.clone(), geo: Geo::Path { pts: xf_loop(pts, &f), closed: *closed } }),
            Item::Fill { layer, src, loops } => prims.push(Prim {
                layer: layer.clone(),
                pen: "fill".into(),
                src: src.clone(),
                geo: Geo::Fill { loops: loops.iter().map(|l| xf_loop(l, &f)).collect() },
            }),
            Item::Hatch { layer, pen, src, lines, .. } => prims.push(Prim {
                layer: layer.clone(),
                pen: pen.clone(),
                src: src.clone(),
                geo: Geo::Lines(
                    lines
                        .iter()
                        .map(|l| {
                            let a = f(pt(l[0], l[1]));
                            let b = f(pt(l[2], l[3]));
                            [a.x, a.y, b.x, b.y]
                        })
                        .collect(),
                ),
            }),
            Item::Text { layer, pen, src, s: text, x, y, h: th, rot, align, valign } => {
                if sheet && src == "footnote" {
                    continue;
                }
                let strokes = font().strokes(text, *th, *x, *y, *rot, align, valign);
                let polys: Vec<Vec<Pt>> = strokes.into_iter().map(|pl| pl.into_iter().map(|p| f(p)).collect()).collect();
                prims.push(Prim { layer: layer.clone(), pen: pen.clone(), src: src.clone(), geo: Geo::Strokes(polys) });
            }
        }
    }
    if sheet {
        sheet_prims(d, style, &mut prims);
    }
    Paper { w, h, origin: (px, py), prims }
}

fn sheet_prims(d: &Drawing, style: &Style, out: &mut Vec<Prim>) {
    let m = style.margin;
    let (w, h) = (style.sheet_w, style.sheet_h);
    let fx1 = w - m;
    let fy1 = h - m;
    let tbh = style.title_block_h;
    out.push(rect_path(style, "frame", "sheet", m, m, fx1, fy1));
    out.push(rect_path(style, "title", "sheet", m, m, fx1, m + tbh));
    // cell widths scale to the frame width
    let total = fx1 - m;
    let ratios = [3.4, 2.0, 1.2, 1.15, 1.2, 1.3];
    let sum: f64 = ratios.iter().sum();
    let widths: Vec<f64> = ratios.iter().map(|r| r * total / sum).collect();
    let info = &d.info;
    let detail_no = if info.sheet.is_empty() { info.number.clone() } else { format!("{}/{}", info.number, info.sheet) };
    let author = if info.author.is_empty() { "KERF".to_string() } else { info.author.to_uppercase() };
    let project = if !info.project.is_empty() {
        info.project.to_uppercase()
    } else if !style.project.is_empty() {
        style.project.to_uppercase()
    } else {
        String::new()
    };
    let title = if info.title.is_empty() { info.doc_title.to_uppercase() } else { info.title.to_uppercase() };
    let cells: [(&str, String); 6] = [
        ("DETAIL", title),
        ("PROJECT", project),
        ("SCALE", info.scale_text.clone()),
        ("DRAWN", author),
        ("DATE", info.date.clone()),
        ("DETAIL NO", detail_no),
    ];
    let lh = style.label_height;
    let mut x = m;
    for (i, (label, value)) in cells.iter().enumerate() {
        let cw = widths[i];
        if i > 0 {
            out.push(line(style, "title", "sheet", pt(x, m), pt(x, m + tbh)));
        }
        out.push(text(style, "anno", "sheet", label, x + 0.06, m + tbh - 0.06 - lh * 0.8, lh * 0.8, "left", "baseline"));
        let vh = fit_h(value, 0.125, cw - 0.14);
        out.push(text(style, "title", "sheet", value, x + 0.07, m + 0.2, vh, "left", "baseline"));
        x += cw;
    }
    // footer strip in the margin below the frame
    let fy = m * 0.42;
    let fh = style.label_height;
    out.push(text(style, "title", "sheet", "KERF", m, fy, fh * 1.1, "left", "baseline"));
    let kw = font().width("KERF", fh * 1.1);
    for k in 0..3 {
        let bx = m + kw + 0.06 + k as f64 * 0.045;
        out.push(Prim {
            layer: layer_name(style, "title"),
            pen: "fill".into(),
            src: "sheet".into(),
            geo: Geo::Fill { loops: vec![vec![v(bx, fy), v(bx + 0.025, fy), v(bx + 0.025, fy + fh * 1.1), v(bx, fy + fh * 1.1)]] },
        });
    }
    let mut foot = String::new();
    if !info.code_basis.is_empty() {
        foot.push_str(&format!("CODE BASIS: {}", info.code_basis.to_uppercase()));
    }
    if info.unverified {
        if !foot.is_empty() {
            foot.push_str("   ");
        }
        foot.push_str(&style.cite_footnote);
    }
    if !foot.is_empty() {
        out.push(text(style, "anno", "sheet", &foot, m + kw + 0.3, fy, fh, "left", "baseline"));
    }
}
