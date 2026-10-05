//! PDF exporter (SPEC section 12): PDF 1.7, vector only, text as stroked paths, no timestamps.

use crate::drawing::Drawing;
use crate::geom::*;
use crate::paper::{Geo, Paper, Prim, paper_for};
use crate::style::Style;

const PT: f64 = 72.0;

fn n(x: f64) -> String {
    let v = (x * 1000.0).round() / 1000.0;
    let v = if v == 0.0 { 0.0 } else { v };
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.3}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn pp(x: f64, y: f64) -> String {
    format!("{} {}", n(x * PT), n(y * PT))
}

fn arc_beziers(a: Pt, b: Pt, bulge: f64, out: &mut String) {
    if let Seg::Arc { c, r, a0, sw, .. } = bulge_to_seg(a, b, bulge) {
        let nseg = ((sw.abs() / (std::f64::consts::PI / 2.0)).ceil() as usize).max(1);
        let step = sw / nseg as f64;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        for i in 0..nseg {
            let t0 = a0 + step * i as f64;
            let t1 = t0 + step;
            let p0 = pt(c.x + r * t0.cos(), c.y + r * t0.sin());
            let p3 = if i + 1 == nseg { b } else { pt(c.x + r * t1.cos(), c.y + r * t1.sin()) };
            let d0 = pt(-t0.sin(), t0.cos()) * (r * k);
            let d1 = pt(-t1.sin(), t1.cos()) * (r * k);
            let p1 = p0 + d0;
            let p2 = pt(c.x + r * t1.cos(), c.y + r * t1.sin()) - d1;
            out.push_str(&format!("{} {} {} c\n", pp(p1.x, p1.y), pp(p2.x, p2.y), pp(p3.x, p3.y)));
        }
    }
}

fn path_ops(pts: &[V], closed: bool, out: &mut String) {
    if pts.is_empty() {
        return;
    }
    out.push_str(&format!("{} m\n", pp(pts[0].x, pts[0].y)));
    let cnt = pts.len();
    let nseg = if closed { cnt } else { cnt - 1 };
    for i in 0..nseg {
        let (a, b) = (pts[i], pts[(i + 1) % cnt]);
        if a.b.abs() > 1e-12 && a.p().dist(b.p()) > 1e-12 {
            arc_beziers(a.p(), b.p(), a.b, out);
        } else {
            out.push_str(&format!("{} l\n", pp(b.x, b.y)));
        }
    }
    if closed {
        out.push_str("h\n");
    }
}

fn rank(layer: &str) -> u8 {
    let up = layer.to_uppercase();
    for (i, k) in ["PATT", "BYND", "HIDN", "CUT", "STL", "BRKL", "NOTE", "DIMS"].iter().enumerate() {
        if up.contains(k) {
            return i as u8;
        }
    }
    8
}

pub fn pdf_from_paper(p: &Paper, style: &Style) -> Vec<u8> {
    let mut cs = String::new();
    cs.push_str("1 J 1 j 0 G 0 g\n");
    let mut order: Vec<&Prim> = p.prims.iter().collect();
    order.sort_by_key(|x| rank(&x.layer)); // stable: keeps document order inside a layer
    let mut cur_pen = String::new();
    for pr in order {
        if pr.pen != cur_pen {
            cur_pen = pr.pen.clone();
            if pr.pen != "fill" {
                let pen = style.pen(&pr.pen);
                cs.push_str(&format!("{} w\n", n(pen.width_mm / 25.4 * PT)));
                match &pen.dash_mm {
                    Some(d) if d.len() >= 2 => {
                        let ds: Vec<String> = d.iter().map(|x| n(x / 25.4 * PT)).collect();
                        cs.push_str(&format!("0 J [{}] 0 d\n", ds.join(" ")));
                    }
                    _ => cs.push_str("1 J [] 0 d\n"),
                }
            }
        }
        match &pr.geo {
            Geo::Path { pts, closed } => {
                path_ops(pts, *closed, &mut cs);
                cs.push_str("S\n");
            }
            Geo::Fill { loops } => {
                for l in loops {
                    path_ops(l, true, &mut cs);
                }
                cs.push_str("f*\n");
                cur_pen.clear();
            }
            Geo::Lines(segs) => {
                for s in segs {
                    cs.push_str(&format!("{} m {} l S\n", pp(s[0], s[1]), pp(s[2], s[3])));
                }
            }
            Geo::Strokes(polys) => {
                for pl in polys {
                    if pl.is_empty() {
                        continue;
                    }
                    cs.push_str(&format!("{} m\n", pp(pl[0].x, pl[0].y)));
                    if pl.len() == 1 {
                        cs.push_str(&format!("{} l\n", pp(pl[0].x, pl[0].y)));
                    }
                    for q in &pl[1..] {
                        cs.push_str(&format!("{} l\n", pp(q.x, q.y)));
                    }
                    cs.push_str("S\n");
                }
            }
        }
    }
    let mut out: Vec<u8> = Vec::new();
    let mut offs: Vec<usize> = vec![];
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut obj = |out: &mut Vec<u8>, offs: &mut Vec<usize>, body: &str| {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", offs.len(), body).as_bytes());
    };
    obj(&mut out, &mut offs, "<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, &mut offs, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        &mut offs,
        &format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << >> /Contents 4 0 R >>", n(p.w * PT), n(p.h * PT)),
    );
    obj(&mut out, &mut offs, &format!("<< /Length {} >>\nstream\n{}endstream", cs.len(), cs));
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offs.len() + 1).as_bytes());
    for o in &offs {
        out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", offs.len() + 1, xref).as_bytes());
    out
}

pub fn export_pdf(d: &Drawing, style: &Style, sheet: bool) -> Vec<u8> {
    // PDF always places the detail on the style's sheet (SPEC 12: one page per view at sheet size).
    let _ = sheet;
    let p = paper_for(d, style, true);
    pdf_from_paper(&p, style)
}
