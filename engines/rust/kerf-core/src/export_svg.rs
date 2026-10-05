//! SVG exporter (SPEC section 12): paper inches x 96 user units, black ink on white.

use crate::drawing::Drawing;
use crate::geom::*;
use crate::paper::{Geo, Paper, Prim, paper_for};
use crate::style::Style;

fn fnum(x: f64) -> String {
    let v = (x * 1000.0).round() / 1000.0;
    let v = if v == 0.0 { 0.0 } else { v };
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.3}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

const PX: f64 = 96.0;

fn pt_str(h: f64, x: f64, y: f64) -> String {
    format!("{} {}", fnum(x * PX), fnum((h - y) * PX))
}

pub fn path_d(h: f64, pts: &[V], closed: bool) -> String {
    let mut d = String::new();
    if pts.is_empty() {
        return d;
    }
    d.push_str(&format!("M{}", pt_str(h, pts[0].x, pts[0].y)));
    let n = pts.len();
    let nseg = if closed { n } else { n - 1 };
    for i in 0..nseg {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        if a.b.abs() > 1e-12 && a.p().dist(b.p()) > 1e-12 {
            if let Seg::Arc { r, sw, .. } = bulge_to_seg(a.p(), b.p(), a.b) {
                let large = if sw.abs() > std::f64::consts::PI { 1 } else { 0 };
                let sweep = if sw > 0.0 { 0 } else { 1 }; // model y is up: CCW stays CCW on screen, SVG sweep 1 = clockwise
                d.push_str(&format!("A{} {} 0 {} {} {}", fnum(r * PX), fnum(r * PX), large, sweep, pt_str(h, b.x, b.y)));
                continue;
            }
        }
        d.push_str(&format!("L{}", pt_str(h, b.x, b.y)));
    }
    if closed {
        d.push('Z');
    }
    d
}

fn layer_rank(name: &str) -> u8 {
    let up = name.to_uppercase();
    if up.contains("PATT") {
        0
    } else if up.contains("BYND") {
        1
    } else if up.contains("HIDN") {
        2
    } else if up.contains("CUT") {
        3
    } else if up.contains("STL") {
        4
    } else if up.contains("BRKL") {
        5
    } else if up.contains("NOTE") {
        6
    } else if up.contains("DIMS") {
        7
    } else {
        8
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn prim_d(h: f64, p: &Prim) -> (String, bool) {
    match &p.geo {
        Geo::Path { pts, closed } => (path_d(h, pts, *closed), false),
        Geo::Fill { loops } => (loops.iter().map(|l| path_d(h, l, true)).collect::<Vec<_>>().join(""), true),
        Geo::Lines(segs) => {
            let mut d = String::new();
            for s in segs {
                d.push_str(&format!("M{}L{}", pt_str(h, s[0], s[1]), pt_str(h, s[2], s[3])));
            }
            (d, false)
        }
        Geo::Strokes(polys) => {
            let mut d = String::new();
            for pl in polys {
                if pl.is_empty() {
                    continue;
                }
                d.push_str(&format!("M{}", pt_str(h, pl[0].x, pl[0].y)));
                if pl.len() == 1 {
                    d.push_str(&format!("L{}", pt_str(h, pl[0].x, pl[0].y)));
                }
                for q in &pl[1..] {
                    d.push_str(&format!("L{}", pt_str(h, q.x, q.y)));
                }
            }
            (d, false)
        }
    }
}

pub fn svg_from_paper(p: &Paper, style: &Style) -> String {
    let (w, h) = (p.w, p.h);
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{}in\" height=\"{}in\" viewBox=\"0 0 {} {}\">\n",
        fnum(w),
        fnum(h),
        fnum(w * PX),
        fnum(h * PX)
    ));
    // pen classes
    out.push_str("<style>path{fill:none;stroke:#000;stroke-linecap:round;stroke-linejoin:round}path.fill{fill:#000;stroke:none;fill-rule:evenodd}");
    for pen in &style.pens {
        let wpx = pen.width_mm / 25.4 * PX;
        out.push_str(&format!(".{}{{stroke-width:{}", pen.name, fnum(wpx)));
        if let Some(d) = &pen.dash_mm {
            let ds: Vec<String> = d.iter().map(|x| fnum(x / 25.4 * PX)).collect();
            out.push_str(&format!(";stroke-dasharray:{};stroke-linecap:butt", ds.join(" ")));
        }
        out.push('}');
    }
    out.push_str("</style>\n");
    out.push_str(&format!("<rect id=\"paper\" width=\"{}\" height=\"{}\" fill=\"#fff\"/>\n", fnum(w * PX), fnum(h * PX)));
    // group by layer
    let mut layers: Vec<String> = vec![];
    for pr in &p.prims {
        if !layers.contains(&pr.layer) {
            layers.push(pr.layer.clone());
        }
    }
    layers.sort_by_key(|l| layer_rank(l));
    for layer in &layers {
        out.push_str(&format!("<g id=\"{}\" inkscape:groupmode=\"layer\" inkscape:label=\"{}\">\n", esc(layer), esc(layer)));
        let mut srcs: Vec<&str> = vec![];
        for pr in p.prims.iter().filter(|x| &x.layer == layer) {
            if !srcs.contains(&pr.src.as_str()) {
                srcs.push(&pr.src);
            }
        }
        for src in srcs {
            out.push_str(&format!("<g data-src=\"{}\">", esc(src)));
            for pr in p.prims.iter().filter(|x| &x.layer == layer && x.src == src) {
                let (d, is_fill) = prim_d(h, pr);
                if d.is_empty() {
                    continue;
                }
                let class = if is_fill { "fill".to_string() } else { pr.pen.clone() };
                out.push_str(&format!("<path class=\"{}\" d=\"{}\"/>", class, d));
            }
            out.push_str("</g>\n");
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

pub fn export_svg(d: &Drawing, style: &Style, sheet: bool) -> String {
    let p = paper_for(d, style, sheet);
    svg_from_paper(&p, style)
}
