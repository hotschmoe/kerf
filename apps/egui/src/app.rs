//! KerfApp: state, frame loop, layout (header | console | viewport | inspector | status line).

use crate::chat::{Chat, ToolHost, ToolOutput, Thumb, apply_line};
use crate::engine;
use crate::fmt;
use crate::gpu3d::{Cam3d, Gpu3d};
use crate::ir::P2;
use crate::platform::{Incoming, Pick, Platform};
use crate::session::{Session, Who};
use crate::theme::*;
use crate::view2d::{View2dState};
use egui::{Align, Layout, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::{Value, json};
use std::collections::HashMap;
use web_time::{Duration, Instant};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ViewTab {
    View(String),
    ThreeD,
    Sheet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspTab {
    Parts,
    Notes,
    Diff,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Open,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NarrowTab {
    Console,
    Viewport,
    Inspector,
}

pub struct KerfApp {
    pub session: Session,
    pub chat: Chat,
    pub platform: Platform,
    pub tab: ViewTab,
    pub insp_tab: InspTab,
    pub narrow_tab: NarrowTab,
    pub selected: Option<String>,
    pub hover: Option<String>,
    pub v2: View2dState,
    pub cam3: Cam3d,
    pub cam3_key: Option<(u64, String)>,
    pub input: String,
    pub attachments: Vec<Thumb>,
    pub settings_open: bool,
    pub status_msg: Option<(String, Instant, bool)>,
    pub cursor_model: Option<P2>,
    pub zoom_now: f64,
    pub menu: Option<Menu>,
    pub thumb_tex: HashMap<usize, egui::TextureHandle>,
    pub note_text_buf: (String, String),
    pub cite_new: (String, String, String),
    pub frame_ms: f32,
    pub frame_avg: f32,
    pub model_idx: usize,
    pub has_gpu: bool,
    pub first_frame: Option<Instant>,
    pub started: Instant,
    pub ready_flag_set: bool,
    pub insp_cache: Option<(u64, String, Value)>,
    pub expand_ops: std::collections::HashSet<usize>,
    pub pending_open_dialog: bool,
    pub scroll_to_sel: bool,
    pub bench: bool,
    pub saved: crate::settings::Settings,
    pub persist: bool,
    pub param_buf: HashMap<(String, String), String>,
    pub cut3d: bool,
    pub cut_cache: Option<((u64, i32), std::sync::Arc<crate::gpu3d::CutInfo>)>,
}

impl KerfApp {
    pub fn new(ctx: &egui::Context) -> KerfApp {
        install_theme(ctx);
        let mut chat = Chat::new();
        chat.ctx = Some(ctx.clone());
        KerfApp {
            session: Session::new(),
            chat,
            platform: Platform::new(ctx.clone()),
            tab: ViewTab::View("A".into()),
            insp_tab: InspTab::Parts,
            narrow_tab: NarrowTab::Viewport,
            selected: None,
            hover: None,
            v2: View2dState::default(),
            cam3: Cam3d::iso(),
            cam3_key: None,
            input: String::new(),
            attachments: Vec::new(),
            settings_open: false,
            status_msg: None,
            cursor_model: None,
            zoom_now: 1.0,
            menu: None,
            thumb_tex: HashMap::new(),
            note_text_buf: (String::new(), String::new()),
            cite_new: Default::default(),
            frame_ms: 0.0,
            frame_avg: 0.0,
            model_idx: 0,
            has_gpu: false,
            first_frame: None,
            started: Instant::now(),
            ready_flag_set: false,
            insp_cache: None,
            expand_ops: Default::default(),
            pending_open_dialog: false,
            scroll_to_sel: false,
            bench: false,
            saved: Default::default(),
            persist: true,
            param_buf: HashMap::new(),
            cut3d: true,
            cut_cache: None,
        }
    }

    pub fn restore(&mut self) {
        let s = crate::settings::load();
        self.chat.api_key = s.api_key;
        if let Some(i) = crate::chat::MODELS.iter().position(|x| *x == s.model) {
            self.model_idx = i;
            self.chat.model = s.model;
        }
        if s.demo {
            self.enable_demo(true);
        }
        self.saved = self.current_settings();
    }

    fn current_settings(&self) -> crate::settings::Settings {
        crate::settings::Settings { api_key: self.chat.api_key.clone(), model: self.chat.model.clone(), demo: self.chat.demo }
    }

    /// Persist when changed (called every frame; cheap compare).
    fn persist_if_changed(&mut self) {
        if !self.persist {
            return;
        }
        let cur = self.current_settings();
        if cur != self.saved {
            crate::settings::save(&cur);
            self.saved = cur;
        }
    }

    pub fn enable_demo(&mut self, on: bool) {
        self.chat.demo = on;
        if on {
            self.chat.set_mock(crate::demo::script());
        }
    }

    pub fn clock(&self) -> String {
        fmt::clock()
    }

    pub fn flash(&mut self, msg: impl Into<String>, err: bool) {
        self.status_msg = Some((msg.into(), Instant::now(), err));
    }

    // -------------------------------------------------------------- document ops

    pub fn load_doc_text(&mut self, text: &str, name: Option<String>) {
        match serde_json::from_str::<Value>(text) {
            Ok(doc) if doc.get("kerf").is_some() || doc.get("components").is_some() => {
                let ck = self.clock();
                let nm = name.clone().unwrap_or_else(|| doc["id"].as_str().unwrap_or("doc").to_owned());
                self.session.load(doc, name, &ck);
                self.selected = None;
                self.hover = None;
                self.v2.cams.clear();
                self.cam3_key = None;
                let first = self.session.views().first().map(|v| v.0.clone());
                self.tab = ViewTab::View(first.unwrap_or_else(|| "A".into()));
                self.flash(format!("LOADED {}", nm.to_uppercase()), false);
            }
            Ok(_) => self.flash("NOT A KERF DOCUMENT (NO \"kerf\" KEY)", true),
            Err(e) => self.flash(format!("PARSE ERROR: {e}"), true),
        }
    }

    pub fn open_sample(&mut self, idx: usize) {
        let (name, text) = engine::SAMPLES[idx];
        self.load_doc_text(text, Some(format!("{}.kerf.json", name.to_lowercase())));
    }

    pub fn current_view_id(&self) -> Option<String> {
        match &self.tab {
            ViewTab::View(v) => Some(v.clone()),
            _ => self.session.views().first().map(|v| v.0.clone()),
        }
    }

    /// Designer edit -> op through the engine, logged with who:DESIGNER.
    pub fn designer_op(&mut self, ops: Value, why: &str) {
        let ck = self.clock();
        match self.session.apply(ops, why, Who::Designer, &ck) {
            Ok(out) if out.ok => {}
            Ok(out) => {
                let msg = out.diagnostics.first().and_then(|d| d["message"].as_str()).unwrap_or("rejected").to_owned();
                self.flash(format!("EDIT REJECTED: {msg}"), true);
            }
            Err(e) => self.flash(format!("ENGINE ERROR: {e}"), true),
        }
    }

    pub fn export(&mut self, format: &str) {
        let Some(doc) = self.session.doc.clone() else {
            self.flash("NOTHING TO EXPORT: NO DETAIL LOADED", true);
            return;
        };
        let Some(view) = self.current_view_id() else {
            self.flash("NO VIEW TO EXPORT", true);
            return;
        };
        let sheet = format != "dxf";
        match engine::export(&doc, &self.session.style, &view, format, sheet) {
            Ok(bytes) => {
                let id = doc["id"].as_str().unwrap_or("detail").to_uppercase();
                let name = format!("{id}-{view}.{format}").to_lowercase();
                let mime = match format {
                    "svg" => "image/svg+xml",
                    "pdf" => "application/pdf",
                    _ => "application/dxf",
                };
                self.platform.save(&name, bytes, mime);
            }
            Err(e) => self.flash(format!("EXPORT FAILED: {e}"), true),
        }
    }

    pub fn save_doc(&mut self) {
        let Some(doc) = &self.session.doc else {
            self.flash("NOTHING TO SAVE", true);
            return;
        };
        // canonical text from the engine's `fmt` (key order, number format, trailing newline)
        let text = engine::call("fmt", json!({"doc": doc})).ok().and_then(|v| v["text"].as_str().map(str::to_owned)).unwrap_or_else(|| {
            let mut t = serde_json::to_string_pretty(doc).unwrap_or_default();
            t.push('\n');
            t
        });
        let name = self.session.file_name.clone().unwrap_or_else(|| format!("{}.kerf.json", doc["id"].as_str().unwrap_or("detail")));
        self.platform.save(&name, text.into_bytes(), "application/json");
    }

    // -------------------------------------------------------------- per-frame

    fn logic(&mut self, ctx: &egui::Context) {
        // inbox from platform (file dialogs, paste)
        self.platform.poll_paste();
        while let Ok(m) = self.platform.rx.try_recv() {
            match m {
                Incoming::OpenDoc { name, bytes } => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    self.load_doc_text(&text, Some(name));
                }
                Incoming::Attach { name, bytes } => self.attach_image(ctx, &name, &bytes),
                Incoming::Dropped { name, bytes } => self.route_dropped(ctx, name, bytes),
                Incoming::Saved(s) => self.flash(s, false),
                Incoming::Error(s) => self.flash(s, true),
            }
        }
        // drag-drop (web: bytes arrive with the event; native: read the path)
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            let name = if f.name.is_empty() { f.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "dropped".into()) } else { f.name.clone() };
            let bytes: Option<Vec<u8>> = f.bytes.as_ref().map(|b| b.to_vec()).or_else(|| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    f.path.as_ref().and_then(|p| std::fs::read(p).ok())
                }
                #[cfg(target_arch = "wasm32")]
                {
                    None
                }
            });
            match bytes {
                Some(bytes) => self.route_dropped(ctx, name, bytes),
                None => self.flash(format!("DROP FAILED: {name}"), true),
            }
        }
        // chat tick
        {
            let KerfApp { session, chat, .. } = self;
            let mut host = Host { session, ctx: ctx.clone() };
            let before = chat.entries.len();
            chat.poll(&mut host);
            if chat.entries.len() != before {
                ctx.request_repaint();
            }
        }
        if let Some((_, t, _)) = &self.status_msg {
            if t.elapsed() > Duration::from_secs(8) {
                self.status_msg = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        // keyboard
        ctx.input(|i| {
            let _ = i;
        });
        let (k_a, k_b, k_f) = ctx.input(|i| (i.key_pressed(egui::Key::Num1) && i.modifiers.alt, i.key_pressed(egui::Key::Num2) && i.modifiers.alt, i.key_pressed(egui::Key::F) && i.modifiers.alt));
        let views = self.session.views();
        if k_a {
            if let Some(v) = views.first() {
                self.tab = ViewTab::View(v.0.clone());
            }
        }
        if k_b {
            if let Some(v) = views.get(1) {
                self.tab = ViewTab::View(v.0.clone());
            }
        }
        if k_f {
            self.v2.want_fit = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && ctx.memory(|m| m.focused().is_none()) {
            self.selected = None;
        }
    }

    fn route_dropped(&mut self, ctx: &egui::Context, name: String, bytes: Vec<u8>) {
        if name.to_lowercase().ends_with(".json") {
            self.load_doc_text(&String::from_utf8_lossy(&bytes), Some(name));
        } else {
            self.attach_image(ctx, &name, &bytes);
        }
    }

    pub fn attach_image(&mut self, ctx: &egui::Context, name: &str, bytes: &[u8]) {
        match crate::imaging::prepare_attachment(name, bytes) {
            Ok(t) => {
                self.attachments.push(t);
                ctx.request_repaint();
            }
            Err(e) => self.flash(format!("ATTACH FAILED: {e}"), true),
        }
    }

    pub fn thumb_texture(&mut self, ctx: &egui::Context, t: &Thumb) -> Option<egui::TextureHandle> {
        let key = std::sync::Arc::as_ptr(&t.png_or_jpg) as usize;
        if let Some(h) = self.thumb_tex.get(&key) {
            return Some(h.clone());
        }
        let img = image::load_from_memory(&t.png_or_jpg).ok()?;
        let rgba = img.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
        let h = ctx.load_texture(format!("thumb-{key}"), ci, egui::TextureOptions::LINEAR);
        self.thumb_tex.insert(key, h.clone());
        Some(h)
    }

    pub fn send_chat(&mut self) {
        let text = self.input.trim().to_owned();
        if text.is_empty() && self.attachments.is_empty() || self.chat.busy() {
            return;
        }
        if !self.chat.has_key() {
            self.flash("NO API KEY. ENTER KEY TO ENABLE CLAUDE.", true);
            self.settings_open = true;
            return;
        }
        let imgs = std::mem::take(&mut self.attachments);
        self.input.clear();
        let KerfApp { session, chat, .. } = self;
        let ctx = chat.ctx.clone().unwrap_or_default();
        let mut host = Host { session, ctx };
        chat.send(&mut host, text, imgs);
    }

    // -------------------------------------------------------------- layout

    fn header(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, PAPER);
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 2.0), Vec2::new(rect.width(), 2.0)), 0.0, INK);
        ui.allocate_ui_with_layout(rect.size(), Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(12.0);
            // wordmark: KERF + three saw-kerf bars
            let g = galley(ui, "KERF", bold(22.0), INK, 4.4);
            let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 40.0), Sense::hover());
            ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0 - 1.0), g, INK);
            ui.add_space(6.0);
            for _ in 0..3 {
                let (r, _) = ui.allocate_exact_size(Vec2::new(6.0, 14.0), Sense::hover());
                ui.painter().rect_filled(Rect::from_center_size(r.center(), Vec2::new(6.0, 14.0)), 0.0, INK);
                ui.add_space(-2.0);
            }
            ui.add_space(14.0);
            let compact = ui.available_width() < 900.0;
            if !compact {
                header_field(ui, "DETAIL WORKSTATION", None);
                ui.add_space(18.0);
            }
            let doc = self.session.title();
            if !doc.is_empty() {
                header_field(ui, "DOC:", Some(&doc));
                ui.add_space(14.0);
                let rev = self.session.log.iter().filter(|e| e.who != Who::Load).count();
                header_field(ui, "REV", Some(&format!("{rev}")));
                ui.add_space(14.0);
                if !compact {
                    let style_id = self.session.style["id"].as_str().unwrap_or("?").to_uppercase();
                    header_field(ui, "STYLE:", Some(&style_id));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                if button(ui, "SAVE").clicked() {
                    self.save_doc();
                }
                ui.add_space(-2.0);
                let open = button(ui, "OPEN");
                if open.clicked() {
                    self.menu = if self.menu.is_some() { None } else { Some(Menu::Open) };
                }
                self.open_menu(ui.ctx(), open.rect);
            });
        });
    }

    fn open_menu(&mut self, ctx: &egui::Context, anchor: Rect) {
        if self.menu != Some(Menu::Open) {
            return;
        }
        let pos = Pos2::new((anchor.right() - 300.0).max(4.0), anchor.bottom() + 4.0);
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("open-menu")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            shadowed_frame().show(ui, |ui| {
                ui.set_width(280.0);
                label_caps(ui, "Open sample detail");
                ui.add_space(2.0);
                for (i, (name, _)) in engine::SAMPLES.iter().enumerate() {
                    if menu_row(ui, name) {
                        self.open_sample(i);
                        close = true;
                    }
                }
                ui.add_space(4.0);
                hrule(ui, 1.0);
                ui.add_space(4.0);
                if menu_row(ui, "OPEN .KERF.JSON FILE...") {
                    self.platform.pick(Pick::Doc);
                    close = true;
                }
            });
        });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed()) && !area.response.rect.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())) && !anchor.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default()));
        if close || clicked_outside {
            self.menu = None;
        }
    }

    fn status_line(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let p = ui.painter();
        p.rect_filled(rect, 0.0, TERM_BG);
        let mut segs: Vec<(String, egui::Color32)> = Vec::new();
        if let Some((m, _, err)) = &self.status_msg {
            segs.push((m.to_uppercase(), if *err { egui::Color32::from_rgb(0xFF, 0x6B, 0x6B) } else { TERM_FG }));
        } else if self.session.doc.is_some() {
            segs.push(("READY".into(), TERM_FG));
        } else {
            segs.push(("NO DETAIL LOADED".into(), TERM_FG));
        }
        let (e, w, _) = self.session.counts();
        if self.session.doc.is_some() {
            segs.push((format!("{} COMPONENTS", self.session.components().len()), TERM_FG));
            segs.push((format!("{e} ERR {w} WARN"), if e > 0 { egui::Color32::from_rgb(0xFF, 0x6B, 0x6B) } else if w > 0 { egui::Color32::from_rgb(0xF2, 0xC2, 0x4B) } else { TERM_FG }));
            let vtxt = match &self.tab {
                ViewTab::View(v) => {
                    let scale = self.session.view_doc(v).and_then(|d| d["scale"].as_str()).unwrap_or("");
                    format!("VIEW {v} {scale}")
                }
                ViewTab::ThreeD => "VIEW 3D ORTHO".into(),
                ViewTab::Sheet => "SHEET LETTER LANDSCAPE".into(),
            };
            segs.push((vtxt, TERM_FG));
            if let (Some(c), ViewTab::View(_)) = (self.cursor_model, &self.tab) {
                segs.push((format!("X {}  Y {}", fmt::ft_in(c[0]), fmt::ft_in(c[1])), TERM_FG));
            }
        }
        let (claude, ccol) = if self.chat.busy() {
            let spin = ['|', '/', '-', '\\'][((self.started.elapsed().as_millis() / 150) % 4) as usize];
            match &self.chat.phase {
                crate::chat::Phase::Busy(n) => (format!("CLAUDE BUSY {spin} ROUND {n}"), TERM_FG),
                crate::chat::Phase::Backoff(s) => (format!("CLAUDE RETRY {s} S"), egui::Color32::from_rgb(0xF2, 0xC2, 0x4B)),
                _ => ("CLAUDE OK".into(), TERM_FG),
            }
        } else if self.chat.demo {
            ("CLAUDE DEMO".into(), TERM_FG)
        } else if self.chat.api_key.trim().is_empty() {
            ("NO KEY".into(), egui::Color32::from_rgb(0xF2, 0xC2, 0x4B))
        } else {
            ("CLAUDE OK".into(), TERM_FG)
        };
        segs.push((claude, ccol));
        let mut x = rect.left() + 12.0;
        let cy = rect.center().y;
        let font = regular(12.0);
        for (i, (s, col)) in segs.iter().enumerate() {
            if i > 0 {
                p.rect_filled(Rect::from_center_size(Pos2::new(x + 6.0, cy), Vec2::new(5.0, 11.0)), 0.0, TERM_FG);
                x += 18.0;
            }
            let r = p.text(Pos2::new(x, cy), egui::Align2::LEFT_CENTER, s, font.clone(), *col);
            x = r.right();
        }
        // blinking block cursor, like a 3270 input field
        if (self.started.elapsed().as_millis() / 530) % 2 == 0 {
            p.rect_filled(Rect::from_min_size(Pos2::new(x + 10.0, cy - 6.0), Vec2::new(7.0, 12.0)), 0.0, TERM_FG);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(530));
        // right side: engine + frame time
        let right = format!("{}  {:.1}MS", engine::engine_name().to_uppercase(), self.frame_avg);
        let rw = p.layout_no_wrap(right.clone(), regular(11.0), TERM_FG).size().x;
        if x + 24.0 + rw < rect.right() - 12.0 {
            p.text(Pos2::new(rect.right() - 12.0, cy), egui::Align2::RIGHT_CENTER, right, regular(11.0), TERM_FG.gamma_multiply(0.55));
        }
    }

    fn viewport_panel(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, VELLUM);
        // tab strip
        let strip_h = 32.0;
        let strip = Rect::from_min_size(full.min, Vec2::new(full.width(), strip_h));
        ui.painter().rect_filled(strip, 0.0, PAPER);
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(strip.left(), strip.bottom() - 1.0), Vec2::new(strip.width(), 1.0)), 0.0, INK);
        let views = self.session.views();
        // the document may have been replaced (Claude `set doc`, open): keep the tab valid
        if let ViewTab::View(id) = &self.tab {
            if !views.is_empty() && !views.iter().any(|(v, _)| v == id) {
                self.tab = ViewTab::View(views[0].0.clone());
            }
        }
        let mut strip_ui = ui.new_child(egui::UiBuilder::new().max_rect(strip.shrink2(Vec2::new(8.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
        let compact = full.width() < 780.0;
        for (id, kind) in &views {
            let label = if compact { id.clone() } else { format!("{} {}", kind, id) };
            let active = self.tab == ViewTab::View(id.clone());
            if tab(&mut strip_ui, &label, active).clicked() {
                self.tab = ViewTab::View(id.clone());
            }
        }
        if tab(&mut strip_ui, "3D", self.tab == ViewTab::ThreeD).clicked() {
            self.tab = ViewTab::ThreeD;
        }
        if tab(&mut strip_ui, "SHEET", self.tab == ViewTab::Sheet).clicked() {
            self.tab = ViewTab::Sheet;
        }
        strip_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if matches!(self.tab, ViewTab::ThreeD) {
                for (label, yaw, pitch) in [("RIGHT", 90.0f32, 0.0f32), ("TOP", 0.0, 89.0), ("ISO", 45.0, 35.264), ("FRONT", 0.0, 0.0)] {
                    if small_button(ui, &format!("[{label}]"), false).clicked() {
                        self.cam3.yaw = yaw.to_radians();
                        self.cam3.pitch = pitch.to_radians();
                    }
                }
                ui.add_space(4.0);
                if small_button(ui, "FIT", false).clicked() {
                    self.cam3_key = None;
                }
                if self.section_cut_z().is_some() && small_button(ui, "CUT", self.cut3d).clicked() {
                    self.cut3d = !self.cut3d;
                }
            } else {
                if small_button(ui, "FIT", false).clicked() {
                    self.v2.want_fit = true;
                }
                if small_button(ui, "+", false).clicked() {
                    self.zoom_view(1.25);
                }
                if small_button(ui, "\u{2212}", false).clicked() {
                    self.zoom_view(0.8);
                }
            }
        });
        let body = Rect::from_min_max(Pos2::new(full.left(), strip.bottom()), full.max);
        if self.session.doc.is_none() {
            self.empty_viewport(ui, body);
            return;
        }
        match self.tab.clone() {
            ViewTab::View(id) => self.view2d_ui(ui, body, &id, false),
            ViewTab::Sheet => {
                let id = self.current_view_id().unwrap_or_else(|| "A".into());
                self.view2d_ui(ui, body, &id, true)
            }
            ViewTab::ThreeD => self.view3d_ui(ui, body),
        }
    }

    fn zoom_view(&mut self, f: f64) {
        let key = match &self.tab {
            ViewTab::View(v) => v.clone(),
            ViewTab::Sheet => format!("sheet:{}", self.current_view_id().unwrap_or_default()),
            ViewTab::ThreeD => {
                self.cam3.half_h = (self.cam3.half_h / f as f32).clamp(0.5, 5000.0);
                return;
            }
        };
        if let Some(c) = self.v2.cams.get_mut(&key) {
            c.zoom = (c.zoom * f).clamp(0.05, 400.0);
        }
    }

    fn empty_viewport(&mut self, ui: &mut Ui, body: Rect) {
        let painter = ui.painter_at(body);
        painter.rect_filled(body, 0.0, VELLUM);
        let cam = crate::view2d::Cam2d { center: [0.0, 0.0], zoom: 24.0 };
        crate::view2d::paint_grid_public(&painter, body, &cam);
        let w = 470.0_f32.min(body.width() - 32.0);
        let rect = Rect::from_center_size(body.center(), Vec2::new(w, 200.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)));
        let mut pick = None;
        shadowed_frame().inner_margin(16).show(&mut child, |ui| {
            ui.set_width(w - 34.0);
            ui.vertical_centered(|ui| {
                let mut job = spaced("NO DETAIL LOADED. DESCRIBE ONE IN THE CONSOLE, OR OPEN A .KERF.JSON.", regular(13.0), INK, 0.3);
                job.wrap.max_width = w - 40.0;
                job.halign = egui::Align::Center;
                let g = ui.painter().layout_job(job);
                let (r, _) = ui.allocate_exact_size(Vec2::new(w - 40.0, g.size().y), Sense::hover());
                ui.painter().galley(Pos2::new(r.center().x, r.top()), g, INK);
                ui.add_space(12.0);
                label_caps(ui, "Open a sample");
                ui.add_space(4.0);
                for (i, (name, _)) in engine::SAMPLES.iter().enumerate() {
                    if button(ui, name).clicked() {
                        pick = Some(i);
                    }
                    ui.add_space(2.0);
                }
            });
        });
        if let Some(i) = pick {
            self.open_sample(i);
        }
    }

    fn view2d_ui(&mut self, ui: &mut Ui, body: Rect, view: &str, sheet: bool) {
        let key = if sheet { format!("sheet:{view}") } else { view.to_owned() };
        let prep = match self.session.drawing_ex(view, sheet) {
            Ok(p) => p,
            Err(e) => {
                let painter = ui.painter_at(body);
                painter.rect_filled(body, 0.0, VELLUM);
                painter.text(body.center(), egui::Align2::CENTER_CENTER, format!("DRAWING FAILED: {e}"), regular(13.0), RED);
                return;
            }
        };
        let session = &self.session;
        let notes = |id: &str| session.annotation(id).is_some_and(|(_, a)| a["type"] == "note");
        let place_of = |id: &str| -> Option<P2> {
            let (_, a) = session.annotation(id)?;
            let p = a["place"].as_array()?;
            Some([p.first()?.as_f64()?, p.get(1)?.as_f64()?])
        };
        let inp = crate::view2d::Inputs {
            view_key: &key,
            prep: &prep,
            selected: self.selected.as_deref(),
            hover_prev: self.hover.as_deref(),
            notes: &notes,
            place_of: &place_of,
            show_grid: !sheet,
        };
        let out = crate::view2d::show(ui, &mut self.v2, &inp, body);
        self.hover = out.hover.clone();
        self.cursor_model = if prep.kind == "iso" { None } else { out.cursor_model };
        self.zoom_now = out.zoom;
        if let Some(c) = out.clicked {
            if let Some(id) = &c {
                self.insp_tab = if self.session.annotation(id).is_some() { InspTab::Notes } else { InspTab::Parts };
            }
            if c.is_some() && c != self.selected {
                self.scroll_to_sel = true;
            }
            self.selected = c;
        }
        if let Some((id, place)) = out.note_moved {
            let r = |v: f64| (v * 1000.0).round() / 1000.0;
            if let Some((vid, _)) = self.session.annotation(&id) {
                self.designer_op(
                    json!([{"op": "update", "path": format!("views/{vid}/annotations/{id}"), "value": {"place": [r(place[0]), r(place[1])]}}]),
                    &format!("Move note {id} to [{}, {}]", r(place[0]), r(place[1])),
                );
            }
        }
        // scale/diagnostic overlay
        let painter = ui.painter_at(body);
        let txt = format!("ZOOM {:.0}%", self.zoom_now / crate::view2d::Cam2d::fit(prep.bounds, body.size()).zoom * 100.0);
        painter.text(Pos2::new(body.right() - 10.0, body.bottom() - 8.0), egui::Align2::RIGHT_BOTTOM, txt, medium(11.0), INK2);
    }
}

fn header_field(ui: &mut Ui, label: &str, value: Option<&str>) {
    let g = galley(ui, label, medium(11.0), INK2, 0.9);
    let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 40.0), Sense::hover());
    ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, INK2);
    if let Some(v) = value {
        ui.add_space(4.0);
        let g = galley(ui, v, bold(13.0), INK, 0.4);
        let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 40.0), Sense::hover());
        ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, INK);
    }
}

fn menu_row(ui: &mut Ui, label: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let hov = resp.hovered();
    if hov {
        ui.painter().rect_filled(r, 0.0, INK);
    }
    let col = if hov { PAPER } else { INK };
    let g = galley(ui, label, medium(12.0), col, 0.4);
    ui.painter().galley(Pos2::new(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, col);
    resp.clicked()
}

fn install_theme(ctx: &egui::Context) {
    crate::theme::install(ctx);
}

// ------------------------------------------------------------------ eframe glue

impl eframe::App for KerfApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl KerfApp {
    pub fn open_by_name(&mut self, name: &str) {
        let low = name.to_lowercase();
        if let Some(i) = engine::SAMPLES.iter().position(|(n, _)| n.to_lowercase().contains(&low) || low.contains(&n.to_lowercase())) {
            self.open_sample(i);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        match std::fs::read_to_string(name) {
            Ok(t) => self.load_doc_text(&t, Some(name.rsplit('/').next().unwrap_or(name).to_owned())),
            Err(e) => self.flash(format!("CANNOT READ {name}: {e}"), true),
        }
    }

    /// Web: `?doc=truss-bearing-cmu&demo=1&tab=3d` for deep links and screenshots.
    #[cfg(target_arch = "wasm32")]
    pub fn web_boot(&mut self) {
        let Some(win) = web_sys::window() else { return };
        let search = win.location().search().unwrap_or_default();
        let get = |k: &str| -> Option<String> {
            search.trim_start_matches('?').split('&').find_map(|kv| {
                let (a, b) = kv.split_once('=').unwrap_or((kv, ""));
                (a == k).then(|| b.to_owned())
            })
        };
        self.bench = get("bench").as_deref() == Some("1");
        if get("demo").as_deref() == Some("1") {
            self.enable_demo(true);
        }
        if let Some(d) = get("doc") {
            self.open_by_name(&d);
        }
        self.apply_view_args(get("tab").as_deref(), get("select").as_deref(), get("insp").as_deref());
    }

    pub fn apply_view_args(&mut self, tab: Option<&str>, select: Option<&str>, insp: Option<&str>) {
        if let Some(t) = tab {
            self.tab = match t.to_lowercase().as_str() {
                "3d" => ViewTab::ThreeD,
                "sheet" => ViewTab::Sheet,
                v => ViewTab::View(v.to_uppercase()),
            };
        }
        if let Some(s) = select {
            self.selected = Some(s.to_owned());
        }
        if let Some(i) = insp {
            self.insp_tab = match i.to_lowercase().as_str() {
                "notes" => InspTab::Notes,
                "diff" => InspTab::Diff,
                _ => InspTab::Parts,
            };
        }
    }

    /// One pass of the non-UI per-frame logic (inbox, chat tick) for the headless runner.
    /// Observable state for e2e scripts (`window.__kerf_state` on the web).
    pub fn state_json(&self) -> String {
        json!({
            "doc": self.session.doc.as_ref().and_then(|d| d["id"].as_str()),
            "components": self.session.components().len(),
            "rev": self.session.rev,
            "tab": match &self.tab { ViewTab::View(v) => format!("view:{v}"), ViewTab::ThreeD => "3d".into(), ViewTab::Sheet => "sheet".into() },
            "insp": format!("{:?}", self.insp_tab),
            "selected": self.selected,
            "hover": self.hover,
            "chat_entries": self.chat.entries.len(),
            "chat_busy": self.chat.busy(),
            "history": self.chat.history.len(),
            "log": self.session.log.iter().map(|e| format!("{}:{}", e.who.label(), e.why)).collect::<Vec<_>>(),
            "input_len": self.input.len(),
            "attachments": self.attachments.len(),
            "status": self.status_msg.as_ref().map(|s| s.0.clone()),
        })
        .to_string()
    }

    pub fn headless_tick(&mut self, ctx: &egui::Context) {
        self.logic(ctx);
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let t0 = Instant::now();
        let ctx = ui.ctx().clone();
        if self.first_frame.is_none() {
            self.first_frame = Some(Instant::now());
        }
        self.logic(&ctx);
        let narrow = ui.max_rect().width() < 900.0;
        let frame = egui::Frame::NONE.fill(PAPER);
        egui::Panel::top("header").exact_size(40.0).frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("status").exact_size(24.0).frame(egui::Frame::NONE).resizable(false).show_separator_line(false).show(ui, |ui| self.status_line(ui));
        if narrow {
            egui::Panel::top("narrow-tabs").exact_size(32.0).frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    for (t, l) in [(NarrowTab::Console, "CONSOLE"), (NarrowTab::Viewport, "VIEW"), (NarrowTab::Inspector, "INSPECTOR")] {
                        if tab(ui, l, self.narrow_tab == t).clicked() {
                            self.narrow_tab = t;
                        }
                    }
                });
                hline_at(ui, ui.max_rect(), ui.max_rect().bottom() - 1.0, 1.0, INK);
            });
            egui::CentralPanel::no_frame().show(ui, |ui| match self.narrow_tab {
                NarrowTab::Console => self.console_panel(ui),
                NarrowTab::Viewport => self.viewport_panel(ui),
                NarrowTab::Inspector => self.inspector_panel(ui),
            });
        } else {
            egui::Panel::left("console").exact_size(360.0).frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| {
                self.console_panel(ui);
                let r = ui.max_rect();
                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(r.right() - 1.0, r.top()), Vec2::new(1.0, r.height())), 0.0, INK);
            });
            egui::Panel::right("inspector").exact_size(320.0).frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| {
                self.inspector_panel(ui);
                let r = ui.max_rect();
                ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(1.0, r.height())), 0.0, INK);
            });
            egui::CentralPanel::no_frame().show(ui, |ui| self.viewport_panel(ui));
        }
        // frame stats
        self.frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.frame_avg = if self.frame_avg == 0.0 { self.frame_ms } else { self.frame_avg * 0.9 + self.frame_ms * 0.1 };
        self.persist_if_changed();
        #[cfg(target_arch = "wasm32")]
        crate::state_probe(&self.state_json());
        crate::perf_probe(self.frame_ms, self.frame_avg);
        if self.bench {
            ctx.request_repaint();
        }
        if !self.ready_flag_set && self.first_frame.is_some_and(|t| t.elapsed() > Duration::from_millis(0)) {
            self.ready_flag_set = true;
            crate::set_ready_flag();
        }
    }

}

pub fn install_gpu(rs: &eframe::egui_wgpu::RenderState) {
    rs.renderer.write().callback_resources.insert(Gpu3d::new(&rs.device, rs.target_format));
}

// ------------------------------------------------------------------ tool host

pub struct Host<'a> {
    pub session: &'a mut Session,
    pub ctx: egui::Context,
}

impl ToolHost for Host<'_> {
    fn clock(&self) -> String {
        fmt::clock()
    }

    fn catalog_markdown(&mut self) -> String {
        engine::catalog_markdown()
    }

    fn drain_designer_notes(&mut self) -> Option<String> {
        if self.session.unsent_notes.is_empty() {
            return None;
        }
        let notes = std::mem::take(&mut self.session.unsent_notes);
        Some(format!("[designer edits since your last turn: {}]", notes.join("; ")))
    }

    fn run_tool(&mut self, name: &str, input: &Value) -> ToolOutput {
        match name {
            "kerf_apply" => self.tool_apply(input),
            "kerf_inspect" => self.tool_inspect(input),
            "kerf_render" => self.tool_render(input),
            other => err_out(format!("unknown tool '{other}'. Available: kerf_apply, kerf_inspect, kerf_render."), "UNKNOWN TOOL"),
        }
    }
}

fn err_out(msg: String, line: &str) -> ToolOutput {
    ToolOutput { content: vec![json!({"type": "text", "text": msg})], is_error: true, line: format!("{line}   \u{2717}"), text: msg, image: None }
}

fn diag_text(diags: &[Value]) -> String {
    diags
        .iter()
        .map(|d| {
            let lvl = match d["level"].as_str() {
                Some("error") => "ERROR",
                Some("warning") => "WARN",
                _ => "INFO",
            };
            let mut s = format!("{lvl} {}", d["code"].as_str().unwrap_or(""));
            if let Some(id) = d["id"].as_str() {
                s += &format!(" {id}");
            }
            s += &format!(": {}", d["message"].as_str().unwrap_or(""));
            if let Some(f) = d["fix"].as_str() {
                s += &format!(" FIX: {f}");
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Host<'_> {
    fn tool_apply(&mut self, input: &Value) -> ToolOutput {
        let Some(ops) = input.get("ops").filter(|o| o.is_array()) else {
            return err_out("kerf_apply needs `ops` (array of {op, path, value}) and `why`.".into(), "APPLY");
        };
        let why = input["why"].as_str().unwrap_or("Claude edit").to_owned();
        let n = ops.as_array().map(|a| a.len()).unwrap_or(0);
        let ck = fmt::clock();
        match self.session.apply(ops.clone(), &why, Who::Claude, &ck) {
            Ok(out) => {
                let errs = out.diagnostics.iter().filter(|d| d["level"] == "error").count();
                let warns = out.diagnostics.iter().filter(|d| d["level"] == "warning").count();
                let mut text = format!("ok: {}\n", out.ok);
                if !out.changed.is_empty() {
                    text += &format!("changed: {}\n", out.changed.join(", "));
                }
                if !out.summary.is_empty() {
                    text += &format!("{}\n", out.summary);
                }
                let dt = diag_text(&out.diagnostics);
                if !dt.is_empty() {
                    text += &format!("{dt}\n");
                }
                self.ctx.request_repaint();
                ToolOutput { content: vec![json!({"type": "text", "text": text})], is_error: !out.ok, line: apply_line(n, errs, warns, out.ok), text, image: None }
            }
            Err(e) => err_out(format!("engine error: {e}"), &format!("APPLY  {n} OPS")),
        }
    }

    fn tool_inspect(&mut self, input: &Value) -> ToolOutput {
        let q = input["q"].as_str().unwrap_or("summary");
        let Some(doc) = self.session.doc.clone() else {
            return err_out("No document is loaded yet. Send kerf_apply with {\"op\":\"set\",\"path\":\"doc\",\"value\":{...}} first.".into(), "INSPECT");
        };
        if q == "doc" {
            let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
            return ToolOutput { content: vec![json!({"type": "text", "text": text})], is_error: false, line: "INSPECT  DOC   \u{2713}".into(), text, image: None };
        }
        match engine::inspect(&doc, &self.session.style, input) {
            Ok(v) => {
                let text = match v.get("text").and_then(Value::as_str).or_else(|| v.as_str()) {
                    Some(t) => t.to_owned(),
                    None => serde_json::to_string_pretty(&v).unwrap_or_default(),
                };
                ToolOutput { content: vec![json!({"type": "text", "text": text})], is_error: false, line: format!("INSPECT  {}   \u{2713}", q.to_uppercase()), text, image: None }
            }
            Err(e) => err_out(e, &format!("INSPECT  {}", q.to_uppercase())),
        }
    }

    fn tool_render(&mut self, input: &Value) -> ToolOutput {
        let view = input["view"].as_str().unwrap_or("A").to_owned();
        let sheet = input["mode"].as_str() == Some("sheet");
        let prep = match self.session.drawing_ex(&view, sheet) {
            Ok(p) => p,
            Err(e) => return err_out(format!("render failed: {e}"), &format!("RENDER VIEW {view}")),
        };
        match crate::raster::render(&prep, &crate::raster::RenderOpts::default()) {
            Ok(r) => {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&r.png);
                let (notes, errs) = (
                    self.session.view_doc(&view).and_then(|v| v["annotations"].as_array()).map(|a| a.iter().filter(|x| x["type"] == "note").count()).unwrap_or(0),
                    self.session.counts().0,
                );
                let scale = self.session.view_doc(&view).and_then(|v| v["scale"].as_str()).unwrap_or("NTS").to_owned();
                let text = format!("view {view} rendered at {scale}; {notes} notes; {errs} errors");
                let thumb = Thumb { name: format!("view-{view}.png"), media_type: "image/png".into(), png_or_jpg: std::sync::Arc::new(r.png) };
                ToolOutput {
                    content: vec![json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b64}}), json!({"type": "text", "text": text})],
                    is_error: false,
                    line: format!("RENDER {}{}  \u{2713}", if sheet { "SHEET " } else { "VIEW " }, view),
                    text,
                    image: Some(thumb),
                }
            }
            Err(e) => err_out(format!("render failed: {e}"), &format!("RENDER VIEW {view}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> KerfApp {
        KerfApp::new(&egui::Context::default())
    }

    fn drive(app: &mut KerfApp, ctx: &egui::Context) {
        let t = Instant::now();
        while app.chat.busy() && t.elapsed() < Duration::from_secs(20) {
            app.headless_tick(ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn demo_conversation_drives_the_real_engine() {
        let ctx = egui::Context::default();
        let mut a = KerfApp::new(&ctx);
        a.enable_demo(true);
        a.chat.mock_delay_ms = 0;
        a.input = "truss bearing on CMU".into();
        a.send_chat();
        drive(&mut a, &ctx);
        assert_eq!(a.session.components().len(), 14, "set doc applied");
        // claude's attempt to verify a citation is downgraded by the engine
        let (_, n) = a.session.annotation("n_demo").expect("note added by turn 1");
        assert_eq!(n["cite"][0]["status"], "suggested");
        // tool_result for the two-tool message was ONE user message with an image block
        let two = a.chat.history.iter().find(|m| m["role"] == "user" && m["content"].as_array().is_some_and(|c| c.len() == 2 && c[0]["type"] == "tool_result")).expect("batched results");
        let img = two["content"][1]["content"].as_array().unwrap().iter().any(|b| b["type"] == "image");
        assert!(img, "kerf_render result carries a PNG");
        assert!(a.session.log.iter().filter(|e| e.who == Who::Claude).count() >= 2);
        // turn 2: rejected op is an error tool_result, then the fix lands
        a.input = "remove the bird blocking note".into();
        a.send_chat();
        drive(&mut a, &ctx);
        assert!(!a.session.view_doc("A").unwrap()["annotations"].as_array().unwrap().iter().any(|x| x["id"] == "n_block"));
        let errs = a.chat.history.iter().filter(|m| m["role"] == "user").flat_map(|m| m["content"].as_array().cloned().unwrap_or_default()).filter(|b| b["is_error"] == true).count();
        assert_eq!(errs, 1);
        assert!(a.session.doc.as_ref().unwrap()["components"].as_array().unwrap().len() == 14);
    }

    #[test]
    fn designer_edits_are_ops_undoable_and_reported_to_claude() {
        let mut a = app();
        a.open_sample(0);
        let before = a.session.doc.clone().unwrap();
        a.designer_op(json!([{"op": "update", "path": "views/A/annotations/n_cmu", "value": {"text": "8\" CMU, GROUTED"}}]), "Edit text of n_cmu");
        assert_eq!(a.session.annotation("n_cmu").unwrap().1["text"], "8\" CMU, GROUTED");
        assert_eq!(a.session.log.last().unwrap().who, Who::Designer);
        // designer may verify, and it sticks
        a.designer_op(
            json!([{"op": "update", "path": "views/A/annotations/n_cmu", "value": {"cite": [{"code": "IRC", "edition": 2021, "section": "R606", "title": "General masonry construction", "status": "verified"}]}}]),
            "Verify citation IRC R606 on n_cmu",
        );
        assert_eq!(a.session.annotation("n_cmu").unwrap().1["cite"][0]["status"], "verified");
        // the next LLM turn carries the designer's edits
        let mut host = Host { session: &mut a.session, ctx: egui::Context::default() };
        let note = host.drain_designer_notes().unwrap();
        assert!(note.contains("designer edits since your last turn") && note.contains("Edit text of n_cmu"));
        // undo twice returns to the loaded document
        assert!(a.session.undo().is_some());
        assert!(a.session.undo().is_some());
        assert_eq!(a.session.doc.as_ref().unwrap(), &before);
    }

    #[test]
    fn designer_param_edit_is_an_op_through_the_engine() {
        let mut a = app();
        a.open_sample(0);
        a.designer_op(json!([{"op": "update", "path": "components/sill_plate", "value": {"size": "2x10"}}]), "Set sill_plate.size = 2x10");
        assert_eq!(a.session.component("sill_plate").unwrap()["size"], "2x10");
        assert_eq!(a.session.log.last().unwrap().who, Who::Designer);
        // a bad value is rejected by the engine and leaves the document untouched
        let before = a.session.doc.clone();
        a.designer_op(json!([{"op": "update", "path": "components/sill_plate", "value": {"size": "2x99"}}]), "Set sill_plate.size = 2x99");
        assert_eq!(a.session.doc, before);
        assert!(a.status_msg.as_ref().is_some_and(|m| m.2), "error shown in the status line");
    }

    #[test]
    fn note_drag_sets_place() {
        let mut a = app();
        a.open_sample(0);
        a.designer_op(json!([{"op": "update", "path": "views/A/annotations/n_roof", "value": {"place": [30.5, 12.25]}}]), "Move note n_roof to [30.5, 12.25]");
        assert_eq!(a.session.annotation("n_roof").unwrap().1["place"], json!([30.5, 12.25]));
        let p = a.session.drawing_ex("A", false).unwrap();
        // the note text now sits at the designer's position (anchor within a text height)
        let anchor = p.text_anchor["n_roof"];
        assert!((anchor[0] - 30.5).abs() < 1e-3 && (anchor[1] - 12.25).abs() < 1e-3, "text anchor {:?}", anchor);
    }

    #[test]
    fn exports_pass_through_the_engine() {
        let mut a = app();
        a.open_sample(0);
        let doc = a.session.doc.clone().unwrap();
        let svg = engine::export(&doc, &a.session.style, "A", "svg", true).unwrap();
        assert!(String::from_utf8_lossy(&svg).starts_with("<svg"));
    }
}
