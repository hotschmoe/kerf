//! Operator console (left column): settings strip, message history (designer blocks, manila
//! cards for Claude with collapsible tool activity), the typed input line.

use crate::app::KerfApp;
use crate::chat::{Block, Entry, MODELS, Phase, ToolActivity};
use crate::platform::Pick;
use crate::theme::*;
use egui::{Align, Color32, KeyboardShortcut, Layout, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

impl KerfApp {
    pub fn console_panel(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, PAPER);
        let inner = rect.shrink2(Vec2::new(12.0, 10.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
        let ui = &mut ui;
        ui.set_clip_rect(rect);

        // title row
        ui.horizontal(|ui| {
            heading(ui, "Operator Console");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_button(ui, "KEY", self.settings_open).clicked() {
                    self.settings_open = !self.settings_open;
                }
            });
        });
        ui.add_space(4.0);
        if self.settings_open || (!self.chat.has_key() && self.chat.entries.is_empty() && false) {
            self.settings_block(ui);
        }
        hrule(ui, 1.0);
        ui.add_space(8.0);

        // input area pinned to the bottom
        let input_h = self.input_height();
        let avail = ui.available_rect_before_wrap();
        let hist_rect = Rect::from_min_max(avail.min, Pos2::new(avail.right(), avail.bottom() - input_h - 8.0));
        let input_rect = Rect::from_min_max(Pos2::new(avail.left(), avail.bottom() - input_h), avail.max);

        let mut hist_ui = ui.new_child(egui::UiBuilder::new().max_rect(hist_rect).layout(Layout::top_down(Align::Min)));
        self.history_view(&mut hist_ui);
        let mut in_ui = ui.new_child(egui::UiBuilder::new().max_rect(input_rect).layout(Layout::top_down(Align::Min)));
        self.input_view(&mut in_ui);
    }

    fn input_height(&self) -> f32 {
        let thumbs = if self.attachments.is_empty() { 0.0 } else { 58.0 };
        let hint = if self.chat.has_key() { 0.0 } else { 18.0 };
        150.0 + thumbs + hint
    }

    fn settings_block(&mut self, ui: &mut Ui) {
        shadowed_frame().inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            label_caps(ui, "Anthropic API key");
            let w = field_cell(ui, &mut self.chat.api_key, "sk-ant-...", true, None);
            if w.changed() {
                self.chat.api_key = self.chat.api_key.trim().to_owned();
            }
            ui.add_space(2.0);
            wrapped(ui, "STORED LOCALLY. SENT ONLY TO API.ANTHROPIC.COM.", regular(11.0), INK2);
            ui.add_space(6.0);
            label_caps(ui, "Model");
            ui.horizontal(|ui| {
                for (i, m) in MODELS.iter().enumerate() {
                    let label = {
                        let t = m.trim_start_matches("claude-");
                        match t.rsplit_once('-') {
                            Some((a, b)) => format!("{}.{}", a.replace('-', " "), b),
                            None => t.to_owned(),
                        }
                    };
                    if small_button(ui, &label, self.model_idx == i).clicked() {
                        self.model_idx = i;
                        self.chat.model = (*m).to_owned();
                    }
                }
            });
            ui.add_space(6.0);
            let mut demo = self.chat.demo;
            check_box(ui, &mut demo, "Demo mode (scripted replay, no API)");
            if demo != self.chat.demo {
                self.enable_demo(demo);
            }
        });
        ui.add_space(8.0);
    }

    fn history_view(&mut self, ui: &mut Ui) {
        ui.set_clip_rect(ui.max_rect());
        // make sure every thumbnail has a texture before we borrow entries mutably
        let ctx = ui.ctx().clone();
        let mut need = Vec::new();
        for e in &self.chat.entries {
            match e {
                Entry::Designer { images, .. } => need.extend(images.iter().cloned()),
                Entry::Claude { blocks, .. } => {
                    for b in blocks {
                        if let Block::Tool(t) = b {
                            if let Some(i) = &t.image {
                                need.push(i.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for t in &need {
            self.thumb_texture(&ctx, t);
        }
        let busy = self.chat.busy();
        let phase = self.chat.phase.clone();
        let started = self.started;
        let no_key = !self.chat.has_key();
        let KerfApp { chat, thumb_tex, .. } = self;
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if chat.entries.is_empty() {
                welcome(ui, no_key);
            }
            for e in chat.entries.iter_mut() {
                match e {
                    Entry::Designer { t, text, images } => {
                        designer_card(ui, t, text, images, thumb_tex);
                    }
                    Entry::Claude { t, blocks } => claude_card(ui, t, blocks, thumb_tex),
                    Entry::Error(m) => {
                        line_msg(ui, m, RED, "E");
                    }
                    Entry::Info(m) => {
                        line_msg(ui, m, INK2, "I");
                    }
                }
                ui.add_space(8.0);
            }
            if busy {
                let spin = ['|', '/', '-', '\\'][((started.elapsed().as_millis() / 150) % 4) as usize];
                let txt = match phase {
                    Phase::Backoff(s) => format!("RETRY IN {s} S {spin}"),
                    Phase::Busy(n) => format!("KERF/CLAUDE WORKING {spin}  ROUND {n}"),
                    Phase::Idle => String::new(),
                };
                text(ui, txt, INK2);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
            }
        });
    }

    fn input_view(&mut self, ui: &mut Ui) {
        ui.set_clip_rect(ui.max_rect());
        if !self.chat.has_key() {
            text(ui, "NO API KEY \u{2014} ENTER KEY TO ENABLE CLAUDE.", RED);
            ui.add_space(2.0);
        }
        if !self.attachments.is_empty() {
            let ctx = ui.ctx().clone();
            let atts = self.attachments.clone();
            let mut remove = None;
            ui.horizontal(|ui| {
                for (i, a) in atts.iter().enumerate() {
                    if let Some(tex) = self.thumb_texture(&ctx, a) {
                        let sz = tex.size_vec2();
                        let k = (48.0 / sz.y).min(72.0 / sz.x);
                        let (r, resp) = ui.allocate_exact_size(sz * k, Sense::click());
                        ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, INK), StrokeKind::Outside);
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 0.0, INK.gamma_multiply(0.6));
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "REMOVE", medium(10.0), PAPER);
                        }
                        if resp.clicked() {
                            remove = Some(i);
                        }
                    }
                }
            });
            if let Some(i) = remove {
                self.attachments.remove(i);
            }
            ui.add_space(4.0);
        }
        // the typed line: ">" prompt + bordered cell
        let frame = egui::Frame::new().fill(VELLUM).stroke(Stroke::new(1.0, INK)).inner_margin(egui::Margin::symmetric(8, 6));
        let mut send = false;
        let busy = self.chat.busy();
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let te = egui::TextEdit::multiline(&mut self.input)
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .desired_rows(4)
                    .font(regular(13.0))
                    .hint_text(egui::RichText::new("> DESCRIBE THE DETAIL. SHIFT+ENTER = NEW LINE.").color(INK2.gamma_multiply(0.7)))
                    .return_key(KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::Enter))
                    .text_color(INK);
                let r = ui.add(te);
                if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                    send = true;
                }
                r
            })
            .inner;
        let fr = resp.rect.expand2(Vec2::new(8.0, 6.0));
        if resp.has_focus() {
            ui.painter().rect_stroke(fr, 0.0, Stroke::new(2.0, BLUE), StrokeKind::Inside);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if button(ui, "ATTACH").clicked() {
                self.platform.pick(Pick::Image);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can = !busy;
                if button_k(ui, if busy { "WORKING" } else { "SEND" }, Kind::Primary, can).clicked() && can {
                    send = true;
                }
                if !self.attachments.is_empty() || !self.input.is_empty() {
                    if button(ui, "CLEAR").clicked() {
                        self.input.clear();
                        self.attachments.clear();
                    }
                }
            });
        });
        if send {
            self.send_chat();
        }
    }
}

fn welcome(ui: &mut Ui, no_key: bool) {
    let frame = egui::Frame::new().stroke(Stroke::new(1.0, INK)).inner_margin(8);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        label_caps(ui, "Transmittal");
        ui.add_space(2.0);
        wrapped(ui, "Describe the construction detail you need, or attach a screenshot of an existing one to recreate. Claude builds it with engine tools; you review, edit notes and citations, then export.", regular(12.0), INK);
        if no_key {
            ui.add_space(4.0);
            wrapped(ui, "NO API KEY. PRESS [KEY] TO ENTER ONE, OR TURN ON DEMO MODE.", medium(11.0), RED);
        }
    });
    ui.add_space(8.0);
}

fn line_msg(ui: &mut Ui, msg: &str, color: Color32, tag: &str) {
    ui.horizontal_top(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        ui.painter().rect_filled(r, 0.0, color);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tag, bold(11.0), PAPER);
        wrapped(ui, msg, regular(12.0), color);
    });
}

fn card_header(ui: &mut Ui, who: &str, t: &str) {
    ui.horizontal(|ui| {
        label_caps(ui, who);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            label_caps(ui, t);
        });
    });
    ui.add_space(2.0);
}

fn designer_card(ui: &mut Ui, t: &str, body: &str, images: &[crate::chat::Thumb], tex: &std::collections::HashMap<usize, egui::TextureHandle>) {
    let frame = egui::Frame::new().fill(PAPER).stroke(Stroke::new(1.0, INK)).inner_margin(8);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        card_header(ui, "DESIGNER", t);
        if !body.is_empty() {
            wrapped(ui, body, regular(13.0), INK);
        }
        for im in images {
            thumb(ui, im, tex, 120.0);
        }
    });
}

fn thumb(ui: &mut Ui, im: &crate::chat::Thumb, tex: &std::collections::HashMap<usize, egui::TextureHandle>, max_h: f32) {
    let key = std::sync::Arc::as_ptr(&im.png_or_jpg) as usize;
    if let Some(h) = tex.get(&key) {
        let sz = h.size_vec2();
        let k = (max_h / sz.y).min((ui.available_width() - 4.0) / sz.x).min(1.0);
        ui.add_space(4.0);
        let (r, _) = ui.allocate_exact_size(sz * k, Sense::hover());
        ui.painter().image(h.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, INK), StrokeKind::Outside);
    }
}

fn claude_card(ui: &mut Ui, t: &str, blocks: &mut [Block], tex: &std::collections::HashMap<usize, egui::TextureHandle>) {
    let frame = egui::Frame::new().fill(MANILA).stroke(Stroke::new(1.0, INK)).inner_margin(8);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        card_header(ui, "KERF/CLAUDE", t);
        let n = blocks.len();
        for (i, b) in blocks.iter_mut().enumerate() {
            match b {
                Block::Text(s) => {
                    wrapped(ui, s, regular(13.0), INK);
                }
                Block::Fallback(s) => {
                    tool_row(ui, s, true, false, false);
                }
                Block::Tool(act) => tool_activity(ui, act, tex),
            }
            if i + 1 < n {
                ui.add_space(4.0);
            }
        }
    });
}

fn tool_row(ui: &mut Ui, line: &str, ok: bool, expandable: bool, open: bool) -> egui::Response {
    let font = medium(11.5);
    let col = INK;
    // split trailing check / X glyph so it can be colored
    let (main, mark) = match line.rfind('\u{2713}') {
        Some(_) => (line, Some(true)),
        None if line.ends_with(" X") || line.ends_with("   X") => (line.trim_end_matches('X'), Some(false)),
        None => (line, None),
    };
    let main = if mark == Some(true) {
        // draw the line up to the check, then the check in green with the counts after it
        main
    } else {
        main
    };
    let g = galley(ui, main.replace('\u{2713}', "\u{2713}"), font.clone(), col, 0.3);
    let size = Vec2::new(ui.available_width(), g.size().y + 6.0);
    let (r, resp) = ui.allocate_exact_size(size, if expandable { Sense::click() } else { Sense::hover() });
    let hov = expandable && resp.hovered();
    if hov {
        ui.painter().rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 14));
    }
    let cy = r.center().y;
    // painted disclosure triangle
    let tri = if open {
        vec![Pos2::new(r.left() + 3.0, cy - 3.0), Pos2::new(r.left() + 11.0, cy - 3.0), Pos2::new(r.left() + 7.0, cy + 3.5)]
    } else {
        vec![Pos2::new(r.left() + 4.0, cy - 4.0), Pos2::new(r.left() + 4.0, cy + 4.0), Pos2::new(r.left() + 10.0, cy)]
    };
    ui.painter().add(egui::Shape::convex_polygon(tri, INK, Stroke::NONE));
    // text, with the check/err glyph recolored
    let ok_col = if ok { GREEN } else { RED };
    if let Some(pos) = main.find('\u{2713}') {
        let (a, b) = main.split_at(pos);
        let ga = galley(ui, a, font.clone(), col, 0.3);
        let gb = galley(ui, b, font.clone(), ok_col, 0.3);
        let x = r.left() + 16.0;
        let ty = cy - ga.size().y / 2.0;
        let wa = ga.size().x;
        ui.painter().galley(Pos2::new(x, ty), ga, col);
        ui.painter().galley(Pos2::new(x + wa, ty), gb, ok_col);
    } else {
        let c = if mark == Some(false) { RED } else { col };
        let g = galley(ui, main, font, c, 0.3);
        ui.painter().galley(Pos2::new(r.left() + 16.0, cy - g.size().y / 2.0), g, c);
    }
    resp
}

fn tool_activity(ui: &mut Ui, act: &mut ToolActivity, tex: &std::collections::HashMap<usize, egui::TextureHandle>) {
    let resp = tool_row(ui, &act.line, act.ok, true, act.open);
    if resp.clicked() {
        act.open = !act.open;
    }
    if let Some(im) = &act.image {
        thumb(ui, im, tex, 150.0);
    }
    if act.open {
        let frame = egui::Frame::new().fill(PAPER2).stroke(Stroke::new(1.0, INK2)).inner_margin(6);
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            label_caps(ui, "Input");
            let s = serde_json::to_string_pretty(&act.input).unwrap_or_default();
            code_block(ui, &s, 14);
            if !act.result.is_empty() {
                ui.add_space(4.0);
                label_caps(ui, "Result");
                code_block(ui, &act.result, 12);
            }
        });
    }
}

fn code_block(ui: &mut Ui, s: &str, max_lines: usize) {
    let lines: Vec<&str> = s.lines().collect();
    let shown = if lines.len() > max_lines { lines[..max_lines].join("\n") + &format!("\n... ({} more lines)", lines.len() - max_lines) } else { s.to_owned() };
    wrapped(ui, &shown, regular(11.0), INK);
}
