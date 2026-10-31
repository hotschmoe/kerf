//! Folder-of-documents helpers shared by the CLI and `kerf serve`: the op log (`<file>.log.jsonl`),
//! atomic writes, new-document text and file name rules.
//!
//! Timestamps are the only nondeterministic bytes in Kerf, and they live only in the log.

use kerf_core::json::{self, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sys;

pub const DOC_SUFFIX: &str = ".kerf.json";
pub const LOG_SUFFIX: &str = ".log.jsonl";

/// `2026-10-06T01:23:45Z` (UTC, second resolution).
pub fn iso_time() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    iso_from_seconds(secs)
}

pub fn iso_from_seconds(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[allow(dead_code)]
fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    if ["CON", "PRN", "AUX", "NUL"].iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return true;
    }
    let b = stem.as_bytes();
    b.len() == 4
        && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
        && (b'1'..=b'9').contains(&b[3])
}

/// A document file name the server will touch: a plain `*.kerf.json` base name, no separators or dots-only tricks,
/// and nothing Windows would treat specially (device names, a stem ending in a dot or space, `:` streams).
#[allow(dead_code)]
pub fn valid_doc_file(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() <= DOC_SUFFIX.len() || b.len() > 200 || !name.ends_with(DOC_SUFFIX) {
        return false;
    }
    if b[0] == b'.' || b[0] == b' ' {
        return false;
    }
    let stem_last = b[b.len() - DOC_SUFFIX.len() - 1];
    if stem_last == b'.' || stem_last == b' ' {
        return false;
    }
    let plain = b.iter().all(|c| c.is_ascii_alphanumeric() || b"._- ()".contains(c));
    plain && !is_windows_device_name(name)
}

/// `<file>.log.jsonl` for a document path (the log sits next to the document).
pub fn log_path(doc_path: &str) -> String {
    format!("{doc_path}{LOG_SUFFIX}")
}

fn sync_dir_of(path: &Path) {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Replace `path` atomically (temp file + rename) so readers never see a half-written document. The data is
/// `fsync`ed before the rename and the directory after it.
pub fn write_file_atomic(path: &str, bytes: &[u8]) -> io::Result<()> {
    let p = Path::new(path);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("kerf");
    let tmp =
        p.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let res = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, p)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
        return res;
    }
    sync_dir_of(p);
    Ok(())
}

/// Exclusive advisory lock (`flock`) on `<doc>.log.jsonl`, held across a whole read-modify-write of one document so
/// that two processes cannot interleave and lose each other's edit, and through which the log line is appended.
/// Best effort when the file system has no locks. Dropping the value closes the file and releases the lock.
pub struct DocLock {
    file: File,
}

impl DocLock {
    pub fn acquire(log_path: &str) -> io::Result<DocLock> {
        refuse_symlink(log_path)?;
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(log_path)?;
        crate::sys::lock_exclusive(&file)?;
        Ok(DocLock { file })
    }

    /// Append one line (a newline is added) through the locked handle and `fsync` it.
    pub fn append_line(&mut self, line: &[u8]) -> io::Result<()> {
        self.file.seek(SeekFrom::End(0))?;
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line);
        buf.push(b'\n');
        self.file.write_all(&buf)?;
        self.file.sync_all()
    }
}

/// A plain file or nothing: a planted symlink at `path` must not turn a log append into a write elsewhere.
fn refuse_symlink(path: &str) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(io::Error::other("SymLink")),
        _ => Ok(()),
    }
}

/// Append one line to `path`, creating it when missing, under the exclusive lock. Refuses a symlink at `path`.
pub fn append_line(path: &str, line: &[u8]) -> io::Result<()> {
    DocLock::acquire(path)?.append_line(line)
}

pub struct LogMeta<'a> {
    pub who: &'a str,
    pub tool: &'a str,
    pub why: &'a str,
}

/// One compact JSON log line (no trailing newline). `ops` is JSON text (array, or the `{ops:[...]}` wrapper
/// the engine also accepts); `changed` are the ids the engine reported; `summary` is the engine summary text.
pub fn build_entry(meta: &LogMeta, ops_text: &str, changed: &[String], summary: &str) -> Vec<u8> {
    let ops_v = match json::parse(ops_text.as_bytes()) {
        Ok(v) => match v.get("ops") {
            Some(inner) => inner.clone(),
            None => v,
        },
        Err(_) => Value::Array(Vec::new()),
    };
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"ts\":");
    json::write_string(&mut out, &iso_time());
    out.extend_from_slice(b",\"who\":");
    json::write_string(&mut out, meta.who);
    out.extend_from_slice(b",\"tool\":");
    json::write_string(&mut out, meta.tool);
    out.extend_from_slice(b",\"why\":");
    json::write_string(&mut out, meta.why);
    out.extend_from_slice(b",\"ops\":");
    json::write_compact(&mut out, &ops_v);
    out.extend_from_slice(b",\"changed\":[");
    for (i, c) in changed.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        json::write_string(&mut out, c);
    }
    out.extend_from_slice(b"],\"summary_head\":");
    let head = summary.split('\n').next().unwrap_or("");
    json::write_string(&mut out, head.trim_end_matches('\r'));
    let mut ack_n = 0;
    let mut in_block = false;
    for ln in summary.split('\n') {
        let l = ln.trim_end_matches('\r');
        if l.starts_with("ACKNOWLEDGED ") {
            in_block = true;
            continue;
        }
        if !in_block || !l.starts_with(' ') {
            in_block = false;
            continue;
        }
        out.extend_from_slice(if ack_n == 0 { b",\"ack\":[" } else { b"," });
        json::write_string(&mut out, l.trim_start_matches(' '));
        ack_n += 1;
    }
    if ack_n > 0 {
        out.push(b']');
    }
    out.push(b'}');
    out
}

/// Canonical text of an empty document, like `kerf new` makes. `id_src` becomes the id (lowercased,
/// non-alphanumerics to `-`).
pub fn new_doc_text(id_src: &str, title: &str) -> Option<Vec<u8>> {
    let id = slug(id_src);
    let mut doc = Vec::new();
    doc.extend_from_slice(b"{\"kerf\":\"0.1\",\"id\":");
    json::write_string(&mut doc, &id);
    doc.extend_from_slice(b",\"title\":");
    json::write_string(&mut doc, title);
    doc.extend_from_slice(b",\"meta\":{\"jurisdiction\":{\"code\":\"IRC\",\"edition\":2021}},\"run\":[-24,24],\"components\":[],\"views\":[]}");
    let v = json::parse(&doc).ok()?;
    Some(kerf_core::canon::write(&v))
}

/// Lowercased with every non-alphanumeric byte replaced by `-`.
pub fn slug(src: &str) -> String {
    src.bytes().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() as char } else { '-' }).collect()
}

/// Base name without `.kerf.json` (or without any extension for other names).
pub fn doc_stem(file: &str) -> &str {
    let base = Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or(file);
    if let Some(s) = base.strip_suffix(DOC_SUFFIX) {
        return s;
    }
    base.split('.').next().unwrap_or(base)
}

/// Summary text of the document (`check`), or "" when the engine rejects it.
pub fn check_summary(doc_text: &[u8], style: Option<&Path>) -> String {
    let mut input = b"{\"doc\":".to_vec();
    input.extend_from_slice(trim_ws(doc_text));
    input.extend(style_member(style));
    input.push(b'}');
    let r = kerf_core::api::call(b"check", &input);
    if !r.ok {
        return String::new();
    }
    let Ok(v) = json::parse(&r.bytes) else { return String::new() };
    v.get("summary").and_then(Value::str).unwrap_or("").to_string()
}

/// Why a `meta.references` path does not name a file the server may read (the `reference_files` status names).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefMiss {
    Missing,
    Outside,
}

impl RefMiss {
    pub fn name(self) -> &'static str {
        match self {
            RefMiss::Missing => "missing",
            RefMiss::Outside => "outside",
        }
    }
}

/// The regular file a reference path names inside `root` (a canonical directory, `sys::plain_path`ed). Symbolic links are followed, but
/// the resolved file must still lie inside `root`. `rel` must already be free of `references::path_problem`s.
pub fn resolve_reference(root: &Path, rel: &str) -> Result<PathBuf, RefMiss> {
    let canon = fs::canonicalize(root.join(rel)).map(sys::plain_path).map_err(|_| RefMiss::Missing)?;
    if !canon.starts_with(root) {
        return Err(RefMiss::Outside);
    }
    if !fs::metadata(&canon).is_ok_and(|m| m.is_file()) {
        return Err(RefMiss::Missing);
    }
    Ok(canon)
}

/// The `"reference_files":{...}` member of a `check` input: every looked-up reference path of `doc` that does not
/// resolve inside `folder`. Empty when the document has no reference to look up.
pub fn reference_files_member(doc: &Value, folder: &Path) -> String {
    let lookups = kerf_core::references::lookups(doc);
    if lookups.is_empty() {
        return String::new();
    }
    let root = fs::canonicalize(folder).map(sys::plain_path);
    let mut out = b"\"reference_files\":{".to_vec();
    let mut first = true;
    for p in lookups {
        let miss = match &root {
            Ok(r) => resolve_reference(r, p).err(),
            Err(_) => Some(RefMiss::Missing),
        };
        let Some(miss) = miss else { continue };
        if !first {
            out.push(b',');
        }
        first = false;
        json::write_string(&mut out, p);
        out.push(b':');
        json::write_string(&mut out, miss.name());
    }
    out.push(b'}');
    String::from_utf8_lossy(&out).into_owned()
}

/// The company style `kerf` applies without `--style` (SPEC 25.6).
pub const WORKSPACE_STYLE: &str = "kerf.kerfstyle.json";

/// The nearest `kerf.kerfstyle.json` in `dir` or one of its ancestors (absolute path).
pub fn find_style(dir: &Path) -> Option<PathBuf> {
    let start =
        fs::canonicalize(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }).map(sys::plain_path).ok()?;
    start.ancestors().map(|d| d.join(WORKSPACE_STYLE)).find(|f| f.is_file())
}

/// The workspace style of the document at `doc_path` (its folder or an ancestor).
pub fn style_for_doc(doc_path: &str) -> Option<PathBuf> {
    find_style(Path::new(doc_path).parent().unwrap_or(Path::new(".")))
}

/// `,"style":<file>` for an engine input, or "" when there is no workspace style or it cannot be read.
pub fn style_member(style: Option<&Path>) -> Vec<u8> {
    let Some(text) = style.and_then(|p| fs::read(p).ok()) else { return Vec::new() };
    let mut out = b",\"style\":".to_vec();
    out.extend_from_slice(trim_ws(&text));
    out
}

/// What `kerf init` writes as `kerf.kerfstyle.json`: the company style with the abbreviation rules at their defaults.
pub const STARTER_STYLE: &str = r#"{
  "kerfstyle": "0.1",
  "id": "company",
  "notes": {
    "abbreviations": {
      "extends": "us-structural",
      "rules": { "period_on_shortened": true, "dotted_initials": true, "bare_acronyms": true, "variants": true },
      "approved": {},
      "variants": {},
      "remove": [],
      "always": []
    }
  }
}
"#;

pub fn trim_ws(b: &[u8]) -> &[u8] {
    let is = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let start = b.iter().position(|c| !is(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !is(c)).map_or(start, |i| i + 1);
    &b[start..end]
}
