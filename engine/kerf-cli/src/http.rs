//! HTTP/1.1 for `kerf serve`: strict request-head parsing (the request-smuggling shapes are refused),
//! bounded bodies (Content-Length and chunked), response heads with the fixed security headers.

use kerf_core::json;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::TcpStream;

pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 100;
const MAX_TRAILER_LINES: usize = 32;

pub type In = BufReader<TcpStream>;
pub type Out = BufWriter<TcpStream>;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub head: Vec<u8>,
    pub content_length: usize,
    pub chunked: bool,
    pub keep_alive: bool,
    pub expect_continue: bool,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let mut lines = self.head.split(|&c| c == b'\n');
        lines.next();
        for line in lines {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let Some(colon) = line.iter().position(|&c| c == b':') else { continue };
            if line[..colon].eq_ignore_ascii_case(name.as_bytes()) {
                return std::str::from_utf8(line[colon + 1..].trim_ascii()).ok();
            }
        }
        None
    }

    pub fn param(&self, name: &str) -> Option<String> {
        query_param(&self.query, name)
    }
}

#[derive(Debug, PartialEq)]
pub enum HeadError {
    Closed,
    HeadTooLarge,
    Malformed,
    Unsupported,
    Io,
}

#[derive(Debug, PartialEq)]
pub enum BodyError {
    TooLarge,
    Io,
    ShortBody,
    BadChunk,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read one request head (up to the blank line). `Closed` means the peer closed before sending anything.
pub fn read_head(r: &mut In) -> Result<Request, HeadError> {
    let mut head = Vec::new();
    loop {
        let buf = r.fill_buf().map_err(|_| HeadError::Io)?;
        if buf.is_empty() {
            return Err(if head.is_empty() { HeadError::Closed } else { HeadError::Malformed });
        }
        let had = head.len();
        let take = buf.len().min(MAX_HEAD + 4 - had.min(MAX_HEAD + 4));
        head.extend_from_slice(&buf[..take]);
        let from = had.saturating_sub(3);
        if let Some(i) = find(&head[from..], b"\r\n\r\n") {
            let end = from + i;
            let consumed = end + 4 - had;
            r.consume(consumed);
            head.truncate(end);
            if head.len() > MAX_HEAD {
                return Err(HeadError::HeadTooLarge);
            }
            return parse_head(head);
        }
        if head.len() >= MAX_HEAD {
            return Err(HeadError::HeadTooLarge);
        }
        let n = take;
        r.consume(n);
    }
}

fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}

pub fn parse_head(head: Vec<u8>) -> Result<Request, HeadError> {
    use HeadError::{HeadTooLarge, Malformed, Unsupported};
    if head.iter().any(|&c| (c < 0x20 && c != b'\r' && c != b'\n' && c != b'\t') || c == 0x7f) {
        return Err(Malformed);
    }
    // Lines are split on "\r\n": a bare LF or CR inside a line is malformed.
    let text = std::str::from_utf8(&head).map_err(|_| Malformed)?;
    let mut it = text.split("\r\n");
    let line = it.next().ok_or(Malformed)?;
    if line.contains(['\r', '\n']) {
        return Err(Malformed);
    }
    let sp1 = line.find(' ').ok_or(Malformed)?;
    let sp2 = line[sp1 + 1..].find(' ').map(|i| i + sp1 + 1).ok_or(Malformed)?;
    let (method, target, version) = (&line[..sp1], &line[sp1 + 1..sp2], &line[sp2 + 1..]);
    if method.is_empty() || method.len() > 16 || target.is_empty() || !target.starts_with('/') {
        return Err(Malformed);
    }
    if !method.bytes().all(is_token_char) || target.contains(' ') {
        return Err(Malformed);
    }
    let http11 = version == "HTTP/1.1";
    if !http11 && version != "HTTP/1.0" {
        return Err(Malformed);
    }
    let (path, query) = match target.find('?') {
        Some(i) => (&target[..i], &target[i + 1..]),
        None => (target, ""),
    };
    let (method, path, query) = (method.to_string(), path.to_string(), query.to_string());
    let mut content_length = 0;
    let mut chunked = false;
    let mut keep_alive = http11;
    let mut expect_continue = false;
    let (mut seen_cl, mut seen_te, mut seen_host, mut seen_conn, mut seen_expect) = (false, false, false, false, false);
    let mut n_headers = 0;
    for l in it {
        if l.is_empty() || l.contains(['\r', '\n']) || l.starts_with([' ', '\t']) {
            return Err(Malformed);
        }
        let colon = l.find(':').ok_or(Malformed)?;
        let name = &l[..colon];
        if name.is_empty() || !name.bytes().all(is_token_char) {
            return Err(Malformed);
        }
        n_headers += 1;
        if n_headers > MAX_HEADERS {
            return Err(HeadTooLarge);
        }
        let value = l[colon + 1..].trim_matches([' ', '\t']);
        let lower = name.to_ascii_lowercase();
        let has = |w: &str| value.to_ascii_lowercase().contains(w);
        match lower.as_str() {
            "content-length" => {
                if std::mem::replace(&mut seen_cl, true) {
                    return Err(Malformed);
                }
                content_length = parse_length(value).ok_or(Malformed)?;
            }
            "transfer-encoding" => {
                if std::mem::replace(&mut seen_te, true) {
                    return Err(Malformed);
                }
                if value.eq_ignore_ascii_case("chunked") {
                    chunked = true;
                } else {
                    return Err(Unsupported);
                }
            }
            "host" => {
                if std::mem::replace(&mut seen_host, true) {
                    return Err(Malformed);
                }
            }
            "connection" => {
                if std::mem::replace(&mut seen_conn, true) {
                    return Err(Malformed);
                }
                if has("close") {
                    keep_alive = false;
                }
                if !http11 && has("keep-alive") {
                    keep_alive = true;
                }
            }
            "expect" => {
                if std::mem::replace(&mut seen_expect, true) {
                    return Err(Malformed);
                }
                expect_continue = has("100-continue");
            }
            _ => {}
        }
    }
    if seen_cl && seen_te {
        return Err(Malformed);
    }
    Ok(Request { method, path, query, head, content_length, chunked, keep_alive, expect_continue, body: Vec::new() })
}

fn parse_length(v: &str) -> Option<usize> {
    if v.is_empty() || v.len() > 18 || !v.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    v.parse().ok()
}

fn read_n(r: &mut In, body: &mut Vec<u8>, n: usize) -> Result<(), BodyError> {
    let start = body.len();
    r.by_ref().take(n as u64).read_to_end(body).map_err(|_| BodyError::Io)?;
    if body.len() - start < n {
        return Err(BodyError::ShortBody);
    }
    Ok(())
}

pub fn read_body(r: &mut In, w: &mut Out, req: &mut Request, max: usize) -> Result<(), BodyError> {
    if req.chunked {
        if req.expect_continue {
            w.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").and_then(|()| w.flush()).map_err(|_| BodyError::Io)?;
        }
        return read_chunked(r, req, max);
    }
    if req.content_length == 0 {
        return Ok(());
    }
    if req.content_length > max {
        return Err(BodyError::TooLarge);
    }
    if req.expect_continue {
        w.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").and_then(|()| w.flush()).map_err(|_| BodyError::Io)?;
    }
    let mut body = Vec::new();
    let mut left = req.content_length;
    while left > 0 {
        let step = left.min(64 * 1024);
        read_n(r, &mut body, step)?;
        left -= step;
    }
    req.body = body;
    Ok(())
}

fn take_line(r: &mut In) -> Result<Vec<u8>, BodyError> {
    let mut line = Vec::new();
    let n = r.by_ref().take(MAX_HEAD as u64).read_until(b'\n', &mut line).map_err(|_| BodyError::Io)?;
    if n == 0 {
        return Err(BodyError::ShortBody);
    }
    if line.last() != Some(&b'\n') {
        return Err(if line.len() >= MAX_HEAD { BodyError::BadChunk } else { BodyError::ShortBody });
    }
    Ok(line)
}

pub fn read_chunked(r: &mut In, req: &mut Request, max: usize) -> Result<(), BodyError> {
    let mut body = Vec::new();
    loop {
        let line = take_line(r)?;
        if line.len() < 3 || line[line.len() - 2] != b'\r' {
            return Err(BodyError::BadChunk);
        }
        let mut t = &line[..line.len() - 2];
        if let Some(semi) = t.iter().position(|&c| c == b';') {
            t = &t[..semi];
        }
        if t.is_empty() || t.len() > 8 || !t.iter().all(u8::is_ascii_hexdigit) {
            return Err(BodyError::BadChunk);
        }
        let size =
            std::str::from_utf8(t).ok().and_then(|s| u32::from_str_radix(s, 16).ok()).ok_or(BodyError::BadChunk)?
                as usize;
        if size == 0 {
            break;
        }
        if size > max - body.len() {
            return Err(BodyError::TooLarge);
        }
        let mut left = size;
        while left > 0 {
            let step = left.min(64 * 1024);
            read_n(r, &mut body, step)?;
            left -= step;
        }
        let mut crlf = Vec::new();
        read_n(r, &mut crlf, 2)?;
        if crlf != b"\r\n" {
            return Err(BodyError::BadChunk);
        }
    }
    let mut lines = 0;
    loop {
        let line = take_line(r)?;
        if line.trim_ascii().is_empty() {
            break;
        }
        lines += 1;
        if lines > MAX_TRAILER_LINES {
            return Err(BodyError::BadChunk);
        }
    }
    req.content_length = body.len();
    req.body = body;
    Ok(())
}

pub fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        422 => "Unprocessable Content",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

pub struct Head<'a> {
    pub status: u16,
    pub content_type: Option<&'a str>,
    pub content_length: Option<usize>,
    pub keep_alive: bool,
    pub extra: &'a str,
    pub no_store: bool,
    pub csp: Option<&'a str>,
}

impl Default for Head<'_> {
    fn default() -> Self {
        Head {
            status: 200,
            content_type: None,
            content_length: None,
            keep_alive: true,
            extra: "",
            no_store: true,
            csp: None,
        }
    }
}

pub const API_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

pub fn write_head(w: &mut Out, h: &Head) -> io::Result<()> {
    write!(w, "HTTP/1.1 {} {}\r\n", h.status, reason(h.status))?;
    if let Some(ct) = h.content_type {
        write!(w, "Content-Type: {ct}\r\n")?;
    }
    if let Some(n) = h.content_length {
        write!(w, "Content-Length: {n}\r\n")?;
    }
    if h.no_store {
        w.write_all(b"Cache-Control: no-store\r\n")?;
    }
    w.write_all(b"X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n")?;
    write!(w, "Content-Security-Policy: {}\r\n", h.csp.unwrap_or(API_CSP))?;
    let conn = if h.keep_alive && h.content_length.is_some() { "keep-alive" } else { "close" };
    write!(w, "Connection: {conn}\r\n{}\r\n", h.extra)
}

pub fn send_head_only(w: &mut Out, h: Head, body_len: usize) -> io::Result<()> {
    write_head(w, &Head { content_length: Some(body_len), ..h })?;
    w.flush()
}

pub fn send(w: &mut Out, h: Head, body: &[u8]) -> io::Result<()> {
    write_head(w, &Head { content_length: Some(body.len()), ..h })?;
    w.write_all(body)?;
    w.flush()
}

pub fn send_json(w: &mut Out, status: u16, keep_alive: bool, extra: &str, body: &[u8]) -> io::Result<()> {
    send(
        w,
        Head { status, content_type: Some("application/json; charset=utf-8"), keep_alive, extra, ..Head::default() },
        body,
    )
}

pub fn send_error(
    w: &mut Out,
    status: u16,
    keep_alive: bool,
    extra: &str,
    code: &str,
    message: &str,
) -> io::Result<()> {
    let mut out = b"{\"error\":{\"code\":".to_vec();
    json::write_string(&mut out, code);
    out.extend_from_slice(b",\"message\":");
    json::write_string(&mut out, message);
    out.extend_from_slice(b"}}\n");
    send_json(w, status, keep_alive, extra, &out)
}

pub fn json_string(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    json::write_string(&mut out, s);
    out
}

pub fn percent_decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()));
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if plus && c == b'+' { b' ' } else { c });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(k, true) == name {
            return Some(percent_decode(v, true));
        }
    }
    None
}

/// Constant-time over the longer input; unequal lengths never match.
pub fn secret_eql(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

pub fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "pdf" => "application/pdf",
        "dxf" => "application/dxf",
        _ => "application/octet-stream",
    }
}
