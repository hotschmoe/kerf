//! Document endpoints: info, list, get, create, apply, log, export.

use super::{DocInfo, Server, bad_request, compute_info, etag_of, is_missing, lock, mtime_ns, parse_body};
use crate::agents;
use crate::http::{self, Head, Out, Request};
use crate::workspace::{self, DocLock, LogMeta};
use kerf_core::json::{self, Value};
use kerf_core::{api, canon, model, ops};
use std::fs;
use std::io::{self, Read, Write};

fn not_found(w: &mut Out, ka: bool, extra: &str, file: &str) -> io::Result<bool> {
    http::send_error(w, 404, ka, extra, "E_NOT_FOUND", &format!("{file} does not exist in the served folder"))?;
    Ok(ka)
}

pub fn api_info(s: &Server, req: &Request, w: &mut Out, extra: &str) -> io::Result<bool> {
    let mut out = format!("{{\"version\":\"{}\",\"dir\":", api::VERSION).into_bytes();
    out.extend(http::json_string(&s.dir_abs.to_string_lossy()));
    out.extend_from_slice(
        format!(",\"token_required\":{},\"authenticated\":true,\"proxy\":true,\"agents\":[", s.cfg.token.is_some())
            .as_bytes(),
    );
    for (i, t) in agents::load_templates(&s.dir_abs, s.cfg.trust_agents).iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        let d = s.agent_mgr.detect_cached(t);
        out.extend_from_slice(b"{\"id\":");
        out.extend(http::json_string(&t.id));
        out.extend_from_slice(b",\"name\":");
        out.extend(http::json_string(&t.name));
        let source = if t.from_workspace { "workspace" } else { "builtin" };
        out.extend_from_slice(format!(",\"available\":{},\"source\":\"{source}\"", d.available).as_bytes());
        if t.untrusted {
            out.extend_from_slice(b",\"untrusted\":true");
        }
        if let Some(v) = &d.version {
            out.extend_from_slice(b",\"version\":");
            out.extend(http::json_string(v));
        }
        if let Some(r) = &d.reason {
            out.extend_from_slice(b",\"reason\":");
            out.extend(http::json_string(r));
        }
        out.push(b'}');
    }
    out.extend_from_slice(b"],\"active_run\":");
    match s.agent_mgr.active_info() {
        Some((run_id, agent)) => {
            out.extend_from_slice(b"{\"run_id\":");
            out.extend(http::json_string(&run_id));
            out.extend_from_slice(b",\"agent\":");
            out.extend(http::json_string(&agent));
            out.push(b'}');
        }
        None => out.extend_from_slice(b"null"),
    }
    out.extend_from_slice(b"}\n");
    http::send_json(w, 200, req.keep_alive, extra, &out)?;
    Ok(req.keep_alive)
}

struct Row {
    name: String,
    mtime_ns: i128,
    size: u64,
    info: Option<DocInfo>,
}

pub fn api_list(s: &Server, req: &Request, w: &mut Out, extra: &str) -> io::Result<bool> {
    let mut rows: Vec<Row> = {
        let mut st = lock(&s.scan);
        s.scan_locked(&mut st, true);
        st.files
            .iter()
            .map(|f| Row { name: f.name.clone(), mtime_ns: f.mtime_ns, size: f.size, info: f.info.clone() })
            .collect()
    };
    let mut out = b"[".to_vec();
    let mut first = true;
    for r in &mut rows {
        if r.info.is_none() {
            let Ok(rd) = s.read_doc(&r.name) else { continue };
            let info = compute_info(&rd.bytes);
            r.mtime_ns = rd.mtime_ns;
            r.size = rd.size;
            s.store_info(&r.name, rd.mtime_ns, rd.size, info.clone());
            r.info = Some(info);
        }
        let Some(info) = &r.info else { continue };
        if !first {
            out.push(b',');
        }
        first = false;
        out.extend_from_slice(b"{\"file\":");
        out.extend(http::json_string(&r.name));
        out.extend_from_slice(b",\"id\":");
        out.extend(http::json_string(&info.id));
        out.extend_from_slice(b",\"title\":");
        out.extend(http::json_string(&info.title));
        let ms = r.mtime_ns / 1_000_000;
        out.extend_from_slice(
            format!(
                ",\"mtime_ms\":{ms},\"size\":{},\"etag\":\"\\\"{ms}-{}-{:016x}\\\"\",\"components\":{},\"views\":{},\"errors\":{},\"warnings\":{}}}",
                r.size, r.size, info.hash, info.components, info.views, info.errors, info.warnings
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(b"]\n");
    http::send_json(w, 200, req.keep_alive, extra, &out)?;
    Ok(req.keep_alive)
}

pub fn api_get_doc(s: &Server, req: &Request, w: &mut Out, extra: &str, file: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let rd = match s.read_doc(file) {
        Ok(rd) => rd,
        Err(e) if is_missing(&e) => return not_found(w, ka, extra, file),
        Err(e) => return Err(e),
    };
    let etag = etag_of(rd.mtime_ns, rd.size, &rd.bytes);
    let hdr = format!("ETag: {etag}\r\n{extra}");
    if req.header("if-none-match") == Some(etag.as_str()) {
        http::send(w, Head { status: 304, keep_alive: ka, extra: &hdr, ..Head::default() }, b"")?;
        return Ok(ka);
    }
    http::send_json(w, 200, ka, &hdr, &rd.bytes)?;
    Ok(ka)
}

pub fn api_create(s: &Server, req: &Request, w: &mut Out, extra: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let Some(body) = parse_body(req) else {
        return bad_request(req, w, extra, "E_JSON", "body must be JSON: { file, title?, id? }");
    };
    let Some(file_in) = body.get("file").and_then(Value::str) else {
        return bad_request(req, w, extra, "E_INPUT", "missing \"file\" (for example \"truss-cmu.kerf.json\")");
    };
    let file = if file_in.ends_with(workspace::DOC_SUFFIX) {
        file_in.to_string()
    } else {
        format!("{file_in}{}", workspace::DOC_SUFFIX)
    };
    if !workspace::valid_doc_file(&file) {
        return bad_request(
            req,
            w,
            extra,
            "E_FILE",
            "file must be a plain NAME.kerf.json: letters, digits, . _ - space ( ), no directories",
        );
    }
    let title = body.get("title").map_or("", |v| v.str().unwrap_or(""));
    let id_src = body.get("id").map_or(workspace::doc_stem(&file), |v| v.str().unwrap_or(workspace::doc_stem(&file)));
    let Some(text) = workspace::new_doc_text(id_src, title) else {
        return bad_request(req, w, extra, "E_INPUT", "could not build the document text");
    };
    let _write = lock(&s.write_mu);
    let mut st = lock(&s.scan);
    s.scan_locked(&mut st, true);
    let path = s.path(&file);
    let created = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut f| f.write_all(&text).and_then(|()| f.sync_all()));
    match created {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            http::send_error(w, 409, ka, extra, "E_EXISTS", &format!("{file} already exists"))?;
            return Ok(ka);
        }
        Err(e) => return Err(e),
    }
    let summary = workspace::check_summary(&text);
    let created_ops = format!("[{{\"op\":\"create\",\"file\":\"{file}\"}}]");
    let line = workspace::build_entry(
        &LogMeta { who: "designer", tool: "kerf-serve", why: "create" },
        &created_ops,
        &[],
        &summary,
    );
    let lp = s.path_str(&workspace::log_path(&file));
    if let Err(e) = workspace::append_line(&lp, &line) {
        s.warn_log(&lp, &e);
    }
    s.note_write_locked(&mut st, &file, true, "designer", Some(&line));
    let meta = fs::symlink_metadata(&path)?;
    let etag = etag_of(mtime_ns(&meta), meta.len(), &text);
    let mut out = b"{\"file\":".to_vec();
    out.extend(http::json_string(&file));
    out.extend_from_slice(b",\"etag\":");
    out.extend(http::json_string(&etag));
    out.extend_from_slice(b",\"summary\":");
    out.extend(http::json_string(&summary));
    out.extend_from_slice(b"}\n");
    let hdr = format!("ETag: {etag}\r\nLocation: /api/docs/{file}\r\n{extra}");
    http::send_json(w, 201, ka, &hdr, &out)?;
    Ok(ka)
}

pub fn api_apply(s: &Server, req: &Request, w: &mut Out, extra: &str, file: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let Some(body) = parse_body(req) else {
        return bad_request(req, w, extra, "E_JSON", "body must be JSON: { ops, why, actor, if_match? }");
    };
    let is_obj = matches!(body, Value::Object(_));
    let bare_ops = matches!(body, Value::Array(_)) || (is_obj && body.get("ops").is_none() && body.get("op").is_some());
    if !is_obj && !matches!(body, Value::Array(_)) {
        return bad_request(req, w, extra, "E_INPUT", "body must be a JSON object");
    }
    let ops_v = if bare_ops {
        &body
    } else {
        match body.get("ops") {
            Some(v) => v,
            None => return bad_request(req, w, extra, "E_INPUT", "missing \"ops\": an array of ops (see SPEC 14)"),
        }
    };
    let ops_diags = model::Diags::new();
    let Some(norm) = ops::normalize(ops_v, &ops_diags) else {
        let list = ops_diags.list();
        return match list.first() {
            Some(d) => bad_request(req, w, extra, d.code, &d.message),
            None => bad_request(req, w, extra, "E_INPUT", "invalid ops"),
        };
    };
    let why =
        body.get("why").map_or_else(|| norm.why.clone().unwrap_or_default(), |v| v.str().unwrap_or("").to_string());
    let mut who = "designer".to_string();
    if let Some(ac) = body.get("actor").and_then(Value::str)
        && !ac.is_empty() && ac.len() <= 32 && ac.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
            who = ac.to_string();
        }
    let mut if_match = None;
    if let Some(v) = body.get("if_match") {
        match v.str() {
            Some(im) => if_match = Some(im),
            None if !v.is_null() => {
                return bad_request(
                    req,
                    w,
                    extra,
                    "E_INPUT",
                    "\"if_match\" must be the ETag string of the version the ops were made against (or omitted)",
                );
            }
            None => {}
        }
    }

    let _write = lock(&s.write_mu);
    let lp = s.path_str(&workspace::log_path(file));
    let mut doc_lock = match DocLock::acquire(&lp) {
        Ok(l) => l,
        Err(e) => {
            http::send_error(
                w,
                500,
                ka,
                extra,
                "E_WRITE",
                &format!("cannot open or lock the op log {}: {e}", workspace::log_path(file)),
            )?;
            return Ok(ka);
        }
    };
    let rd = match s.read_doc(file) {
        Ok(rd) => rd,
        Err(e) if is_missing(&e) => return not_found(w, ka, extra, file),
        Err(e) => return Err(e),
    };
    let etag = etag_of(rd.mtime_ns, rd.size, &rd.bytes);
    if let Some(im) = if_match {
        let want = im.trim_matches(' ').trim_matches('"');
        if want != etag.trim_matches('"') {
            let msg = format!("{file} was changed by someone else (current ETag {etag}); reload it and re-apply");
            let mut out = b"{\"error\":{\"code\":\"E_CONFLICT\",\"message\":".to_vec();
            out.extend(http::json_string(&msg));
            out.extend_from_slice(b"},\"etag\":");
            out.extend(http::json_string(&etag));
            out.extend_from_slice(b"}\n");
            http::send_json(w, 409, ka, &format!("ETag: {etag}\r\n{extra}"), &out)?;
            return Ok(ka);
        }
    }
    let mut ops_text = Vec::new();
    json::write_compact(&mut ops_text, &Value::Array(norm.ops.clone()));
    let engine_actor = if who == "designer" { "designer" } else { "llm" };
    let mut input = b"{\"doc\":".to_vec();
    input.extend_from_slice(rd.bytes.trim_ascii());
    input.extend_from_slice(b",\"ops\":");
    input.extend_from_slice(&ops_text);
    input.extend_from_slice(format!(",\"actor\":\"{engine_actor}\"}}").as_bytes());
    let r = api::call(b"apply", &input);
    if !r.ok {
        http::send_json(w, 400, ka, extra, &r.bytes)?;
        return Ok(ka);
    }
    let res = json::parse(&r.bytes).map_err(|_| io::Error::other("BadEngineOutput"))?;
    let ok = matches!(res.get("ok"), Some(Value::Bool(true)));
    let mut final_etag = etag;
    if ok {
        let dv = res.get("doc").ok_or_else(|| io::Error::other("BadEngineOutput"))?;
        let text = canon::write(dv);
        let changed: Vec<String> = res
            .get("changed")
            .and_then(Value::arr)
            .map_or_else(Vec::new, |a| a.iter().filter_map(|v| v.str().map(str::to_string)).collect());
        let summary = res.get("summary").and_then(Value::str).unwrap_or("");
        let line = workspace::build_entry(
            &LogMeta { who: &who, tool: "kerf-serve", why: &why },
            &String::from_utf8_lossy(&ops_text),
            &changed,
            summary,
        );
        let mut st = lock(&s.scan);
        if let Err(e) = workspace::write_file_atomic(&s.path_str(file), &text) {
            http::send_error(w, 500, ka, extra, "E_WRITE", &format!("could not write {file}: {e}"))?;
            return Ok(ka);
        }
        if let Err(e) = doc_lock.append_line(&line) {
            s.warn_log(&lp, &e);
        }
        s.note_write_locked(&mut st, file, false, &who, Some(&line));
        if let Ok(m) = fs::symlink_metadata(s.path(file)) {
            final_etag = etag_of(mtime_ns(&m), m.len(), &text);
        }
    }
    let trimmed = r.bytes.trim_ascii_end();
    let mut out = trimmed[..trimmed.len().saturating_sub(1)].to_vec();
    out.extend_from_slice(b",\"etag\":");
    out.extend(http::json_string(&final_etag));
    out.extend_from_slice(b"}\n");
    http::send_json(w, 200, ka, &format!("ETag: {final_etag}\r\n{extra}"), &out)?;
    Ok(ka)
}

fn read_no_follow(s: &Server, name: &str, limit: u64) -> Option<Vec<u8>> {
    let path = s.path(name);
    if !fs::symlink_metadata(&path).ok()?.file_type().is_file() {
        return None;
    }
    let mut buf = Vec::new();
    fs::File::open(path).ok()?.take(limit).read_to_end(&mut buf).ok()?;
    Some(buf)
}

pub fn api_log(s: &Server, req: &Request, w: &mut Out, extra: &str, file: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let since: usize = match req.param("since") {
        Some(sv) => match sv.parse() {
            Ok(n) => n,
            Err(_) => return bad_request(req, w, extra, "E_INPUT", "since must be a line index (integer)"),
        },
        None => 0,
    };
    if !fs::symlink_metadata(s.path(file)).is_ok_and(|m| m.file_type().is_file()) {
        return not_found(w, ka, extra, file);
    }
    let data = read_no_follow(s, &workspace::log_path(file), 64 << 20).unwrap_or_default();
    let mut out = b"{\"entries\":[".to_vec();
    let (mut idx, mut emitted) = (0usize, 0usize);
    for raw in data.split(|&c| c == b'\n') {
        let line = raw.trim_ascii();
        if line.is_empty() {
            continue;
        }
        let i = idx;
        idx += 1;
        if i < since {
            continue;
        }
        if emitted > 0 {
            out.push(b',');
        }
        emitted += 1;
        if json::parse(line).is_ok() {
            out.extend_from_slice(line);
        } else {
            out.extend_from_slice(b"{\"raw\":");
            out.extend(http::json_string(&String::from_utf8_lossy(line)));
            out.push(b'}');
        }
    }
    out.extend_from_slice(format!("],\"next\":{idx}}}\n").as_bytes());
    http::send_json(w, 200, ka, extra, &out)?;
    Ok(ka)
}

pub fn api_export(s: &Server, req: &Request, w: &mut Out, extra: &str, file: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let Some(view) = req.param("view") else {
        return bad_request(req, w, extra, "E_INPUT", "missing ?view=<view id>, for example view=A");
    };
    let format = req.param("format").unwrap_or_else(|| "png".into());
    let Some((mime, ext)) =
        [("png", "image/png"), ("svg", "image/svg+xml"), ("dxf", "application/dxf"), ("pdf", "application/pdf")]
            .iter()
            .find(|f| f.0 == format)
            .map(|f| (f.1, f.0))
    else {
        return bad_request(req, w, extra, "E_INPUT", "format must be png, svg, dxf or pdf");
    };
    let sheet = req.param("sheet").is_some_and(|v| v == "1" || v == "true");
    let rd = match s.read_doc(file) {
        Ok(rd) => rd,
        Err(e) if is_missing(&e) => return not_found(w, ka, extra, file),
        Err(e) => return Err(e),
    };
    let mut input = b"{\"doc\":".to_vec();
    input.extend_from_slice(rd.bytes.trim_ascii());
    input.extend_from_slice(b",\"view\":");
    input.extend(http::json_string(&view));
    input.extend_from_slice(format!(",\"format\":\"{format}\",\"sheet\":{sheet}").as_bytes());
    if let Some(px) = req.param("px") {
        let Ok(n) = px.parse::<u32>() else {
            return bad_request(req, w, extra, "E_INPUT", "px must be an integer (output width in pixels)");
        };
        input.extend_from_slice(format!(",\"px\":{n}").as_bytes());
    }
    input.push(b'}');
    let r = api::call(b"export", &input);
    if !r.ok {
        let status = if r.bytes.windows(6).any(|x| x == b"E_VIEW") { 404 } else { 400 };
        http::send_json(w, status, ka, extra, &r.bytes)?;
        return Ok(ka);
    }
    let safe_view: String =
        view.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let disp = if req.param("inline").is_some() { "inline" } else { "attachment" };
    let hdr =
        format!("Content-Disposition: {disp}; filename=\"{}-{safe_view}.{ext}\"\r\n{extra}", workspace::doc_stem(file));
    http::send(
        w,
        Head { status: 200, content_type: Some(mime), keep_alive: ka, extra: &hdr, ..Head::default() },
        &r.bytes,
    )?;
    Ok(ka)
}
