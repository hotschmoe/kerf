//! `POST /api/llm`: forward one HTTP request to an LLM provider (avoids browser CORS) and stream the response back
//! verbatim. The URL rules live in `plan` (pure): only https:// URLs, plus http:// to localhost/LAN hosts for the
//! `custom` provider. Keys travel in `headers` and are never stored or logged.
//!
//! This build has no TLS (std has none and the engine takes no crates): plain http upstreams are forwarded, https
//! upstreams are accepted by `plan` and refused by `forward` with an explanatory E_UPSTREAM.

use crate::http::{self, Head, Out};
use kerf_core::json::{self, Value};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub struct Plan {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub target: String,
    pub post: bool,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

pub struct Rejection {
    pub status: u16,
    pub code: &'static str,
    pub message: &'static str,
}

pub enum PlanResult {
    Ok(Plan),
    Rejected(Rejection),
}

const PROVIDERS: [(&str, Option<&str>); 6] = [
    ("anthropic", Some("https://api.anthropic.com")),
    ("openai", Some("https://api.openai.com")),
    ("gemini", Some("https://generativelanguage.googleapis.com")),
    ("xai", Some("https://api.x.ai")),
    ("openrouter", Some("https://openrouter.ai/api")),
    ("custom", None),
];

const BLOCKED_HEADERS: [&str; 15] = [
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "accept-encoding",
    "upgrade",
    "te",
    "trailer",
    "expect",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "x-forwarded-for",
    "forwarded",
    "content-encoding",
];

fn reject(code: &'static str, message: &'static str) -> PlanResult {
    PlanResult::Rejected(Rejection { status: 400, code, message })
}

fn is_private4(b: [u8; 4]) -> bool {
    b[0] == 127
        || b[0] == 10
        || b[0] == 0
        || (b[0] == 172 && (16..=31).contains(&b[1]))
        || (b[0] == 192 && b[1] == 168)
        || (b[0] == 169 && b[1] == 254)
        || (b[0] == 100 && (64..=127).contains(&b[1]))
}

pub fn is_local_or_lan(host: &str) -> bool {
    if host.is_empty() {
        return true;
    }
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || [".localhost", ".local", ".lan", ".internal"].iter().any(|s| lower.ends_with(s)) {
        return true;
    }
    if let Ok(ip4) = host.parse::<Ipv4Addr>() {
        return is_private4(ip4.octets());
    }
    if let Ok(ip6) = host.parse::<Ipv6Addr>() {
        let b = ip6.octets();
        if b == [0; 16] || b == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] {
            return true;
        }
        if b[0] & 0xfe == 0xfc || (b[0] == 0xfe && b[1] & 0xc0 == 0x80) {
            return true;
        }
        if b[..10].iter().all(|&x| x == 0) && b[10] == 0xff && b[11] == 0xff {
            return is_private4([b[12], b[13], b[14], b[15]]);
        }
        return false;
    }
    !host.contains('.')
}

fn is_public_name(host: &str) -> bool {
    if is_local_or_lan(host) {
        return false;
    }
    let Some(dot) = host.rfind('.') else { return false };
    let tld = &host[dot + 1..];
    if tld.is_empty() || tld.bytes().all(|c| c.is_ascii_digit()) || tld.to_ascii_lowercase().starts_with("0x") {
        return false;
    }
    !host.contains(':')
}

fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 4096
        && !path.bytes().any(|c| c <= 0x20 || c == 0x7f || c == b'\\')
        && !path.contains("://")
        && !path.starts_with("//")
}

fn valid_header_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 100 && n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

struct Url<'a> {
    https: bool,
    host: &'a str,
    port: u16,
    target: &'a str,
}

fn parse_url(url: &str) -> Result<Url<'_>, PlanResult> {
    let bad = || reject("E_URL", "base_url + path is not a valid URL");
    let (scheme, rest) = url.split_once("://").ok_or_else(bad)?;
    if !scheme.bytes().all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c)) || scheme.is_empty() {
        return Err(bad());
    }
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, target) = rest.split_at(auth_end);
    if authority.contains('@') {
        return Err(reject("E_URL", "credentials in the URL are not allowed; send keys in \"headers\""));
    }
    if target.contains('#') {
        return Err(reject("E_URL", "URL fragments are not allowed"));
    }
    let https = scheme.eq_ignore_ascii_case("https");
    if !https && !scheme.eq_ignore_ascii_case("http") {
        return Err(reject(
            "E_URL",
            "only https:// URLs are allowed (http:// only for localhost/LAN with provider \"custom\")",
        ));
    }
    let (host, port_text) = if let Some(inner) = authority.strip_prefix('[') {
        let close = inner.find(']').ok_or_else(bad)?;
        let after = &inner[close + 1..];
        (&inner[..close], if after.is_empty() { None } else { Some(after.strip_prefix(':').ok_or_else(bad)?) })
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    let no_host = || reject("E_URL", "URL has no valid host");
    if host.is_empty() || !host.bytes().all(|c| c.is_ascii_alphanumeric() || b".-_:%".contains(&c)) {
        return Err(no_host());
    }
    let port = match port_text {
        None | Some("") => {
            if https {
                443
            } else {
                80
            }
        }
        Some(p) => p.parse::<u16>().map_err(|_| bad())?,
    };
    let target = if target.is_empty() { "/" } else { target };
    Ok(Url { https, host, port, target })
}

pub fn plan(root: &Value) -> PlanResult {
    if !matches!(root, Value::Object(_)) {
        return reject("E_INPUT", "body must be a JSON object { provider, base_url?, path, headers, body }");
    }
    let Some(provider) = root.get("provider").and_then(Value::str) else {
        return reject("E_INPUT", "missing \"provider\" (anthropic | openai | gemini | xai | openrouter | custom)");
    };
    let Some(&(_, default_base)) = PROVIDERS.iter().find(|p| p.0 == provider) else {
        return reject("E_PROVIDER", "unknown provider; use anthropic, openai, gemini, xai, openrouter or custom");
    };
    let is_custom = provider == "custom";
    let base_in = match root.get("base_url") {
        None | Some(Value::Null) => None,
        Some(v) => match v.str() {
            Some(s) => Some(s),
            None => return reject("E_INPUT", "\"base_url\" must be a string"),
        },
    };
    let Some(base_raw) = base_in.or(default_base) else {
        return reject("E_INPUT", "provider \"custom\" needs \"base_url\" (for example http://localhost:11434/v1)");
    };
    let base = base_raw.trim_end_matches('/');
    let Some(path) = root.get("path").and_then(Value::str) else {
        return reject("E_INPUT", "missing \"path\" (for example /v1/messages)");
    };
    if !valid_path(path) {
        return reject(
            "E_PATH",
            "\"path\" must start with a single \"/\" and contain no spaces, backslashes or \"://\"",
        );
    }
    let url = format!("{base}{path}");
    let u = match parse_url(&url) {
        Ok(u) => u,
        Err(r) => return r,
    };
    if is_custom {
        if !u.https && !is_local_or_lan(u.host) {
            return reject(
                "E_URL",
                "plain http:// is only allowed for localhost and LAN hosts; use https:// for public servers",
            );
        }
    } else {
        if !u.https {
            return reject(
                "E_URL",
                "only https:// URLs are allowed for this provider (use provider \"custom\" for a local server)",
            );
        }
        if !is_public_name(u.host) {
            return reject(
                "E_URL",
                "this provider must be a public https host; use provider \"custom\" for localhost/LAN servers",
            );
        }
    }
    let mut headers = Vec::new();
    if let Some(hv) = root.get("headers") {
        let Value::Object(members) = hv else {
            return reject("E_INPUT", "\"headers\" must be an object of string values");
        };
        for m in members {
            let Some(val) = m.value.str() else { return reject("E_HEADER", "every header value must be a string") };
            if !valid_header_name(&m.key) {
                return reject("E_HEADER", "invalid header name");
            }
            if val.bytes().any(|c| c == b'\r' || c == b'\n' || c == 0) {
                return reject("E_HEADER", "header values must not contain line breaks");
            }
            if BLOCKED_HEADERS.iter().any(|b| b.eq_ignore_ascii_case(&m.key)) {
                continue;
            }
            headers.push((m.key.clone(), val.to_string()));
        }
    }
    let mut body = None;
    if let Some(bv) = root.get("body") {
        if !bv.is_null() {
            body = Some(match bv.str() {
                Some(s) => s.as_bytes().to_vec(),
                None => {
                    let mut out = Vec::new();
                    json::write_compact(&mut out, bv);
                    out
                }
            });
        }
    }
    let mut post = body.is_some();
    if let Some(ms) = root.get("method").and_then(Value::str) {
        if ms.eq_ignore_ascii_case("GET") {
            post = false;
        } else if ms.eq_ignore_ascii_case("POST") {
            post = true;
        } else {
            return reject("E_INPUT", "\"method\" must be GET or POST");
        }
    }
    if !post && body.is_some() {
        return reject("E_INPUT", "a GET request cannot have a \"body\"");
    }
    PlanResult::Ok(Plan {
        https: u.https,
        host: u.host.to_string(),
        port: u.port,
        target: u.target.to_string(),
        post,
        headers,
        body,
    })
}

fn forward_response_header(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    ["content-type", "content-encoding", "retry-after", "request-id", "x-request-id", "x-should-retry"]
        .contains(&l.as_str())
        || l.starts_with("x-ratelimit-")
        || l.starts_with("anthropic-ratelimit-")
        || l.starts_with("openai-")
}

pub enum ForwardError {
    Upstream(String),
    HeadSent,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub first_byte_ms: u64,
    pub idle_ms: u64,
    pub total_ms: u64,
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { first_byte_ms: 5 * 60_000, idle_ms: 2 * 60_000, total_ms: 30 * 60_000, max_bytes: 256 << 20 }
    }
}

fn read_line(r: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    r.take(8192).read_until(b'\n', &mut line)?;
    Ok(line)
}

/// Forward the planned request and stream the answer; the response is close-delimited.
pub fn forward(p: &Plan, w: &mut Out, lim: Limits) -> Result<(), ForwardError> {
    let up = |m: String| ForwardError::Upstream(m);
    if p.https {
        return Err(up(format!(
            "could not connect to {}: this kerf build has no TLS, so https:// upstreams cannot be proxied (use a local http:// server with provider \"custom\")",
            p.host
        )));
    }
    let started = Instant::now();
    let addr = (p.host.as_str(), p.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .ok_or_else(|| up(format!("could not connect to {}: name not found", p.host)))?;
    let sock = TcpStream::connect_timeout(&addr, Duration::from_millis(lim.first_byte_ms.max(1)))
        .map_err(|e| up(format!("could not connect to {}: {}", p.host, e.kind())))?;
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}:{}\r\nUser-Agent: kerf-serve/{}\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
        if p.post { "POST" } else { "GET" },
        p.target,
        p.host,
        p.port,
        kerf_core::api::VERSION
    );
    for (k, v) in &p.headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(b) = &p.body {
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req.push_str("\r\n");
    let mut s = &sock;
    s.write_all(req.as_bytes())
        .and_then(|()| p.body.as_deref().map_or(Ok(()), |b| s.write_all(b)))
        .map_err(|e| up(format!("sending the request to {} failed: {}", p.host, e.kind())))?;
    let _ = sock.set_read_timeout(Some(Duration::from_millis(lim.first_byte_ms.max(1))));
    let mut r = BufReader::new(&sock);
    let timed_out = |e: &io::Error| matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut);
    let status_line = read_line(&mut r).map_err(|e| {
        up(if timed_out(&e) {
            "the upstream did not answer in time".to_string()
        } else {
            format!("no valid response from {}: {}", p.host, e.kind())
        })
    })?;
    let status: u16 = std::str::from_utf8(&status_line)
        .ok()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| up(format!("no valid response from {}: bad status line", p.host)))?;
    let mut extra = String::new();
    let (mut chunked, mut content_length) = (false, None::<usize>);
    loop {
        let line = read_line(&mut r).map_err(|e| {
            up(if timed_out(&e) {
                "the upstream did not answer in time".to_string()
            } else {
                format!("no valid response from {}: {}", p.host, e.kind())
            })
        })?;
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return Err(up(format!("no valid response from {}: truncated head", p.host)));
        }
        if text.is_empty() {
            break;
        }
        let Some((name, value)) = text.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().ok();
        }
        if !forward_response_header(name)
            || (name.eq_ignore_ascii_case("content-encoding") && value.eq_ignore_ascii_case("identity"))
        {
            continue;
        }
        extra.push_str(&format!("{name}: {value}\r\n"));
    }
    extra.push_str("X-Accel-Buffering: no\r\n");
    http::write_head(
        w,
        &Head { status, content_type: None, content_length: None, keep_alive: false, extra: &extra, ..Head::default() },
    )
    .and_then(|()| w.flush())
    .map_err(|_| ForwardError::HeadSent)?;
    let _ = sock.set_read_timeout(Some(Duration::from_millis(lim.idle_ms.max(1))));
    let mut sent = 0usize;
    let mut pump = |data: &[u8], w: &mut Out| -> bool {
        sent += data.len();
        w.write_all(data).and_then(|()| w.flush()).is_ok()
            && sent <= lim.max_bytes
            && started.elapsed().as_millis() < u128::from(lim.total_ms)
    };
    if chunked {
        loop {
            let Ok(line) = read_line(&mut r) else { break };
            let text = String::from_utf8_lossy(&line);
            let size_text = text.split(';').next().unwrap_or("").trim();
            let Ok(mut size) = usize::from_str_radix(size_text, 16) else { break };
            if size == 0 {
                break;
            }
            let mut buf = [0u8; 8192];
            while size > 0 {
                let n = match r.read(&mut buf[..size.min(8192)]) {
                    Ok(0) | Err(_) => return Ok(()),
                    Ok(n) => n,
                };
                size -= n;
                if !pump(&buf[..n], w) {
                    return Ok(());
                }
            }
            let _ = read_line(&mut r);
        }
    } else {
        let mut left = content_length.unwrap_or(usize::MAX);
        let mut buf = [0u8; 8192];
        while left > 0 {
            let n = match r.read(&mut buf[..left.min(8192)]) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            left -= n;
            if !pump(&buf[..n], w) {
                break;
            }
        }
    }
    Ok(())
}
