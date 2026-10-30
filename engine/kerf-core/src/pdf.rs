//! PDF exporter (SPEC 12): PDF 1.7, one page at the style's sheet size, vector only, text as stroked
//! paths (no fonts), deterministic object order, no timestamps.

pub mod writer;

use crate::drawing::{Drawing, Item};
use crate::font::Font;
use crate::geom::{self, Pt, V2};
use crate::json;
use crate::math;
use crate::num;
use crate::pen::Pen;
use crate::style::Style;
use crate::textgeom;
use alloc::vec::Vec;

const PT_PER_IN: f64 = 72.0;

/// Byte-identical to the Zig goldens, which carry the Zig engine name.
const INFO: &[u8] = b"<< /Title (Kerf detail) /Producer (kerf-zig) >>";

fn push_num(out: &mut Vec<u8>, x: f64) {
    let mut b = [0u8; 40];
    out.extend_from_slice(json::fmt_fixed(&mut b, x, 3).as_bytes());
}

struct Cs {
    out: Vec<u8>,
    s: f64,
    x0: f64,
    y0: f64,
}

impl Cs {
    fn px(&self, x: f64) -> f64 {
        (x - self.x0) / self.s * PT_PER_IN
    }

    fn py(&self, y: f64) -> f64 {
        (y - self.y0) / self.s * PT_PER_IN
    }

    fn xy(&mut self, x: f64, y: f64) {
        let (px, py) = (self.px(x), self.py(y));
        push_num(&mut self.out, px);
        self.out.push(b' ');
        push_num(&mut self.out, py);
    }

    fn op(&mut self, x: f64, y: f64, o: &str) {
        self.xy(x, y);
        self.out.push(b' ');
        self.out.extend_from_slice(o.as_bytes());
        self.out.push(b'\n');
    }

    fn curve(&mut self, p1: V2, p2: V2, p3: V2) {
        self.xy(p1.x, p1.y);
        self.out.push(b' ');
        self.xy(p2.x, p2.y);
        self.out.push(b' ');
        self.xy(p3.x, p3.y);
        self.out.extend_from_slice(b" c\n");
    }

    fn segments(&mut self, pts: &[Pt], closed: bool) {
        let Some(first) = pts.first() else { return };
        self.op(first.x, first.y, "m");
        let nseg = if closed { pts.len() } else { pts.len() - 1 };
        for i in 0..nseg {
            let p = pts[i];
            let q = pts[(i + 1) % pts.len()];
            if p.b == 0.0 {
                self.op(q.x, q.y, "l");
            } else {
                let arc = geom::arc_of(p.v(), q.v(), p.b);
                let n = num::to_int_clamped::<usize>(math::ceil(arc.sweep.abs() / (math::PI / 2.0) - 1e-9), 1, 4);
                let step = arc.sweep / n as f64;
                let k = 4.0 / 3.0 * math::tan(step / 4.0);
                let mut a0 = arc.a0;
                for _ in 0..n {
                    let a1 = a0 + step;
                    let c0 = V2::init(math::cos(a0), math::sin(a0));
                    let c1 = V2::init(math::cos(a1), math::sin(a1));
                    let e1 = arc.c.add(c1.scale(arc.r));
                    let cp1 = arc.c.add(c0.add(c0.perp().scale(k)).scale(arc.r));
                    let cp2 = arc.c.add(c1.sub(c1.perp().scale(k)).scale(arc.r));
                    self.curve(cp1, cp2, e1);
                    a0 = a1;
                }
            }
        }
        if closed {
            self.out.extend_from_slice(b"h\n");
        }
    }

    fn set_pen(&mut self, st: &Style, pen: Pen) {
        let p = st.pen(pen);
        let w_mm = p.map_or(0.25, |x| x.width_mm);
        push_num(&mut self.out, w_mm / 25.4 * PT_PER_IN);
        self.out.extend_from_slice(b" w\n");
        if let Some(d) = p.and_then(|x| x.dash_mm.as_deref()) {
            self.out.push(b'[');
            for (i, v) in d.iter().enumerate() {
                if i > 0 {
                    self.out.push(b' ');
                }
                push_num(&mut self.out, v / 25.4 * PT_PER_IN);
            }
            self.out.extend_from_slice(b"] 0 d\n");
            return;
        }
        self.out.extend_from_slice(b"[] 0 d\n");
    }
}

fn content_stream(d: &Drawing<'_>, font: &Font) -> Vec<u8> {
    let st = d.style;
    let mut cs = Cs { out: Vec::new(), s: d.scale, x0: d.bounds[0], y0: d.bounds[1] };
    cs.out.extend_from_slice(b"1 J 1 j 0 0 0 RG 0 0 0 rg\n");
    for layer in &d.layers {
        for it in &d.items {
            if it.layer() != layer.name {
                continue;
            }
            match it {
                Item::Path(p) => {
                    cs.set_pen(st, p.pen);
                    cs.segments(&p.pts, p.closed);
                    cs.out.extend_from_slice(b"S\n");
                }
                Item::Fill(f) => {
                    for l in &f.loops {
                        cs.segments(l, true);
                    }
                    cs.out.extend_from_slice(b"f*\n");
                }
                Item::Hatch(h) => {
                    if h.lines.is_empty() {
                        continue;
                    }
                    cs.set_pen(st, h.pen);
                    for ln in &h.lines {
                        cs.op(ln[0], ln[1], "m");
                        if ln[0] == ln[2] && ln[1] == ln[3] {
                            cs.op(ln[2] + 0.001 * d.scale / 72.0, ln[3], "l");
                        } else {
                            cs.op(ln[2], ln[3], "l");
                        }
                    }
                    cs.out.extend_from_slice(b"S\n");
                }
                Item::Region(_) => {}
                Item::Text(t) => {
                    let sts = textgeom::strokes(font, t);
                    if sts.is_empty() {
                        continue;
                    }
                    cs.set_pen(st, t.pen);
                    for pl in &sts {
                        for (k, p) in pl.iter().enumerate() {
                            cs.op(p.x, p.y, if k == 0 { "m" } else { "l" });
                        }
                    }
                    cs.out.extend_from_slice(b"S\n");
                }
            }
        }
    }
    cs.out
}

pub fn render(d: &Drawing<'_>, font: &Font) -> Vec<u8> {
    let page_w = if d.page_w > 0.0 { d.page_w } else { (d.bounds[2] - d.bounds[0]) / d.scale };
    let page_h = if d.page_h > 0.0 { d.page_h } else { (d.bounds[3] - d.bounds[1]) / d.scale };
    let content = content_stream(d, font);

    let mut w = writer::Writer::new();
    w.object(b"<< /Type /Catalog /Pages 2 0 R >>");
    w.object(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let mut page = Vec::new();
    page.extend_from_slice(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ");
    push_num(&mut page, math::round(page_w * PT_PER_IN * 1000.0) / 1000.0);
    page.push(b' ');
    push_num(&mut page, math::round(page_h * PT_PER_IN * 1000.0) / 1000.0);
    page.extend_from_slice(b"] /Contents 4 0 R /Resources << >> >>");
    w.object(&page);
    w.stream_object(&content);
    w.object(INFO);
    w.finish(1, 5)
}
