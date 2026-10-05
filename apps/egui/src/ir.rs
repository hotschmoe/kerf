//! Drawing IR (SPEC §10) and Mesh (SPEC §11): serde types, plus the "prepared" form the
//! painters consume (arcs tessellated, fills triangulated, text expanded to stroke polylines).
//! Everything here is pure data + geometry; no egui, no wgpu.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::OnceLock;

// ---------------------------------------------------------------- raw IR

/// A path vertex: `[x, y]` or `[x, y, bulge]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pt(pub [f64; 3]);

impl<'de> Deserialize<'de> for Pt {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Vec::<f64>::deserialize(d)?;
        Ok(Pt([
            v.first().copied().unwrap_or(0.0),
            v.get(1).copied().unwrap_or(0.0),
            v.get(2).copied().unwrap_or(0.0),
        ]))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PenDef {
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default)]
    pub dash_mm: Option<Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum RawItem {
    Path {
        #[serde(default)]
        layer: String,
        #[serde(default)]
        pen: String,
        #[serde(default)]
        src: String,
        #[serde(default)]
        closed: bool,
        pts: Vec<Pt>,
    },
    Fill {
        #[serde(default)]
        layer: String,
        #[serde(default)]
        src: String,
        loops: Vec<Vec<Pt>>,
    },
    Hatch {
        #[serde(default)]
        layer: String,
        #[serde(default)]
        pen: String,
        #[serde(default)]
        src: String,
        #[serde(default)]
        loops: Vec<Vec<Pt>>,
        #[serde(default)]
        lines: Vec<[f64; 4]>,
    },
    Text {
        #[serde(default)]
        layer: String,
        #[serde(default)]
        pen: String,
        #[serde(default)]
        src: String,
        s: String,
        x: f64,
        y: f64,
        h: f64,
        #[serde(default)]
        rot: f64,
        #[serde(default)]
        align: String,
        #[serde(default)]
        valign: String,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RawDrawing {
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub view: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub bounds: [f64; 4],
    #[serde(default)]
    pub pens: BTreeMap<String, PenDef>,
    #[serde(default)]
    pub items: Vec<RawItem>,
    #[serde(default)]
    pub diagnostics: Vec<serde_json::Value>,
}

fn one() -> f64 {
    1.0
}

// ---------------------------------------------------------------- raw mesh

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MeshPart {
    pub src: String,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default)]
    pub instance: u32,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub color: String,
    pub positions: Vec<f32>,
    #[serde(default)]
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    #[serde(default)]
    pub edges: Vec<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Mesh {
    #[serde(default)]
    pub parts: Vec<MeshPart>,
}

impl Mesh {
    /// Axis-aligned bounds `(min, max)` over all vertices.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let mut any = false;
        for p in &self.parts {
            for v in p.positions.chunks_exact(3) {
                any = true;
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        any.then_some((lo, hi))
    }
}

pub fn parse_hex(c: &str) -> [u8; 3] {
    let s = c.trim_start_matches('#');
    if s.len() >= 6 {
        let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(128);
        [p(0), p(2), p(4)]
    } else {
        [160, 160, 160]
    }
}

// ---------------------------------------------------------------- geometry

pub type P2 = [f64; 2];

/// Append the tessellation of the segment `a -> b` with DXF `bulge` (excluding `a`, including `b`).
pub fn seg_points(a: P2, b: P2, bulge: f64, max_step: f64, out: &mut Vec<P2>) {
    if bulge.abs() < 1e-9 {
        out.push(b);
        return;
    }
    let theta = 4.0 * bulge.atan();
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let c = (dx * dx + dy * dy).sqrt();
    if c < 1e-12 {
        out.push(b);
        return;
    }
    let (nx, ny) = (-dy / c, dx / c);
    let off = c / 2.0 * (1.0 - bulge * bulge) / (2.0 * bulge);
    let (mx, my) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let (cx, cy) = (mx + nx * off, my + ny * off);
    let r = ((a[0] - cx).powi(2) + (a[1] - cy).powi(2)).sqrt();
    let a0 = (a[1] - cy).atan2(a[0] - cx);
    let n = ((theta.abs() / max_step).ceil() as usize).max(2);
    for i in 1..n {
        let ang = a0 + theta * (i as f64 / n as f64);
        out.push([cx + r * ang.cos(), cy + r * ang.sin()]);
    }
    out.push(b);
}

/// Polyline for a bulge path. `closed` appends the closing segment (the last point is NOT repeated).
pub fn tessellate(pts: &[Pt], closed: bool) -> Vec<P2> {
    let step = 4.0_f64.to_radians();
    let mut out = Vec::with_capacity(pts.len() * 2);
    if pts.is_empty() {
        return out;
    }
    out.push([pts[0].0[0], pts[0].0[1]]);
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let a = pts[i].0;
        let b = pts[(i + 1) % n].0;
        seg_points([a[0], a[1]], [b[0], b[1]], a[2], step, &mut out);
    }
    if closed && out.len() > 1 {
        // last pushed point equals the first: drop the duplicate.
        let f = out[0];
        let l = *out.last().unwrap();
        if (f[0] - l[0]).abs() < 1e-9 && (f[1] - l[1]).abs() < 1e-9 {
            out.pop();
        }
    }
    out
}

pub fn polygon_area(p: &[P2]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let j = (i + 1) % p.len();
        a += p[i][0] * p[j][1] - p[j][0] * p[i][1];
    }
    a / 2.0
}

pub fn point_in_loop(pt: P2, lp: &[P2]) -> bool {
    let mut inside = false;
    let n = lp.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (lp[i], lp[j]);
        if (a[1] > pt[1]) != (b[1] > pt[1]) && pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Triangulate an outer loop with holes (earcut). Returns (vertices, indices).
pub fn triangulate(loops: &[Vec<P2>]) -> (Vec<[f32; 2]>, Vec<u32>) {
    let mut flat: Vec<f64> = Vec::new();
    let mut holes: Vec<usize> = Vec::new();
    let mut verts = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        if l.len() < 3 {
            continue;
        }
        if i > 0 {
            holes.push(verts.len());
        }
        for p in l {
            flat.extend_from_slice(p);
            verts.push([p[0] as f32, p[1] as f32]);
        }
    }
    let idx = earcutr::earcut(&flat, &holes, 2).unwrap_or_default();
    (verts, idx.into_iter().map(|i| i as u32).collect())
}

/// Even-odd grouping: depth-even loops are outers, depth-odd loops become holes of their parent.
pub fn even_odd_groups(loops: Vec<Vec<P2>>) -> Vec<Vec<Vec<P2>>> {
    let n = loops.len();
    let areas: Vec<f64> = loops.iter().map(|l| polygon_area(l).abs()).collect();
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut depth = vec![0usize; n];
    for i in 0..n {
        let mut best: Option<usize> = None;
        for j in 0..n {
            if i != j && areas[j] > areas[i] && point_in_loop(loops[i][0], &loops[j]) {
                depth[i] += 1;
                if best.is_none_or(|b| areas[j] < areas[b]) {
                    best = Some(j);
                }
            }
        }
        parent[i] = best;
    }
    let mut groups: Vec<(usize, Vec<Vec<P2>>)> = Vec::new();
    for i in 0..n {
        if depth[i] % 2 == 0 {
            groups.push((i, vec![loops[i].clone()]));
        }
    }
    for i in 0..n {
        if depth[i] % 2 == 1 {
            if let Some(p) = parent[i] {
                if let Some(g) = groups.iter_mut().find(|g| g.0 == p) {
                    g.1.push(loops[i].clone());
                }
            }
        }
    }
    groups.into_iter().map(|g| g.1).collect()
}


// ---------------------------------------------------------------- stroke font

#[derive(Deserialize)]
struct RawGlyph {
    adv: f64,
    strokes: Vec<Vec<[f64; 2]>>,
}
#[derive(Deserialize)]
struct RawFont {
    cap_height: f64,
    glyphs: HashMap<String, RawGlyph>,
}

pub struct StrokeFont {
    cap: f64,
    glyphs: HashMap<char, RawGlyph>,
}

pub fn font() -> &'static StrokeFont {
    static F: OnceLock<StrokeFont> = OnceLock::new();
    F.get_or_init(|| {
        let raw: RawFont =
            serde_json::from_str(include_str!("../../../spec/fonts/kerf-simplex.json")).expect("kerf-simplex.json");
        StrokeFont {
            cap: raw.cap_height,
            glyphs: raw.glyphs.into_iter().filter_map(|(k, v)| k.chars().next().map(|c| (c, v))).collect(),
        }
    })
}

impl StrokeFont {
    fn glyph(&self, c: char) -> &RawGlyph {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?')).expect("font has '?'")
    }

    pub fn width(&self, s: &str, h: f64) -> f64 {
        let k = h / self.cap;
        s.chars().map(|c| self.glyph(c).adv * k).sum()
    }

    /// Expand text into model-space polylines. `x,y` is the anchor; `h` is cap height.
    pub fn layout(&self, s: &str, x: f64, y: f64, h: f64, rot_deg: f64, align: &str, valign: &str) -> Vec<Vec<P2>> {
        let k = h / self.cap;
        let mut out = Vec::new();
        let lines: Vec<&str> = s.split('\n').collect();
        let (sr, cr) = rot_deg.to_radians().sin_cos();
        for (li, line) in lines.iter().enumerate() {
            let w = self.width(line, h);
            let ox = match align {
                "center" | "middle" => -w / 2.0,
                "right" => -w,
                _ => 0.0,
            };
            let block_h = h + (lines.len() as f64 - 1.0) * h * 1.6;
            let oy = match valign {
                "middle" => -block_h / 2.0 + (h * 1.6) * 0.0,
                "top" => -h,
                "bottom" => 0.0,
                _ => 0.0,
            } - li as f64 * h * 1.6;
            let mut pen = 0.0;
            for c in line.chars() {
                let g = self.glyph(c);
                for st in &g.strokes {
                    let pts: Vec<P2> = st
                        .iter()
                        .map(|p| {
                            let lx = ox + pen + p[0] * k;
                            let ly = oy + p[1] * k;
                            [x + lx * cr - ly * sr, y + lx * sr + ly * cr]
                        })
                        .collect();
                    out.push(pts);
                }
                pen += g.adv * k;
            }
        }
        out
    }
}

// ---------------------------------------------------------------- prepared drawing

#[derive(Clone, Debug)]
pub enum PKind {
    Line { pts: Vec<[f32; 2]>, closed: bool },
    Fill { verts: Vec<[f32; 2]>, idx: Vec<u32> },
    Hatch { segs: Vec<[f32; 4]> },
    Text { strokes: Vec<Vec<[f32; 2]>> },
}

#[derive(Clone, Debug)]
pub struct PItem {
    pub src: String,
    pub pen: String,
    pub layer: String,
    pub kind: PKind,
    /// model-space bbox [x0,y0,x1,y1]
    pub bbox: [f32; 4],
}

/// A pickable area: a src with its outer loop (+ holes) in model space.
#[derive(Clone, Debug)]
pub struct Region {
    pub src: String,
    pub loops: Vec<Vec<P2>>,
    pub area: f64,
    pub bbox: [f64; 4],
    /// fill items (rebar dots, steel) win over the host they sit inside
    pub is_fill: bool,
    /// false = approximated (convex hull of the member's cut linework); the engine only
    /// emits exact loops for hatched/filled regions
    pub exact: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Prep {
    pub view: String,
    pub kind: String,
    pub scale: f64,
    pub bounds: [f64; 4],
    pub pens: BTreeMap<String, PenDef>,
    pub items: Vec<PItem>,
    pub regions: Vec<Region>,
    /// per-src text bbox for notes/dims/labels: [x0,y0,x1,y1]
    pub text_boxes: BTreeMap<String, [f64; 4]>,
    /// top-left (x, first baseline + h) of each src's text block: the value `place` would hold
    pub text_anchor: BTreeMap<String, [f64; 2]>,
    pub diagnostics: Vec<serde_json::Value>,
}

fn bbox_of_pts<'a>(it: impl Iterator<Item = &'a [f32; 2]>) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in it {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

fn to_f32(p: &[P2]) -> Vec<[f32; 2]> {
    p.iter().map(|p| [p[0] as f32, p[1] as f32]).collect()
}

fn bbox64(loops: &[Vec<P2>]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for l in loops {
        for p in l {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
    }
    b
}

/// Component/annotation id of an IR `src` (`truss#1` and `cmu.bond_beam` -> `truss`, `cmu`).
pub fn base_id(src: &str) -> &str {
    src.split(['#', '.']).next().unwrap_or(src)
}

fn convex_hull(mut pts: Vec<P2>) -> Vec<P2> {
    pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap().then(a[1].partial_cmp(&b[1]).unwrap()));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: P2, a: P2, b: P2| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<P2> = Vec::new();
    for &p in &pts {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 1e-9 {
            h.pop();
        }
        h.push(p);
    }
    let lower = h.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while h.len() >= lower && cross(h[h.len() - 2], h[h.len() - 1], p) <= 1e-9 {
            h.pop();
        }
        h.push(p);
    }
    h.pop();
    h
}

fn dist_pt_seg(p: P2, a: [f32; 2], b: [f32; 2]) -> f64 {
    let (ax, ay, bx, by) = (a[0] as f64, a[1] as f64, b[0] as f64, b[1] as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-12 { 0.0 } else { (((p[0] - ax) * dx + (p[1] - ay) * dy) / l2).clamp(0.0, 1.0) };
    ((p[0] - (ax + t * dx)).powi(2) + (p[1] - (ay + t * dy)).powi(2)).sqrt()
}

impl Prep {
    pub fn from_json(json: &str) -> Result<Prep, String> {
        let raw: RawDrawing = serde_json::from_str(json).map_err(|e| format!("drawing IR: {e}"))?;
        Ok(Prep::new(raw))
    }

    pub fn new(raw: RawDrawing) -> Prep {
        let f = font();
        let mut items = Vec::with_capacity(raw.items.len());
        let mut regions = Vec::new();
        let mut text_boxes: BTreeMap<String, [f64; 4]> = BTreeMap::new();
        let mut text_anchor: BTreeMap<String, [f64; 2]> = BTreeMap::new();
        for it in &raw.items {
            match it {
                RawItem::Path { layer, pen, src, closed, pts } => {
                    let src = &base_id(src).to_owned();
                    let poly = tessellate(pts, *closed);
                    if poly.len() < 2 {
                        continue;
                    }
                    let pts32 = to_f32(&poly);
                    let bbox = bbox_of_pts(pts32.iter());
                    if *closed && poly.len() >= 3 && !src.is_empty() && !layer.contains("ANNO") && !layer.contains("BRKL") {
                        let area = polygon_area(&poly).abs();
                        if area > 1e-6 {
                            regions.push(Region { src: src.clone(), bbox: bbox64(std::slice::from_ref(&poly)), loops: vec![poly], area, is_fill: false, exact: true });
                        }
                    }
                    items.push(PItem { src: src.clone(), pen: pen.clone(), layer: layer.clone(), kind: PKind::Line { pts: pts32, closed: *closed }, bbox });
                }
                RawItem::Fill { layer, src, loops } => {
                    let src = &base_id(src).to_owned();
                    let ls: Vec<Vec<P2>> = loops.iter().map(|l| tessellate(l, true)).collect();
                    let (verts, idx) = triangulate(&ls);
                    if idx.is_empty() {
                        continue;
                    }
                    let bbox = bbox_of_pts(verts.iter());
                    if !src.is_empty() && !layer.contains("ANNO") {
                        let area = polygon_area(&ls[0]).abs();
                        regions.push(Region { src: src.clone(), bbox: bbox64(&ls[..1]), loops: ls.clone(), area, is_fill: true, exact: true });
                    }
                    items.push(PItem { src: src.clone(), pen: String::new(), layer: layer.clone(), kind: PKind::Fill { verts, idx }, bbox });
                }
                RawItem::Hatch { layer, pen, src, loops, lines } => {
                    let src = &base_id(src).to_owned();
                    let segs: Vec<[f32; 4]> = lines.iter().map(|l| [l[0] as f32, l[1] as f32, l[2] as f32, l[3] as f32]).collect();
                    let ls: Vec<Vec<P2>> = loops.iter().map(|l| tessellate(l, true)).collect();
                    let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                    if !ls.is_empty() {
                        let b = bbox64(&ls);
                        bbox = [b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32];
                    } else {
                        for s in &segs {
                            bbox = [bbox[0].min(s[0]).min(s[2]), bbox[1].min(s[1]).min(s[3]), bbox[2].max(s[0]).max(s[2]), bbox[3].max(s[1]).max(s[3])];
                        }
                    }
                    if !src.is_empty() && !ls.is_empty() {
                        let area = polygon_area(&ls[0]).abs();
                        if area > 1e-6 {
                            regions.push(Region { src: src.clone(), bbox: bbox64(&ls[..1]), loops: ls, area, is_fill: false, exact: true });
                        }
                    }
                    items.push(PItem { src: src.clone(), pen: pen.clone(), layer: layer.clone(), kind: PKind::Hatch { segs }, bbox });
                }
                RawItem::Text { layer, pen, src, s, x, y, h, rot, align, valign } => {
                    let src = &base_id(src).to_owned();
                    let strokes = f.layout(s, *x, *y, *h, *rot, align, valign);
                    let strokes32: Vec<Vec<[f32; 2]>> = strokes.iter().map(|st| to_f32(st)).collect();
                    let mut bbox = bbox_of_pts(strokes32.iter().flatten());
                    if bbox[0] > bbox[2] {
                        bbox = [*x as f32, *y as f32, *x as f32, *y as f32];
                    }
                    // box includes descender/ascender room so short strings stay pickable
                    let pad = (*h * 0.25) as f32;
                    let tb = [bbox[0] as f64 - pad as f64, bbox[1] as f64 - pad as f64, bbox[2] as f64 + pad as f64, bbox[3] as f64 + pad as f64];
                    text_boxes
                        .entry(src.clone())
                        .and_modify(|b| {
                            b[0] = b[0].min(tb[0]);
                            b[1] = b[1].min(tb[1]);
                            b[2] = b[2].max(tb[2]);
                            b[3] = b[3].max(tb[3]);
                        })
                        .or_insert(tb);
                    // `place` is the top-left of the text block: first baseline + cap height
                    text_anchor.entry(src.clone()).or_insert([*x, *y + *h]);
                    items.push(PItem { src: src.clone(), pen: pen.clone(), layer: layer.clone(), kind: PKind::Text { strokes: strokes32 }, bbox });
                }
            }
        }
        let mut prep = Prep {
            view: raw.view,
            kind: raw.kind,
            scale: raw.scale,
            bounds: raw.bounds,
            pens: raw.pens,
            items,
            regions,
            text_boxes,
            text_anchor,
            diagnostics: raw.diagnostics,
        };
        prep.add_hull_regions();
        prep
    }

    /// Members with no exact region (unhatched wood, panels, steel) get the convex hull of their
    /// cut-pen linework as an approximate pick/tint region.
    fn add_hull_regions(&mut self) {
        let mut pts: BTreeMap<String, Vec<P2>> = BTreeMap::new();
        for it in &self.items {
            if it.src.is_empty() || it.layer.contains("ANNO") || it.layer.contains("BRKL") {
                continue;
            }
            if let PKind::Line { pts: p, .. } = &it.kind {
                if matches!(it.pen.as_str(), "cut" | "steel" | "membrane" | "profile" | "rebar") {
                    pts.entry(it.src.clone()).or_default().extend(p.iter().map(|q| [q[0] as f64, q[1] as f64]));
                }
            }
        }
        for (src, p) in pts {
            if self.regions.iter().any(|r| r.src == src && !r.is_fill) {
                continue;
            }
            let hull = convex_hull(p);
            if hull.len() < 3 {
                continue;
            }
            let area = polygon_area(&hull).abs();
            if area < 1e-4 {
                continue;
            }
            self.regions.push(Region { src, bbox: bbox64(std::slice::from_ref(&hull)), loops: vec![hull], area, is_fill: false, exact: false });
        }
    }

    /// Pen width in model inches.
    pub fn pen_model_width(&self, pen: &str) -> f64 {
        let mm = self.pens.get(pen).map(|p| p.width_mm).unwrap_or(0.18);
        mm / 25.4 * self.scale
    }

    pub fn pen_dash_model(&self, pen: &str) -> Option<(f64, f64)> {
        let d = self.pens.get(pen)?.dash_mm.as_ref()?;
        if d.len() >= 2 {
            Some((d[0] / 25.4 * self.scale, d[1] / 25.4 * self.scale))
        } else {
            None
        }
    }

    /// The src under a model-space point. `slop` is the pick radius in model inches (~4 px).
    /// Order: annotation text, filled dots, linework within `slop`, exact regions (smallest),
    /// then approximate hull regions (smallest).
    pub fn pick(&self, p: P2, slop: f64) -> Option<String> {
        for (src, b) in &self.text_boxes {
            if p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3] {
                return Some(src.clone());
            }
        }
        let inside = |r: &Region| -> bool {
            if p[0] < r.bbox[0] - slop || p[0] > r.bbox[2] + slop || p[1] < r.bbox[1] - slop || p[1] > r.bbox[3] + slop {
                return false;
            }
            let mut ins = point_in_loop(p, &r.loops[0]);
            if ins {
                for h in &r.loops[1..] {
                    if point_in_loop(p, h) {
                        ins = false;
                    }
                }
            }
            if !ins && r.is_fill && r.area < 4.0 {
                let c = [(r.bbox[0] + r.bbox[2]) / 2.0, (r.bbox[1] + r.bbox[3]) / 2.0];
                ins = (c[0] - p[0]).hypot(c[1] - p[1]) < slop.max((r.bbox[2] - r.bbox[0]) / 2.0);
            }
            ins
        };
        // filled dots (rebar, bolts) first
        let mut best: Option<(&Region, f64)> = None;
        for r in self.regions.iter().filter(|r| r.is_fill && inside(r)) {
            if best.is_none_or(|(_, a)| r.area < a) {
                best = Some((r, r.area));
            }
        }
        if let Some((r, _)) = best {
            return Some(r.src.clone());
        }
        // linework within slop (nearest wins; annotation leaders and dim lines count)
        let mut near: Option<(&str, f64)> = None;
        for it in &self.items {
            if it.src.is_empty() || it.layer.contains("BRKL") || it.src == "crop" {
                continue;
            }
            let b = it.bbox;
            if p[0] < b[0] as f64 - slop || p[0] > b[2] as f64 + slop || p[1] < b[1] as f64 - slop || p[1] > b[3] as f64 + slop {
                continue;
            }
            if let PKind::Line { pts, closed } = &it.kind {
                if it.pen == "hidden" {
                    continue;
                }
                let n = pts.len();
                let segs = if *closed { n } else { n - 1 };
                for i in 0..segs {
                    let d = dist_pt_seg(p, pts[i], pts[(i + 1) % n]);
                    if d <= slop && near.is_none_or(|(_, bd)| d < bd) {
                        near = Some((&it.src, d));
                    }
                }
            }
        }
        if let Some((s, _)) = near {
            return Some(s.to_owned());
        }
        let mut best: Option<(&Region, f64)> = None;
        for r in self.regions.iter().filter(|r| !r.is_fill && inside(r)) {
            let score = if r.exact { r.area } else { r.area + 1e6 };
            if best.is_none_or(|(_, a)| score < a) {
                best = Some((r, score));
            }
        }
        best.map(|(r, _)| r.src.clone())
    }

    /// Triangulated cut regions of a src (for the selection tint).
    pub fn region_tris(&self, src: &str) -> Vec<(Vec<[f32; 2]>, Vec<u32>)> {
        self.regions
            .iter()
            .filter(|r| r.src == src && !r.is_fill && (r.exact || self.kind == "section"))
            .map(|r| triangulate(&r.loops))
            .filter(|(_, i)| !i.is_empty())
            .collect()
    }

    /// Stroked polylines of a src (for the hover/selection outline). Hidden/break lines excluded.
    pub fn outlines(&self, src: &str) -> impl Iterator<Item = &PItem> {
        let src = src.to_owned();
        self.items.iter().filter(move |i| i.src == src && !i.layer.contains("BRKL") && i.pen != "hidden" && matches!(i.kind, PKind::Line { .. }))
    }
}
