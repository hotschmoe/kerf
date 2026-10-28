//! `POST /api/agent/run` and `POST /api/agent/stop`: validate the request, save attached images into
//! `.kerf/attachments`, start or stop a run of the agent bridge.

use super::{ATTACHMENT_TTL_S, Server, bad_request, parse_body};
use crate::agents::{StartError, StartParams, StopResult};
use crate::http::{self, Out, Request};
use crate::workspace;
use kerf_core::json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_MESSAGE_BYTES: usize = 100_000;
const MAX_IMAGES: usize = 8;
const MAX_IMAGE_BYTES: usize = 12 << 20;

struct BadImages;

pub fn api_agent_run(s: &Server, req: &Request, w: &mut Out, extra: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let Some(body) = parse_body(req) else {
        return bad_request(req, w, extra, "E_JSON", "body must be JSON: { agent, message, session_id?, file? }");
    };
    let Some(agent_id) = body.get("agent").and_then(Value::str) else {
        return bad_request(
            req,
            w,
            extra,
            "E_INPUT",
            "missing \"agent\" (claude, grok, codex, or an id from .kerf/agents.json)",
        );
    };
    let Some(message) = body.get("message").and_then(Value::str) else {
        return bad_request(req, w, extra, "E_INPUT", "missing \"message\"");
    };
    if message.trim_ascii().is_empty() || message.len() > MAX_MESSAGE_BYTES {
        return bad_request(
            req,
            w,
            extra,
            "E_INPUT",
            "\"message\" must be 1..100000 bytes (it is one command-line argument; the OS limit is 128 KiB)",
        );
    }
    let mut session = None;
    if let Some(sid) = body.get("session_id").and_then(Value::str).filter(|s| !s.is_empty()) {
        let ok = sid.len() <= 200
            && !sid.starts_with('-')
            && sid.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c));
        if !ok {
            return bad_request(req, w, extra, "E_INPUT", "invalid session_id");
        }
        session = Some(sid);
    }
    let mut file = None;
    if let Some(f) = body.get("file").and_then(Value::str).filter(|f| !f.is_empty()) {
        if !workspace::valid_doc_file(f) {
            return bad_request(req, w, extra, "E_FILE", "file must be a plain NAME.kerf.json");
        }
        file = Some(f);
    }
    let mut message_full = message.to_owned();
    if let Some(iv) = body.get("images").filter(|v| !v.is_null()) {
        let Ok(paths) = save_images(s, iv) else {
            return bad_request(
                req,
                w,
                extra,
                "E_IMAGES",
                "images must be [{ name, data_base64 }] (png/jpg/gif/webp, at most 8, 12 MB each)",
            );
        };
        if !paths.is_empty() {
            message_full.push_str("\n\nThe designer attached these images (open them with your file-reading tool):\n");
            for p in paths {
                message_full.push_str(&format!("- {p}\n"));
            }
        }
    }
    let templates = s.agent_mgr.templates();
    let Some(t) = templates.iter().find(|t| t.id == agent_id) else {
        http::send_error(
            w,
            404,
            ka,
            extra,
            "E_AGENT",
            &format!("unknown agent '{agent_id}'; GET /api/info lists them"),
        )?;
        return Ok(ka);
    };
    match s.agent_mgr.start(StartParams { template: t, message: &message_full, session_id: session, file }) {
        Ok(run_id) => {
            let mut out = b"{\"run_id\":".to_vec();
            out.extend(http::json_string(&run_id));
            out.extend_from_slice(b"}\n");
            http::send_json(w, 200, ka, extra, &out)?;
        }
        Err(StartError::Busy(id)) => http::send_error(
            w,
            409,
            ka,
            extra,
            "E_BUSY",
            &format!("run {id} is still active (one run at a time); stop it first"),
        )?,
        Err(StartError::TooLong(why)) => http::send_error(w, 400, ka, extra, "E_INPUT", &why)?,
        Err(StartError::Unavailable(why)) => {
            let code = if t.untrusted { "E_UNTRUSTED" } else { "E_UNAVAILABLE" };
            http::send_error(w, 409, ka, extra, code, &format!("agent '{agent_id}' is not available: {why}"))?
        }
        Err(StartError::SpawnFailed(why)) => http::send_error(w, 500, ka, extra, "E_SPAWN", &why)?,
    }
    Ok(ka)
}

/// Creates `path` under the served folder when missing and insists it is a real directory, not a symlink.
fn ensure_real_dir(root: &Path, path: &str) -> Result<(), BadImages> {
    let full = root.join(path);
    match fs::create_dir(&full) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(BadImages),
    }
    match fs::symlink_metadata(&full) {
        Ok(m) if m.is_dir() => Ok(()),
        _ => Err(BadImages),
    }
}

pub fn cleanup_attachments(s: &Server) {
    let Ok(rd) = fs::read_dir(s.dir_abs.join(".kerf/attachments")) else { return };
    let now = SystemTime::now();
    for e in rd.flatten() {
        let Ok(m) = fs::symlink_metadata(e.path()) else { continue };
        if !m.is_file() {
            continue;
        }
        let old =
            m.modified().ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|d| d.as_secs() > ATTACHMENT_TTL_S);
        if old {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn random_tag() -> u32 {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() ^ (d.as_secs() as u32));
    let mut x =
        nanos ^ std::process::id().rotate_left(16) ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^ (x >> 13)
}

fn save_images(s: &Server, iv: &Value) -> Result<Vec<String>, BadImages> {
    let items = iv.arr().ok_or(BadImages)?;
    if items.len() > MAX_IMAGES {
        return Err(BadImages);
    }
    let mut out = Vec::new();
    if items.is_empty() {
        return Ok(out);
    }
    ensure_real_dir(&s.dir_abs, ".kerf")?;
    ensure_real_dir(&s.dir_abs, ".kerf/attachments")?;
    cleanup_attachments(s);
    for it in items {
        let name_in = it.get("name").and_then(Value::str).ok_or(BadImages)?;
        let mut data = it.get("data_base64").and_then(Value::str).ok_or(BadImages)?;
        if data.starts_with("data:") {
            data = &data[data.find(',').ok_or(BadImages)? + 1..];
        }
        let base = name_in.rsplit(['/', '\\']).next().unwrap_or("");
        let mut safe: String = base
            .chars()
            .take(60)
            .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
            .collect();
        let ext = safe.rfind('.').map_or("", |i| &safe[i..]);
        if ![".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| ext.eq_ignore_ascii_case(e)) {
            safe.push_str(".png");
        }
        let bytes = base64_decode(data).ok_or(BadImages)?;
        let rel = format!(".kerf/attachments/{:08x}-{safe}", random_tag());
        let full = s.dir_abs.join(&rel);
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&full).map_err(|_| BadImages)?;
        f.write_all(&bytes).map_err(|_| BadImages)?;
        out.push(full.to_string_lossy().into_owned());
    }
    Ok(out)
}

/// Standard base64 with padding, as `std.base64.standard.Decoder`: no whitespace, trailing bits must be zero.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 4 != 0 {
        return None;
    }
    let pad = b.iter().rev().take_while(|&&c| c == b'=').count();
    if pad > 2 || b.len() / 4 * 3 - pad > MAX_IMAGE_BYTES {
        return None;
    }
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let body = &b[..b.len() - pad];
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 3);
    for chunk in body.chunks(4) {
        let mut acc = 0u32;
        for &c in chunk {
            acc = acc << 6 | val(c)?;
        }
        let n = chunk.len();
        acc <<= 6 * (4 - n) as u32;
        let bytes = acc.to_be_bytes();
        let produced = n * 6 / 8;
        if bytes[1 + produced..4].iter().any(|&x| x != 0) {
            return None;
        }
        out.extend_from_slice(&bytes[1..1 + produced]);
    }
    Some(out)
}

pub fn api_agent_stop(s: &Server, req: &Request, w: &mut Out, extra: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let Some(body) = parse_body(req) else {
        return bad_request(req, w, extra, "E_JSON", "body must be JSON: { run_id }");
    };
    let Some(run_id) = body.get("run_id").and_then(Value::str) else {
        return bad_request(req, w, extra, "E_INPUT", "missing \"run_id\"");
    };
    let text: &[u8] = match s.agent_mgr.stop(run_id) {
        StopResult::Stopped => b"{\"stopped\":true}\n",
        StopResult::AlreadyFinished => b"{\"stopped\":false,\"reason\":\"already finished\"}\n",
        StopResult::NoSuchRun => {
            http::send_error(w, 404, ka, extra, "E_NOT_FOUND", "no such active run")?;
            return Ok(ka);
        }
    };
    http::send_json(w, 200, ka, extra, text)?;
    Ok(ka)
}
