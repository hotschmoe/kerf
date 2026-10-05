//! 3D viewport UI: camera control, picking, and the wgpu paint callback.

use crate::app::{InspTab, KerfApp};
use crate::gpu3d::{self, Callback, Params};
use crate::theme::*;
use egui::{Pos2, Rect, Sense, Ui, Vec2};
use glam::Vec3;

impl KerfApp {
    pub fn view3d_ui(&mut self, ui: &mut Ui, body: Rect) {
        let mesh = match self.session.mesh() {
            Ok(m) => m,
            Err(e) => {
                ui.painter().rect_filled(body, 0.0, PAPER);
                ui.painter().text(body.center(), egui::Align2::CENTER_CENTER, format!("MESH FAILED: {e}"), regular(13.0), RED);
                return;
            }
        };
        let Some((lo, hi)) = mesh.bounds() else {
            ui.painter().rect_filled(body, 0.0, PAPER);
            ui.painter().text(body.center(), egui::Align2::CENTER_CENTER, "NO GEOMETRY", regular(13.0), INK2);
            return;
        };
        let center = (Vec3::from(lo) + Vec3::from(hi)) * 0.5;
        let radius = (Vec3::from(hi) - Vec3::from(lo)).length() * 0.5;
        let aspect_fit = body.width() / body.height().max(1.0);
        // fit when the document (or view) changes
        let key = (self.session.rev, "fit".to_owned());
        if self.cam3_key.as_ref() != Some(&key) {
            self.cam3.target = center;
            self.cam3.half_h = radius * 1.08 / aspect_fit.min(1.0);
            self.cam3_key = Some(key);
        }
        let resp = ui.allocate_rect(body, Sense::click_and_drag());
        let aspect = body.width() / body.height().max(1.0);
        // orbit / pan / zoom
        if resp.dragged() {
            let d = resp.drag_delta();
            let pan = resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) || ui.input(|i| i.modifiers.shift);
            if pan {
                let per_px = 2.0 * self.cam3.half_h / body.height().max(1.0);
                let view = self.cam3.view(radius).inverse();
                let right = view.transform_vector3(Vec3::X);
                let up = view.transform_vector3(Vec3::Y);
                self.cam3.target += (-right * d.x + up * d.y) * per_px;
            } else {
                self.cam3.yaw -= d.x * 0.008;
                self.cam3.pitch = (self.cam3.pitch + d.y * 0.008).clamp(-1.55, 1.55);
            }
        }
        if resp.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = (-(scroll * 0.0025)).exp() / pinch;
            self.cam3.half_h = (self.cam3.half_h * f).clamp(0.5, 5000.0);
        }
        // picking
        let mut hover = None;
        if let Some(p) = resp.hover_pos() {
            let ndc = [(p.x - body.left()) / body.width() * 2.0 - 1.0, 1.0 - (p.y - body.top()) / body.height() * 2.0];
            let (o, d) = self.cam3.ray(ndc, aspect, radius);
            hover = gpu3d::pick(&mesh, o, d).map(|(_, s)| s);
            ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        if resp.clicked() {
            self.selected = hover.clone();
            if let Some(h) = &hover {
                self.insp_tab = if self.session.annotation(h).is_some() { InspTab::Notes } else { InspTab::Parts };
            }
        }
        self.hover = hover.clone();

        let ppp = ui.ctx().pixels_per_point();
        let size_px = [(body.width() * ppp).round() as u32, (body.height() * ppp).round() as u32];
        let paper = [PAPER.r() as f64 / 255.0, PAPER.g() as f64 / 255.0, PAPER.b() as f64 / 255.0];
        let params = Params {
            rev: self.session.rev,
            mesh: mesh.clone(),
            selected: self.selected.clone(),
            hover,
            view_proj: self.cam3.view_proj(aspect, radius),
            size_px,
            bg: paper,
            line_scale: 1.0,
            grid_y: lo[1],
        };
        ui.painter().add(egui_wgpu_callback(body, params));
        // overlays
        let painter = ui.painter_at(body);
        let label = format!("3D  ORTHO  YAW {:.0}  PITCH {:.0}", self.cam3.yaw.to_degrees().rem_euclid(360.0), self.cam3.pitch.to_degrees());
        painter.text(Pos2::new(body.left() + 10.0, body.bottom() - 8.0), egui::Align2::LEFT_BOTTOM, label, medium(11.0), INK2);
        if let Some(h) = &self.hover {
            painter.text(Pos2::new(body.right() - 10.0, body.bottom() - 8.0), egui::Align2::RIGHT_BOTTOM, h.to_uppercase(), medium(11.0), BLUE);
        }
        let _ = Vec2::ZERO;
    }
}

fn egui_wgpu_callback(rect: Rect, p: Params) -> egui::epaint::PaintCallback {
    eframe::egui_wgpu::Callback::new_paint_callback(rect, Callback { p })
}
