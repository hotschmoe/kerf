//! `kerf serve`: local workspace server (spec/SERVE.md). One process serves the embedded web UI and a JSON/SSE API
//! over a folder of `*.kerf.json` documents. The folder is the source of truth: a 500 ms poller (mtime + size) turns
//! CLI/agent edits into SSE events, designer edits go through `apply` with optimistic concurrency (ETag / if_match)
//! and are logged like CLI edits.
//!
//! Every connection is a thread, so long-lived SSE streams never block other requests. This file holds the `Server`
//! (the folder scan, the edit log, the ETag rules); the rest is in `serve/`: `cli` (arguments, bind, startup), `conn`
//! (limits, deadlines, access rules), `routes` (route table, body caps, dispatcher), the handlers `docs`, `agent` and
//! `stream`, and `ui` (static files and the CSP).

pub mod agent;
mod cli;
mod conn;
mod docs;
mod routes;
mod stream;
mod ui;

pub use cli::cli_main;

use crate::agents::{self, Manager};
use crate::events::Hub;
use crate::hash;
use crate::http::{self, Out, Request};
use crate::proxy;
use crate::workspace;
use conn::{ConnState, Timeouts};
use kerf_core::json::{self, Value};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, MutexGuard};
use std::time::{Instant, UNIX_EPOCH};

pub const DEFAULT_PORT: u16 = 7700;
pub const POLL_MS: u64 = 500;
pub const ATTACHMENT_TTL_S: u64 = 24 * 3600;
pub const MAX_SSE_STREAMS: u32 = conn::MAX_SSE_STREAMS;
pub const MAX_PROXY_CALLS: u32 = conn::MAX_PROXY_CALLS;

pub struct Config {
    pub dir_path: String,
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    pub allow_origin: Option<String>,
    pub open: bool,
    pub trust_agents: bool,
    pub timeouts: Timeouts,
    pub llm_limits: proxy::Limits,
    pub agent_timeout_s: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dir_path: ".".into(),
            host: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            token: None,
            allow_origin: None,
            open: false,
            trust_agents: false,
            timeouts: Timeouts::default(),
            llm_limits: proxy::Limits::default(),
            agent_timeout_s: agents::DEFAULT_MAX_RUN_S,
        }
    }
}

#[derive(Clone, Default)]
pub struct DocInfo {
    pub id: String,
    pub title: String,
    pub components: usize,
    pub views: usize,
    pub errors: usize,
    pub warnings: usize,
    pub hash: u64,
}

pub struct FileState {
    pub name: String,
    pub mtime_ns: i128,
    pub size: u64,
    pub log_size: u64,
    pub info: Option<DocInfo>,
}

#[derive(Default)]
pub struct Scan {
    pub files: Vec<FileState>,
}

impl Scan {
    fn find(&mut self, name: &str) -> Option<&mut FileState> {
        self.files.iter_mut().find(|f| f.name == name)
    }
}

pub struct Server {
    pub cfg: Config,
    pub dir_abs: PathBuf,
    pub hub: std::sync::Arc<Hub>,
    pub scan: Mutex<Scan>,
    pub write_mu: Mutex<()>,
    pub agent_mgr: std::sync::Arc<Manager>,
    pub loopback_only: bool,
    pub started: Instant,
    pub conns: Mutex<Vec<std::sync::Arc<ConnState>>>,
    pub active_conns: AtomicU32,
    pub sse_active: AtomicU32,
    pub proxy_active: AtomicU32,
    pub ui_csp: String,
}

pub struct DocRead {
    pub bytes: Vec<u8>,
    pub mtime_ns: i128,
    pub size: u64,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn mtime_ns(meta: &fs::Metadata) -> i128 {
    match meta.modified().ok().map(|t| t.duration_since(UNIX_EPOCH)) {
        Some(Ok(d)) => d.as_nanos() as i128,
        Some(Err(e)) => -(e.duration().as_nanos() as i128),
        None => 0,
    }
}

fn is_regular(meta: &fs::Metadata) -> bool {
    meta.file_type().is_file()
}

pub fn is_missing(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::IsADirectory | io::ErrorKind::NotADirectory)
}

fn json_obj_head(name: &str) -> Vec<u8> {
    let mut out = b"{\"file\":".to_vec();
    out.extend(http::json_string(name));
    out
}

impl Server {
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir_abs.join(name)
    }

    pub fn path_str(&self, name: &str) -> String {
        self.path(name).to_string_lossy().into_owned()
    }

    pub fn cors(&self, req: &Request) -> String {
        let (Some(ao), Some(origin)) = (self.cfg.allow_origin.as_deref(), req.header("origin")) else {
            return String::new();
        };
        if origin != ao {
            return String::new();
        }
        format!(
            "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: authorization, content-type, if-match\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Expose-Headers: etag\r\n"
        )
    }

    pub fn scan(&self, emit: bool) {
        let mut st = lock(&self.scan);
        self.scan_locked(&mut st, emit);
    }

    pub fn scan_locked(&self, st: &mut Scan, emit: bool) {
        let mut names = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.dir_abs) {
            for e in rd.flatten() {
                // A symlink is never served (V-10): `file_type` does not follow it.
                if !e.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                let Ok(name) = e.file_name().into_string() else { continue };
                if workspace::valid_doc_file(&name) {
                    names.push(name);
                }
            }
        }
        names.sort();
        let mut added = Vec::new();
        let mut changed = Vec::new();
        for name in &names {
            let Ok(meta) = fs::symlink_metadata(self.path(name)) else { continue };
            if !is_regular(&meta) {
                continue;
            }
            let (m, size) = (mtime_ns(&meta), meta.len());
            if let Some(f) = st.find(name) {
                if f.mtime_ns != m || f.size != size {
                    f.mtime_ns = m;
                    f.size = size;
                    f.info = None;
                    changed.push(name.clone());
                }
            } else {
                let log_size = if emit { 0 } else { self.log_size(name) };
                st.files.push(FileState { name: name.clone(), mtime_ns: m, size, log_size, info: None });
                added.push(name.clone());
            }
        }
        let mut removed = Vec::new();
        st.files.retain(|f| {
            let present = names.contains(&f.name);
            if !present {
                removed.push(f.name.clone());
            }
            present
        });
        let mut logs: Vec<(String, Vec<String>)> = Vec::new();
        for f in &mut st.files {
            let lines = self.read_new_log(f);
            if !lines.is_empty() {
                logs.push((f.name.clone(), lines));
            }
        }
        if !emit {
            return;
        }
        for n in &added {
            self.publish_doc(st, "doc_added", n, None);
        }
        for n in &changed {
            let who = logs.iter().find(|l| &l.0 == n).and_then(|l| l.1.last()).and_then(|line| who_of(line));
            self.publish_doc(st, "doc_changed", n, who.as_deref());
        }
        for n in &removed {
            let mut out = json_obj_head(n);
            out.push(b'}');
            self.hub.publish("doc_removed", &String::from_utf8_lossy(&out));
        }
        for (name, lines) in &logs {
            for line in lines {
                self.publish_log(name, line);
            }
        }
    }

    fn publish_doc(&self, st: &mut Scan, kind: &str, name: &str, who: Option<&str>) {
        let Some(f) = st.find(name) else { return };
        let mut out = json_obj_head(name);
        out.extend_from_slice(format!(",\"mtime_ms\":{}", f.mtime_ns / 1_000_000).as_bytes());
        if let Some(w) = who {
            out.extend_from_slice(b",\"who\":");
            out.extend(http::json_string(w));
        }
        out.push(b'}');
        self.hub.publish(kind, &String::from_utf8_lossy(&out));
    }

    fn publish_log(&self, name: &str, line: &str) {
        let mut out = json_obj_head(name);
        out.extend_from_slice(b",\"entry\":");
        out.extend_from_slice(line.as_bytes());
        out.push(b'}');
        self.hub.publish("log", &String::from_utf8_lossy(&out));
    }

    fn log_size(&self, name: &str) -> u64 {
        fs::symlink_metadata(self.path(&workspace::log_path(name))).map_or(0, |m| m.len())
    }

    /// New complete, valid log lines of a document since the last scan (at most the newest 200).
    fn read_new_log(&self, f: &mut FileState) -> Vec<String> {
        const WINDOW: u64 = 4 << 20;
        let lp = self.path(&workspace::log_path(&f.name));
        let Ok(meta) = fs::symlink_metadata(&lp) else {
            f.log_size = 0;
            return Vec::new();
        };
        if !is_regular(&meta) {
            return Vec::new();
        }
        if meta.len() < f.log_size {
            f.log_size = meta.len();
            return Vec::new();
        }
        if meta.len() == f.log_size {
            return Vec::new();
        }
        let want = (meta.len() - f.log_size).min(WINDOW) as usize;
        let read = || -> io::Result<Vec<u8>> {
            use std::io::{Seek, SeekFrom};
            let mut file = fs::File::open(&lp)?;
            file.seek(SeekFrom::Start(f.log_size))?;
            let mut buf = Vec::new();
            file.take(want as u64).read_to_end(&mut buf)?;
            Ok(buf)
        };
        let Ok(data) = read() else { return Vec::new() };
        let Some(last_nl) = data.iter().rposition(|&c| c == b'\n') else {
            // A line longer than the read window would be re-read every tick forever: skip it.
            if data.len() as u64 >= WINDOW {
                f.log_size += data.len() as u64;
            }
            return Vec::new();
        };
        f.log_size += last_nl as u64 + 1;
        let mut out: Vec<String> = data[..last_nl]
            .split(|&c| c == b'\n')
            .map(<[u8]>::trim_ascii)
            .filter(|l| !l.is_empty() && json::parse(l).is_ok())
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect();
        if out.len() > 200 {
            out.drain(..out.len() - 200);
        }
        out
    }

    pub fn store_info(&self, name: &str, mtime_ns: i128, size: u64, info: DocInfo) {
        let mut st = lock(&self.scan);
        if let Some(f) = st.find(name) {
            if f.info.is_none() && f.mtime_ns == mtime_ns && f.size == size {
                f.info = Some(info);
            }
        }
    }

    pub fn read_doc(&self, name: &str) -> io::Result<DocRead> {
        let path = self.path(name);
        // A symlink planted in the folder (an agent can create one) is not a document (V-10).
        let meta = fs::symlink_metadata(&path)?;
        if !is_regular(&meta) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let f = fs::File::open(&path)?;
        let meta = f.metadata()?;
        let mut bytes = Vec::new();
        f.take(256 << 20).read_to_end(&mut bytes)?;
        Ok(DocRead { bytes, mtime_ns: mtime_ns(&meta), size: meta.len() })
    }

    pub fn warn_log(&self, lp: &str, e: &io::Error) {
        eprintln!("kerf serve: warning: could not append to op log {lp}: {e} (the document was written)");
    }

    /// Record our own write of `name`: refresh its state, publish doc_added/doc_changed and the log line.
    pub fn note_write_locked(&self, st: &mut Scan, name: &str, is_new: bool, who: &str, log_line: Option<&[u8]>) {
        let Ok(meta) = fs::metadata(self.path(name)) else { return };
        let (m, size) = (mtime_ns(&meta), meta.len());
        if let Some(f) = st.find(name) {
            f.mtime_ns = m;
            f.size = size;
            f.info = None;
        } else {
            st.files.push(FileState { name: name.to_string(), mtime_ns: m, size, log_size: 0, info: None });
        }
        let log_size = log_line.map(|_| self.log_size(name));
        if let (Some(ls), Some(f)) = (log_size, st.find(name)) {
            f.log_size = ls;
        }
        self.publish_doc(st, if is_new { "doc_added" } else { "doc_changed" }, name, Some(who));
        if let Some(line) = log_line {
            self.publish_log(name, &String::from_utf8_lossy(line));
        }
    }
}

fn who_of(line: &str) -> Option<String> {
    json::parse(line.as_bytes()).ok()?.get("who")?.str().map(str::to_string)
}

pub fn etag_of(mtime_ns: i128, size: u64, bytes: &[u8]) -> String {
    format!("\"{}-{}-{:016x}\"", mtime_ns / 1_000_000, size, hash::wyhash(0, bytes))
}

pub fn compute_info(text: &[u8]) -> DocInfo {
    let hash = hash::wyhash(0, text);
    let Ok(parsed) = json::parse(text) else {
        return DocInfo { title: "(invalid JSON)".into(), errors: 1, hash, ..DocInfo::default() };
    };
    let text_of = |k: &str| parsed.get(k).and_then(Value::str).unwrap_or("").to_string();
    let count = |k: &str| parsed.get(k).and_then(Value::arr).map_or(0, <[Value]>::len);
    let (mut errors, mut warnings) = (0, 0);
    let mut input = b"{\"doc\":".to_vec();
    input.extend_from_slice(text.trim_ascii());
    input.push(b'}');
    let r = kerf_core::api::call(b"check", &input);
    if r.ok {
        if let Some(items) = json::parse(&r.bytes)
            .ok()
            .and_then(|res| res.get("diagnostics").and_then(Value::arr).map(<[Value]>::to_vec))
        {
            for d in &items {
                match d.get("level").and_then(Value::str) {
                    Some("error") => errors += 1,
                    Some("warning") => warnings += 1,
                    _ => {}
                }
            }
        }
    } else {
        errors += 1;
    }
    DocInfo {
        id: text_of("id"),
        title: text_of("title"),
        components: count("components"),
        views: count("views"),
        errors,
        warnings,
        hash,
    }
}

pub const SERVE_USAGE: &str = "\
usage: kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --no-token] [--allow-origin URL]
  --dir DIR         folder of *.kerf.json documents (default: current directory)
  --host HOST       bind address; 0.0.0.0 serves the LAN and then requires a token
  --port N          default 7700 (0 = pick a free port)
  --open            open the UI in the default browser
  --token T         require this token (Authorization: Bearer T, or ?token=T); visible in `ps`, prefer:
  --token-file F    read the token from file F (or set KERF_TOKEN); the default is a random token, printed in the URL
  --no-token        no token at all (trusted machine and network only; localhost then also checks Host/Origin)
  --llm-timeout S   longest pause of a proxied LLM call (first byte and between chunks; default 300 s / 120 s)
  --timeouts-ms I,H,W  tuning/testing: keep-alive idle, request head and streaming write deadlines (30000,10000,20000)
  --allow-origin U  also accept browser requests from origin U (dev server, e.g. http://localhost:5173)
  --trust-agents    run agents defined in <dir>/.kerf/agents.json (they execute commands from the folder; without
                    this flag, or a yes at the interactive prompt, they are listed as untrusted and never started)
  --agent-timeout S wall-clock limit of one agent run in seconds (default 1800, 0 = none)
";

pub fn bad_request(req: &Request, w: &mut Out, extra: &str, code: &str, msg: &str) -> io::Result<bool> {
    http::send_error(w, 400, req.keep_alive, extra, code, msg)?;
    Ok(req.keep_alive)
}

pub fn parse_body(req: &Request) -> Option<Value> {
    json::parse(&req.body).ok()
}
