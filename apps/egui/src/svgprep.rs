//! SHEET preview: the engine's sheet SVG (what PDF/SVG export write) is parsed into a `Prep`
//! (scale 1, paper inches) so the same painter and rasterizer draw "exactly what will be
//! exported". Handles the subset the engine emits: `<style>` pen classes, `<g data-src>`,
//! `<path class d>` with M / L / A / Z, and `path.fill` solids (even-odd).

use crate::ir::{P2, PItem, PKind, PenDef, Prep, Region, even_odd_groups, polygon_area, triangulate};
use std::collections::BTreeMap;

const PX_PER_IN: f64 = 96.0;

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)?;
    let rest = &tag[i + key.len()..];
    Some(&rest[..rest.find('"')?])
}

fn parse_style(css: &str) -> BTreeMap<String, PenDef> {
    let mut pens = BTreeMap::new();
    for rule in css.split('}') {
        let Some((sel, body)) = rule.split_once('{') else { continue };
        let sel = sel.trim();
        let Some(name) = sel.strip_prefix('.') else { continue };
        let mut def = PenDef::default();
        for decl in body.split(';') {
            let Some((k, v)) = decl.split_once(':') else { continue };
            match k.trim() {
                "stroke-width" => def.width_mm = v.trim().parse::<f64>().unwrap_or(0.18) / PX_PER_IN * 25.4,
                "stroke-dasharray" => {
                    let d: Vec<f64> = v.split_whitespace().filter_map(|x| x.parse::<f64>().ok()).map(|px| px / PX_PER_IN * 25.4).collect();
                    if d.len() >= 2 {
                        def.dash_mm = Some(d);
                    }
                }
                _ => {}
            }
        }
        pens.insert(name.to_owned(), def);
    }
    pens
}

/// Tokenize path data into (command, numbers).
fn tokens(d: &str) -> Vec<(char, Vec<f64>)> {
    let mut out: Vec<(char, Vec<f64>)> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            out.push((c, Vec::new()));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let s = i;
            i += 1;
            while i < b.len() && ((b[i] as char).is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if let (Some(last), Ok(v)) = (out.last_mut(), d[s..i].parse::<f64>()) {
                last.1.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn arc_points(a: P2, b: P2, r: f64, large: bool, sweep: bool, out: &mut Vec<P2>) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let c = dx.hypot(dy);
    if c < 1e-9 {
        return;
    }
    let r = r.max(c / 2.0);
    let h = (r * r - c * c / 4.0).max(0.0).sqrt();
    let (mx, my) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let (nx, ny) = (-dy / c, dx / c);
    // choose the center side from the flags (SVG y-down coordinates)
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let (cx, cy) = (mx + sign * nx * h, my + sign * ny * h);
    let a0 = (a[1] - cy).atan2(a[0] - cx);
    let a1 = (b[1] - cy).atan2(b[0] - cx);
    let mut da = a1 - a0;
    if sweep && da < 0.0 {
        da += std::f64::consts::TAU;
    }
    if !sweep && da > 0.0 {
        da -= std::f64::consts::TAU;
    }
    let n = ((da.abs() / 4f64.to_radians()).ceil() as usize).max(2);
    for k in 1..n {
        let ang = a0 + da * k as f64 / n as f64;
        out.push([cx + r * ang.cos(), cy + r * ang.sin()]);
    }
    out.push(b);
}

/// Subpaths in SVG px (y down) with a closed flag.
fn subpaths(d: &str) -> Vec<(Vec<P2>, bool)> {
    let mut out: Vec<(Vec<P2>, bool)> = Vec::new();
    let mut cur: Vec<P2> = Vec::new();
    for (cmd, n) in tokens(d) {
        match cmd {
            'M' => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), false));
                } else {
                    cur.clear();
                }
                for p in n.chunks_exact(2) {
                    if cur.is_empty() {
                        cur.push([p[0], p[1]]);
                    } else {
                        cur.push([p[0], p[1]]);
                    }
                }
            }
            'L' => {
                for p in n.chunks_exact(2) {
                    cur.push([p[0], p[1]]);
                }
            }
            'A' => {
                for a in n.chunks_exact(7) {
                    if let Some(&start) = cur.last() {
                        arc_points(start, [a[5], a[6]], a[0], a[3] != 0.0, a[4] != 0.0, &mut cur);
                    }
                }
            }
            'Z' | 'z' => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), true));
                }
            }
            _ => {}
        }
    }
    if cur.len() > 1 {
        out.push((cur, false));
    }
    out
}

fn to_model(p: P2) -> P2 {
    [p[0] / PX_PER_IN, -p[1] / PX_PER_IN]
}

fn bbox32(pts: &[[f32; 2]]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in pts {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

pub fn svg_to_prep(svg: &str, view: &str) -> Prep {
    let style = svg.find("<style>").and_then(|i| svg[i + 7..].find("</style>").map(|j| &svg[i + 7..i + 7 + j])).unwrap_or("");
    let pens = parse_style(style);
    let (w_px, h_px) = {
        let vb = attr(svg, "viewBox").unwrap_or("0 0 1056 816");
        let v: Vec<f64> = vb.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (v.get(2).copied().unwrap_or(1056.0), v.get(3).copied().unwrap_or(816.0))
    };
    let mut items = Vec::new();
    let mut regions = Vec::new();
    let mut cur_src = String::new();
    let mut pos = 0;
    while let Some(rel) = svg[pos..].find('<') {
        let start = pos + rel;
        let Some(end_rel) = svg[start..].find('>') else { break };
        let tag = &svg[start..start + end_rel + 1];
        pos = start + end_rel + 1;
        if tag.starts_with("<g ") {
            cur_src = attr(tag, "data-src").unwrap_or("").to_owned();
        } else if tag.starts_with("</g") {
            cur_src.clear();
        } else if tag.starts_with("<path ") {
            let class = attr(tag, "class").unwrap_or("");
            let Some(d) = attr(tag, "d") else { continue };
            let subs = subpaths(d);
            if class.split_whitespace().any(|c| c == "fill") {
                let loops: Vec<Vec<P2>> = subs.into_iter().filter(|(p, _)| p.len() >= 3).map(|(p, _)| p.into_iter().map(to_model).collect()).collect();
                for group in even_odd_groups(loops) {
                    let (verts, idx) = triangulate(&group);
                    if idx.is_empty() {
                        continue;
                    }
                    let bbox = bbox32(&verts);
                    if !cur_src.is_empty() {
                        let b = group[0].iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
                        regions.push(Region { src: cur_src.clone(), area: polygon_area(&group[0]).abs(), loops: group.clone(), bbox: b, is_fill: true, exact: true });
                    }
                    items.push(PItem { src: cur_src.clone(), pen: String::new(), layer: String::new(), kind: PKind::Fill { verts, idx }, bbox });
                }
            } else if class == "hatch" && subs.iter().all(|(p, _)| p.len() == 2) {
                let segs: Vec<[f32; 4]> = subs
                    .iter()
                    .map(|(p, _)| {
                        let (a, b) = (to_model(p[0]), to_model(p[1]));
                        [a[0] as f32, a[1] as f32, b[0] as f32, b[1] as f32]
                    })
                    .collect();
                let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for s in &segs {
                    bbox = [bbox[0].min(s[0]).min(s[2]), bbox[1].min(s[1]).min(s[3]), bbox[2].max(s[0]).max(s[2]), bbox[3].max(s[1]).max(s[3])];
                }
                items.push(PItem { src: cur_src.clone(), pen: class.to_owned(), layer: String::new(), kind: PKind::Hatch { segs }, bbox });
            } else {
                for (p, closed) in subs {
                    let pts: Vec<[f32; 2]> = p.into_iter().map(to_model).map(|q| [q[0] as f32, q[1] as f32]).collect();
                    let bbox = bbox32(&pts);
                    items.push(PItem { src: cur_src.clone(), pen: class.to_owned(), layer: String::new(), kind: PKind::Line { pts, closed }, bbox });
                }
            }
        }
    }
    Prep {
        view: view.to_owned(),
        kind: "sheet".into(),
        scale: 1.0,
        bounds: [0.0, -h_px / PX_PER_IN, w_px / PX_PER_IN, 0.0],
        pens,
        items,
        regions,
        text_boxes: BTreeMap::new(),
        text_anchor: BTreeMap::new(),
        diagnostics: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths_arcs_and_fills() {
        let svg = r#"<svg viewBox="0 0 96 96"><style>path{fill:none}.cut{stroke-width:1.89}.hidden{stroke-width:0.68;stroke-dasharray:7.559 3.78}path.fill{fill:#000}</style>
<g data-src="a"><path class="cut" d="M0 0L48 0A24 24 0 0 1 96 0Z"/><path class="fill" d="M10 10L20 10L20 20L10 20ZM12 12L18 12L18 18L12 18Z"/></g></svg>"#;
        let p = svg_to_prep(svg, "A");
        assert_eq!(p.items.len(), 2);
        assert!((p.pens["cut"].width_mm - 1.89 / 96.0 * 25.4).abs() < 1e-9);
        assert!(p.pens["hidden"].dash_mm.is_some());
        match &p.items[0].kind {
            PKind::Line { pts, closed } => {
                assert!(*closed);
                assert!(pts.len() > 5, "arc must be tessellated, got {}", pts.len());
            }
            _ => panic!("expected line"),
        }
        match &p.items[1].kind {
            // outer square with a hole: 8 verts, 8 triangles
            PKind::Fill { verts, idx } => {
                assert_eq!(verts.len(), 8);
                assert_eq!(idx.len() / 3, 8);
            }
            _ => panic!("expected fill"),
        }
        assert_eq!(p.items[0].src, "a");
        // minor clockwise arc (0,0)->(10,0), r=10: bulges toward screen-up (SVG y negative)
        let mut v = vec![[0.0, 0.0]];
        arc_points([0.0, 0.0], [10.0, 0.0], 10.0, false, true, &mut v);
        assert!(v.iter().map(|p| p[1]).fold(f64::MAX, f64::min) < -1.0);
        assert_eq!(p.bounds, [0.0, -1.0, 1.0, 0.0]);
    }
}
