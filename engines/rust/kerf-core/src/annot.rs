//! Annotations: note layout (SPEC 6.3), dimensions (6.4, 16), labels, titles (6.5).

use crate::diag::Diag;
use crate::drawing::{Item, SheetInfo, layer_name, text_width};
use crate::font::wrap;
use crate::geom::*;
use crate::num::fmt_ftin;
use crate::poly::{Shape, shape_area};
use crate::resolve::Lookup;
use crate::section::VisInfo;
use crate::style::Style;
use serde_json::Value;

pub fn text_item(style: &Style, layer_key: &str, pen: &str, src: &str, s: &str, x: f64, y: f64, h: f64, rot: f64, align: &str, valign: &str) -> Item {
    Item::Text {
        layer: layer_name(style, layer_key),
        pen: pen.into(),
        src: src.into(),
        s: crate::font::ascii_fold(s),
        x,
        y,
        h,
        rot,
        align: align.into(),
        valign: valign.into(),
    }
}

pub fn path_item(style: &Style, pen: &str, src: &str, pts: &[Pt], closed: bool) -> Item {
    Item::Path {
        layer: layer_name(style, crate::drawing::pen_layer_key(pen)),
        pen: pen.into(),
        src: src.into(),
        closed,
        pts: pts.iter().map(|p| v(p.x, p.y)).collect(),
    }
}

fn shape_contains(sh: &Shape, p: Pt) -> bool {
    if sh.is_empty() || !poly_contains(&sh[0], p) {
        return false;
    }
    !sh[1..].iter().any(|h| poly_contains(h, p))
}

/// SPEC 6.3 step 2: label point of the largest visible polygon.
pub fn label_point(shapes: &[Shape]) -> Option<Pt> {
    let best = shapes.iter().filter(|s| !s.is_empty() && s[0].len() >= 3).max_by(|a, b| shape_area(a).partial_cmp(&shape_area(b)).unwrap_or(std::cmp::Ordering::Equal))?;
    let c = poly_centroid(&best[0]);
    if shape_contains(best, c) {
        return Some(c);
    }
    // horizontal scan through the centroid's y
    let mut xs: Vec<f64> = vec![];
    for cont in best.iter() {
        let n = cont.len();
        for i in 0..n {
            let (a, b) = (cont[i], cont[(i + 1) % n]);
            if (a.y > c.y) != (b.y > c.y) {
                xs.push(a.x + (c.y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut best_mid: Option<(f64, f64)> = None; // (distance to centroid x, mid)
    let mut i = 0;
    while i + 1 < xs.len() {
        let (a, b) = (xs[i], xs[i + 1]);
        i += 2;
        let mid = (a + b) * 0.5;
        let d = if c.x >= a && c.x <= b { 0.0 } else { (c.x - mid).abs() };
        if best_mid.map_or(true, |(bd, _)| d < bd) {
            best_mid = Some((d, mid));
        }
    }
    match best_mid {
        Some((_, m)) => Some(pt(m, c.y)),
        None => Some(c),
    }
}

/// Landing point for a note target (component id or `comp.part`) from the view's visible shapes.
pub fn target_landing(target: &str, vis: &[VisInfo], model: &crate::model::Model, crop: &Rect) -> Option<Pt> {
    let (cid, part) = match target.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (target, None),
    };
    let cid = cid.split('#').next().unwrap_or(cid);
    let comp = model.comps.iter().find(|c| c.id == cid)?;
    let mut shapes: Vec<Shape> = vec![];
    for vi in vis.iter().filter(|v| v.comp == comp.idx) {
        match part {
            Some(p) if vi.part.as_deref() != Some(p) => continue,
            _ => shapes.extend(vi.shapes.iter().cloned()),
        }
    }
    if shapes.is_empty() {
        if let Some(p) = part {
            // zone parts: use the zone region clipped to the crop
            if let Some(inst) = comp.insts.first() {
                if let Some(pi) = inst.parts.iter().find(|x| x.name == p) {
                    for r in crate::poly::clip_region_rect(&pi.region, crop) {
                        shapes.push(crate::poly::region_contours(&r, 0.002));
                    }
                }
            }
        }
    }
    label_point(&shapes)
}

pub struct NoteIn {
    pub id: String,
    pub text: String,
    pub landing: Pt,
    pub place: Option<Pt>,
}

pub fn note_text(style: &Style, text: &str, cites: &[Value]) -> (String, bool) {
    let mut s = if style.text_upper { text.to_uppercase() } else { text.to_string() };
    let mut unverified = false;
    for c in cites {
        let code = c.get("code").and_then(|x| x.as_str()).unwrap_or("");
        let section = c.get("section").and_then(|x| x.as_str()).unwrap_or("");
        let edition = c.get("edition").and_then(|x| x.as_f64()).map(|e| crate::num::fmt_num(e)).unwrap_or_default();
        let status = c.get("status").and_then(|x| x.as_str()).unwrap_or("suggested");
        let t = style
            .cite_format
            .replace("{code}", code)
            .replace("{section}", section)
            .replace("{edition}", &edition);
        s.push_str(&t);
        if status != "verified" && style.cite_unverified == "flag" {
            s.push_str(&style.cite_flag);
            unverified = true;
        }
    }
    (s, unverified)
}

struct Placed {
    lines: Vec<String>,
    width: f64,
    height: f64,
    landing: Pt,
    top: f64,
    x: f64,
    left_side: bool,
    fixed: bool,
}

/// Rotated text bounding polygon (for obstacle tests).
pub fn text_poly(it: &Item, pad: f64) -> Option<Vec<Pt>> {
    if let Item::Text { s, x, y, h, rot, align, valign, .. } = it {
        let w = text_width(s, *h);
        let ox = match align.as_str() {
            "center" => -w * 0.5,
            "right" => -w,
            _ => 0.0,
        };
        let oy = match valign.as_str() {
            "middle" => -h * 0.5,
            "top" => -h,
            _ => 0.0,
        };
        let ang = rot.to_radians();
        let pts = [(ox - pad, oy - pad), (ox + w + pad, oy - pad), (ox + w + pad, oy + h + pad), (ox - pad, oy + h + pad)];
        return Some(pts.iter().map(|&(px, py)| {
            let q = pt(px, py).rot(ang);
            pt(x + q.x, y + q.y)
        }).collect());
    }
    None
}

fn seg_hits_poly(a: Pt, b: Pt, poly: &[Pt]) -> bool {
    let sg = Seg::Line(a, b);
    let n = poly.len();
    for i in 0..n {
        if !intersect(&sg, &Seg::Line(poly[i], poly[(i + 1) % n])).is_empty() {
            return true;
        }
    }
    poly_contains(poly, a) || poly_contains(poly, b)
}

fn polylines_cross(a: &[Pt], b: &[Pt]) -> bool {
    for i in 0..a.len() - 1 {
        for j in 0..b.len() - 1 {
            if !intersect(&Seg::Line(a[i], a[i + 1]), &Seg::Line(b[j], b[j + 1])).is_empty() {
                return true;
            }
        }
    }
    false
}

struct Geo {
    h: f64,
    pitch: f64,
    gap: f64,
    shoulder: f64,
    pad: f64,
}

fn leader_of(p: &Placed, g: &Geo) -> [Pt; 3] {
    let ymid = p.top - p.height * 0.5;
    let (edge_x, dir) = if p.left_side { (p.x + p.width + g.pad, 1.0) } else { (p.x - g.pad, -1.0) };
    [pt(edge_x, ymid), pt(edge_x + dir * g.shoulder, ymid), p.landing]
}

/// Stack a column top-down in `order`: each note starts centered on its landing y and is pushed down
/// until it clears the previous note; then the whole column is shifted into the crop height.
fn stack(order: &[usize], placed: &mut [Placed], crop: &Rect, g: &Geo) {
    let mut prev_bottom = f64::INFINITY;
    for &i in order {
        let p = &mut placed[i];
        p.top = p.landing.y + p.height * 0.5;
        if p.top > prev_bottom - g.gap {
            p.top = prev_bottom - g.gap;
        }
        prev_bottom = p.top - p.height;
    }
    if let Some(&last) = order.last() {
        let bottom = placed[last].top - placed[last].height;
        if bottom < crop.y0 {
            let d = crop.y0 - bottom;
            for &i in order {
                placed[i].top += d;
            }
        }
        let top = placed[order[0]].top;
        if top > crop.y1 {
            let d = top - crop.y1;
            for &i in order {
                placed[i].top -= d;
            }
        }
    }
}

/// Lay out notes (SPEC 6.3). `ext` is the extent of the drawing including dimensions and labels
/// (columns start beyond it); `obstacles` are text boxes leaders must not cross.
pub fn layout_notes(notes: &[NoteIn], style: &Style, crop: &Rect, ext: &Rect, obstacles: &[Vec<Pt>], s: f64, side: &str, tag_r: Option<f64>, out: &mut Vec<Vec<Item>>) -> f64 {
    out.clear();
    out.resize(notes.len(), vec![]);
    let h = style.text_height * s;
    let g = Geo { h, pitch: h * style.line_spacing, gap: style.note_gap * s, shoulder: style.shoulder * s, pad: 0.04 * s };
    let gutter = style.gutter * s;
    let xr = crop.x1.max(ext.x1);
    let xl = crop.x0.min(ext.x0);
    let mut placed: Vec<Placed> = vec![];
    for n in notes.iter() {
        let lines = wrap(&n.text, style.wrap_chars);
        let (width, height) = match tag_r {
            Some(r) => (2.0 * r, 2.0 * r),
            None => (lines.iter().map(|l| text_width(l, h)).fold(0.0, f64::max), h + (lines.len() as f64 - 1.0) * g.pitch),
        };
        let left_side = match side {
            "left" => true,
            "both" => (n.landing.x - crop.x0).abs() < (crop.x1 - n.landing.x).abs(),
            _ => false,
        };
        placed.push(Placed { lines, width, height, landing: n.landing, top: n.landing.y + height * 0.5, x: 0.0, left_side, fixed: n.place.is_some() });
    }
    let mut max_extent: f64 = 0.0;
    for want_left in [false, true] {
        let mut order: Vec<usize> = (0..placed.len()).filter(|&i| placed[i].left_side == want_left && !placed[i].fixed).collect();
        order.sort_by(|&a, &b| placed[b].landing.y.partial_cmp(&placed[a].landing.y).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        for &i in &order {
            placed[i].x = if want_left { xl - gutter - placed[i].width } else { xr + gutter };
            max_extent = max_extent.max(placed[i].width + gutter);
        }
        if order.is_empty() {
            continue;
        }
        stack(&order, &mut placed, crop, &g);
        // swap adjacent notes whose leaders cross (bounded, deterministic)
        for _ in 0..(order.len() * 4 + 8) {
            let mut swapped = false;
            for k in 0..order.len().saturating_sub(1) {
                let (a, b) = (order[k], order[k + 1]);
                let (la, lb) = (leader_of(&placed[a], &g), leader_of(&placed[b], &g));
                if polylines_cross(&la, &lb) {
                    order.swap(k, k + 1);
                    stack(&order, &mut placed, crop, &g);
                    swapped = true;
                    break;
                }
            }
            if !swapped {
                break;
            }
        }
        // nudge notes whose leaders run through dimension/label text
        let hits = |p: &Placed| -> bool { let l = leader_of(p, &g); obstacles.iter().any(|o| seg_hits_poly(l[0], l[1], o) || seg_hits_poly(l[1], l[2], o)) };
        for k in 0..order.len() {
            let i = order[k];
            if !hits(&placed[i]) {
                continue;
            }
            let base = placed[i].top;
            let mut found = false;
            'search: for step in 1..=16 {
                for sign in [1.0, -1.0] {
                    let t = base + sign * step as f64 * g.pitch * 0.5;
                    let hgt = placed[i].height;
                    if k > 0 && t > placed[order[k - 1]].top - placed[order[k - 1]].height - g.gap {
                        continue;
                    }
                    if k + 1 < order.len() && placed[order[k + 1]].top > t - hgt - g.gap {
                        continue;
                    }
                    let saved = placed[i].top;
                    placed[i].top = t;
                    let ok = !hits(&placed[i])
                        && !order.iter().filter(|&&o| o != i).any(|&o| polylines_cross(&leader_of(&placed[i], &g), &leader_of(&placed[o], &g)));
                    if ok {
                        found = true;
                        break 'search;
                    }
                    placed[i].top = saved;
                }
            }
            let _ = found;
        }
    }
    for (i, p) in placed.iter_mut().enumerate() {
        if let Some(pl) = notes[i].place {
            p.x = pl.x;
            p.top = pl.y;
            p.left_side = pl.x + p.width * 0.5 < p.landing.x;
        }
    }
    for (i, p) in placed.iter().enumerate() {
        let n = &notes[i];
        let o = &mut out[i];
        if let Some(r) = tag_r {
            let c = pt(p.x + r, p.top - r);
            let hex: Vec<Pt> = (0..6).map(|k| { let a = std::f64::consts::FRAC_PI_3 * k as f64; pt(c.x + r * a.cos(), c.y + r * a.sin()) }).collect();
            o.push(path_item(style, "anno", &n.id, &hex, true));
            o.push(text_item(style, "notes", "anno", &n.id, &n.text, c.x, c.y, h, 0.0, "center", "middle"));
        } else {
            for (j, line) in p.lines.iter().enumerate() {
                o.push(text_item(style, "notes", "anno", &n.id, line, p.x, p.top - h - j as f64 * g.pitch, h, 0.0, "left", "baseline"));
            }
        }
        let l = leader_of(p, &g);
        let land = p.landing;
        let d = (land - l[1]).norm();
        let alen = style.arrow_len * s;
        let aw = style.arrow_width * s;
        let base = land - d * alen;
        let perp = d.perp();
        let tri = [land, base + perp * aw, base - perp * aw];
        o.push(path_item(style, "anno", &n.id, &[l[0], l[1], base], false));
        o.push(Item::Fill { layer: layer_name(style, "notes"), src: n.id.clone(), loops: vec![tri.iter().map(|q| v(q.x, q.y)).collect()] });
    }
    max_extent
}

pub fn dim_items(style: &Style, id: &str, from: Pt, to: Pt, dir: &str, offset: f64, text: Option<&str>, s: f64, out: &mut Vec<Item>) {
    let gap = style.ext_gap * s;
    let over = style.ext_over * s;
    let tick = style.tick_len * s;
    let th = style.text_height * s;
    let tgap = style.text_gap * s;
    let (a, b, u, nrm): (Pt, Pt, Pt, Pt); // dimension line endpoints, direction, normal toward text side
    let (pa, pb);
    match dir {
        "v" => {
            let x = if offset >= 0.0 { from.x.max(to.x) + offset } else { from.x.min(to.x) + offset };
            pa = pt(x, from.y);
            pb = pt(x, to.y);
            let (lo, hi) = if from.y <= to.y { (pa, pb) } else { (pb, pa) };
            a = lo;
            b = hi;
            u = pt(0.0, 1.0);
            nrm = pt(-1.0, 0.0);
        }
        "aligned" => {
            let d = (to - from).norm();
            let n = d.perp();
            pa = from + n * offset;
            pb = to + n * offset;
            a = pa;
            b = pb;
            u = d;
            nrm = n;
        }
        _ => {
            let y = if offset >= 0.0 { from.y.max(to.y) + offset } else { from.y.min(to.y) + offset };
            pa = pt(from.x, y);
            pb = pt(to.x, y);
            let (lo, hi) = if from.x <= to.x { (pa, pb) } else { (pb, pa) };
            a = lo;
            b = hi;
            u = pt(1.0, 0.0);
            nrm = pt(0.0, 1.0);
        }
    }
    // extension lines: from the measured point toward the dimension line point, gap at the object, overshoot beyond
    for (p, q) in [(from, pa), (to, pb)] {
        let d = (q - p).norm();
        if (q - p).len() < 1e-9 {
            continue;
        }
        let start = p + d * gap;
        let end = q + d * over;
        out.push(path_item(style, "dim", id, &[start, end], false));
    }
    let dist = match dir {
        "v" => (to.y - from.y).abs(),
        "aligned" => from.dist(to),
        _ => (to.x - from.x).abs(),
    };
    let label = text.map(|t| t.to_string()).unwrap_or_else(|| fmt_ftin(dist));
    let tw = text_width(&label, th);
    let fits = tw + 2.0 * tgap <= (b - a).len() - tick;
    // dimension line
    let mut la = a;
    let mut lb = b;
    if !fits {
        lb = b + u * (tw + 4.0 * tgap);
        // keep the line short but reaching the outside text
    }
    out.push(path_item(style, "dim", id, &[la, lb], false));
    // ticks (45 degree slashes)
    let tdir = (u + nrm).norm();
    for p in [a, b] {
        out.push(path_item(style, "profile", id, &[p - tdir * (tick * 0.5), p + tdir * (tick * 0.5)], false));
    }
    let _ = &mut la;
    // text
    let mut ang = u.y.atan2(u.x).to_degrees();
    if ang > 90.0 + 1e-9 || ang <= -90.0 + 1e-9 {
        ang += 180.0;
    }
    if dir == "v" {
        ang = 90.0;
    }
    let tn = pt(-ang.to_radians().sin(), ang.to_radians().cos()); // text "up"
    let (center, valign): (Pt, &str) = if fits {
        (a.lerp(b, 0.5) + tn * tgap, "baseline")
    } else {
        // outside: beyond the end of the dimension line
        let pos = b + u * (2.0 * tgap + tw * 0.5 + tick);
        (pos + tn * tgap, "baseline")
    };
    out.push(text_item(style, "dims", "dim", id, &label, center.x, center.y, th, ang, "center", valign));
}

pub fn label_items(style: &Style, id: &str, text: &str, at: Pt, s: f64, out: &mut Vec<Item>) {
    let t = if style.text_upper { text.to_uppercase() } else { text.to_string() };
    out.push(text_item(style, "notes", "anno", id, &t, at.x, at.y, style.label_height * s, 0.0, "center", "middle"));
}

/// Title under the view: bubble, title text with heavy underline, scale (SPEC 6.5). Returns lowest y used.
pub fn title_items(style: &Style, info: &SheetInfo, crop: &Rect, s: f64, view_id: &str, out: &mut Vec<Item>) -> f64 {
    let r = 0.3125 * s; // bubble radius 5/16"
    let top_gap = 0.4 * s;
    let cx = crop.x0 + r;
    let cy = crop.y0 - top_gap - r;
    let src = format!("title:{}", view_id);
    // bubble: two semicircle arcs
    out.push(Item::Path {
        layer: layer_name(style, "title"),
        pen: "title".into(),
        src: src.clone(),
        closed: true,
        pts: vec![vb(cx - r, cy, 1.0), vb(cx + r, cy, 1.0)],
    });
    let th = style.title_height * s;
    let nh = style.text_height * s;
    if info.sheet.is_empty() {
        out.push(text_item(style, "title", "title", &src, &info.number, cx, cy, th, 0.0, "center", "middle"));
    } else {
        out.push(path_item(style, "anno", &src, &[pt(cx - r, cy), pt(cx + r, cy)], false));
        out.push(text_item(style, "title", "title", &src, &info.number, cx, cy + r * 0.5, th, 0.0, "center", "middle"));
        out.push(text_item(style, "title", "anno", &src, &info.sheet, cx, cy - r * 0.5, style.label_height * 0.9 * s, 0.0, "center", "middle"));
    }
    let tx = cx + r + 0.15 * s;
    let title = if style.text_upper { info.title.to_uppercase() } else { info.title.clone() };
    let ty = cy + 0.02 * s;
    out.push(text_item(style, "title", "title", &src, &title, tx, ty, th, 0.0, "left", "baseline"));
    let tw = text_width(&title, th);
    let uy = ty - 0.07 * s;
    out.push(path_item(style, "title", &src, &[pt(tx, uy), pt(tx + tw, uy)], false));
    let scale_line = format!("SCALE: {}", info.scale_text);
    let sy = uy - 0.06 * s - nh;
    out.push(text_item(style, "title", "anno", &src, &scale_line, tx, sy, nh, 0.0, "left", "baseline"));
    let mut low = (cy - r).min(sy - 0.02 * s);
    if info.unverified {
        let fy = low - 0.1 * s - nh;
        out.push(text_item(style, "title", "anno", "footnote", &style.cite_footnote, crop.x0, fy, nh * 0.85, 0.0, "left", "baseline"));
        low = fy - 0.02 * s;
    }
    low
}

pub fn warn_target(id: &str, view: &str, target: &str) -> Diag {
    Diag::warn(
        "W_NOTE_TARGET",
        format!("note \"{}\" in view {}: target \"{}\" is not visible in this view (outside the crop, behind the cut plane, or hidden). The note was not drawn.", id, view, target),
    )
    .id(id)
    .path(format!("views/{}/annotations/{}", view, id))
    .fix("move the view crop or cut_z so the target is visible, change target, or give the note an explicit \"at\" Ref")
}

pub fn lookup_point(lk: &Lookup, v: &Value) -> Result<Pt, Diag> {
    lk.point_value(v)
}
