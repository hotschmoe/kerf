//! 2D viewport: paints the Drawing IR with egui's painter on a vellum sheet with a blue grid.
//! Pen widths are true (paper width x scale x zoom, clamped to 1 device px). Hover outlines a
//! component, selection tints its cut region, notes drag to a new `place`.

use crate::ir::{P2, PKind, Prep};
use crate::theme::*;
use egui::epaint::{Mesh as EMesh, Vertex};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Cam2d {
    pub center: P2,
    /// screen points per model inch
    pub zoom: f64,
}

#[derive(Default)]
pub struct View2dState {
    pub cams: HashMap<String, Cam2d>,
    drag: Option<NoteDrag>,
    pub want_fit: bool,
    /// cached tint triangles per (view, src)
    tint_cache: Option<(String, String, Vec<(Vec<[f32; 2]>, Vec<u32>)>)>,
}

#[derive(Clone, Debug)]
struct NoteDrag {
    id: String,
    delta: P2,
    start_model: P2,
}

pub struct Inputs<'a> {
    pub view_key: &'a str,
    pub prep: &'a Prep,
    pub selected: Option<&'a str>,
    pub hover_prev: Option<&'a str>,
    /// ids that are draggable notes in this view
    pub notes: &'a dyn Fn(&str) -> bool,
    /// current `place` for a note, if set
    pub place_of: &'a dyn Fn(&str) -> Option<P2>,
    pub show_grid: bool,
}

#[derive(Default)]
pub struct Outputs {
    pub hover: Option<String>,
    pub clicked: Option<Option<String>>,
    pub note_moved: Option<(String, P2)>,
    pub cursor_model: Option<P2>,
    pub zoom: f64,
}

impl Cam2d {
    pub fn fit(bounds: [f64; 4], size: Vec2) -> Cam2d {
        let (w, h) = ((bounds[2] - bounds[0]).max(1.0), (bounds[3] - bounds[1]).max(1.0));
        let z = ((size.x as f64 - 40.0) / w).min((size.y as f64 - 40.0) / h).max(0.01);
        Cam2d { center: [(bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0], zoom: z }
    }
    fn to_screen(&self, rect: Rect, p: [f32; 2], off: P2) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((p[0] as f64 + off[0] - self.center[0]) * self.zoom) as f32,
            c.y - ((p[1] as f64 + off[1] - self.center[1]) * self.zoom) as f32,
        )
    }
    fn to_model(&self, rect: Rect, s: Pos2) -> P2 {
        let c = rect.center();
        [self.center[0] + (s.x - c.x) as f64 / self.zoom, self.center[1] - (s.y - c.y) as f64 / self.zoom]
    }
}

pub fn show(ui: &mut Ui, st: &mut View2dState, inp: &Inputs, rect: Rect) -> Outputs {
    let ppp = ui.ctx().pixels_per_point();
    let mut out = Outputs::default();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let prep = inp.prep;

    let mut cam = match st.cams.get(inp.view_key) {
        Some(c) if !st.want_fit => *c,
        _ => {
            st.want_fit = false;
            Cam2d::fit(prep.bounds, rect.size())
        }
    };

    // ---- input: zoom
    if resp.hovered() {
        let (scroll, pinch, hp) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        let f = (scroll as f64 * 0.0025).exp() * pinch as f64;
        if (f - 1.0).abs() > 1e-6 {
            if let Some(hp) = hp {
                let before = cam.to_model(rect, hp);
                cam.zoom = (cam.zoom * f).clamp(0.05, 400.0);
                let after = cam.to_model(rect, hp);
                cam.center[0] += before[0] - after[0];
                cam.center[1] += before[1] - after[1];
            }
        }
    }

    // ---- input: hover / pick
    let ptr = resp.hover_pos();
    if let Some(p) = ptr {
        let m = cam.to_model(rect, p);
        out.cursor_model = Some(m);
        if st.drag.is_none() {
            out.hover = prep.pick(m, 4.0 / cam.zoom);
        }
    }

    // ---- input: drag (pan or note)
    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let (Some(h), Some(p)) = (out.hover.clone().or_else(|| inp.hover_prev.map(str::to_owned)), ptr) {
            if (inp.notes)(&h) && out.hover.as_deref() == Some(h.as_str()) {
                st.drag = Some(NoteDrag { id: h, delta: [0.0, 0.0], start_model: cam.to_model(rect, p) });
            }
        }
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        if let Some(nd) = st.drag.as_mut() {
            if let Some(p) = resp.interact_pointer_pos() {
                let m = cam.to_model(rect, p);
                nd.delta = [m[0] - nd.start_model[0], m[1] - nd.start_model[1]];
            }
        } else {
            cam.center[0] -= d.x as f64 / cam.zoom;
            cam.center[1] += d.y as f64 / cam.zoom;
        }
    }
    if resp.drag_stopped() {
        if let Some(nd) = st.drag.take() {
            if nd.delta[0].hypot(nd.delta[1]) * cam.zoom > 2.0 {
                let base = (inp.place_of)(&nd.id).or_else(|| prep.text_anchor.get(&nd.id).copied()).unwrap_or([0.0, 0.0]);
                out.note_moved = Some((nd.id, [base[0] + nd.delta[0], base[1] + nd.delta[1]]));
            }
        }
    }
    if resp.clicked() {
        out.clicked = Some(out.hover.clone());
    }
    if resp.hovered() {
        if st.drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if out.hover.as_deref().is_some_and(|h| (inp.notes)(h)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        } else if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }
    st.cams.insert(inp.view_key.to_owned(), cam);
    out.zoom = cam.zoom;

    // ---- paint
    let painter = ui.painter_at(rect);
    if inp.show_grid {
        painter.rect_filled(rect, 0.0, VELLUM);
        paint_grid(&painter, rect, &cam);
    } else {
        // SHEET: a white page lying on the desk, with the 2px hard offset shadow
        painter.rect_filled(rect, 0.0, PAPER2);
        let b = prep.bounds;
        let page = Rect::from_two_pos(cam.to_screen(rect, [b[0] as f32, b[3] as f32], [0.0, 0.0]), cam.to_screen(rect, [b[2] as f32, b[1] as f32], [0.0, 0.0]));
        painter.rect_filled(page.translate(Vec2::new(3.0, 3.0)), 0.0, INK);
        painter.rect_filled(page, 0.0, Color32::WHITE);
    }
    let vis_min = cam.to_model(rect, rect.left_bottom());
    let vis_max = cam.to_model(rect, rect.right_top());
    let visible = |b: &[f32; 4]| -> bool {
        (b[2] as f64) >= vis_min[0] - 2.0 && (b[0] as f64) <= vis_max[0] + 2.0 && (b[3] as f64) >= vis_min[1] - 2.0 && (b[1] as f64) <= vis_max[1] + 2.0
    };
    let min_px = 1.0 / ppp;
    let drag_id = st.drag.as_ref().map(|d| d.id.as_str());
    let drag_off = st.drag.as_ref().map(|d| d.delta).unwrap_or([0.0, 0.0]);

    // selection tint
    let sel = inp.selected;
    if let Some(sel) = sel {
        let key = (inp.view_key.to_owned(), sel.to_owned());
        let stale = !matches!(&st.tint_cache, Some((v, s, _)) if *v == key.0 && *s == key.1);
        if stale {
            st.tint_cache = Some((key.0, key.1, prep.region_tris(sel)));
        }
        if let Some((_, _, tris)) = &st.tint_cache {
            let tint = BLUE.gamma_multiply(0.15);
            for (verts, idx) in tris {
                let mut m = EMesh::default();
                for v in verts {
                    m.vertices.push(Vertex { pos: cam.to_screen(rect, *v, [0.0, 0.0]), uv: egui::epaint::WHITE_UV, color: tint });
                }
                m.indices = idx.clone();
                painter.add(Shape::mesh(m));
            }
        }
    } else {
        st.tint_cache = None;
    }

    for it in &prep.items {
        if !visible(&it.bbox) {
            continue;
        }
        let off = if Some(it.src.as_str()) == drag_id && matches!(it.kind, PKind::Text { .. }) { drag_off } else { [0.0, 0.0] };
        let w = ((prep.pen_model_width(&it.pen) * cam.zoom) as f32).max(min_px);
        let stroke = Stroke::new(w, INK);
        match &it.kind {
            PKind::Line { pts, closed } => {
                let sp: Vec<Pos2> = pts.iter().map(|p| cam.to_screen(rect, *p, off)).collect();
                if let Some((d, g)) = prep.pen_dash_model(&it.pen) {
                    let (d, g) = ((d * cam.zoom) as f32, (g * cam.zoom) as f32);
                    let mut sp = sp;
                    if *closed {
                        sp.push(sp[0]);
                    }
                    if d > 0.5 {
                        painter.extend(Shape::dashed_line(&sp, stroke, d.max(2.0), g.max(1.5)));
                        continue;
                    }
                    painter.add(Shape::line(sp, stroke));
                } else if *closed {
                    painter.add(Shape::closed_line(sp, stroke));
                } else {
                    painter.add(Shape::line(sp, stroke));
                }
            }
            PKind::Fill { verts, idx } => {
                let mut m = EMesh::default();
                for v in verts {
                    m.vertices.push(Vertex { pos: cam.to_screen(rect, *v, off), uv: egui::epaint::WHITE_UV, color: INK });
                }
                m.indices = idx.clone();
                painter.add(Shape::mesh(m));
            }
            PKind::Hatch { segs } => {
                for s in segs {
                    painter.line_segment([cam.to_screen(rect, [s[0], s[1]], off), cam.to_screen(rect, [s[2], s[3]], off)], stroke);
                }
            }
            PKind::Text { strokes } => {
                for st in strokes {
                    if st.len() == 1 {
                        continue;
                    }
                    painter.add(Shape::line(st.iter().map(|p| cam.to_screen(rect, *p, off)).collect(), stroke));
                }
            }
        }
    }

    // hover + selection outlines
    let hl_stroke = Stroke::new(2.0, BLUE);
    for (id, solid) in [(out.hover.as_deref().or(inp.hover_prev.filter(|_| ptr.is_some())), false), (sel, true)] {
        let Some(id) = id else { continue };
        if !solid && Some(id) == sel {
            continue;
        }
        let mut any = false;
        for it in prep.outlines(id) {
            if let PKind::Line { pts, closed } = &it.kind {
                let sp: Vec<Pos2> = pts.iter().map(|p| cam.to_screen(rect, *p, [0.0, 0.0])).collect();
                painter.add(if *closed { Shape::closed_line(sp, hl_stroke) } else { Shape::line(sp, hl_stroke) });
                any = true;
            }
        }
        let _ = any;
        {
            if let Some(b) = prep.text_boxes.get(id) {
                let off = if Some(id) == drag_id { drag_off } else { [0.0, 0.0] };
                let r = Rect::from_two_pos(
                    cam.to_screen(rect, [b[0] as f32, b[3] as f32], off),
                    cam.to_screen(rect, [b[2] as f32, b[1] as f32], off),
                );
                painter.rect_stroke(r.expand(2.0), 0.0, Stroke::new(1.5, BLUE), egui::StrokeKind::Outside);
            }
        }
    }
    out
}

pub fn paint_grid_public(p: &egui::Painter, rect: Rect, cam: &Cam2d) {
    paint_grid(p, rect, cam)
}

fn paint_grid(p: &egui::Painter, rect: Rect, cam: &Cam2d) {
    // levels in model inches; each fades in as its pixel spacing grows past ~6 px
    let levels: [(f64, Color32); 4] = [(0.25, GRID2), (1.0, GRID), (12.0, GRID), (120.0, GRID)];
    let tl = cam.to_model(rect, rect.left_top());
    let br = cam.to_model(rect, rect.right_bottom());
    for (i, (step, color)) in levels.iter().enumerate() {
        let px = step * cam.zoom;
        // the 1" level is always the "major" (darker); 12" shows only when 1" is too dense
        let next_px = levels.get(i + 1).map(|l| l.0 * cam.zoom).unwrap_or(f64::MAX);
        let fade_in = ((px - 6.0) / 8.0).clamp(0.0, 1.0);
        // minor levels fade out once the next level is itself clearly visible
        let _ = next_px;
        if fade_in <= 0.0 {
            continue;
        }
        // skip the 12"/120" levels while the finer majors are still legible
        if i >= 2 && cam.zoom * levels[i - 1].0 >= 6.0 {
            continue;
        }
        let a = (fade_in * 255.0) as u8;
        let c = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a);
        let stroke = Stroke::new(if *step >= 1.0 { 1.0 } else { 0.5 }, c);
        let (x0, x1) = ((tl[0] / step).floor() as i64, (br[0] / step).ceil() as i64);
        let (y0, y1) = ((br[1] / step).floor() as i64, (tl[1] / step).ceil() as i64);
        if (x1 - x0) > 600 || (y1 - y0) > 600 {
            continue;
        }
        // minor lines that coincide with a major are drawn by the major pass
        for k in x0..=x1 {
            if i == 0 && k % 4 == 0 {
                continue;
            }
            let sx = rect.center().x + ((k as f64 * step - cam.center[0]) * cam.zoom) as f32;
            p.vline(sx.round() + 0.5, rect.y_range(), stroke);
        }
        for k in y0..=y1 {
            if i == 0 && k % 4 == 0 {
                continue;
            }
            let sy = rect.center().y - ((k as f64 * step - cam.center[1]) * cam.zoom) as f32;
            p.hline(rect.x_range(), sy.round() + 0.5, stroke);
        }
    }
}
