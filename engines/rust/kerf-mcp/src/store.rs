//! The workspace: a directory of `<id>.kerf.json` documents plus the "active document" pointer.
//! Files are the source of truth and are re-read on every call, so a designer (or git) can edit
//! a document between tool calls without the server going stale.

use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

pub const DOC_SUFFIX: &str = ".kerf.json";

pub struct Workspace {
    dir: PathBuf,
    pub style: Option<Value>,
    active: Option<PathBuf>,
}

pub fn doc_id_of(path: &Path) -> String {
    let n = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    n.strip_suffix(DOC_SUFFIX).unwrap_or(&n).to_string()
}

pub fn valid_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !id.contains("..");
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid document id {:?}: use 1-64 characters from A-Z a-z 0-9 - _ . starting with a letter or digit, e.g. \"truss-bearing-cmu\" (it becomes the file name <id>{})",
            id, DOC_SUFFIX
        ))
    }
}

/// Lexical normalization (no filesystem access): folds `.` and `..`.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl Workspace {
    pub fn new(dir: &str, style: Option<Value>) -> Result<Workspace, String> {
        let p = PathBuf::from(dir);
        std::fs::create_dir_all(&p).map_err(|e| format!("cannot create workspace dir {}: {}", dir, e))?;
        let dir = p.canonicalize().map_err(|e| format!("cannot resolve workspace dir {}: {}", dir, e))?;
        Ok(Workspace { dir, style, active: None })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for_id(&self, id: &str) -> Result<PathBuf, String> {
        valid_id(id)?;
        Ok(self.dir.join(format!("{}{}", id, DOC_SUFFIX)))
    }

    /// All `*.kerf.json` files directly in the workspace dir, sorted by name.
    pub fn list(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(DOC_SUFFIX))).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    pub fn active(&self) -> Option<&Path> {
        self.active.as_deref()
    }

    pub fn set_active(&mut self, p: PathBuf) {
        self.active = Some(p);
    }

    /// `spec` is a document id ("truss-bearing-cmu") or a path ending in `.kerf.json`
    /// (relative paths resolve against the workspace dir).
    pub fn resolve_spec(&self, spec: &str) -> Result<PathBuf, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("empty document path".into());
        }
        let path = if spec.ends_with(DOC_SUFFIX) {
            let p = PathBuf::from(spec);
            if p.is_absolute() { normalize(&p) } else { normalize(&self.dir.join(p)) }
        } else if spec.contains('/') || spec.contains('\\') {
            return Err(format!("{:?} is not a Kerf document: paths must end in {}; or pass a bare document id from kerf_list", spec, DOC_SUFFIX));
        } else {
            self.path_for_id(spec)?
        };
        if !path.is_file() {
            let ids: Vec<String> = self.list().iter().map(|p| doc_id_of(p)).collect();
            return Err(format!(
                "no document at {}. Documents in {}: {}. Create one with kerf_new.",
                path.display(),
                self.dir.display(),
                if ids.is_empty() { "(none)".to_string() } else { ids.join(", ") }
            ));
        }
        Ok(path)
    }

    /// The document a tool call operates on: the explicit `doc` argument, else the active
    /// document, else the only document in the workspace.
    pub fn target(&self, explicit: Option<&str>) -> Result<PathBuf, String> {
        if let Some(d) = explicit {
            return self.resolve_spec(d);
        }
        if let Some(a) = &self.active {
            if a.is_file() {
                return Ok(a.clone());
            }
            return Err(format!("the active document {} no longer exists on disk. Use kerf_list / kerf_open / kerf_new.", a.display()));
        }
        let l = self.list();
        match l.len() {
            0 => Err(format!("no active document and none in {}. Call kerf_new {{\"id\":\"my-detail\",\"title\":\"...\"}} first.", self.dir.display())),
            1 => Ok(l[0].clone()),
            _ => Err(format!(
                "no active document and {} documents in {}: {}. Call kerf_open {{\"path\":\"<id>\"}} (or pass \"doc\":\"<id>\").",
                l.len(),
                self.dir.display(),
                l.iter().map(|p| doc_id_of(p)).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    pub fn load(path: &Path) -> Result<Value, String> {
        let t = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        kerf_core::json::parse(&t).map_err(|e| format!("{} is not valid JSON ({}). Fix the file by hand or restore it from git.", path.display(), e))
    }

    /// Canonical pretty JSON, written atomically so a crash cannot leave a half-written document.
    pub fn save_text(path: &Path, text: &str) -> Result<(), String> {
        let mut t = text.to_string();
        if !t.ends_with('\n') {
            t.push('\n');
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, t).map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {}", path.display(), e))
    }

    pub fn log_path(doc: &Path) -> PathBuf {
        let id = doc_id_of(doc);
        doc.with_file_name(format!("{}.kerf.log.jsonl", id))
    }

    /// Append one op-log entry (`who`, `why`, `ops`, `changed`, UTC timestamp).
    pub fn append_log(doc: &Path, why: &str, ops: &Value, changed: &Value) -> Result<(), String> {
        use std::io::Write;
        let entry = json!({"ts": crate::text::now_iso(), "who": "CLAUDE", "why": why, "ops": ops, "changed": changed});
        let p = Self::log_path(doc);
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| format!("cannot open {}: {}", p.display(), e))?;
        writeln!(f, "{}", kerf_core::json::compact(&entry)).map_err(|e| format!("cannot write {}: {}", p.display(), e))
    }

    /// Resolve an export destination; it must stay inside the workspace dir.
    pub fn export_path(&self, requested: Option<&str>, default_name: &str) -> Result<PathBuf, String> {
        let p = match requested {
            Some(r) if !r.trim().is_empty() => {
                let rp = PathBuf::from(r.trim());
                if rp.is_absolute() { normalize(&rp) } else { normalize(&self.dir.join(rp)) }
            }
            _ => self.dir.join("exports").join(default_name),
        };
        if !p.starts_with(&self.dir) {
            return Err(format!("export path {} is outside the workspace {}. Use a relative path such as \"exports/{}\".", p.display(), self.dir.display(), default_name));
        }
        if p.is_dir() {
            return Err(format!("{} is a directory; give a file name such as {}", p.display(), p.join(default_name).display()));
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids() {
        assert!(valid_id("truss-bearing-cmu").is_ok());
        assert!(valid_id("a.b_c").is_ok());
        for bad in ["", "../x", "a/b", ".hidden", "a..b", "a b"] {
            assert!(valid_id(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn norm() {
        assert_eq!(normalize(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    }
}
