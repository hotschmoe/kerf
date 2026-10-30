//! Connection limits, deadlines and the access rules (token, Host, Origin).
//!
//! One thread per connection. Every phase arms a deadline on the connection (`arm`); a watchdog thread shuts the
//! socket down once it passes, which wakes the blocked read or write. `arm(0)` disarms (SSE waiting).

use super::{Server, lock, routes};
use crate::http::{self, In, Out, Request};
use std::cell::RefCell;
use std::io::{BufReader, BufWriter, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

pub const MAX_CONNS: u32 = 256;
pub const MAX_SSE_STREAMS: u32 = 32;
pub const MAX_REQUESTS_PER_CONN: u32 = 1000;
/// One handler, apart from SSE.
pub const BUSY_MS: u64 = 120_000;

#[derive(Clone, Copy)]
pub struct Timeouts {
    pub idle_ms: u32,
    pub head_ms: u32,
    pub write_ms: u32,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts { idle_ms: 30_000, head_ms: 10_000, write_ms: 20_000 }
    }
}

pub struct ConnState {
    sock: TcpStream,
    deadline_ns: AtomicI64,
    expired: AtomicBool,
}

thread_local! {
    static CURRENT: RefCell<Option<(Arc<ConnState>, std::time::Instant)>> = const { RefCell::new(None) };
}

fn elapsed_ns(base: std::time::Instant) -> i64 {
    i64::try_from(base.elapsed().as_nanos()).unwrap_or(i64::MAX)
}

/// Deadline for the rest of what this connection is doing; 0 disarms.
pub fn arm(ms: u64) {
    CURRENT.with(|c| {
        if let Some((conn, base)) = c.borrow().as_ref() {
            let d = if ms == 0 {
                0
            } else {
                elapsed_ns(*base).saturating_add(i64::try_from(ms).unwrap_or(i64::MAX).saturating_mul(1_000_000))
            };
            conn.deadline_ns.store(d, Ordering::Release);
        }
    });
}

pub fn conn_watchdog(s: Arc<Server>) {
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let now = elapsed_ns(s.started);
        for c in lock(&s.conns).iter() {
            let d = c.deadline_ns.load(Ordering::Acquire);
            if d != 0 && now > d && !c.expired.swap(true, Ordering::AcqRel) {
                let _ = c.sock.shutdown(Shutdown::Both);
            }
        }
    }
}

pub fn serve_conn(s: Arc<Server>, stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let Ok(clone) = stream.try_clone() else {
        s.active_conns.fetch_sub(1, Ordering::AcqRel);
        return;
    };
    let conn = Arc::new(ConnState { sock: clone, deadline_ns: AtomicI64::new(0), expired: AtomicBool::new(false) });
    lock(&s.conns).push(conn.clone());
    CURRENT.with(|c| *c.borrow_mut() = Some((conn.clone(), s.started)));
    run(&s, stream);
    CURRENT.with(|c| *c.borrow_mut() = None);
    lock(&s.conns).retain(|c| !Arc::ptr_eq(c, &conn));
    s.active_conns.fetch_sub(1, Ordering::AcqRel);
}

fn run(s: &Server, stream: TcpStream) {
    let Ok(wsock) = stream.try_clone() else { return };
    let mut r: In = BufReader::with_capacity(http::MAX_HEAD, stream);
    let mut w: Out = BufWriter::with_capacity(16 * 1024, wsock);
    let t = s.cfg.timeouts;
    for _ in 0..MAX_REQUESTS_PER_CONN {
        arm(u64::from(t.idle_ms));
        match std::io::BufRead::fill_buf(&mut r) {
            Ok(b) if !b.is_empty() => {}
            _ => return,
        }
        arm(u64::from(t.head_ms));
        let mut req = match http::read_head(&mut r) {
            Ok(req) => req,
            Err(e) => {
                let _ = match e {
                    http::HeadError::HeadTooLarge => {
                        http::send_error(&mut w, 431, false, "", "E_HEAD", "request headers too large")
                    }
                    http::HeadError::Malformed => {
                        http::send_error(&mut w, 400, false, "", "E_HTTP", "malformed HTTP request")
                    }
                    http::HeadError::Unsupported => http::send_error(
                        &mut w,
                        501,
                        false,
                        "",
                        "E_HTTP",
                        "unsupported Transfer-Encoding (only chunked or Content-Length bodies)",
                    ),
                    _ => Ok(()),
                };
                return;
            }
        };
        arm(BUSY_MS);
        let keep = routes::handle_request(s, &mut req, &mut r, &mut w).unwrap_or(false);
        arm(u64::from(t.write_ms));
        if w.flush().is_err() || !keep || !req.keep_alive {
            return;
        }
    }
}

pub struct Reject {
    pub status: u16,
    pub code: &'static str,
    pub message: &'static str,
}

pub fn host_is_loopback_name(host_header: &str) -> bool {
    let mut h = host_header;
    if let Some(inner) = h.strip_prefix('[') {
        let Some(close) = inner.find(']') else { return false };
        h = &inner[..close];
    } else if let Some(c) = h.rfind(':') {
        h = &h[..c];
    }
    h.eq_ignore_ascii_case("localhost")
        || h.to_ascii_lowercase().ends_with(".localhost")
        || h == "127.0.0.1"
        || h == "::1"
}

pub fn origin_matches_host(origin: &str, host: &str) -> bool {
    origin.split_once("://").is_some_and(|(_, rest)| rest.eq_ignore_ascii_case(host))
}

pub fn check_access(s: &Server, req: &Request) -> Option<Reject> {
    let host = req.header("host").unwrap_or("");
    if let Some(origin) = req.header("origin") {
        let same = origin_matches_host(origin, host);
        let extra = s.cfg.allow_origin.as_deref() == Some(origin);
        if !same && !extra {
            return Some(Reject {
                status: 403,
                code: "E_ORIGIN",
                message: "cross-origin request refused (the UI must be served by this kerf serve)",
            });
        }
    }
    if let Some(tok) = &s.cfg.token {
        if req.method == "OPTIONS" {
            return None;
        }
        let mut supplied = None;
        if let Some(auth) = req.header("authorization")
            && auth.len() > 7 && auth[..7].eq_ignore_ascii_case("bearer ") {
                supplied = Some(auth[7..].trim_matches(' ').to_string());
            }
        if supplied.is_none() {
            supplied = req.param("token");
        }
        if !supplied.is_some_and(|t| http::secret_eql(t.as_bytes(), tok.as_bytes())) {
            return Some(Reject {
                status: 401,
                code: "E_TOKEN",
                message: "missing or wrong token: send Authorization: Bearer <token> (SSE and downloads: ?token=<token>)",
            });
        }
    } else if s.loopback_only && !host_is_loopback_name(host) {
        return Some(Reject {
            status: 403,
            code: "E_HOST",
            message: "unexpected Host header; use http://localhost or http://127.0.0.1",
        });
    }
    None
}
