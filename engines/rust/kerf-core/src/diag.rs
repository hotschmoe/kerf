//! Diagnostics (SPEC section 9). Messages are written for an LLM reader.

use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warning,
    Info,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub level: Level,
    pub code: &'static str,
    pub id: Option<String>,
    pub path: Option<String>,
    pub message: String,
    pub fix: Option<String>,
}

impl Diag {
    pub fn new(level: Level, code: &'static str, message: impl Into<String>) -> Diag {
        Diag { level, code, id: None, path: None, message: message.into(), fix: None }
    }
    pub fn error(code: &'static str, message: impl Into<String>) -> Diag {
        Diag::new(Level::Error, code, message)
    }
    pub fn warn(code: &'static str, message: impl Into<String>) -> Diag {
        Diag::new(Level::Warning, code, message)
    }
    pub fn info(code: &'static str, message: impl Into<String>) -> Diag {
        Diag::new(Level::Info, code, message)
    }
    pub fn id(mut self, id: impl Into<String>) -> Diag {
        self.id = Some(id.into());
        self
    }
    pub fn path(mut self, p: impl Into<String>) -> Diag {
        self.path = Some(p.into());
        self
    }
    pub fn fix(mut self, f: impl Into<String>) -> Diag {
        self.fix = Some(f.into());
        self
    }
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("level".into(), Value::String(self.level.as_str().into()));
        m.insert("code".into(), Value::String(self.code.into()));
        if let Some(id) = &self.id {
            m.insert("id".into(), Value::String(id.clone()));
        }
        if let Some(p) = &self.path {
            m.insert("path".into(), Value::String(p.clone()));
        }
        m.insert("message".into(), Value::String(self.message.clone()));
        if let Some(f) = &self.fix {
            m.insert("fix".into(), Value::String(f.clone()));
        }
        Value::Object(m)
    }
    pub fn line(&self) -> String {
        let tag = match self.level {
            Level::Error => "ERROR",
            Level::Warning => "WARN",
            Level::Info => "INFO",
        };
        let mut s = format!("{} {}", tag, self.code);
        if let Some(id) = &self.id {
            s.push(' ');
            s.push_str(id);
        }
        s.push_str(": ");
        s.push_str(&self.message);
        if let Some(f) = &self.fix {
            s.push_str(" Fix: ");
            s.push_str(f);
        }
        s
    }
}

pub fn count(diags: &[Diag]) -> (usize, usize, usize) {
    let e = diags.iter().filter(|d| d.level == Level::Error).count();
    let w = diags.iter().filter(|d| d.level == Level::Warning).count();
    let i = diags.iter().filter(|d| d.level == Level::Info).count();
    (e, w, i)
}

pub fn diags_json(diags: &[Diag]) -> Value {
    Value::Array(diags.iter().map(|d| d.to_json()).collect())
}

/// Levenshtein distance.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Nearest candidate by edit distance (first wins ties), if reasonably close.
pub fn nearest<'a, I: IntoIterator<Item = &'a str>>(target: &str, cands: I) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for c in cands {
        let d = edit_distance(target, c);
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, c));
        }
    }
    best.filter(|(d, c)| *d <= (target.len().max(c.len()) / 2).max(2)).map(|(_, c)| c.to_string())
}
