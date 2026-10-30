//! SSE hub: a bounded ring of pre-formatted frames. Publishers append, every SSE connection thread waits on the
//! condition and copies the frames it has not seen yet. A slow client can lose old frames, never block publishers.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};

pub const MAX_FRAMES: usize = 512;
pub const MAX_BYTES: usize = 8 << 20;

struct Frame {
    seq: u64,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct State {
    seq: u64,
    frames: VecDeque<Frame>,
    bytes: usize,
    closed: bool,
}

#[derive(Default)]
pub struct Hub {
    state: Mutex<State>,
    cond: Condvar,
}

impl Hub {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn publish(&self, name: &str, data_json: &str) {
        // A CR or LF inside the data would split the SSE frame: the data is one line by contract.
        let data: String = data_json.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
        let bytes = format!("event: {name}\ndata: {data}\n\n").into_bytes();
        let mut st = self.lock();
        st.seq += 1;
        let seq = st.seq;
        st.bytes += bytes.len();
        st.frames.push_back(Frame { seq, bytes });
        while st.frames.len() > 1 && (st.frames.len() > MAX_FRAMES || st.bytes > MAX_BYTES) {
            if let Some(old) = st.frames.pop_front() {
                st.bytes -= old.bytes.len();
            }
        }
        self.cond.notify_all();
    }

    pub fn current(&self) -> u64 {
        self.lock().seq
    }

    /// Block until frames newer than `*last` exist, append them to `out`, advance `*last`. False once closed.
    pub fn wait(&self, last: &mut u64, out: &mut Vec<u8>) -> bool {
        let mut st = self.lock();
        while st.seq == *last && !st.closed {
            st = self.cond.wait(st).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if st.closed && st.seq == *last {
            return false;
        }
        for f in st.frames.iter().filter(|f| f.seq > *last) {
            out.extend_from_slice(&f.bytes);
        }
        *last = st.seq;
        true
    }

    #[allow(dead_code)]
    pub fn close(&self) {
        self.lock().closed = true;
        self.cond.notify_all();
    }
}
