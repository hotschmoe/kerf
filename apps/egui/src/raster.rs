//! Software rasterizer for the Drawing IR (tiny-skia): this is what `kerf_render` returns to
//! the model (white background, ink, true pen weights, ~1400 px wide) and what the headless
//! tests compare against.

use crate::ir::{PKind, Prep};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

pub struct RenderOpts {
    pub width_px: u32,
    pub margin_px: f32,
    /// ink color
    pub ink: [u8; 3],
    pub bg: [u8; 3],
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts { width_px: 1400, margin_px: 24.0, ink: [0, 0, 0], bg: [255, 255, 255] }
    }
}

pub struct Raster {
    pub pixmap: Pixmap,
    pub png: Vec<u8>,
}

fn paint(c: [u8; 3]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], 255));
    p.anti_alias = true;
    p
}

/// Rasterize a prepared drawing to a PNG. The page keeps the drawing's aspect ratio.
pub fn render(prep: &Prep, opts: &RenderOpts) -> Result<Raster, String> {
    let [x0, y0, x1, y1] = prep.bounds;
    let (bw, bh) = ((x1 - x0).max(1e-6), (y1 - y0).max(1e-6));
    let scale = (opts.width_px as f64 - 2.0 * opts.margin_px as f64) / bw;
    let w = opts.width_px;
    let h = (bh * scale + 2.0 * opts.margin_px as f64).ceil().clamp(64.0, 4000.0) as u32;
    let mut pm = Pixmap::new(w, h).ok_or("pixmap alloc failed")?;
    pm.fill(Color::from_rgba8(opts.bg[0], opts.bg[1], opts.bg[2], 255));
    let m = opts.margin_px;
    // model (x,y up) -> pixel (x,y down)
    let tf = Transform::from_row(scale as f32, 0.0, 0.0, -(scale as f32), m - (x0 * scale) as f32, h as f32 - m + (y0 * scale) as f32);
    let ink = paint(opts.ink);
    let px_per_model = scale;
    // 96 dpi paper: one paper inch = 96 px; model inch = paper inch * prep.scale -> px/model inch (at 1400 px)
    for it in &prep.items {
        let pen_w = (prep.pen_model_width(&it.pen) * px_per_model).max(1.0) as f32;
        match &it.kind {
            PKind::Line { pts, closed } => {
                let mut pb = PathBuilder::new();
                for (i, p) in pts.iter().enumerate() {
                    if i == 0 {
                        pb.move_to(p[0], p[1]);
                    } else {
                        pb.line_to(p[0], p[1]);
                    }
                }
                if *closed {
                    pb.close();
                }
                if let Some(path) = pb.finish() {
                    let mut stroke = Stroke { width: pen_w / scale as f32, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
                    if let Some((d, g)) = prep.pen_dash_model(&it.pen) {
                        stroke.dash = StrokeDash::new(vec![d as f32, g as f32], 0.0);
                    }
                    pm.stroke_path(&path, &ink, &stroke, tf, None);
                }
            }
            PKind::Fill { verts, idx } => {
                for t in idx.chunks_exact(3) {
                    let mut pb = PathBuilder::new();
                    let a = verts[t[0] as usize];
                    let b = verts[t[1] as usize];
                    let c = verts[t[2] as usize];
                    pb.move_to(a[0], a[1]);
                    pb.line_to(b[0], b[1]);
                    pb.line_to(c[0], c[1]);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        pm.fill_path(&path, &ink, FillRule::Winding, tf, None);
                    }
                }
            }
            PKind::Hatch { segs } => {
                let mut pb = PathBuilder::new();
                for s in segs {
                    pb.move_to(s[0], s[1]);
                    pb.line_to(s[2], s[3]);
                }
                if let Some(path) = pb.finish() {
                    let stroke = Stroke { width: pen_w / scale as f32, ..Default::default() };
                    pm.stroke_path(&path, &ink, &stroke, tf, None);
                }
            }
            PKind::Text { strokes } => {
                let mut pb = PathBuilder::new();
                for st in strokes {
                    for (i, p) in st.iter().enumerate() {
                        if i == 0 {
                            pb.move_to(p[0], p[1]);
                        } else {
                            pb.line_to(p[0], p[1]);
                        }
                    }
                    if st.len() == 1 {
                        pb.line_to(st[0][0] + 1e-3, st[0][1]);
                    }
                }
                if let Some(path) = pb.finish() {
                    let stroke = Stroke { width: pen_w / scale as f32, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
                    pm.stroke_path(&path, &ink, &stroke, tf, None);
                }
            }
        }
    }
    let png = pm.encode_png().map_err(|e| format!("png encode: {e}"))?;
    Ok(Raster { pixmap: pm, png })
}
