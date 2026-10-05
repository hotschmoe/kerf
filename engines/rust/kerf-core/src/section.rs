//! Section view (SPEC section 8.1): exact 2D cut/beyond classification, occlusion, hatch, breaks.

use crate::diag::Diag;
use crate::drawing::{Item, layer_name, pen_layer_key};
use crate::geom::*;
use crate::stroke::offset_poly;
use crate::hatch::{clip_line_rect, hatch_lines};
use crate::model::*;
use crate::poly::{self, Shape};
use crate::style::Style;
use crate::view::ViewParams;

#[derive(Clone, Debug)]
pub struct VisInfo {
    pub comp: usize,
    pub src: String,
    pub part: Option<String>,
    pub shapes: Vec<Shape>,
    pub cut: bool,
    pub embedded: bool,
    /// Prism order in the model (component, instance, part): tie-break for note landing.
    pub ord: usize,
    pub inst: usize,
    /// Exact visible regions (arcs kept where untouched); empty = use `shapes`.
    pub exact: Vec<Region>,
}

pub struct ViewBase {
    pub items: Vec<Item>,
    pub vis: Vec<VisInfo>,
    pub crop: Rect,
    pub s: f64,
}

#[derive(Clone)]
struct Piece {
    seg: Seg,
    pen: String,
}

struct Stroke {
    pieces: Vec<Piece>,
    closed_loop: bool,
    src: String,
    comp: usize,
    internal_ok: bool,
    kind_rank: u8, // 0 beyond, 1 cut, 2 embedded
    overlay: bool,
}

struct Occ {
    region: Region,
    bbox: Rect,
    flat_outer: Vec<Pt>,
    flat_holes: Vec<Vec<Pt>>,
    segs: Vec<Seg>,
    z1: f64,
}

impl Occ {
    fn new(region: &Region, z1: f64) -> Occ {
        let mut segs = loop_segs(&region.outer);
        for h in &region.holes {
            segs.extend(loop_segs(h));
        }
        Occ {
            region: region.clone(),
            bbox: region_bbox(region),
            flat_outer: flatten_loop(&region.outer, 0.0005),
            flat_holes: region.holes.iter().map(|h| flatten_loop(h, 0.0005)).collect(),
            segs,
            z1,
        }
    }
    fn locate(&self, p: Pt) -> Loc {
        if !self.bbox.contains(p, 1e-6) {
            return Loc::Outside;
        }
        for s in &self.segs {
            if s.dist(p) <= 1e-6 {
                return Loc::Boundary;
            }
        }
        if !poly_contains(&self.flat_outer, p) {
            return Loc::Outside;
        }
        for h in &self.flat_holes {
            if poly_contains(h, p) {
                return Loc::Outside;
            }
        }
        Loc::Inside
    }
}

fn seg_outward_normal(s: &Seg) -> Pt {
    let t = s.tangent(0.5);
    pt(t.y, -t.x) // right of travel for CCW outer loops
}

fn visible_segments(seg: &Seg, occ: &[&Occ]) -> Vec<Seg> {
    let sb = seg.bbox().inflate(1e-6);
    let mut ts = vec![0.0, 1.0];
    let mut relevant: Vec<&Occ> = vec![];
    for o in occ {
        if !o.bbox.overlaps(&sb, 0.0) {
            continue;
        }
        relevant.push(o);
        for os in &o.segs {
            if !os.bbox().overlaps(&sb, 0.0) {
                continue;
            }
            for (t, _) in intersect(seg, os) {
                ts.push(t);
            }
            if let Some((a, b)) = collinear_overlap(seg, os, 1e-6) {
                ts.push(a);
                ts.push(b);
            }
        }
    }
    if relevant.is_empty() {
        return vec![*seg];
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut out: Vec<(f64, f64)> = vec![];
    for w in ts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a < 1e-9 {
            continue;
        }
        let m = seg.at((a + b) * 0.5);
        if relevant.iter().any(|o| o.locate(m) == Loc::Inside) {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if (last.1 - a).abs() < 1e-9 {
                last.1 = b;
                continue;
            }
        }
        out.push((a, b));
    }
    out.into_iter().map(|(a, b)| seg.sub(a, b)).collect()
}

fn clip_segments(segs: Vec<Seg>, crop: &Rect) -> Vec<Seg> {
    let mut out = vec![];
    for s in segs {
        let bb = s.bbox();
        if bb.x0 >= crop.x0 - 1e-9 && bb.x1 <= crop.x1 + 1e-9 && bb.y0 >= crop.y0 - 1e-9 && bb.y1 <= crop.y1 + 1e-9 {
            out.push(s);
            continue;
        }
        if !bb.overlaps(crop, 0.0) {
            continue;
        }
        for (a, b) in clip_seg_rect(&s, crop) {
            out.push(s.sub(a, b));
        }
    }
    out
}

fn region_thickness(r: &Region) -> f64 {
    let mut area = loop_area(&r.outer).abs();
    for h in &r.holes {
        area -= loop_area(h).abs();
    }
    let per: f64 = loop_segs(&r.outer).iter().map(|s| s.len()).sum();
    if per < 1e-9 { 0.0 } else { 2.0 * area / per }
}

fn is_fill_material(m: &str) -> bool {
    matches!(m, "earth" | "gravel" | "sand" | "compacted_fill")
}

fn seg_len_tol(s: &Seg) -> bool {
    s.len() > 1e-7
}

/// Split a piece by a parameter interval; returns (before, inside, after).
fn split3(p: &Piece, t0: f64, t1: f64) -> (Option<Piece>, Piece, Option<Piece>) {
    let before = if t0 > 1e-9 { Some(Piece { seg: p.seg.sub(0.0, t0), pen: p.pen.clone() }) } else { None };
    let after = if t1 < 1.0 - 1e-9 { Some(Piece { seg: p.seg.sub(t1, 1.0), pen: p.pen.clone() }) } else { None };
    (before, Piece { seg: p.seg.sub(t0.max(0.0), t1.min(1.0)), pen: p.pen.clone() }, after)
}

fn overlap_in_a(a: &Seg, b: &Seg) -> Option<(f64, f64)> {
    if !a.is_line() || !b.is_line() {
        return None;
    }
    let (ba, bb) = (a.bbox().inflate(1e-6), b.bbox());
    if !ba.overlaps(&bb, 0.0) {
        return None;
    }
    collinear_overlap(a, b, 1e-6)
}

fn dedupe(strokes: &mut Vec<Stroke>, style: &Style) {
    let n = strokes.len();
    // phase 1: internal edges between parts of one component become light
    for a in 0..n {
        if !strokes[a].internal_ok {
            continue;
        }
        for b in (a + 1)..n {
            if !strokes[b].internal_ok || strokes[a].comp != strokes[b].comp {
                continue;
            }
            let mut ia = 0;
            let mut guard = 0;
            while ia < strokes[a].pieces.len() && guard < 10000 {
                guard += 1;
                let pa = strokes[a].pieces[ia].clone();
                if !(pa.pen == "cut" || pa.pen == "profile") {
                    ia += 1;
                    continue;
                }
                let mut hit = None;
                for (ib, pb) in strokes[b].pieces.iter().enumerate() {
                    if !(pb.pen == "cut" || pb.pen == "profile") {
                        continue;
                    }
                    if let Some(ov) = overlap_in_a(&pa.seg, &pb.seg) {
                        hit = Some((ib, ov));
                        break;
                    }
                }
                match hit {
                    None => ia += 1,
                    Some((ib, (ta0, ta1))) => {
                        let pb = strokes[b].pieces[ib].clone();
                        let (tb0, tb1) = collinear_overlap(&pb.seg, &pa.seg, 1e-6).unwrap_or((0.0, 1.0));
                        let (ab, ai, aa) = split3(&pa, ta0, ta1);
                        let mut rep = vec![];
                        rep.extend(ab);
                        let mut ai = ai;
                        ai.pen = "beyond".into();
                        rep.push(ai);
                        rep.extend(aa);
                        strokes[a].pieces.splice(ia..ia + 1, rep);
                        let (bb_, bi, ba_) = split3(&pb, tb0, tb1);
                        let mut rep = vec![];
                        rep.extend(bb_);
                        let mut bi = bi;
                        bi.pen = "beyond".into();
                        rep.push(bi);
                        rep.extend(ba_);
                        strokes[b].pieces.splice(ib..ib + 1, rep);
                    }
                }
            }
        }
    }
    // phase 2: coincident edges draw once; heavier pen wins, earlier wins ties
    let rank = |pen: &str| style.pen(pen).width_mm;
    for l in 0..n {
        for w in 0..n {
            if l == w || strokes[l].overlay || strokes[w].overlay {
                continue;
            }
            let wins = strokes[w].pieces.clone();
            let mut next: Vec<Piece> = vec![];
            let pieces = std::mem::take(&mut strokes[l].pieces);
            for pa in pieces {
                let mut remaining = vec![pa];
                for pb in &wins {
                    let mut nr = vec![];
                    for pa in remaining {
                        let wins_over = {
                            let (ra, rb) = (rank(&pa.pen), rank(&pb.pen));
                            rb > ra + 1e-9 || ((rb - ra).abs() <= 1e-9 && w < l)
                        };
                        match if wins_over { overlap_in_a(&pa.seg, &pb.seg) } else { None } {
                            None => nr.push(pa),
                            Some((t0, t1)) => {
                                let (b, _i, a) = split3(&pa, t0, t1);
                                nr.extend(b);
                                nr.extend(a);
                            }
                        }
                    }
                    remaining = nr;
                }
                next.extend(remaining);
            }
            strokes[l].pieces = next;
        }
    }
}

fn path_from_segs(segs: &[Seg], closed: bool) -> Vec<V> {
    let mut pts: Vec<V> = segs.iter().map(|s| vb(s.start().x, s.start().y, s.bulge())).collect();
    if !closed {
        if let Some(last) = segs.last() {
            pts.push(v(last.end().x, last.end().y));
        }
    }
    pts
}

fn emit_strokes(strokes: &[Stroke], style: &Style, out: &mut Vec<Item>) {
    for st in strokes {
        let mut chains: Vec<(String, Vec<Seg>)> = vec![];
        for p in &st.pieces {
            if !seg_len_tol(&p.seg) {
                continue;
            }
            match chains.last_mut() {
                Some((pen, segs)) if *pen == p.pen && segs.last().unwrap().end().near(p.seg.start(), 1e-7) => segs.push(p.seg),
                _ => chains.push((p.pen.clone(), vec![p.seg])),
            }
        }
        // wrap-around merge for loops
        if chains.len() == 1 && st.closed_loop {
            let (pen, segs) = &chains[0];
            let closed = segs.last().unwrap().end().near(segs[0].start(), 1e-7);
            out.push(Item::Path {
                layer: layer_name(style, pen_layer_key(pen)),
                pen: pen.clone(),
                src: st.src.clone(),
                closed,
                pts: path_from_segs(segs, closed),
            });
            continue;
        }
        if chains.len() > 1 && st.closed_loop {
            let first_start = chains[0].1[0].start();
            let ok = {
                let last = chains.last().unwrap();
                last.0 == chains[0].0 && last.1.last().unwrap().end().near(first_start, 1e-7)
            };
            if ok {
                let last = chains.pop().unwrap();
                let mut merged = last.1;
                merged.extend(chains.remove(0).1);
                chains.insert(0, (last.0, merged));
            }
        }
        for (pen, segs) in chains {
            out.push(Item::Path {
                layer: layer_name(style, pen_layer_key(&pen)),
                pen: pen.clone(),
                src: st.src.clone(),
                closed: false,
                pts: path_from_segs(&segs, false),
            });
        }
    }
}

/// Sutherland-style polygon `Region` to IR loops.
fn loops_of(r: &Region) -> Vec<Vec<V>> {
    let mut l = vec![r.outer.clone()];
    for h in &r.holes {
        l.push(h.clone());
    }
    l
}

pub fn build_section(model: &Model, vp: &ViewParams, style: &Style, _diags: &mut Vec<Diag>) -> ViewBase {
    let cut_z = vp.cut_z;
    // classify
    let mut cuts: Vec<&Prism> = vec![];
    let mut beyonds: Vec<&Prism> = vec![];
    let mut cut_ord: Vec<usize> = vec![];
    let mut bey_ord: Vec<usize> = vec![];
    let mut ord_counter = 0usize;
    for c in &model.comps {
        if !c.visible || c.failed || vp.omit.contains(&c.id) {
            continue;
        }
        for inst in &c.insts {
            for p in &inst.prisms {
                ord_counter += 1;
                if p.only == Only::Solid3d {
                    continue;
                }
                if p.z0 < cut_z && cut_z < p.z1 {
                    cuts.push(p);
                    cut_ord.push(ord_counter);
                } else if p.z1 <= cut_z {
                    beyonds.push(p);
                    bey_ord.push(ord_counter);
                }
            }
        }
    }
    // crop
    let crop = match vp.crop {
        Some(c) => c,
        None => {
            let mut r = Rect::empty();
            for p in cuts.iter().chain(beyonds.iter()) {
                r.union(&region_bbox(&p.region));
            }
            if r.is_empty() { Rect::new(0.0, 0.0, 12.0, 12.0) } else { r }
        }
    };
    let s = match vp.factor {
        Some(f) => f,
        None => (crop.w().max(crop.h()) / 6.0).max(1.0),
    };

    let cut_occ: Vec<Occ> = cuts.iter().map(|p| Occ::new(&p.region, p.z1)).collect();
    let bey_occ: Vec<Option<Occ>> = beyonds.iter().map(|p| if p.embedded { None } else { Some(Occ::new(&p.region, p.z1)) }).collect();

    let mut strokes: Vec<Stroke> = vec![];
    let mut vis: Vec<VisInfo> = vec![];

    // --- strokes for cut prisms
    let pen_of = |p: &Prism, default: &str| p.pen.clone().unwrap_or_else(|| default.to_string());
    let mut push_prism_strokes = |p: &Prism, occ: &[&Occ], default_pen: &str, kind_rank: u8, strokes: &mut Vec<Stroke>| {
        let mut pen = pen_of(p, default_pen);
        {
            let metal = matches!(p.material.as_str(), "steel" | "aluminum" | "flashing_membrane");
            if kind_rank != 0 && metal && p.center.is_none() && p.bar.is_none() && region_thickness(&p.region) / s < 2.0 * style.pen_width_in(&pen) {
                pen = "steel".to_string(); // thin sheet metal: fill + steel-pen outline
            }
        }
        let mut loops: Vec<&Loop> = vec![&p.region.outer];
        loops.extend(p.region.holes.iter());
        let thin_center = p.center.as_ref().filter(|(_, t)| t.abs() / s < 2.0 * style.pen_width_in(&pen));
        if let Some((cl, t)) = thin_center {
            // keep the dashed/heavy line clear of the host's cut outline
            let clear = (style.pen_width_in("cut") + style.pen_width_in(&pen)) * 0.5 * s;
            let off = t.signum() * (t.abs() * 0.5).max(clear);
            let cl = offset_poly(cl, off);
            let mut pieces = vec![];
            for sg in poly_segs(&cl.iter().map(|q| v(q.x, q.y)).collect::<Vec<V>>()) {
                let vis_segs = if occ.is_empty() { vec![sg] } else { visible_segments(&sg, occ) };
                for vs in clip_segments(vis_segs, &crop) {
                    pieces.push(Piece { seg: vs, pen: pen.clone() });
                }
            }
            if !pieces.is_empty() {
                strokes.push(Stroke { pieces, closed_loop: false, src: p.src.clone(), comp: p.comp, internal_ok: false, kind_rank, overlay: true });
            }
        } else if p.outline != Outline::None {
            for l in loops {
                let mut pieces = vec![];
                let mut all_kept = true;
                for sg in loop_segs(l) {
                    if p.outline == Outline::Top && seg_outward_normal(&sg).y <= 0.01 {
                        all_kept = false;
                        continue;
                    }
                    let vis_segs = if occ.is_empty() { vec![sg] } else { visible_segments(&sg, occ) };
                    if vis_segs.len() != 1 || (vis_segs[0].len() - sg.len()).abs() > 1e-9 {
                        all_kept = false;
                    }
                    for vs in clip_segments(vis_segs, &crop) {
                        pieces.push(Piece { seg: vs, pen: pen.clone() });
                    }
                }
                if !pieces.is_empty() {
                    strokes.push(Stroke {
                        pieces,
                        closed_loop: all_kept && p.outline == Outline::Full,
                        src: p.src.clone(),
                        comp: p.comp,
                        internal_ok: kind_rank == 1 && !p.embedded,
                        kind_rank,
                        overlay: false,
                    });
                }
            }
        }
        for e in &p.extra {
            let segs = if e.closed { loop_segs(&e.pts) } else { poly_segs(&e.pts) };
            let mut pieces = vec![];
            for sg in segs {
                let vis_segs = if occ.is_empty() { vec![sg] } else { visible_segments(&sg, occ) };
                for vs in clip_segments(vis_segs, &crop) {
                    pieces.push(Piece { seg: vs, pen: e.pen.clone() });
                }
            }
            if !pieces.is_empty() {
                strokes.push(Stroke { pieces, closed_loop: false, src: p.src.clone(), comp: p.comp, internal_ok: false, kind_rank, overlay: false });
            }
        }
    };

    // beyond strokes
    for (bi, p) in beyonds.iter().enumerate() {
        let mut occ: Vec<&Occ> = cut_occ.iter().collect();
        if !p.embedded {
            for (qi, _q) in beyonds.iter().enumerate() {
                if qi != bi {
                    if let Some(o) = &bey_occ[qi] {
                        if o.z1 > p.z1 + 1e-9 {
                            occ.push(o);
                        }
                    }
                }
            }
        } else {
            occ.clear();
        }
        let kr = if p.embedded { 2 } else { 0 };
        push_prism_strokes(p, &occ, "beyond", kr, &mut strokes);
    }
    let n_beyond_strokes = strokes.len();
    // cut strokes
    let none: Vec<&Occ> = vec![];
    for p in &cuts {
        let kr = if p.embedded { 2 } else { 1 };
        push_prism_strokes(p, &none, "cut", kr, &mut strokes);
    }
    let _ = n_beyond_strokes;
    dedupe(&mut strokes, style);

    // --- hatch + fills + marks for cut prisms
    let mut items: Vec<Item> = vec![];
    let mut hatch_items: Vec<Item> = vec![];
    let mut mark_items: Vec<Item> = vec![];
    let mut fill_items: Vec<Item> = vec![];
    let embedded_cuts: Vec<&&Prism> = cuts.iter().filter(|p| p.embedded).collect();
    for (cut_i, p) in cuts.iter().enumerate() {
        let mst = style.material(&p.material);
        let pen_w = style.pen_width_in(p.pen.as_deref().unwrap_or("cut"));
        let metal = matches!(p.material.as_str(), "steel" | "aluminum" | "flashing_membrane");
        let thin = metal && p.center.is_none() && region_thickness(&p.region) / s < 2.0 * pen_w;
        let thin_membrane = p.center.as_ref().map_or(false, |(_, t)| *t / s < 2.0 * style.pen_width_in(p.pen.as_deref().unwrap_or("membrane")));
        // visible region (for notes) and hatch loops
        let mut region = p.region.clone();
        // punch embedded cut prisms out of the hatch region
        let mut holes_added = false;
        let mut need_bool = false;
        if !p.embedded && !mst.hatch.is_empty() {
            let bb = region_bbox(&p.region);
            for e in &embedded_cuts {
                if e.src == p.src {
                    continue;
                }
                let eb = region_bbox(&e.region);
                if !eb.overlaps(&bb, 0.0) {
                    continue;
                }
                let c = pt(eb.cx(), eb.cy());
                match region_locate(&p.region, c, 1e-7) {
                    Loc::Inside => {
                        let inside_all = e.region.outer.iter().all(|q| region_locate(&p.region, q.p(), 1e-7) != Loc::Outside)
                            && eb.x0 >= bb.x0 - 1e-9
                            && eb.x1 <= bb.x1 + 1e-9
                            && eb.y0 >= bb.y0 - 1e-9
                            && eb.y1 <= bb.y1 + 1e-9;
                        if inside_all && loop_dist(&p.region.outer, c) > eb.w().max(eb.h()) * 0.5 {
                            region.holes.push(make_cw(&e.region.outer));
                            holes_added = true;
                        } else {
                            need_bool = true;
                        }
                    }
                    Loc::Boundary => need_bool = true,
                    Loc::Outside => {
                        if eb.overlaps(&bb, 0.0) {
                            // may still straddle the boundary
                            if e.region.outer.iter().any(|q| region_locate(&p.region, q.p(), 1e-7) == Loc::Inside) {
                                need_bool = true;
                            }
                        }
                    }
                }
            }
        }
        let _ = holes_added;
        let mut regions: Vec<Region> = vec![];
        if need_bool {
            let subj = poly::region_contours(&p.region, 0.002);
            let clip: Vec<Vec<Pt>> = embedded_cuts
                .iter()
                .filter(|e| e.src != p.src)
                .map(|e| flatten_loop(&e.region.outer, 0.002))
                .collect();
            for sh in poly::difference(&subj, &clip) {
                regions.extend(poly::clip_region_rect(&poly::shape_to_region(&sh), &crop));
            }
        } else {
            regions.extend(poly::clip_region_rect(&region, &crop));
        }
        // visible shapes for note landing
        let shapes: Vec<Shape> = regions.iter().map(|r| poly::region_contours(r, 0.002)).collect();
        vis.push(VisInfo { comp: p.comp, src: p.src.clone(), part: p.part.clone(), shapes, cut: true, embedded: p.embedded, ord: cut_ord[cut_i], inst: p.inst, exact: regions.clone() });

        // hatch
        if !p.embedded && !thin && !thin_membrane {
            for spec in &mst.hatch {
                let Some(pat) = style.pattern(&spec.pattern) else { continue };
                for r in &regions {
                    let flat: Vec<Vec<Pt>> = r.loops().map(|l| flatten_loop(l, 0.002)).collect();
                    let lines = hatch_lines(&flat, pat, spec.scale * s, spec.angle, None);
                    hatch_items.push(Item::Hatch {
                        layer: layer_name(style, "hatch"),
                        pen: "hatch".into(),
                        src: p.src.clone(),
                        pattern: spec.pattern.clone(),
                        scale: spec.scale,
                        angle: spec.angle,
                        loops: loops_of(r),
                        lines,
                    });
                }
            }
        }
        // solid fill
        if (p.fill_solid || thin) && !thin_membrane {
            for r in poly::clip_region_rect(&Region::new(p.region.outer.clone()), &crop) {
                fill_items.push(Item::Fill { layer: layer_name(style, pen_layer_key(p.pen.as_deref().unwrap_or(if p.fill_solid { "steel" } else { "cut" }))), src: p.src.clone(), loops: loops_of(&r) });
            }
        }
        // wood marks
        if p.along_z && !p.marks.is_empty() && !thin {
            if let Some(mark) = &mst.cut_mark {
                for q in &p.marks {
                    let mut diags_: Vec<(Pt, Pt)> = vec![];
                    if p.diag_mark || mark == "diagonal" {
                        diags_.push((q[0], q[2]));
                    } else {
                        diags_.push((q[0], q[2]));
                        diags_.push((q[1], q[3]));
                    }
                    for (a, b) in diags_ {
                        if let Some((ca, cb)) = clip_line_rect(a, b, &crop) {
                            mark_items.push(Item::Path {
                                layer: layer_name(style, "beyond"),
                                pen: "beyond".into(),
                                src: p.src.clone(),
                                closed: false,
                                pts: vec![v(ca.x, ca.y), v(cb.x, cb.y)],
                            });
                        }
                    }
                }
            }
        }
    }

    // visible shapes for beyond prisms
    for (bi, p) in beyonds.iter().enumerate() {
        let subj = poly::region_contours(&p.region, 0.002);
        let bb = region_bbox(&p.region);
        let mut clip: Vec<Vec<Pt>> = vec![];
        if !p.embedded {
            for o in &cut_occ {
                if o.bbox.overlaps(&bb, 0.0) {
                    clip.push(o.flat_outer.clone());
                }
            }
            for (qi, o) in bey_occ.iter().enumerate() {
                if qi == bi {
                    continue;
                }
                if let Some(o) = o {
                    if o.z1 > p.z1 + 1e-9 && o.bbox.overlaps(&bb, 0.0) {
                        clip.push(o.flat_outer.clone());
                    }
                }
            }
        }
        let mut bregions: Vec<Region> = vec![];
        if clip.is_empty() {
            bregions.extend(poly::clip_region_rect(&p.region, &crop));
        } else {
            for sh in poly::difference(&subj, &clip) {
                let r = poly::shape_to_region(&sh);
                bregions.extend(poly::clip_region_rect(&r, &crop));
            }
        }
        let shapes: Vec<Shape> = bregions.iter().map(|cr| poly::region_contours(cr, 0.002)).collect();
        vis.push(VisInfo { comp: p.comp, src: p.src.clone(), part: p.part.clone(), shapes, cut: false, embedded: p.embedded, ord: bey_ord[bi], inst: p.inst, exact: bregions });
    }

    // --- break lines along crop edges where cut solids are clipped
    let mut brk_items: Vec<Item> = vec![];
    {
        // (vertical edge?, coordinate of the edge, lo, hi); order: left, right, bottom, top
        let edges: [(bool, f64, f64, f64); 4] = [
            (true, crop.x0, crop.y0, crop.y1),
            (true, crop.x1, crop.y0, crop.y1),
            (false, crop.y0, crop.x0, crop.x1),
            (false, crop.y1, crop.x0, crop.x1),
        ];
        for (vertical, c, elo, ehi) in edges {
            let (a, b) = if vertical { (pt(c, elo), pt(c, ehi)) } else { (pt(elo, c), pt(ehi, c)) };
            let eseg = Seg::Line(a, b);
            let len = ehi - elo;
            let mut intervals: Vec<(f64, f64)> = vec![];
            for (pi, p) in cuts.iter().enumerate() {
                let mst = style.material(&p.material);
                if p.embedded || p.center.is_some() || is_fill_material(&p.material) || mst.fill {
                    continue;
                }
                let o = &cut_occ[pi];
                if !o.bbox.overlaps(&eseg.bbox(), 1e-9) {
                    continue;
                }
                let mut ts = vec![0.0, 1.0];
                for os in &o.segs {
                    for (t, _) in intersect(&eseg, os) {
                        ts.push(t);
                    }
                }
                ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
                ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
                for w in ts.windows(2) {
                    if (w[1] - w[0]) * len < 1e-6 {
                        continue;
                    }
                    let m = eseg.at((w[0] + w[1]) * 0.5);
                    if o.locate(m) == Loc::Inside {
                        intervals.push((elo + w[0] * len, elo + w[1] * len));
                    }
                }
            }
            intervals.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            let mut merged: Vec<(f64, f64)> = vec![];
            for iv in intervals {
                if let Some(last) = merged.last_mut() {
                    if iv.0 <= last.1 + 1e-3 {
                        last.1 = last.1.max(iv.1);
                        continue;
                    }
                }
                merged.push(iv);
            }
            for (u0, u1) in merged {
                // SPEC 16 parity: start, a, peak, valley, b, end (one zigzag at the middle)
                let a_ = u0 - style.brk_over * s;
                let b_ = u1 + style.brk_over * s;
                let l = b_ - a_;
                let mid = (a_ + b_) * 0.5;
                let half = (style.brk_period * s * 0.5).min(l * 0.35);
                let zig = (style.brk_zig * s).min(l * 0.2);
                let uv = [(a_, 0.0), (mid - half, 0.0), (mid - half * 0.4, zig), (mid + half * 0.4, -zig), (mid + half, 0.0), (b_, 0.0)];
                let pts: Vec<V> = uv.iter().map(|&(u, off)| if vertical { v(c + off, u) } else { v(u, c + off) }).collect();
                brk_items.push(Item::Path { layer: layer_name(style, "break"), pen: "break".into(), src: "crop".into(), closed: false, pts });
            }
        }
    }

    // --- assemble in drawing order
    items.extend(hatch_items);
    let (embedded_strokes, normal_strokes): (Vec<Stroke>, Vec<Stroke>) = strokes.into_iter().partition(|s| s.kind_rank == 2);
    emit_strokes(&normal_strokes.iter().filter(|s| s.kind_rank == 0).map(clone_stroke).collect::<Vec<_>>(), style, &mut items);
    emit_strokes(&normal_strokes.iter().filter(|s| s.kind_rank == 1).map(clone_stroke).collect::<Vec<_>>(), style, &mut items);
    items.extend(mark_items);
    items.extend(fill_items);
    emit_strokes(&embedded_strokes, style, &mut items);
    items.extend(brk_items);
    items.extend(region_items(&vis));
    ViewBase { items, vis, crop, s }
}

fn clone_stroke(s: &Stroke) -> Stroke {
    Stroke { pieces: s.pieces.clone(), closed_loop: s.closed_loop, src: s.src.clone(), comp: s.comp, internal_ok: s.internal_ok, kind_rank: s.kind_rank, overlay: s.overlay }
}

/// Non-drawn region items (SPEC 16) from the visible shapes, in drawing order (cut first, then beyond).
pub fn region_items(vis: &[VisInfo]) -> Vec<Item> {
    let mut out = vec![];
    for cut in [true, false] {
        for vi in vis.iter().filter(|v| v.cut == cut) {
            let loops_of_region = |r: &Region| -> Vec<Vec<V>> {
                let mut l = vec![r.outer.clone()];
                l.extend(r.holes.iter().cloned());
                l
            };
            let regions: Vec<Vec<Vec<V>>> = if !vi.exact.is_empty() {
                vi.exact.iter().map(loops_of_region).collect()
            } else {
                vi.shapes.iter().map(|sh| sh.iter().map(|c| c.iter().map(|p| v(p.x, p.y)).collect()).collect()).collect()
            };
            for loops in regions {
                if loops.first().map_or(true, |l| l.len() < 2) {
                    continue;
                }
                out.push(Item::Region { src: vi.src.clone(), part: vi.part.clone(), instance: vi.inst, cut: vi.cut, loops });
            }
        }
    }
    out
}
