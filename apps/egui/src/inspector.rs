//! Inspector (right column): PARTS / NOTES / DIFF tabs, selected-component fields, the note and
//! citation editor (every edit is a designer op), diagnostics, and the export buttons.

use crate::app::{InspTab, KerfApp};
use crate::engine;
use crate::session::Who;
use crate::theme::*;
use egui::{Align, Color32, Layout, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::{Value, json};

const ROW_H: f32 = 22.0;

impl KerfApp {
    pub fn inspector_panel(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, PAPER);
        let inner = rect.shrink2(Vec2::new(12.0, 10.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
        let ui = &mut ui;
        ui.set_clip_rect(rect);

        heading(ui, "Inspector");
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (t, l) in [(InspTab::Parts, "PARTS"), (InspTab::Notes, "NOTES"), (InspTab::Diff, "DIFF")] {
                if tab(ui, l, self.insp_tab == t).clicked() {
                    self.insp_tab = t;
                }
            }
        });
        hrule(ui, 1.0);
        ui.add_space(6.0);

        // bottom block: diagnostics + exports
        egui::Panel::bottom("insp-bottom").frame(egui::Frame::NONE).resizable(false).show_separator_line(false).show(ui, |ui| {
            ui.add_space(4.0);
            hrule(ui, 1.0);
            ui.add_space(6.0);
            self.diagnostics_block(ui);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if button(ui, "Export DXF").clicked() {
                    self.export("dxf");
                }
                if button(ui, "PDF").clicked() {
                    self.export("pdf");
                }
                if button(ui, "SVG").clicked() {
                    self.export("svg");
                }
            });
            ui.add_space(4.0);
        });

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            match self.insp_tab {
                InspTab::Parts => self.parts_tab(ui),
                InspTab::Notes => self.notes_tab(ui),
                InspTab::Diff => self.diff_tab(ui),
            }
        });
    }

    // ------------------------------------------------------------ parts

    fn parts_tab(&mut self, ui: &mut Ui) {
        if self.session.doc.is_none() {
            wrapped(ui, "NO DETAIL LOADED.", regular(12.0), INK2);
            return;
        }
        table_header(ui, &[("NO", 30.0), ("ID", 118.0), ("TYPE", 0.0)]);
        let comps: Vec<(String, String, String)> = self
            .session
            .components()
            .iter()
            .map(|c| (c["id"].as_str().unwrap_or("?").to_owned(), c["type"].as_str().unwrap_or("?").to_owned(), c["label"].as_str().unwrap_or("").to_owned()))
            .collect();
        let mut hover = None;
        for (i, (id, ty, _label)) in comps.iter().enumerate() {
            let sel = self.selected.as_deref() == Some(id.as_str());
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
            row_bg(ui, r, i, sel, resp.hovered());
            let fg = if sel { PAPER } else { INK };
            row_text(ui, r, 0.0, &format!("{:02}", i + 1), if sel { PAPER } else { INK2 }, false);
            row_text(ui, r, 34.0, id, fg, sel);
            row_text(ui, r, 34.0 + 118.0, &ty.to_uppercase(), fg, false);
            if resp.hovered() {
                hover = Some(id.clone());
            }
            if resp.clicked() {
                self.selected = Some(id.clone());
            }
        }
        if hover.is_some() {
            self.hover = hover;
        }
        ui.add_space(10.0);
        if let Some(sel) = self.selected.clone() {
            if let Some(c) = self.session.component(&sel).cloned() {
                self.component_fields(ui, &sel, &c);
            } else if self.session.annotation(&sel).is_some() {
                wrapped(ui, &format!("{sel} IS AN ANNOTATION. SEE [NOTES]."), regular(12.0), INK2);
            }
        } else {
            wrapped(ui, "SELECT A COMPONENT IN THE VIEWPORT OR THE TABLE.", regular(12.0), INK2);
        }
    }

    fn component_fields(&mut self, ui: &mut Ui, id: &str, c: &Value) {
        heading(ui, id);
        ui.add_space(4.0);
        table_header(ui, &[("FIELD", 84.0), ("VALUE", 0.0)]);
        let mut i = 0;
        if let Some(o) = c.as_object() {
            for (k, v) in o {
                if k == "id" {
                    continue;
                }
                let val = compact(v);
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
                row_bg(ui, r, i, false, false);
                row_text(ui, r, 0.0, &k.to_uppercase(), INK2, false);
                row_text_clip(ui, r, 88.0, &val, INK, r.width() - 92.0);
                i += 1;
            }
        }
        // resolved anchors from the engine
        let key = (self.session.rev, id.to_owned());
        let cached = matches!(&self.insp_cache, Some((r, i, _)) if *r == key.0 && *i == key.1);
        if !cached {
            let v = match &self.session.doc {
                Some(doc) => engine::inspect(doc, &self.session.style, &json!({"q": "component", "id": id})).unwrap_or(Value::Null),
                None => Value::Null,
            };
            self.insp_cache = Some((key.0, key.1.clone(), v));
        }
        if let Some((_, _, v)) = &self.insp_cache {
            let anchors = extract_anchors(v);
            if !anchors.is_empty() {
                ui.add_space(8.0);
                table_header(ui, &[("ANCHOR", 130.0), ("X", 70.0), ("Y", 0.0)]);
                for (n, (name, x, y)) in anchors.iter().enumerate() {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
                    row_bg(ui, r, n, false, false);
                    row_text(ui, r, 0.0, name, INK2, false);
                    row_text(ui, r, 134.0, &crate::fmt::ft_in(*x), INK, false);
                    row_text(ui, r, 204.0, &crate::fmt::ft_in(*y), INK, false);
                }
            }
        }
    }

    // ------------------------------------------------------------ notes

    fn notes_tab(&mut self, ui: &mut Ui) {
        let Some(view) = self.current_view_id() else {
            wrapped(ui, "NO DETAIL LOADED.", regular(12.0), INK2);
            return;
        };
        let anns: Vec<Value> = self.session.view_doc(&view).and_then(|v| v["annotations"].as_array()).cloned().unwrap_or_default();
        table_header(ui, &[("ID", 74.0), ("TYPE", 44.0), ("TEXT", 0.0)]);
        for (i, a) in anns.iter().enumerate() {
            let id = a["id"].as_str().unwrap_or("?");
            let sel = self.selected.as_deref() == Some(id);
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
            row_bg(ui, r, i, sel, resp.hovered());
            let fg = if sel { PAPER } else { INK };
            row_text(ui, r, 0.0, id, fg, sel);
            row_text(ui, r, 78.0, a["type"].as_str().unwrap_or(""), if sel { PAPER } else { INK2 }, false);
            let txt = a["text"].as_str().unwrap_or("");
            row_text_clip(ui, r, 126.0, txt, fg, r.width() - 130.0);
            if resp.hovered() {
                self.hover = Some(id.to_owned());
            }
            if resp.clicked() {
                self.selected = Some(id.to_owned());
            }
        }
        ui.add_space(10.0);
        let Some(sel) = self.selected.clone() else {
            wrapped(ui, "SELECT A NOTE IN THE VIEWPORT OR THE TABLE TO EDIT IT.", regular(12.0), INK2);
            return;
        };
        let Some((vid, ann)) = self.session.annotation(&sel).map(|(v, a)| (v, a.clone())) else {
            wrapped(ui, "SELECT A NOTE (NOT A COMPONENT) TO EDIT TEXT AND CITATIONS.", regular(12.0), INK2);
            return;
        };
        self.note_editor(ui, &vid, &sel, &ann);
    }

    fn note_editor(&mut self, ui: &mut Ui, vid: &str, id: &str, ann: &Value) {
        let ty = ann["type"].as_str().unwrap_or("note").to_owned();
        ui.spacing_mut().item_spacing.y = 3.0;
        heading(ui, &format!("{} {}", ty, id));
        ui.add_space(6.0);
        // text
        if ty == "note" || ty == "label" {
            if self.note_text_buf.0 != id || (!ui.memory(|m| m.focused().is_some()) && self.note_text_buf.1 != ann["text"].as_str().unwrap_or("")) {
                self.note_text_buf = (id.to_owned(), ann["text"].as_str().unwrap_or("").to_owned());
            }
            label_caps(ui, "Text");
            let r = field_cell(ui, &mut self.note_text_buf.1, "NOTE TEXT", false, Some(3));
            if r.lost_focus() && self.note_text_buf.1 != ann["text"].as_str().unwrap_or("") {
                let t = self.note_text_buf.1.trim().to_uppercase();
                self.note_text_buf.1 = t.clone();
                self.designer_op(json!([{"op": "update", "path": format!("views/{vid}/annotations/{id}"), "value": {"text": t}}]), &format!("Edit text of {id}"));
            }
            ui.add_space(8.0);
        }
        if ty == "note" {
            if let Some(t) = ann["target"].as_str() {
                label_caps(ui, "Target");
                text(ui, t, INK);
                ui.add_space(6.0);
            }
            if let Some(p) = ann["place"].as_array() {
                label_caps(ui, "Place (model in.)");
                text(ui, format!("[{}, {}]", p[0], p[1]), INK);
                ui.add_space(2.0);
                if small_button(ui, "Reset place", false).clicked() {
                    self.designer_op(json!([{"op": "update", "path": format!("views/{vid}/annotations/{id}"), "value": {"place": null}}]), &format!("Reset place of {id}"));
                }
                ui.add_space(6.0);
            } else {
                wrapped(ui, "DRAG THE NOTE IN THE VIEWPORT TO SET ITS PLACE.", regular(11.0), INK2);
                ui.add_space(6.0);
            }
            self.cite_editor(ui, vid, id, ann);
        } else if ty == "dim" {
            for k in ["from", "to", "dir", "offset"] {
                if !ann[k].is_null() {
                    label_caps(ui, k);
                    text(ui, compact(&ann[k]), INK);
                }
            }
        }
    }

    fn cite_editor(&mut self, ui: &mut Ui, vid: &str, id: &str, ann: &Value) {
        label_caps(ui, "Code citations");
        ui.add_space(2.0);
        let cites: Vec<Value> = ann["cite"].as_array().cloned().unwrap_or_default();
        let mut set_cites: Option<(Vec<Value>, String)> = None;
        if cites.is_empty() {
            wrapped(ui, "NONE.", regular(12.0), INK2);
        }
        for (i, c) in cites.iter().enumerate() {
            let verified = c["status"] == "verified";
            let code = format!("{} {}", c["code"].as_str().unwrap_or(""), c["section"].as_str().unwrap_or(""));
            ui.horizontal(|ui| {
                text(ui, code.clone(), INK);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if small_button(ui, "X", false).clicked() {
                        let mut v = cites.clone();
                        v.remove(i);
                        set_cites = Some((v, format!("Remove citation {code} from {id}")));
                    }
                    let st = if verified { stamp(ui, "VERIFIED", GREEN) } else { stamp(ui, "UNVERIFIED", RED) };
                    if st.clicked() {
                        let mut v = cites.clone();
                        v[i]["status"] = json!(if verified { "suggested" } else { "verified" });
                        set_cites = Some((v, format!("{} citation {code} on {id}", if verified { "Unverify" } else { "Verify" })));
                    }
                });
            });
            if let Some(t) = c["title"].as_str() {
                wrapped(ui, t, regular(11.0), INK2);
            }
            ui.add_space(4.0);
        }
        ui.add_space(4.0);
        label_caps(ui, "Add citation");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let w = (ui.available_width() - 12.0) / 3.0;
            for (buf, hint) in [(&mut self.cite_new.0, "CODE"), (&mut self.cite_new.1, "SECTION"), (&mut self.cite_new.2, "TITLE")] {
                ui.allocate_ui(Vec2::new(w, 24.0), |ui| {
                    field_cell(ui, buf, hint, false, None);
                });
            }
        });
        ui.add_space(4.0);
        if button(ui, "Add citation").clicked() && !self.cite_new.1.trim().is_empty() {
            let mut v = cites.clone();
            let edition = self.session.doc.as_ref().and_then(|d| d["meta"]["jurisdiction"]["edition"].as_i64()).unwrap_or(2021);
            let code = if self.cite_new.0.trim().is_empty() { "IRC".to_owned() } else { self.cite_new.0.trim().to_uppercase() };
            v.push(json!({"code": code, "edition": edition, "section": self.cite_new.1.trim(), "title": self.cite_new.2.trim(), "status": "suggested"}));
            set_cites = Some((v, format!("Add citation {} {} to {id}", code, self.cite_new.1.trim())));
            self.cite_new = Default::default();
        }
        if let Some((v, why)) = set_cites {
            self.designer_op(json!([{"op": "update", "path": format!("views/{vid}/annotations/{id}"), "value": {"cite": v}}]), &why);
        }
        ui.add_space(4.0);
        wrapped(ui, "ONLY THE DESIGNER CAN VERIFY. CLAUDE'S CITATIONS ARE ALWAYS UNVERIFIED.", regular(10.0), INK2);
    }

    // ------------------------------------------------------------ diff

    fn diff_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let can = self.session.log.iter().any(|e| e.before.is_some());
            if button_k(ui, "Undo last op group", Kind::Normal, can).clicked() && can {
                if let Some(n) = self.session.undo() {
                    self.flash(n.to_uppercase(), false);
                }
            }
        });
        ui.add_space(6.0);
        if self.session.log.is_empty() {
            wrapped(ui, "NO OPS YET.", regular(12.0), INK2);
            return;
        }
        let entries: Vec<_> = self.session.log.iter().rev().cloned().collect();
        for e in entries {
            let open = self.expand_ops.contains(&e.n);
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
            let (bg, fg) = if resp.hovered() { (PAPER2, INK) } else { (PAPER, INK) };
            ui.painter().rect_filled(r, 0.0, bg);
            let tagc = match e.who {
                Who::Claude => MANILA,
                Who::Designer => PAPER,
                Who::Load => PAPER2,
            };
            let tag = Rect::from_min_size(Pos2::new(r.left(), r.top() + 3.0), Vec2::new(64.0, 16.0));
            ui.painter().rect_filled(tag, 0.0, tagc);
            ui.painter().rect_stroke(tag, 0.0, Stroke::new(1.0, INK), egui::StrokeKind::Inside);
            ui.painter().text(tag.center(), egui::Align2::CENTER_CENTER, e.who.label(), medium(10.0), INK);
            ui.painter().text(Pos2::new(tag.right() + 8.0, tag.center().y), egui::Align2::LEFT_CENTER, format!("#{:02}  {}", e.n, e.t), regular(11.0), INK2);
            let nops = e.ops.as_array().map(|a| a.len()).unwrap_or(0);
            ui.painter().text(Pos2::new(r.right(), tag.center().y), egui::Align2::RIGHT_CENTER, format!("{nops} OPS"), regular(11.0), INK2);
            let g = galley(ui, e.why.clone(), regular(12.0), fg, 0.0);
            ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(r.left(), r.top() + 20.0), r.max)).galley(Pos2::new(r.left(), r.top() + 22.0), g, fg);
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(r.left(), r.bottom() - 1.0), Vec2::new(r.width(), 1.0)), 0.0, INK2.gamma_multiply(0.5));
            if resp.clicked() {
                if open {
                    self.expand_ops.remove(&e.n);
                } else {
                    self.expand_ops.insert(e.n);
                }
            }
            if open {
                let s = serde_json::to_string_pretty(&e.ops).unwrap_or_default();
                let lines: Vec<&str> = s.lines().collect();
                let shown = if lines.len() > 40 { lines[..40].join("\n") + "\n..." } else { s.clone() };
                egui::Frame::new().fill(PAPER2).inner_margin(6).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    wrapped(ui, &shown, regular(10.5), INK);
                });
            }
        }
    }

    // ------------------------------------------------------------ diagnostics

    fn diagnostics_block(&mut self, ui: &mut Ui) {
        let diags = self.session.diagnostics.clone();
        let (e, w, i) = self.session.counts();
        label_caps(ui, &format!("Diagnostics  {e}E {w}W {i}I"));
        ui.add_space(2.0);
        if diags.is_empty() {
            text(ui, "CLEAN.", GREEN);
            return;
        }
        egui::ScrollArea::vertical().max_height(132.0).auto_shrink([false, true]).id_salt("diag").show(ui, |ui| {
            for d in &diags {
                let (tag, col) = match d["level"].as_str() {
                    Some("error") => ("E", RED),
                    Some("warning") => ("W", AMBER),
                    _ => ("I", INK2),
                };
                let msg = format!("{} {}", d["code"].as_str().unwrap_or(""), d["message"].as_str().unwrap_or(""));
                let mut job = spaced(msg, regular(11.0), INK, 0.0);
                job.wrap.max_width = ui.available_width() - 22.0;
                let g = ui.painter().layout_job(job);
                let h = g.size().y.max(16.0) + 4.0;
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, PAPER2);
                }
                let sq = Rect::from_min_size(Pos2::new(r.left(), r.top() + 2.0), Vec2::splat(16.0));
                ui.painter().rect_filled(sq, 0.0, col);
                ui.painter().text(sq.center(), egui::Align2::CENTER_CENTER, tag, bold(11.0), if col == AMBER { INK } else { Color32::WHITE });
                ui.painter().galley(Pos2::new(sq.right() + 6.0, r.top() + 2.0), g, INK);
                if resp.clicked() {
                    if let Some(id) = d["id"].as_str() {
                        self.selected = Some(id.to_owned());
                        self.insp_tab = InspTab::Parts;
                    }
                }
            }
        });
    }
}

// ------------------------------------------------------------------ table helpers

fn table_header(ui: &mut Ui, cols: &[(&str, f32)]) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, PAPER2);
    ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), 1.0)), 0.0, INK);
    ui.painter().rect_filled(Rect::from_min_size(Pos2::new(r.left(), r.bottom() - 1.0), Vec2::new(r.width(), 1.0)), 0.0, INK);
    let mut x = 4.0;
    for (label, w) in cols {
        let g = galley(ui, label.to_uppercase(), medium(10.0), INK2, 0.9);
        ui.painter().galley(Pos2::new(r.left() + x, r.center().y - g.size().y / 2.0), g, INK2);
        x += w + 4.0;
    }
}

fn row_bg(ui: &Ui, r: Rect, i: usize, sel: bool, hov: bool) {
    let bg = if sel {
        BLUE
    } else if hov {
        MANILA.gamma_multiply(0.7)
    } else if i % 2 == 1 {
        PAPER2
    } else {
        PAPER
    };
    ui.painter().rect_filled(r, 0.0, bg);
    ui.painter().rect_filled(Rect::from_min_size(Pos2::new(r.left(), r.bottom() - 1.0), Vec2::new(r.width(), 1.0)), 0.0, INK.gamma_multiply(0.25));
}

fn row_text(ui: &Ui, r: Rect, x: f32, s: &str, color: Color32, bold_: bool) {
    let font = if bold_ { bold(12.0) } else { regular(12.0) };
    ui.painter().text(Pos2::new(r.left() + 4.0 + x, r.center().y), egui::Align2::LEFT_CENTER, s, font, color);
}

fn row_text_clip(ui: &Ui, r: Rect, x: f32, s: &str, color: Color32, max_w: f32) {
    let clip = Rect::from_min_max(Pos2::new(r.left() + 4.0 + x, r.top()), Pos2::new(r.left() + 4.0 + x + max_w.max(10.0), r.bottom()));
    ui.painter().with_clip_rect(clip).text(Pos2::new(clip.left(), r.center().y), egui::Align2::LEFT_CENTER, s, regular(12.0), color);
}

fn compact(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// Pull `(name, x, y)` triples out of whatever shape the engine's component inspect uses.
fn extract_anchors(v: &Value) -> Vec<(String, f64, f64)> {
    let a = v.get("anchors").or_else(|| v.get("anchor"));
    let mut out = Vec::new();
    match a {
        Some(Value::Object(o)) => {
            for (k, p) in o {
                if let Some(xy) = xy_of(p) {
                    out.push((k.clone(), xy.0, xy.1));
                }
            }
        }
        Some(Value::Array(arr)) => {
            for p in arr {
                let name = p["name"].as_str().unwrap_or("?").to_owned();
                if let Some(xy) = xy_of(p) {
                    out.push((name, xy.0, xy.1));
                }
            }
        }
        _ => {}
    }
    out
}

fn xy_of(p: &Value) -> Option<(f64, f64)> {
    if let Some(a) = p.as_array() {
        return Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?));
    }
    if let Some(a) = p.get("pt").or_else(|| p.get("xy")).or_else(|| p.get("point")).and_then(Value::as_array) {
        return Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?));
    }
    Some((p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))
}
