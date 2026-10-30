//! PDF object writer with a classic cross-reference table:
//! every xref entry is the offset of its object's `N 0 obj` header, entries are 20 bytes and in order, and
//! `startxref` points at `xref`. Self-contained: byte slices in, byte vector out.

use alloc::vec::Vec;

pub fn push_dec(out: &mut Vec<u8>, n: usize) {
    if n >= 10 {
        push_dec(out, n / 10);
    }
    out.push(b'0' + (n % 10) as u8);
}

pub fn push_dec_fixed(out: &mut Vec<u8>, n: usize, w: usize) {
    if w > 0 {
        push_dec_fixed(out, n / 10, w - 1);
        out.push(b'0' + (n % 10) as u8);
    } else {
    }
}

/// A PDF writer: the file so far and the offset of each object's `N 0 obj` header.
pub struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    /// `%PDF-1.7` and the binary-comment line.
    pub fn new() -> Self {
        let mut out = Vec::new();
        push_bytes(
            &mut out,
            &[
                37u8, 80, 68, 70, 45, 49, 46, 55, 10, 37, 0xE2, 0xE3, 0xCF, 0xD3, 10,
            ],
        );
        let w = Writer {
            out,
            offsets: Vec::new(),
        };
        w
    }

    /// Starts the next object: records its offset and writes `N 0 obj\n`. Returns N.
    pub fn begin_object(&mut self) -> usize {
        let off = self.out.len();
        let n = self.offsets.len() + 1;
        push_dec(&mut self.out, n);
        push_bytes(&mut self.out, &[32u8, 48, 32, 111, 98, 106, 10]);
        self.offsets.push(off);
        n
    }

    /// Appends raw bytes (dictionary text, stream data).
    pub fn put(&mut self, bytes: &[u8]) {
        push_bytes(&mut self.out, bytes);
    }

    /// Appends the decimal form of `n` (`{d}`).
    pub fn push_usize(&mut self, n: usize) {
        push_dec(&mut self.out, n);
    }

    pub fn end_object(&mut self) {
        self.put(&[101u8, 110, 100, 111, 98, 106, 10]);
    }

    /// A whole object whose body is `body` (a dictionary, say), then a newline and `endobj`.
    pub fn object(&mut self, body: &[u8]) -> usize {
        let n = self.begin_object();
        self.put(body);
        self.put(&[10u8]);
        self.end_object();
        n
    }

    /// A stream object with only a /Length entry.
    pub fn stream_object(&mut self, data: &[u8]) -> usize {
        let n = self.begin_object();
        self.put(&[60u8, 60, 32, 47, 76, 101, 110, 103, 116, 104, 32]);
        self.push_usize(data.len());
        self.put(&[32u8, 62, 62, 10, 115, 116, 114, 101, 97, 109, 10]);
        self.put(data);
        self.put(&[101u8, 110, 100, 115, 116, 114, 101, 97, 109, 10]);
        self.end_object();
        n
    }

    /// Appends the xref table and trailer; `root` and `info` are object numbers. Entries are padded
    /// to at least 10 digits (`{d:0>10}`), so an offset of 10^10 or more widens its entry, as in Zig.
    pub fn finish(self, root: usize, info: usize) -> Vec<u8> {
        let mut out = self.out;
        let offsets = self.offsets;
        let k = offsets.len();
        let x = out.len();
        let count = k + 1;
        push_xref_head(&mut out, count);
        let mut i: usize = 0;
        while i < k {
            push_entry(&mut out, offsets[i]);
            i += 1;
        }
        push_trailer(&mut out, count, root, info, x);
        out
    }
}

fn push_xref_head(out: &mut Vec<u8>, count: usize) {
    push_bytes(out, &[120u8, 114, 101, 102, 10, 48, 32]);
    push_dec(out, count);
    push_bytes(
        out,
        &[
            10u8, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 32, 54, 53, 53, 51, 53, 32, 102, 32, 10,
        ],
    );
}

fn push_trailer(out: &mut Vec<u8>, count: usize, root: usize, info: usize, x: usize) {
    push_bytes(
        out,
        &[
            116u8, 114, 97, 105, 108, 101, 114, 10, 60, 60, 32, 47, 83, 105, 122, 101, 32,
        ],
    );
    push_dec(out, count);
    push_bytes(out, &[32u8, 47, 82, 111, 111, 116, 32]);
    push_dec(out, root);
    push_bytes(out, &[32u8, 48, 32, 82, 32, 47, 73, 110, 102, 111, 32]);
    push_dec(out, info);
    push_bytes(
        out,
        &[
            32u8, 48, 32, 82, 32, 62, 62, 10, 115, 116, 97, 114, 116, 120, 114, 101, 102, 10,
        ],
    );
    push_dec(out, x);
    push_bytes(out, &[10u8, 37, 37, 69, 79, 70, 10]);
}

fn push_entry(out: &mut Vec<u8>, o: usize) {
    if (o as u64) < 10_000_000_000u64 {
        push_dec_fixed(out, o, 10);
    } else {
        push_dec(out, o);
    }
    push_bytes(out, &[32u8, 48, 48, 48, 48, 48, 32, 110, 32, 10]);
}

pub fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
