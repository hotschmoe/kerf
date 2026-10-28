//! The SSE stream (`GET /api/events`).

use super::conn::arm;
use super::{MAX_SSE_STREAMS, Server};
use crate::http::{self, Head, Out};
use std::io::{self, Write};
use std::sync::atomic::Ordering;

struct Slot<'a>(&'a std::sync::atomic::AtomicU32);

impl<'a> Slot<'a> {
    fn take(counter: &'a std::sync::atomic::AtomicU32, max: u32) -> Option<Slot<'a>> {
        if counter.fetch_add(1, Ordering::AcqRel) >= max {
            counter.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Slot(counter))
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn api_events(s: &Server, w: &mut Out, extra: &str) -> io::Result<bool> {
    let Some(_slot) = Slot::take(&s.sse_active, MAX_SSE_STREAMS) else {
        http::send_error(w, 503, false, extra, "E_BUSY", "too many open event streams")?;
        return Ok(false);
    };
    let hdr = format!("X-Accel-Buffering: no\r\n{extra}");
    arm(u64::from(s.cfg.timeouts.write_ms));
    http::write_head(
        w,
        &Head {
            status: 200,
            content_type: Some("text/event-stream; charset=utf-8"),
            keep_alive: false,
            extra: &hdr,
            ..Head::default()
        },
    )?;
    w.write_all(b"retry: 2000\n\nevent: ping\ndata: {}\n\n")?;
    w.flush()?;
    let mut last = s.hub.current();
    let mut buf = Vec::new();
    loop {
        arm(0);
        buf.clear();
        if !s.hub.wait(&mut last, &mut buf) {
            return Ok(false);
        }
        arm(u64::from(s.cfg.timeouts.write_ms));
        if w.write_all(&buf).and_then(|()| w.flush()).is_err() {
            return Ok(false);
        }
    }
}
