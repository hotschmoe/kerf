//! PDF object writer with a classic cross-reference table. Self-contained: byte slices in, byte vector out.

use alloc::vec::Vec;

pub struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

fn push_uint(out: &mut Vec<u8>, mut v: usize) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

fn push_padded(out: &mut Vec<u8>, v: usize, width: usize) {
    let mut digits = Vec::new();
    push_uint(&mut digits, v);
    for _ in digits.len()..width {
        out.push(b'0');
    }
    out.extend_from_slice(&digits);
}

impl Writer {
    pub fn new() -> Self {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        Writer { out, offsets: Vec::new() }
    }

    /// Starts the next indirect object and returns its number (objects are numbered 1, 2, 3, ...).
    pub fn begin_object(&mut self) -> usize {
        self.offsets.push(self.out.len());
        let n = self.offsets.len();
        push_uint(&mut self.out, n);
        self.out.extend_from_slice(b" 0 obj\n");
        n
    }

    pub fn put(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    pub fn end_object(&mut self) {
        self.out.extend_from_slice(b"endobj\n");
    }

    /// A whole object whose body is `body` (a dictionary, say).
    pub fn object(&mut self, body: &[u8]) -> usize {
        let n = self.begin_object();
        self.put(body);
        self.put(b"\n");
        self.end_object();
        n
    }

    /// A stream object with only a /Length entry.
    pub fn stream_object(&mut self, data: &[u8]) -> usize {
        let n = self.begin_object();
        self.put(b"<< /Length ");
        push_uint(&mut self.out, data.len());
        self.put(b" >>\nstream\n");
        self.put(data);
        self.put(b"endstream\n");
        self.end_object();
        n
    }

    /// Appends the xref table and trailer; `root` and `info` are object numbers.
    pub fn finish(mut self, root: usize, info: usize) -> Vec<u8> {
        let xref = self.out.len();
        let count = self.offsets.len() + 1;
        self.out.extend_from_slice(b"xref\n0 ");
        push_uint(&mut self.out, count);
        self.out.extend_from_slice(b"\n0000000000 65535 f \n");
        for &off in &self.offsets {
            push_padded(&mut self.out, off, 10);
            self.out.extend_from_slice(b" 00000 n \n");
        }
        self.out.extend_from_slice(b"trailer\n<< /Size ");
        push_uint(&mut self.out, count);
        self.out.extend_from_slice(b" /Root ");
        push_uint(&mut self.out, root);
        self.out.extend_from_slice(b" 0 R /Info ");
        push_uint(&mut self.out, info);
        self.out.extend_from_slice(b" 0 R >>\nstartxref\n");
        push_uint(&mut self.out, xref);
        self.out.extend_from_slice(b"\n%%EOF\n");
        self.out
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
