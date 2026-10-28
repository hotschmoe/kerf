//! SVG exporter (SPEC 12): paper inches x 96 as user units, black ink, stroke-font text.

use crate::drawing::{Drawing, Item};
use crate::font::Font;
use crate::geom::{self, Pt};
use crate::json;
use crate::math;
use crate::pen::Pen;
use crate::style::Style;
use crate::textgeom;
use alloc::vec;
use alloc::vec::Vec;

pub const PX_PER_IN: f64 = 96.0;

pub struct Options {
    pub margin_in: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { margin_in: 0.25 }
    }
}

/// Model -> paper mapping for a drawing placed with its bounds' lower-left at paper (ox, oy_bottom).
#[derive(Clone, Copy)]
pub struct Map {
    /// model inches per paper inch
    pub s: f64,
    /// model x at paper x = ox
    pub x0: f64,
    /// model y at paper y = oy (top)
    pub y1: f64,
    pub ox: f64,
    pub oy: f64,
}

impl Map {
    pub fn px(&self, x: f64) -> f64 {
        (self.ox + (x - self.x0) / self.s) * PX_PER_IN
    }
    pub fn py(&self, y: f64) -> f64 {
        (self.oy + (self.y1 - y) / self.s) * PX_PER_IN
    }
}

/// SVG coordinates: 3 decimals of a px.
fn num(out: &mut Vec<u8>, x: f64) {
    let mut b = [0u8; 40];
    out.extend_from_slice(json::fmt_fixed(&mut b, x, 3).as_bytes());
}

fn path_data(out: &mut Vec<u8>, m: Map, pts: &[Pt], closed: bool, start_move: bool) {
    let Some(first) = pts.first() else { return };
    if start_move {
        out.push(b'M');
        num(out, m.px(first.x));
        out.push(b' ');
        num(out, m.py(first.y));
    }
    let nseg = if closed { pts.len() } else { pts.len() - 1 };
    for i in 0..nseg {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        if p.b == 0.0 {
            out.push(b'L');
        } else {
            let arc = geom::arc_of(p.v(), q.v(), p.b);
            let r_px = arc.r / m.s * PX_PER_IN;
            out.push(b'A');
            num(out, r_px);
            out.push(b' ');
            num(out, r_px);
            out.extend_from_slice(if arc.sweep.abs() > math::PI { b" 0 1 " } else { b" 0 0 " });
            out.extend_from_slice(if p.b > 0.0 { b"1 " } else { b"0 " });
        }
        num(out, m.px(q.x));
        out.push(b' ');
        num(out, m.py(q.y));
    }
    if closed {
        out.push(b'Z');
    }
}

fn stroke_attrs(out: &mut Vec<u8>, st: &Style, pen: Pen) {
    let p = st.pen(pen);
    let w_mm = p.map_or(0.25, |x| x.width_mm);
    out.extend_from_slice(
        b" fill=\"none\" stroke=\"#000\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"",
    );
    num(out, w_mm / 25.4 * PX_PER_IN);
    out.push(b'"');
    if let Some(d) = p.and_then(|x| x.dash_mm.as_deref()) {
        out.extend_from_slice(b" stroke-dasharray=\"");
        for (i, v) in d.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            num(out, v / 25.4 * PX_PER_IN);
        }
        out.push(b'"');
    }
}

fn escape_attr(out: &mut Vec<u8>, s: &str) {
    for c in s.bytes() {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(c),
        }
    }
}

/// Emit the drawing's layers. The caller has opened the <svg>.
pub fn emit_items(out: &mut Vec<u8>, d: &Drawing<'_>, m: Map, font: &Font) {
    let st = d.style;
    for layer in &d.layers {
        out.extend_from_slice(b"<g id=\"");
        escape_attr(out, &layer.name);
        out.extend_from_slice(b"\" inkscape:groupmode=\"layer\" inkscape:label=\"");
        escape_attr(out, &layer.name);
        out.extend_from_slice(b"\">\n");
        let mut done = vec![false; d.items.len()];
        for (i, it) in d.items.iter().enumerate() {
            if done[i] || it.layer() != layer.name {
                continue;
            }
            let src = it.src();
            out.extend_from_slice(b"<g data-src=\"");
            escape_attr(out, src);
            out.extend_from_slice(b"\">\n");
            for (j, it2) in d.items.iter().enumerate().skip(i) {
                if done[j] || it2.layer() != layer.name || it2.src() != src {
                    continue;
                }
                done[j] = true;
                emit_item(out, st, it2, m, font);
            }
            out.extend_from_slice(b"</g>\n");
        }
        out.extend_from_slice(b"</g>\n");
    }
}

fn emit_item(out: &mut Vec<u8>, st: &Style, it: &Item, m: Map, font: &Font) {
    match it {
        Item::Path(p) => {
            out.extend_from_slice(b"<path d=\"");
            path_data(out, m, &p.pts, p.closed, true);
            out.push(b'"');
            stroke_attrs(out, st, p.pen);
            out.extend_from_slice(b"/>\n");
        }
        Item::Fill(f) => {
            out.extend_from_slice(b"<path d=\"");
            for l in &f.loops {
                path_data(out, m, l, true, true);
            }
            out.extend_from_slice(b"\" fill=\"#000\" fill-rule=\"evenodd\" stroke=\"none\"/>\n");
        }
        Item::Hatch(h) => {
            if h.lines.is_empty() {
                return;
            }
            out.extend_from_slice(b"<path d=\"");
            for ln in &h.lines {
                out.push(b'M');
                num(out, m.px(ln[0]));
                out.push(b' ');
                num(out, m.py(ln[1]));
                if ln[0] == ln[2] && ln[1] == ln[3] {
                    out.extend_from_slice(b"h0");
                } else {
                    out.push(b'L');
                    num(out, m.px(ln[2]));
                    out.push(b' ');
                    num(out, m.py(ln[3]));
                }
            }
            out.push(b'"');
            stroke_attrs(out, st, h.pen);
            out.extend_from_slice(b"/>\n");
        }
        Item::Region(_) => {}
        Item::Text(t) => {
            let sts = textgeom::strokes(font, t);
            if sts.is_empty() {
                return;
            }
            out.extend_from_slice(b"<path d=\"");
            for pl in &sts {
                for (k, p) in pl.iter().enumerate() {
                    out.push(if k == 0 { b'M' } else { b'L' });
                    num(out, m.px(p.x));
                    out.push(b' ');
                    num(out, m.py(p.y));
                }
            }
            out.push(b'"');
            stroke_attrs(out, st, t.pen);
            out.extend_from_slice(b"/>\n");
        }
    }
}

/// Bare detail SVG: the drawing's bounds plus a margin, true paper scale.
pub fn render(d: &Drawing<'_>, font: &Font, opt: &Options) -> Vec<u8> {
    let s = d.scale;
    let margin = if d.page_w > 0.0 { 0.0 } else { opt.margin_in };
    let w_in = (d.bounds[2] - d.bounds[0]) / s + 2.0 * margin;
    let h_in = (d.bounds[3] - d.bounds[1]) / s + 2.0 * margin;
    let m = Map { s, x0: d.bounds[0], y1: d.bounds[3], ox: margin, oy: margin };
    let mut out = Vec::new();
    header(&mut out, w_in, h_in);
    out.extend_from_slice(b"<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>\n");
    emit_items(&mut out, d, m, font);
    out.extend_from_slice(b"</svg>\n");
    out
}

pub fn header(out: &mut Vec<u8>, w_in: f64, h_in: f64) {
    out.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"");
    num(out, w_in);
    out.extend_from_slice(b"in\" height=\"");
    num(out, h_in);
    out.extend_from_slice(b"in\" viewBox=\"0 0 ");
    num(out, w_in * PX_PER_IN);
    out.push(b' ');
    num(out, h_in * PX_PER_IN);
    out.extend_from_slice(b"\">\n");
}
