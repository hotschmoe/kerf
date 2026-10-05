//! The working document: current doc JSON, style, op log with per-entry undo snapshots, and the
//! drawing/mesh caches keyed by revision.

use crate::engine;
use crate::ir::{Mesh, Prep};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Who {
    Designer,
    Claude,
    Load,
}

impl Who {
    pub fn label(self) -> &'static str {
        match self {
            Who::Designer => "DESIGNER",
            Who::Claude => "CLAUDE",
            Who::Load => "FILE",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub n: usize,
    pub who: Who,
    pub why: String,
    pub ops: Value,
    pub t: String,
    pub before: Option<Value>,
    pub changed: Vec<String>,
}

pub struct Session {
    pub doc: Option<Value>,
    pub style: Value,
    pub rev: u64,
    pub log: Vec<LogEntry>,
    pub file_name: Option<String>,
    /// designer edits since the last LLM turn (why lines)
    pub unsent_notes: Vec<String>,
    pub diagnostics: Vec<Value>,
    pub summary: String,
    drawings: HashMap<(String, u64), Arc<Prep>>,
    mesh: Option<(u64, Arc<Mesh>)>,
    pub last_error: Option<String>,
    counter: usize,
}

impl Session {
    pub fn new() -> Session {
        Session {
            doc: None,
            style: engine::default_style(),
            rev: 0,
            log: Vec::new(),
            file_name: None,
            unsent_notes: Vec::new(),
            diagnostics: Vec::new(),
            summary: String::new(),
            drawings: HashMap::new(),
            mesh: None,
            last_error: None,
            counter: 0,
        }
    }

    pub fn load(&mut self, doc: Value, name: Option<String>, clock: &str) {
        self.log.clear();
        self.unsent_notes.clear();
        self.counter = 0;
        let title = name.clone().unwrap_or_else(|| doc["id"].as_str().unwrap_or("doc").to_owned());
        self.doc = Some(doc);
        self.file_name = name;
        self.bump();
        self.push_log(Who::Load, format!("Loaded {title}"), json!([]), None, vec![], clock);
    }

    fn bump(&mut self) {
        self.rev += 1;
        self.drawings.clear();
        self.mesh = None;
        if let Some(doc) = &self.doc {
            match engine::check(doc, &self.style) {
                Ok(v) => {
                    self.diagnostics = v["diagnostics"].as_array().cloned().unwrap_or_default();
                    self.summary = v["summary"].as_str().unwrap_or("").to_owned();
                }
                Err(e) => {
                    self.diagnostics = vec![json!({"level": "error", "code": "E_ENGINE", "message": e})];
                }
            }
        } else {
            self.diagnostics.clear();
            self.summary.clear();
        }
    }

    fn push_log(&mut self, who: Who, why: String, ops: Value, before: Option<Value>, changed: Vec<String>, clock: &str) {
        self.counter += 1;
        self.log.push(LogEntry { n: self.counter, who, why, ops, t: clock.to_owned(), before, changed });
    }

    /// Apply ops through the engine. On success the doc is replaced and the op log gains an entry.
    pub fn apply(&mut self, ops: Value, why: &str, who: Who, clock: &str) -> Result<engine::ApplyOut, String> {
        let doc = self.doc.clone().unwrap_or_else(|| json!({"kerf": "0.1", "id": "untitled", "components": [], "views": []}));
        let actor = if who == Who::Designer { "designer" } else { "llm" };
        let out = engine::apply(&doc, &self.style, &ops, actor)?;
        if out.ok {
            if let Some(newdoc) = out.doc.clone() {
                let before = self.doc.take();
                self.doc = Some(newdoc);
                self.bump();
                self.push_log(who, why.to_owned(), ops, before, out.changed.clone(), clock);
                if who == Who::Designer {
                    self.unsent_notes.push(why.to_owned());
                }
            }
        }
        Ok(out)
    }

    /// Undo the most recent op-log entry that has a snapshot. Returns its description.
    pub fn undo(&mut self) -> Option<String> {
        let idx = self.log.iter().rposition(|e| e.before.is_some())?;
        let e = self.log.remove(idx);
        // everything after it also becomes invalid for undo order; drop later entries' snapshots only
        // if they were not removed (undo is strictly LIFO so idx is the last snapshot-bearing entry).
        self.doc = e.before;
        self.bump();
        let note = format!("undid: {}", e.why);
        self.unsent_notes.push(note.clone());
        Some(note)
    }

    pub fn drawing_ex(&mut self, view: &str, sheet: bool) -> Result<Arc<Prep>, String> {
        let key = (if sheet { format!("sheet:{view}") } else { view.to_owned() }, self.rev);
        if let Some(p) = self.drawings.get(&key) {
            return Ok(p.clone());
        }
        let doc = self.doc.as_ref().ok_or("no document")?;
        let json = engine::drawing_json(doc, &self.style, view, sheet)?;
        let prep = Arc::new(Prep::from_json(&json)?);
        self.drawings.insert(key, prep.clone());
        Ok(prep)
    }

    pub fn mesh(&mut self) -> Result<Arc<Mesh>, String> {
        if let Some((r, m)) = &self.mesh {
            if *r == self.rev {
                return Ok(m.clone());
            }
        }
        let doc = self.doc.as_ref().ok_or("no document")?;
        let json = engine::mesh_json(doc, &self.style)?;
        let m: Mesh = serde_json::from_str(&json).map_err(|e| format!("mesh: {e}"))?;
        let m = Arc::new(m);
        self.mesh = Some((self.rev, m.clone()));
        Ok(m)
    }

    // ---- doc queries

    pub fn views(&self) -> Vec<(String, String)> {
        self.doc
            .as_ref()
            .and_then(|d| d["views"].as_array())
            .map(|a| {
                a.iter()
                    .map(|v| (v["id"].as_str().unwrap_or("?").to_owned(), v["kind"].as_str().unwrap_or("section").to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn components(&self) -> Vec<&Value> {
        self.doc.as_ref().and_then(|d| d["components"].as_array()).map(|a| a.iter().collect()).unwrap_or_default()
    }

    pub fn component(&self, id: &str) -> Option<&Value> {
        self.components().into_iter().find(|c| c["id"] == id)
    }

    /// (view id, annotation) for an annotation id.
    pub fn annotation(&self, id: &str) -> Option<(String, &Value)> {
        for v in self.doc.as_ref()?["views"].as_array()? {
            for a in v["annotations"].as_array().into_iter().flatten() {
                if a["id"] == id {
                    return Some((v["id"].as_str().unwrap_or("").to_owned(), a));
                }
            }
        }
        None
    }

    pub fn view_doc(&self, id: &str) -> Option<&Value> {
        self.doc.as_ref()?["views"].as_array()?.iter().find(|v| v["id"] == id)
    }

    pub fn title(&self) -> String {
        self.doc.as_ref().and_then(|d| d["id"].as_str()).unwrap_or("").to_uppercase()
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let mut e = 0;
        let mut w = 0;
        let mut i = 0;
        for d in &self.diagnostics {
            match d["level"].as_str() {
                Some("error") => e += 1,
                Some("warning") => w += 1,
                _ => i += 1,
            }
        }
        (e, w, i)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
