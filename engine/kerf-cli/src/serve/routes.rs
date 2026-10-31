//! The route table, the per-route body caps and the request dispatcher.

use super::conn::{BUSY_MS, arm, check_access};
use super::{Server, agent, docs, stream, ui};
use crate::http::{self, In, Out, Request};
use crate::workspace;
use std::io;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Options,
    Info,
    DocsList,
    DocsCreate,
    DocGet,
    DocApply,
    DocLog,
    DocExport,
    DocReference,
    Events,
    AgentRun,
    AgentStop,
    BadFile,
    MethodNotAllowed,
    NotFound,
    Static,
}

pub struct Route<'a> {
    pub kind: Kind,
    pub file: &'a str,
    pub api: bool,
}

const fn r(kind: Kind, api: bool) -> Route<'static> {
    Route { kind, file: "", api }
}

struct Methods {
    get: Option<Kind>,
    post: Option<Kind>,
}

const ENDPOINTS: [(&str, Methods); 5] = [
    ("/info", Methods { get: Some(Kind::Info), post: None }),
    ("/docs", Methods { get: Some(Kind::DocsList), post: Some(Kind::DocsCreate) }),
    ("/events", Methods { get: Some(Kind::Events), post: None }),
    ("/agent/run", Methods { get: None, post: Some(Kind::AgentRun) }),
    ("/agent/stop", Methods { get: None, post: Some(Kind::AgentStop) }),
];

const DOC_ACTIONS: [(&str, Methods); 5] = [
    ("", Methods { get: Some(Kind::DocGet), post: None }),
    ("apply", Methods { get: None, post: Some(Kind::DocApply) }),
    ("log", Methods { get: Some(Kind::DocLog), post: None }),
    ("export", Methods { get: Some(Kind::DocExport), post: None }),
    ("reference", Methods { get: Some(Kind::DocReference), post: None }),
];

pub fn route_of<'a>(method: &str, path: &'a str) -> Route<'a> {
    let (is_get, is_post) = (method == "GET", method == "POST");
    if !(path == "/api" || path.starts_with("/api/")) {
        return r(if is_get || method == "HEAD" { Kind::Static } else { Kind::MethodNotAllowed }, false);
    }
    if method == "OPTIONS" {
        return r(Kind::Options, true);
    }
    let rest = &path["/api".len()..];
    let pick = |m: &Methods| {
        if is_get {
            m.get
        } else if is_post {
            m.post
        } else {
            None
        }
    };
    for (p, m) in &ENDPOINTS {
        if rest == *p {
            return r(pick(m).unwrap_or(Kind::MethodNotAllowed), true);
        }
    }
    if let Some(tail) = rest.strip_prefix("/docs/") {
        let mut parts = tail.split('/');
        let file = parts.next().unwrap_or("");
        let action = parts.next().unwrap_or("");
        if parts.next().is_some() {
            return r(Kind::NotFound, true);
        }
        if !workspace::valid_doc_file(file) {
            return r(Kind::BadFile, true);
        }
        for (a, m) in &DOC_ACTIONS {
            if action == *a {
                return Route { kind: pick(m).unwrap_or(Kind::MethodNotAllowed), file, api: true };
            }
        }
    }
    r(Kind::NotFound, true)
}

pub fn body_cap(kind: Kind) -> usize {
    match kind {
        Kind::AgentStop => 4 << 10,
        Kind::DocsCreate => 64 << 10,
        Kind::DocApply => 16 << 20,
        Kind::AgentRun => 64 << 20,
        _ => 0,
    }
}

pub fn handle_request(s: &Server, req: &mut Request, rd: &mut In, w: &mut Out) -> io::Result<bool> {
    let path = http::percent_decode(&req.path, false);
    let route = route_of(&req.method, &path);
    let body_pending = req.chunked || req.content_length != 0;
    let ka = req.keep_alive && !body_pending;
    let extra = if route.api { s.cors(req) } else { String::new() };

    if route.kind == Kind::Options {
        http::send(w, http::Head { status: 204, keep_alive: ka, extra: &extra, ..http::Head::default() }, b"")?;
        return Ok(ka);
    }
    if route.api
        && let Some(rej) = check_access(s, req)
    {
        if route.kind == Kind::Info && rej.status == 401 {
            let body = format!(
                "{{\"version\":\"{}\",\"token_required\":true,\"authenticated\":false}}\n",
                kerf_core::api::VERSION
            );
            http::send_json(w, 200, ka, &extra, body.as_bytes())?;
            return Ok(ka);
        }
        http::send_error(w, rej.status, ka, &extra, rej.code, rej.message)?;
        return Ok(ka);
    }
    match route.kind {
        Kind::MethodNotAllowed => {
            let m = if route.api { "method not allowed for this path" } else { "method not allowed" };
            http::send_error(w, 405, ka, &extra, "E_METHOD", m)?;
            return Ok(ka);
        }
        Kind::NotFound => {
            http::send_error(w, 404, ka, &extra, "E_NOT_FOUND", "no such endpoint")?;
            return Ok(ka);
        }
        Kind::BadFile => {
            http::send_error(w, 400, ka, &extra, "E_FILE", "file must be a plain NAME.kerf.json (no directories)")?;
            return Ok(ka);
        }
        _ => {}
    }
    let cap = body_cap(route.kind);
    if body_pending {
        if cap == 0 {
            http::send_error(w, 400, false, &extra, "E_BODY", "this endpoint takes no request body")?;
            return Ok(false);
        }
        let head = u64::from(s.cfg.timeouts.head_ms);
        arm(head * 3 + (req.content_length.min(256 << 20) as u64 / (512 << 10)) * 1000);
        if let Err(e) = http::read_body(rd, w, req, cap) {
            let _ = match e {
                http::BodyError::TooLarge => http::send_error(
                    w,
                    413,
                    false,
                    &extra,
                    "E_TOO_LARGE",
                    &format!("request body over {cap} bytes for this endpoint"),
                ),
                http::BodyError::BadChunk | http::BodyError::ShortBody => {
                    http::send_error(w, 400, false, &extra, "E_HTTP", "malformed request body")
                }
                http::BodyError::Io => Ok(()),
            };
            return Ok(false);
        }
        arm(BUSY_MS);
    }
    let rq: &Request = req;
    match route.kind {
        Kind::Info => docs::api_info(s, rq, w, &extra),
        Kind::DocsList => docs::api_list(s, rq, w, &extra),
        Kind::DocsCreate => docs::api_create(s, rq, w, &extra),
        Kind::DocGet => docs::api_get_doc(s, rq, w, &extra, route.file),
        Kind::DocApply => docs::api_apply(s, rq, w, &extra, route.file),
        Kind::DocLog => docs::api_log(s, rq, w, &extra, route.file),
        Kind::DocExport => docs::api_export(s, rq, w, &extra, route.file),
        Kind::DocReference => docs::api_reference(s, rq, w, &extra, route.file),
        Kind::Events => stream::api_events(s, w, &extra),
        Kind::AgentRun => agent::api_agent_run(s, rq, w, &extra),
        Kind::AgentStop => agent::api_agent_stop(s, rq, w, &extra),
        Kind::Static => ui::static_file(s, rq, w, &path),
        Kind::Options | Kind::BadFile | Kind::MethodNotAllowed | Kind::NotFound => Ok(false),
    }
}
